//! Keep top-level panes above the icon desktop and below ordinary applications.
use super::*;
use windows_sys::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};

const MANAGED: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.DesktopOwner");
const HOST: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.DesktopHost");
const RETURN_TO_HOST: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.ReturnToDesktopHost");
const SHELL_OWNED: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.ShellOwnedPane");
#[derive(Default)]
struct Watch {
    panes: Vec<usize>,
    timer: usize,
    pending: Option<(usize, usize)>,
    hooks: Vec<HWINEVENTHOOK>,
    retries: u8,
    desktop: [usize; 4],
    restored: Vec<usize>,
}
thread_local! {
    static WATCH: RefCell<Watch> = RefCell::new(Watch::default());
    static UPDATING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NEXT_REQUEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SHORTCUT: std::cell::Cell<HHOOK> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

fn watch(hwnd: HWND) {
    WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        if !watch.panes.contains(&(hwnd as usize)) { watch.panes.push(hwnd as usize); }
    });
    ensure_hooks();
    request_sync();
}

fn ensure_hooks() {
    if WATCH.with(|watch| !watch.borrow().hooks.is_empty() || watch.borrow().panes.is_empty()) { return; }
    let hooks = register_hooks(WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
    if hooks.is_empty() {
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn, "pane.desktop_owner",
            "WinEvent registration failed; retry with 5-second fallback");
    }
    WATCH.with(|watch| watch.borrow_mut().hooks = hooks);
}

fn register_hooks(flags: u32) -> Vec<HWINEVENTHOOK> {
    let mut hooks = Vec::new();
    // Process-independent subscriptions survive Explorer replacement. Leave
    // out focus, selection and location events between reorder and parent change.
    for (first, last) in [(EVENT_OBJECT_CREATE, EVENT_OBJECT_REORDER),
        (EVENT_OBJECT_PARENTCHANGE, EVENT_OBJECT_PARENTCHANGE),
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZEEND)] {
        if first == EVENT_SYSTEM_MINIMIZEEND && !uses_shell_owner() { continue; }
        // Ordinary windows in our own process can also end Show Desktop.
        let hook_flags = if first < EVENT_OBJECT_CREATE {
            flags & !WINEVENT_SKIPOWNPROCESS
        } else {
            flags
        };
        let hook = unsafe { SetWinEventHook(first, last, std::ptr::null_mut(), Some(changed), 0, 0, hook_flags) };
        if hook.is_null() {
            for hook in hooks { unsafe { UnhookWinEvent(hook); } }
            return Vec::new();
        }
        hooks.push(hook);
    }
    hooks
}

fn relevant(event: u32, hwnd: HWND, object: i32, child: i32) -> bool {
    if hwnd.is_null() || child != 0 || (object != OBJID_WINDOW && object != OBJID_CLIENT)
        || !matches!(event, EVENT_OBJECT_CREATE..=EVENT_OBJECT_REORDER
            | EVENT_OBJECT_PARENTCHANGE | EVENT_SYSTEM_FOREGROUND) {
        return false;
    }
    if WATCH.with(|watch| watch.borrow().desktop.contains(&(hwnd as usize))) { return true; }
    if event == EVENT_SYSTEM_FOREGROUND
        && WATCH.with(|watch| watch.borrow().panes.iter()
            .any(|&pane| unsafe { !GetPropW(pane as HWND, RETURN_TO_HOST).is_null() })) {
        return true;
    }
    let mut class = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) }.max(0) as usize;
    ["Progman", "WorkerW", "SHELLDLL_DefView"].into_iter().any(|name| {
        class[..len].iter().copied().eq(name.encode_utf16())
    })
}

unsafe extern "system" fn changed(hook: HWINEVENTHOOK, event: u32, hwnd: HWND,
    object: i32, child: i32, _: u32, _: u32) {
    if !WATCH.with(|watch| watch.borrow().hooks.contains(&hook)) { return; }
    if event == EVENT_SYSTEM_MINIMIZEEND && object == OBJID_WINDOW && child == 0 {
        let pending = WATCH.with(|watch| {
            let mut watch = watch.borrow_mut();
            if !watch.panes.iter().any(|&pane| unsafe {
                shell_owned(pane as HWND) && !GetPropW(pane as HWND, RETURN_TO_HOST).is_null()
            }) { return false; }
            if !watch.restored.contains(&(hwnd as usize)) {
                watch.restored.push(hwnd as usize);
            }
            true
        });
        if pending { request_sync(); }
    } else if relevant(event, hwnd, object, child) {
        // Never discover the desktop or reorder windows from the WinEvent
        // callback: Explorer may still be in the middle of a transition.
        request_sync();
    }
}

pub(super) fn request_sync() {
    let request = WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        let &pane = watch.panes.first()?;
        watch.retries = 2;
        if watch.pending.is_some() { return None; }
        let token = NEXT_REQUEST.with(|next| {
            let token = next.get().wrapping_add(1);
            next.set(token);
            token
        });
        watch.pending = Some((pane, token));
        Some((pane, token, std::mem::take(&mut watch.timer)))
    });
    let Some((pane, token, timer)) = request else { return; };
    if timer != 0 { unsafe { KillTimer(std::ptr::null_mut(), timer); } }
    // Fresh Shell events must not wait behind 250/1000 ms settle timers.
    // Posted work runs outside the hook callback and is coalesced without
    // delaying it until the event stream becomes quiet.
    if !post_action(pane as HWND, move || {
        let active = WATCH.with(|watch| {
            let mut watch = watch.borrow_mut();
            if watch.pending != Some((pane, token)) { return false; }
            watch.pending = None;
            true
        });
        if active { run_sync(); }
    }) {
        WATCH.with(|watch| watch.borrow_mut().pending = None);
        schedule(USER_TIMER_MINIMUM);
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn, "pane.desktop_owner",
            "could not post immediate host synchronization; using timer fallback");
    }
}

fn schedule(delay: u32) {
    WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        if watch.panes.is_empty() || watch.pending.is_some() || watch.timer != 0 { return; }
        watch.timer = unsafe { SetTimer(std::ptr::null_mut(), 0, delay, Some(dispatch)) };
        if watch.timer == 0 {
            luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Error,
                "pane.desktop_owner", "could not schedule desktop host synchronization");
        }
    });
}

unsafe extern "system" fn dispatch(_: HWND, _: u32, timer: usize, _: u32) {
    let active = WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        if watch.timer != timer || timer == 0 { return false; }
        watch.timer = 0;
        true
    });
    if !active { return; }
    unsafe { KillTimer(std::ptr::null_mut(), timer); }
    run_sync();
}

fn run_sync() {
    let view = luciddesk_explorer::desktop_view().unwrap_or_default() as HWND;
    if !view.is_null() {
        let desktop = unsafe { [view, GetParent(view), host(view, std::ptr::null_mut()), GetShellWindow()] };
        WATCH.with(|watch| watch.borrow_mut().desktop = desktop.map(|hwnd| hwnd as usize));
    }
    synchronize(view);
    ensure_hooks();
    let next = WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        if watch.panes.is_empty() || watch.pending.is_some() { return None; }
        if watch.hooks.is_empty() { return Some(5_000); }
        if watch.retries == 0 { return None; }
        watch.retries -= 1;
        Some(if watch.retries == 1 { 250 } else { 1_000 })
    });
    if let Some(delay) = next {
        schedule(delay);
    } else {
        // Foreground can change before Explorer finishes lowering the desktop.
        // Retain the return marker through the finite settle retries, otherwise
        // an early successful placement follows a still-raised host only once.
        let foreground = unsafe { GetForegroundWindow() };
        let owner = host(view, std::ptr::null_mut());
        let settled = WATCH.with(|watch| {
            let watch = watch.borrow();
            watch.pending.is_none() && watch.timer == 0 && watch.retries == 0
        });
        let restored = settled && WATCH.with(|watch| {
            let restored = std::mem::take(&mut watch.borrow_mut().restored);
            restored.into_iter().any(|hwnd| application_boundary(hwnd as HWND))
        });
        if settled && !owner.is_null() && (restored || (unsafe { GetAncestor(foreground, GA_ROOT) != owner }
            && application_boundary(foreground))) {
            let panes = WATCH.with(|watch| watch.borrow().panes.clone());
            for pane in panes { unsafe { RemovePropW(pane as HWND, RETURN_TO_HOST); } }
        }
    }
}

pub(super) fn detach(hwnd: HWND) {
    let (stopped, repost) = WATCH.with(|watch| {
        let mut watch = watch.borrow_mut();
        watch.panes.retain(|&pane| pane != hwnd as usize);
        let repost = watch.pending.is_some_and(|(pane, _)| pane == hwnd as usize);
        if repost { watch.pending = None; }
        (watch.panes.is_empty().then(|| std::mem::take(&mut *watch)), repost)
    });
    if let Some(watch) = stopped {
        if watch.timer != 0 { unsafe { KillTimer(std::ptr::null_mut(), watch.timer); } }
        for hook in watch.hooks { unsafe { UnhookWinEvent(hook); } }
        let shortcut = SHORTCUT.with(|hook| hook.replace(std::ptr::null_mut()));
        if !shortcut.is_null() { unsafe { UnhookWindowsHookEx(shortcut); } }
    } else if repost {
        // The queued action belongs to the destroyed pane and will be cancelled.
        // Move the work to a surviving pane rather than leaving it stuck pending.
        request_sync();
    }
    unsafe {
        RemovePropW(hwnd, MANAGED);
        RemovePropW(hwnd, HOST);
        RemovePropW(hwnd, RETURN_TO_HOST);
        RemovePropW(hwnd, SHELL_OWNED);
    }
}

fn synchronize(view: HWND) {
    if UPDATING.with(|busy| busy.replace(true)) { return; }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { UPDATING.with(|busy| busy.set(false)); }
    }
    let _reset = Reset;
    // Do not fall back to Progman during a temporary discovery failure.
    let owner = host(view, std::ptr::null_mut());
    if owner.is_null() { return; }
    let foreground = unsafe { GetForegroundWindow() };
    let showing_desktop = unsafe { GetAncestor(foreground, GA_ROOT) == owner };
    let returning = !showing_desktop && application_boundary(foreground);
    // The Shell can keep focus after handing back the minimized applications.
    let restored = WATCH.with(|watch| {
        watch.borrow().restored.iter().any(|&hwnd| application_boundary(hwnd as HWND))
    });
    let desktop_is_exposed = showing_desktop && !restored;
    let panes = WATCH.with(|watch| watch.borrow().panes.clone());
    let Some(order) = window_order() else { return; };
    for pane in panes {
        let hwnd = pane as HWND;
        if unsafe { IsWindow(hwnd) == 0 || GetPropW(hwnd, MANAGED).is_null() } {
            detach(hwnd);
            continue;
        }
        let changed = update_host(hwnd, view, owner);
        let owned_by_shell = shell_owned(hwnd);
        let restore_band = (returning || (restored && owned_by_shell))
            && unsafe { !GetPropW(hwnd, RETURN_TO_HOST).is_null() };
        // Win10 can change the icon host as it shows the desktop. A pane
        // already above that host should stay there while Explorer settles.
        if restore_band || (changed && (owned_by_shell || !desktop_is_exposed)) {
            lower_in_order(hwnd, owner, &order);
        }
        repair_in_order(hwnd, owner, &order, desktop_is_exposed);
        // An explicitly revealed pane may already be above the desktop and
        // need no upward repair, but must still return when Win+D restores apps.
        if desktop_is_exposed && is_visible_unpinned(hwnd) {
            unsafe { SetPropW(hwnd, RETURN_TO_HOST, 1usize as _); }
        }
    }
}

fn host(view: HWND, fallback: HWND) -> HWND {
    let root = unsafe { GetAncestor(view, GA_ROOT) };
    if root.is_null() { fallback } else { root }
}

fn uses_shell_owner() -> bool {
    let os = windows_version::OsVersion::current();
    os.major > 10 || (os.major == 10 && os.build >= 22000)
}

pub(super) fn shell_owned(hwnd: HWND) -> bool {
    unsafe { !GetPropW(hwnd, SHELL_OWNED).is_null() }
}

pub(in crate::pane) enum DesktopPane {
    Regular,
    Search,
}

pub(in crate::pane) fn attach(hwnd: HWND, pane: DesktopPane) {
    // Win10 search keeps its own window ordering.
    if matches!(pane, DesktopPane::Search) && !uses_shell_owner() {
        return;
    }
    let shell = unsafe { GetShellWindow() };
    if uses_shell_owner() && !shell.is_null() {
        // A Win32 owner gives Explorer its original Show Desktop behavior
        // while leaving the pane top-level for DWM host backdrops.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, shell as isize);
            if GetWindow(hwnd, GW_OWNER) == shell {
                SetPropW(hwnd, SHELL_OWNED, 1usize as _);
            }
        }
    }
    unsafe { SetPropW(hwnd, MANAGED, 1usize as _); }
    let view = luciddesk_explorer::desktop_view().unwrap_or_default() as HWND;
    let host = host(view, shell);
    update_host(hwnd, view, host);
}

pub(in crate::pane) fn start(hwnd: HWND) {
    if shell_owned(hwnd) {
        SHORTCUT.with(|slot| {
            if slot.get().is_null() {
                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(shortcut),
                    windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()), 0) };
                slot.set(hook);
                if hook.is_null() {
                    luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn, "pane.desktop_owner",
                        "could not register Win+D focus handoff");
                }
            }
        });
    }
    if unsafe { !GetPropW(hwnd, MANAGED).is_null() } {
        watch(hwnd);
    }
}

unsafe extern "system" fn shortcut(code: i32, wp: usize, lp: isize) -> isize {
    unsafe {
        if code == HC_ACTION as i32 && matches!(wp as u32, WM_KEYDOWN | WM_SYSKEYDOWN)
            && (*(lp as *const KBDLLHOOKSTRUCT)).vkCode == 0x44
            && (GetAsyncKeyState(VK_LWIN as i32) < 0 || GetAsyncKeyState(VK_RWIN as i32) < 0)
            && [VK_CONTROL, VK_SHIFT, VK_MENU].iter().all(|&key| GetAsyncKeyState(key as i32) >= 0) {
            let foreground = GetForegroundWindow();
            let pane = if shell_owned(foreground) { foreground } else { GetWindow(foreground, GW_OWNER) };
            let shell = GetShellWindow();
            if !shell.is_null() && shell_owned(pane) && GetWindow(pane, GW_OWNER) == shell
                && WATCH.with(|watch| watch.borrow().panes.contains(&(pane as usize))) {
                // Explorer must get focus before it saves which apps to
                // restore. Continue passing the shortcut to the Shell.
                SetForegroundWindow(shell);
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
    }
}

pub(super) unsafe fn raise_owned(hwnd: HWND) -> i32 {
    unsafe {
        // HWND_TOP alone can leave an Explorer-owned popup behind the
        // foreground app. Reset its stacking position synchronously.
        if SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, REORDER_WITHOUT_ACTIVATION) == 0 {
            return 0;
        }
        if SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, REORDER_WITHOUT_ACTIVATION) != 0 {
            return 1;
        }
        SetWindowPos(
            hwnd, HWND_NOTOPMOST, 0, 0, 0, 0,
            REORDER_WITHOUT_ACTIVATION & !SWP_NOOWNERZORDER,
        )
    }
}

pub(super) fn refresh(hwnd: HWND) {
    let view = luciddesk_explorer::desktop_view().unwrap_or_default() as HWND;
    update_host(hwnd, view, host(view, std::ptr::null_mut()));
}

fn update_host(hwnd: HWND, view: HWND, owner: HWND) -> bool {
    unsafe {
        if GetPropW(hwnd, MANAGED).is_null() { return false; }
        let previous = GetPropW(hwnd, HOST);
        if owner.is_null() || owner == hwnd || previous == owner { return false; }
        // The icon host can move between Explorer windows. Track it
        // independently of any native ownership used by the pane.
        let tracked = SetPropW(hwnd, HOST, owner) != 0;
        if tracked { InvalidateRect(hwnd, std::ptr::null(), 0); }
        luciddesk_diagnostics::emit!(
            luciddesk_diagnostics::Level::Info,
            "pane.desktop_owner",
            "pane={hwnd:?} view={view:?} host={owner:?} tracked={tracked} native_owner={:?}",
            GetWindow(hwnd, GW_OWNER)
        );
        tracked && !previous.is_null()
    }
}

fn is_visible_unpinned(hwnd: HWND) -> bool {
    unsafe {
        IsWindowVisible(hwnd) != 0
            && IsIconic(hwnd) == 0
            && GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0
    }
}

// When Show Desktop ends, its host moves down but an independent pane stays
// where it was raised. Follow that downward transition without activating it.
// Run on a host change or a tracked Show Desktop return, so unrelated events
// in the normal desktop state do not undo explicit reveal.
fn lower_in_order(hwnd: HWND, host: HWND, order: &[HWND]) {
    unsafe {
        if !is_visible_unpinned(hwnd) { return; }
        let (Some(pane_rank), Some(host_rank)) = (
            order.iter().position(|&window| window == hwnd),
            order.iter().position(|&window| window == host),
        ) else { return; };
        if host_rank <= pane_rank { return; }
        let after = if shell_owned(hwnd) {
            // Stay above the Shell's own window group. Inserting just above
            // the host can briefly put an owned popup behind the icon view.
            let Some(&app) = order[pane_rank + 1..host_rank]
                .iter()
                .rev()
                .find(|&&window| application_boundary(window))
            else {
                return;
            };
            app
        } else {
            GetWindow(host, GW_HWNDPREV)
        };
        if after == hwnd || after.is_null() { return; }
        let desktop = RemovePropW(hwnd, DESKTOP_LAYER);
        let moved = SetWindowPos(hwnd, after, 0, 0, 0, 0, REORDER_WITHOUT_ACTIVATION);
        let error = if moved == 0 { windows_sys::Win32::Foundation::GetLastError() } else { 0 };
        if !desktop.is_null() { SetPropW(hwnd, DESKTOP_LAYER, desktop); }
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info, "pane.desktop_owner",
            "restore desktop band; pane={hwnd:?} host={host:?} moved={moved} error={error}");
    }
}

// Follow actual Z order, not EnumWindows callback order. Hidden helpers may
// put the desktop more than 512 entries away. Detect cycles if windows change
// during traversal rather than silently truncating a valid crowded desktop.
pub(super) fn window_order() -> Option<Vec<HWND>> {
    let mut order = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut current = unsafe { GetTopWindow(std::ptr::null_mut()) };
    while !current.is_null() {
        if !seen.insert(current as usize) { return None; }
        order.push(current);
        current = unsafe { GetWindow(current, GW_HWNDNEXT) };
    }
    Some(order)
}

pub(super) fn application_boundary(hwnd: HWND) -> bool {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || hwnd == GetShellWindow() {
            return false;
        }
        // WS_VISIBLE also describes zero-area and entirely offscreen helpers
        // (for example NarratorHelperWindow at -48000, -48000 on Win10).
        // Check actual monitors, not coordinate signs or the virtual bounding
        // box: negative-coordinate monitors and partly visible apps still count.
        // Keep failed geometry queries conservative, just like failed cloak queries.
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &raw mut rect) != 0
            && (rect.right <= rect.left || rect.bottom <= rect.top
                || MonitorFromRect(&raw const rect, MONITOR_DEFAULTTONULL).is_null()) {
            return false;
        }
        let thread = GetCurrentThreadId();
        let mut owner = hwnd;
        for _ in 0..64 {
            if owner.is_null() { break; }
            if GetWindowThreadProcessId(owner, std::ptr::null_mut()) == thread
                && (!GetPropW(owner, MANAGED).is_null() || is_desktop_layer(owner)) {
                return false;
            }
            owner = GetWindow(owner, GW_OWNER);
        }
        let mut cloak = 0u32;
        let result = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            windows::Win32::Foundation::HWND(hwnd), windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
            (&raw mut cloak).cast(), 4);
        // A failed query must not allow crossing a potentially visible app.
        result.is_err() || cloak == 0
    }
}

#[cfg(test)]
fn repair_order(hwnd: HWND, owner: HWND) {
    let Some(order) = window_order() else { return; };
    let showing_desktop = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) == owner };
    repair_in_order(hwnd, owner, &order, showing_desktop);
}

fn repair_in_order(hwnd: HWND, owner: HWND, order: &[HWND], showing_desktop: bool) {
    unsafe {
        if !is_visible_unpinned(hwnd) { return; }
        let (Some(pane_rank), Some(host_rank)) = (
            order.iter().position(|&window| window == hwnd),
            order.iter().position(|&window| window == owner),
        ) else { return; };
        if host_rank >= pane_rank { return; }
        // When the icon desktop itself is foreground, everything below it is
        // already covered by Show Desktop, including non-minimizable helpers.
        // Such windows must not prevent panes joining the visible desktop.
        // Apps above the desktop are never crossed by insertion just above it.
        if !showing_desktop && let Some(blocker) = order[host_rank + 1..pane_rank].iter().rev()
            .copied().find(|&window| application_boundary(window)) {
            luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info, "pane.desktop_owner",
                "repair blocked; pane={hwnd:?} host={owner:?} blocker=[{}]",
                super::desktop_diagnostics::facts(blocker));
            return;
        }
        let previous = GetWindow(owner, GW_HWNDPREV);
        let after = if previous.is_null()
            || GetWindowLongW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
            HWND_TOP
        } else { previous };
        // Repair just above the desktop, without activation or permanent topmost.
        let desktop = RemovePropW(hwnd, DESKTOP_LAYER);
        let moved = SetWindowPos(hwnd, after, 0, 0, 0, 0, REORDER_WITHOUT_ACTIVATION);
        let error = if moved == 0 { windows_sys::Win32::Foundation::GetLastError() } else { 0 };
        if !desktop.is_null() { SetPropW(hwnd, DESKTOP_LAYER, desktop); }
        if moved != 0 && showing_desktop { SetPropW(hwnd, RETURN_TO_HOST, 1usize as _); }
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Info, "pane.desktop_owner",
            "repair order; pane={hwnd:?} host={owner:?} after={after:?} moved={moved} error={error} showing_desktop={showing_desktop}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, SetActiveWindow};

    fn fixture() -> Window {
        Window::new("Desktop owner regression").size(100, 80).style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap()
    }

    fn managed(owner: HWND) -> (Window, Rc<std::cell::Cell<u32>>) {
        let positions = Rc::new(std::cell::Cell::new(0));
        let count = positions.clone();
        let pane = Window::new("Managed owner regression").size(100, 80).style(WS_POPUP)
            .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
            .on_message(move |_, msg, _, _| {
                if msg == WM_WINDOWPOSCHANGING { count.set(count.get() + 1); }
                (msg == WM_DESTROY).then_some(0)
            }).create().unwrap();
        unsafe {
            let hwnd = pane.hwnd().cast();
            assert_ne!(SetPropW(hwnd, MANAGED, 1usize as _), 0);
            update_host(hwnd, std::ptr::null_mut(), owner);
            assert_ne!(windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(borderless_proc), 1, 0), 0);
        }
        start(pane.hwnd().cast());
        (pane, positions)
    }

    fn above(first: HWND, second: HWND) -> bool {
        let mut current = unsafe { GetWindow(second, GW_HWNDPREV) };
        let mut seen = std::collections::HashSet::new();
        while !current.is_null() && seen.insert(current as usize) {
            if current == first { return true; }
            current = unsafe { GetWindow(current, GW_HWNDPREV) };
        }
        false
    }

    unsafe fn position(hwnd: HWND, after: HWND) {
        unsafe {
            assert_ne!(SetWindowPos(hwnd, after, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER), 0);
        }
    }

    #[test]
    fn returning_host_lowers_pane_below_restored_apps_without_changing_focus() {
        let shown_desktop = fixture();
        let normal_desktop = fixture();
        let view = fixture();
        let app = fixture();
        let (pane, positions) = managed(shown_desktop.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            SetWindowLongPtrW(view.hwnd().cast(), GWL_STYLE, WS_CHILD as isize);
            SetParent(view.hwnd().cast(), normal_desktop.hwnd().cast());
            ShowWindow(shown_desktop.hwnd().cast(), SW_HIDE);
            for window in [normal_desktop.hwnd(), app.hwnd(), pane.hwnd()] {
                ShowWindow(window.cast(), SW_SHOWNOACTIVATE);
            }
            RemovePropW(hwnd, DESKTOP_LAYER);
            position(hwnd, HWND_TOP);
            position(app.hwnd().cast(), hwnd);
            position(normal_desktop.hwnd().cast(), app.hwnd().cast());
            SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
            assert!(above(hwnd, app.hwnd().cast()), "model the stale raised position");
            let foreground = GetForegroundWindow();
            synchronize(view.hwnd().cast());
            assert!(above(app.hwnd().cast(), hwnd), "restored app must cover the pane without a click");
            assert!(above(hwnd, normal_desktop.hwnd().cast()));
            assert_eq!(GetForegroundWindow(), foreground);
            assert!(GetWindow(hwnd, GW_OWNER).is_null());
            // A later explicit raise is retained while the host stays unchanged.
            RemovePropW(hwnd, DESKTOP_LAYER);
            position(hwnd, HWND_TOP);
            SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
            positions.set(0);
            synchronize(view.hwnd().cast());
            assert_eq!(positions.get(), 0);
            assert!(above(hwnd, app.hwnd().cast()));
        }
    }

    #[test]
    fn active_pane_stays_out_of_desktop_popup_chain_across_host_switches() {
        let first = fixture();
        let second = fixture();
        let view = fixture();
        let (pane, _) = managed(first.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowLongPtrW(view.hwnd().cast(), GWL_STYLE, WS_CHILD as isize);
            SetActiveWindow(hwnd);
            assert_eq!(GetActiveWindow(), hwnd);
            for target in [second.hwnd(), first.hwnd(), second.hwnd(), first.hwnd()] {
                SetParent(view.hwnd().cast(), target.cast());
                synchronize(view.hwnd().cast());
                assert!(GetWindow(hwnd, GW_OWNER).is_null());
                assert_eq!(GetAncestor(hwnd, GA_ROOTOWNER), hwnd);
                assert_ne!(GetLastActivePopup(first.hwnd().cast()), hwnd);
                assert_ne!(GetLastActivePopup(second.hwnd().cast()), hwnd);
                assert_eq!(GetActiveWindow(), hwnd, "host synchronization must not change keyboard activation");
            }
        }
    }

    #[test]
    fn dynamic_host_switch_preserves_apps_geometry_visibility_and_topmost() {
        let first = fixture();
        let second = fixture();
        let view = fixture();
        let app = fixture();
        let (pane, positions) = managed(first.hwnd().cast());
        let (hidden, _) = managed(first.hwnd().cast());
        let (pinned, _) = managed(first.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            SetWindowLongPtrW(view.hwnd().cast(), GWL_STYLE, WS_CHILD as isize);
            SetParent(view.hwnd().cast(), first.hwnd().cast());
            ShowWindow(hidden.hwnd().cast(), SW_HIDE);
            set_layer(pinned.hwnd().cast(), true);
            let mut before = RECT::default();
            GetWindowRect(hwnd, &raw mut before);
            for (old, new) in [(first.hwnd().cast(), second.hwnd().cast()),
                (second.hwnd().cast(), first.hwnd().cast())] {
                ShowWindow(old, SW_HIDE);
                ShowWindow(new, SW_SHOWNOACTIVATE);
                position(app.hwnd().cast(), HWND_TOP);
                position(new, app.hwnd().cast());
                RemovePropW(hwnd, DESKTOP_LAYER);
                position(hwnd, new);
                SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
                assert!(above(new, hwnd), "fixture must start with the new desktop covering the pane");
                SetParent(view.hwnd().cast(), new);
                let foreground = GetForegroundWindow();
                synchronize(view.hwnd().cast());
                assert_eq!(GetPropW(hwnd, HOST), new);
                assert!(GetWindow(hwnd, GW_OWNER).is_null(), "pane must not join Explorer's activation chain");
                assert!(above(hwnd, new), "pane must follow the live icon host");
                assert!(above(app.hwnd().cast(), hwnd), "ordinary application must stay above pane");
                assert_eq!(GetForegroundWindow(), foreground, "repair must not activate");
                assert_eq!(IsWindowVisible(hidden.hwnd().cast()), 0);
                assert_eq!(GetPropW(hidden.hwnd().cast(), HOST), new);
                assert_eq!(GetPropW(pinned.hwnd().cast(), HOST), new);
                assert!(GetWindow(hidden.hwnd().cast(), GW_OWNER).is_null());
                assert!(GetWindow(pinned.hwnd().cast(), GW_OWNER).is_null());
                assert_ne!(GetWindowLongW(pinned.hwnd().cast(), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
                assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
                let mut after = RECT::default();
                GetWindowRect(hwnd, &raw mut after);
                assert_eq!((after.left, after.top, after.right, after.bottom),
                    (before.left, before.top, before.right, before.bottom));
                positions.set(0);
                synchronize(view.hwnd().cast());
                synchronize(std::ptr::null_mut());
                assert_eq!(positions.get(), 0, "unchanged or unavailable host must not reorder panes");
                assert_eq!(GetPropW(hwnd, HOST), new);
            }
        }
        WATCH.with(|watch| {
            assert_eq!(watch.borrow().panes.len(), 3);
            assert!(watch.borrow().pending.is_some());
        });
        drop(pinned);
        drop(hidden);
        drop(pane);
        WATCH.with(|watch| {
            assert!(watch.borrow().panes.is_empty());
            assert_eq!(watch.borrow().timer, 0);
            assert!(watch.borrow().hooks.is_empty());
        });
    }

    #[test]
    fn winevent_delivery_coalesces_work_and_stops_after_settling() {
        let owner = fixture();
        let event_window = fixture();
        let (pane, _) = managed(owner.hwnd().cast());
        // Include this test process to exercise real out-of-context delivery
        // with NotifyWinEvent; production skips its own window mutations.
        let old = WATCH.with(|watch| std::mem::take(&mut watch.borrow_mut().hooks));
        for hook in old { unsafe { UnhookWinEvent(hook); } }
        let hooks = register_hooks(WINEVENT_OUTOFCONTEXT);
        assert_eq!(hooks.len(), if uses_shell_owner() { 4 } else { 3 });
        WATCH.with(|watch| watch.borrow_mut().hooks = hooks);
        fn pump_until(done: impl Fn() -> bool) {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                unsafe {
                    let mut message = MSG::default();
                    while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        DispatchMessageW(&message);
                    }
                }
                if done() { break; }
                assert!(std::time::Instant::now() < until, "WinEvent work did not settle");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let idle = || WATCH.with(|watch| {
            let watch = watch.borrow();
            watch.timer == 0 && watch.pending.is_none()
        });
        pump_until(idle);
        let target = event_window.hwnd().cast();
        WATCH.with(|watch| watch.borrow_mut().desktop[0] = target as usize);
        assert!(!relevant(EVENT_OBJECT_NAMECHANGE, target, OBJID_WINDOW, 0));
        assert!(!relevant(EVENT_OBJECT_PARENTCHANGE, target, OBJID_CLIENT, 1));
        assert!(!relevant(EVENT_OBJECT_PARENTCHANGE, owner.hwnd().cast(), OBJID_WINDOW, 0));
        for event in [EVENT_OBJECT_REORDER, EVENT_OBJECT_PARENTCHANGE] {
            WATCH.with(|watch| watch.borrow_mut().desktop[0] = target as usize);
            unsafe { NotifyWinEvent(event, target, OBJID_WINDOW, 0); }
            pump_until(|| WATCH.with(|watch| watch.borrow().timer != 0));
            pump_until(idle);
        }
        for delay in [250, 1_000] {
            schedule(delay);
            let slow_timer = WATCH.with(|watch| watch.borrow().timer);
            assert_ne!(slow_timer, 0);
            request_sync();
            let pending = WATCH.with(|watch| watch.borrow().pending);
            assert!(pending.is_some(), "fresh event must post work immediately");
            assert_eq!(WATCH.with(|watch| watch.borrow().timer), 0, "cancel the slow retry");
            for _ in 0..20 { request_sync(); }
            assert_eq!(WATCH.with(|watch| watch.borrow().pending), pending,
                "a continuous burst must not postpone already queued work");
            // KillTimer does not remove already queued timer messages.
            unsafe { dispatch(std::ptr::null_mut(), WM_TIMER, slow_timer, 0); }
            assert_eq!(WATCH.with(|watch| watch.borrow().pending), pending);
            pump_until(idle);
        }
        WATCH.with(|watch| assert_eq!(watch.borrow().retries, 0));
        drop(pane);
        WATCH.with(|watch| {
            assert!(watch.borrow().hooks.is_empty());
            assert_eq!(watch.borrow().timer, 0);
            assert!(watch.borrow().pending.is_none());
        });
    }

    #[test]
    fn destroying_pending_receiver_reposts_to_surviving_pane() {
        let owner = fixture();
        let (first, _) = managed(owner.hwnd().cast());
        let (second, _) = managed(owner.hwnd().cast());
        let old = WATCH.with(|watch| watch.borrow().pending.unwrap());
        assert_eq!(old.0, first.hwnd() as usize);
        drop(first);
        let new = WATCH.with(|watch| watch.borrow().pending.unwrap());
        assert_eq!(new.0, second.hwnd() as usize);
        assert_ne!(new.1, old.1);
        drop(second);
        WATCH.with(|watch| {
            let watch = watch.borrow();
            assert!(watch.pending.is_none() && watch.hooks.is_empty() && watch.panes.is_empty());
            assert_eq!(watch.timer, 0);
        });
    }

    #[test]
    fn host_repair_crosses_zero_area_helpers_but_keeps_apps_above() {
        let owner = fixture();
        let app = fixture();
        let helper = fixture();
        let (pane, _) = managed(owner.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            for window in [owner.hwnd(), app.hwnd(), helper.hwnd(), pane.hwnd()] {
                ShowWindow(window.cast(), SW_SHOWNOACTIVATE);
            }
            position(app.hwnd().cast(), HWND_TOP);
            position(owner.hwnd().cast(), app.hwnd().cast());
            assert_ne!(SetWindowPos(helper.hwnd().cast(), owner.hwnd().cast(), 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOOWNERZORDER), 0);
            position(hwnd, helper.hwnd().cast());
            assert_ne!(IsWindowVisible(helper.hwnd().cast()), 0);
            assert!(!application_boundary(helper.hwnd().cast()));
            assert!(above(owner.hwnd().cast(), hwnd));
            repair_order(hwnd, owner.hwnd().cast());
            assert!(above(hwnd, owner.hwnd().cast()));
            assert!(above(app.hwnd().cast(), hwnd));
            assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        }
    }

    #[test]
    #[ignore = "Toggles the real Shell desktop; run alone in an interactive session"]
    fn real_shell_show_desktop_preserves_pane_pixels() {
        use windows_sys::Win32::Graphics::Gdi::*;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        fn marker_visible(pixel: u32) -> bool {
            pixel & 0xff >= 200 && (pixel >> 16) & 0xff >= 200 && (pixel >> 8) & 0xff < 16
        }
        fn toggle() {
            let keys = [(VK_LWIN, 0), (0x44, 0), (0x44, KEYEVENTF_KEYUP), (VK_LWIN, KEYEVENTF_KEYUP)];
            let input: Vec<_> = keys.into_iter().map(|(key, flags)| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: key, dwFlags: flags, ..Default::default() } },
            }).collect();
            assert_eq!(unsafe { SendInput(input.len() as u32, input.as_ptr(), size_of::<INPUT>() as i32) }, 4);
        }
        unsafe fn focus(hwnd: HWND) {
            unsafe {
                SetForegroundWindow(hwnd);
                SendMessageW(hwnd, WM_NULL, 0, 0);
                if GetForegroundWindow() != hwnd {
                    let mut bounds = RECT::default();
                    GetWindowRect(hwnd, &raw mut bounds);
                    let desktop = RemovePropW(hwnd, DESKTOP_LAYER);
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                    let point = POINT { x: bounds.left + 40, y: bounds.top + 60 };
                    SetCursorPos(point.x, point.y);
                    assert_eq!(WindowFromPoint(point), hwnd, "click only the test window");
                    let input: Vec<_> = [MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP].into_iter().map(|flags| INPUT {
                        r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: flags, ..Default::default() } },
                    }).collect();
                    SendInput(2, input.as_ptr(), size_of::<INPUT>() as i32);
                    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
                    while std::time::Instant::now() < until {
                        let mut msg = MSG::default();
                        while PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 { TranslateMessage(&msg); DispatchMessageW(&msg); }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                    if !desktop.is_null() { SetPropW(hwnd, DESKTOP_LAYER, desktop); }
                }
                assert_eq!(GetForegroundWindow(), GetAncestor(hwnd, GA_ROOT), "test must actually obtain foreground");
            }
        }
        let _apartment = crate::pane::test_support::apartment();
        struct TestDesktop { marker: Window, manager: windows::Win32::UI::Shell::IVirtualDesktopManager }
        fn desktop_key(key: u16) {
            let keys = [(VK_LWIN, 0), (VK_CONTROL, 0), (key, 0), (key, KEYEVENTF_KEYUP), (VK_CONTROL, KEYEVENTF_KEYUP), (VK_LWIN, KEYEVENTF_KEYUP)];
            let input: Vec<_> = keys.into_iter().map(|(key, flags)| INPUT {
                r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: key, dwFlags: flags, ..Default::default() } },
            }).collect();
            unsafe { assert_eq!(SendInput(6, input.as_ptr(), size_of::<INPUT>() as i32), 6); }
        }
        impl Drop for TestDesktop {
            fn drop(&mut self) {
                if unsafe { self.manager.IsWindowOnCurrentVirtualDesktop(windows::Win32::Foundation::HWND(self.marker.hwnd())) }.is_ok_and(|v| v.as_bool()) {
                    desktop_key(VK_F4);
                    std::thread::sleep(std::time::Duration::from_millis(700));
                }
            }
        }
        let _desktop = {
            unsafe {
                let manager: windows::Win32::UI::Shell::IVirtualDesktopManager = windows::Win32::System::Com::CoCreateInstance(
                    &windows::Win32::UI::Shell::VirtualDesktopManager, None, windows::Win32::System::Com::CLSCTX_ALL).unwrap();
                let previous_marker = Window::new("Original desktop test marker").size(1, 1).style(WS_OVERLAPPEDWINDOW)
                    .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0)).create().unwrap();
                ShowWindow(previous_marker.hwnd().cast(), SW_SHOWNOACTIVATE);
                SetWindowPos(previous_marker.hwnd().cast(), HWND_BOTTOM, -32000, -32000, 1, 1, SWP_NOACTIVATE);
                let previous = manager.GetWindowDesktopId(windows::Win32::Foundation::HWND(previous_marker.hwnd())).unwrap();
                assert_ne!(previous, windows::core::GUID::zeroed(), "require a known original desktop");
                desktop_key(0x44);
                std::thread::sleep(std::time::Duration::from_millis(1000));
                let marker = Window::new("Desktop test marker").size(1, 1).style(WS_OVERLAPPEDWINDOW)
                    .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0)).create().unwrap();
                ShowWindow(marker.hwnd().cast(), SW_SHOWNOACTIVATE);
                SetWindowPos(marker.hwnd().cast(), HWND_BOTTOM, -32000, -32000, 1, 1, SWP_NOACTIVATE);
                let created = manager.GetWindowDesktopId(windows::Win32::Foundation::HWND(marker.hwnd())).unwrap();
                assert_ne!(created, windows::core::GUID::zeroed(), "require a known test desktop");
                assert_ne!(previous, created, "must create an isolated desktop before testing");
                Some(TestDesktop { marker, manager })
            }
        };
        let diagnostic_dir = tempfile::tempdir().unwrap();
        luciddesk_diagnostics::initialize(&diagnostic_dir.path().join("probe.db"), "test", "test", String::new);
        luciddesk_diagnostics::set_level(luciddesk_diagnostics::Level::Info);
        unsafe {
            let _graphics = crate::pane::native_graphics::GraphicsLifetime;
            let model = Rc::new(RefCell::new(crate::pane::tests::test_model("Shell visibility regression")));
            model.borrow_mut().backdrop = luciddesk_core::Backdrop::Acrylic;
            let pane = super::super::create(RectDip::new(50.0, 50.0, 200.0, 160.0), model, |_| false).unwrap();
            let hwnd = pane.hwnd().cast();
            set_layer(hwnd, false);
            let mut extra_panes = Vec::new();
            for index in 0..7 {
                let model = Rc::new(RefCell::new(crate::pane::tests::test_model("Shell visibility peer")));
                let peer = super::super::create(RectDip::new(400.0 + (index % 3) as f32 * 270.0,
                    50.0 + (index / 3) as f32 * 180.0, 200.0, 160.0), model, |_| false).unwrap();
                set_layer(peer.hwnd().cast(), false);
                extra_panes.push(peer);
            }
            let search_model = Rc::new(RefCell::new(crate::pane::tests::test_model("Shell search visibility regression")));
            search_model.borrow_mut().backdrop = luciddesk_core::Backdrop::Acrylic;
            let search = crate::pane::search::create(RectDip::new(50.0, 350.0, 240.0, 56.0), search_model, |_| false).unwrap();
            let search_hwnd = search.hwnd().cast();
            set_layer(search_hwnd, false);
            let shell = GetShellWindow();
            let shell_owned = uses_shell_owner();
            let expected_owner = if shell_owned { shell } else { std::ptr::null_mut() };
            assert_eq!(GetWindow(hwnd, GW_OWNER), expected_owner);
            assert_eq!(GetWindow(search_hwnd, GW_OWNER), expected_owner);
            // An ordinary application has its own input queue. Sharing the pane
            // thread changes Show Desktop activation and invalidates this probe.
            struct Application { handle: usize, stop: std::sync::mpsc::Sender<()>, thread: Option<std::thread::JoinHandle<()>> }
            impl Application { fn hwnd(&self) -> HWND { self.handle as HWND } }
            impl Drop for Application {
                fn drop(&mut self) { let _ = self.stop.send(()); let _ = self.thread.take().unwrap().join(); }
            }
            let (ready, receive) = std::sync::mpsc::channel();
            let (stop, stopped) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                let app = Window::new("Show Desktop ordinary application").size(200, 160)
                    .style(WS_OVERLAPPEDWINDOW).on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
                    .create().unwrap();
                ready.send(app.hwnd() as usize).unwrap();
                while stopped.recv_timeout(std::time::Duration::from_millis(5)) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                    let mut msg = MSG::default();
                    while PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg); DispatchMessageW(&msg);
                    }
                }
            });
            let app = Application { handle: receive.recv().unwrap(), stop, thread: Some(thread) };
            SetWindowPos(app.hwnd().cast(), HWND_TOP, 450, 200, 200, 160, SWP_NOACTIVATE);
            ShowWindow(app.hwnd().cast(), SW_SHOW);
            focus(app.hwnd().cast());
            fn settle(mut observe: impl FnMut()) {
                let until = std::time::Instant::now() + std::time::Duration::from_millis(1600);
                while std::time::Instant::now() < until {
                    unsafe {
                        let mut msg = MSG::default();
                        while PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 { DispatchMessageW(&msg); }
                    }
                    observe();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            settle(|| {});
            let mut bounds = RECT::default();
            GetWindowRect(hwnd, &raw mut bounds);
            let scale = GetDpiForWindow(hwnd) as f32 / 96.0;
            let mut point = POINT { x: bounds.left + (20.0 * scale) as i32, y: bounds.top + (55.0 * scale) as i32 };
            eprintln!("baseline pane={} parent={} hit={}", super::super::desktop_diagnostics::facts(hwnd),
                super::super::desktop_diagnostics::facts(GetParent(hwnd)), super::super::desktop_diagnostics::facts(WindowFromPoint(point)));
            assert_eq!(WindowFromPoint(point), hwnd, "the pixel baseline must belong to the test pane");
            let dc = GetDC(std::ptr::null_mut());
            let expected = GetPixel(dc, point.x, point.y);
            ReleaseDC(std::ptr::null_mut(), dc);
            assert_eq!(expected, 0xff00ff, "sample opaque test content inside the Acrylic surface");
            eprintln!("before pane={} hit={:?} pixel={expected:#x}", super::super::desktop_diagnostics::facts(hwnd), WindowFromPoint(point));
            let mut search_rect = RECT::default();
            GetWindowRect(search_hwnd, &raw mut search_rect);
            let search_point = POINT { x: search_rect.left + (16.0 * scale) as i32, y: search_rect.top + (16.0 * scale) as i32 };
            let search_editor = GetPropW(search_hwnd, windows_sys::w!("LucidDesk.SearchInput"));
            assert!(!search_editor.is_null());
            assert_eq!(GetWindow(search_editor, GW_OWNER), search_hwnd);
            assert_eq!(GetWindowLongW(search_editor, GWL_STYLE) as u32 & WS_CHILD, 0);
            let assert_backdrops = || {
                for pane in [hwnd, search_hwnd] {
                    assert_eq!(GetWindowLongW(pane, GWL_STYLE) as u32 & WS_CHILD, 0);
                    assert_eq!(SendMessageW(pane, TEST_HOST_BACKDROP, 0, 0), 1,
                        "expected native Acrylic on {pane:?}");
                }
            };
            if shell_owned {
                assert_backdrops();
                // The application does not overlap search at this point.
                let px = search_point.x; let py = search_point.y;
                let (ready, started) = std::sync::mpsc::channel();
                let observer = std::thread::spawn(move || {
                    let dc = GetDC(std::ptr::null_mut());
                    let until = std::time::Instant::now() + std::time::Duration::from_millis(450);
                    ready.send(()).unwrap();
                    let mut missing = Vec::new();
                    while std::time::Instant::now() < until {
                        let pixel = GetPixel(dc, px, py);
                        if !marker_visible(pixel) { missing.push(pixel); }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    ReleaseDC(std::ptr::null_mut(), dc);
                    missing
                });
                started.recv().unwrap();
                for _ in 0..3 { super::super::raise_once(search_hwnd); }
                assert_eq!(GetWindow(search_hwnd, GW_OWNER), expected_owner);
                settle(|| {});
                assert!(observer.join().unwrap().is_empty(), "already-visible reveal must not flicker");
            }
            for (reveal, focus_pane, drag) in [(false, false, false), (true, false, false), (false, true, false), (true, true, false), (false, true, true), (true, true, true)] {
                focus(app.hwnd().cast());
                // Let Explorer consume the preceding foreground change before
                // moving focus back to a pane for the next scenario.
                settle(|| {});
                if reveal {
                    let px = point.x; let py = point.y;
                    let (ready, started) = std::sync::mpsc::channel();
                    let observer = std::thread::spawn(move || {
                        let dc = GetDC(std::ptr::null_mut());
                        let until = std::time::Instant::now() + std::time::Duration::from_millis(450);
                        ready.send(()).unwrap();
                        let mut missing = Vec::new();
                        while std::time::Instant::now() < until {
                            let pixel = GetPixel(dc, px, py);
                            if !marker_visible(pixel) { missing.push(pixel); }
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        ReleaseDC(std::ptr::null_mut(), dc);
                        missing
                    });
                    started.recv().unwrap();
                    super::super::raise_once(hwnd);
                    settle(|| {});
                    let missing = observer.join().unwrap();
                    assert!(missing.is_empty(), "explicit reveal flickered: {missing:?}");
                    assert!(above(hwnd, app.hwnd().cast()), "explicit reveal must actually cover the application");
                }
                if focus_pane {
                    focus(hwnd);
                    assert_eq!(GetFocus(), hwnd, "pane must actually receive keyboard focus");
                }
                if drag {
                    GetWindowRect(hwnd, &raw mut bounds);
                    let x = bounds.left + (80.0 * scale) as i32;
                    let y = bounds.top + (15.0 * scale) as i32;
                    assert_eq!(WindowFromPoint(POINT { x, y }), hwnd);
                    let before = bounds;
                    // Native caption dragging pumps a modal loop. Supply motion
                    // from another thread, as a real mouse driver would.
                    let mouse = std::thread::spawn(move || {
                        let send = |flags| {
                            let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 {
                                mi: MOUSEINPUT { dwFlags: flags, ..Default::default() } } };
                            assert_eq!(SendInput(1, &input, size_of::<INPUT>() as i32), 1);
                        };
                        SetCursorPos(x, y);
                        send(MOUSEEVENTF_LEFTDOWN);
                        for step in 1..=20 {
                            std::thread::sleep(std::time::Duration::from_millis(260));
                            SetCursorPos(x + step * 3, y + step * 2);
                        }
                        send(MOUSEEVENTF_LEFTUP);
                    });
                    settle(|| {});
                    mouse.join().unwrap();
                    GetWindowRect(hwnd, &raw mut bounds);
                    assert_ne!((bounds.left, bounds.top), (before.left, before.top), "long drag must really move the pane");
                    assert_eq!(IsIconic(app.hwnd()), 0, "drag itself must not minimize the ordinary app");
                    point = POINT { x: bounds.left + (20.0 * scale) as i32, y: bounds.top + (55.0 * scale) as i32 };
                }
                toggle();
                struct Restore(bool);
                impl Drop for Restore { fn drop(&mut self) { if self.0 { toggle(); } } }
                let mut restore = Restore(true);
                let started = std::time::Instant::now();
                let mut missing = Vec::new();
                let mut samples = 0;
                settle(|| {
                    let dc = GetDC(std::ptr::null_mut());
                    let pixel = GetPixel(dc, point.x, point.y);
                    let search_pixel = GetPixel(dc, search_point.x, search_point.y);
                    assert!(marker_visible(search_pixel), "search pane vanished: {search_pixel:#x}");
                    assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0, "Win+D must not use temporary topmost protection");
                    ReleaseDC(std::ptr::null_mut(), dc);
                    samples += 1;
                    if !marker_visible(pixel) { missing.push((started.elapsed().as_millis(), pixel)); }
                });
                eprintln!("transition reveal={reveal} focus_pane={focus_pane} drag={drag} samples={samples} missing={} first={:?} last={:?}", missing.len(), missing.first(), missing.last());
                eprintln!("after toggle parent={:?} facts={} foreground={}", GetParent(hwnd), super::super::desktop_diagnostics::facts(hwnd), super::super::desktop_diagnostics::facts(GetForegroundWindow()));
                assert!(missing.is_empty(), "pane disappeared during Win+D: {missing:?}");
                let view = luciddesk_explorer::desktop_view().unwrap_or_default() as HWND;
                let host = host(view, GetShellWindow());
                let dc = GetDC(std::ptr::null_mut());
                let pixel = GetPixel(dc, point.x, point.y);
                ReleaseDC(std::ptr::null_mut(), dc);
                eprintln!("reveal={reveal} pane={} host={} above={} pixel={pixel:#x}",
                    super::super::desktop_diagnostics::facts(hwnd), super::super::desktop_diagnostics::facts(host), above(hwnd, host));
                assert_ne!(IsIconic(app.hwnd().cast()), 0, "Win+D must actually minimize the ordinary application");
                assert!(above(hwnd, host));
                assert!(marker_visible(pixel), "pane content must remain on the desktop: {pixel:#x}");
                toggle();
                restore.0 = false;
                let mut return_missing = Vec::new();
                settle(|| {
                    let dc = GetDC(std::ptr::null_mut());
                    let pixel = GetPixel(dc, point.x, point.y);
                    let search_pixel = GetPixel(dc, search_point.x, search_point.y);
                    assert!(marker_visible(search_pixel), "search pane vanished: {search_pixel:#x}");
                    assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0, "Win+D must not use temporary topmost protection");
                    ReleaseDC(std::ptr::null_mut(), dc);
                    if !marker_visible(pixel) { return_missing.push(pixel); }
                });
                assert!(return_missing.is_empty(), "pane disappeared on desktop return: {return_missing:?}");
                assert_eq!(IsIconic(app.hwnd().cast()), 0, "second Win+D must restore the application");
                let order = window_order().unwrap();
                eprintln!("returned fore={} ranks pane={:?} app={:?} host={:?} pending={} app_boundary={} timer={:?}",
                    super::super::desktop_diagnostics::facts(GetForegroundWindow()),
                    order.iter().position(|&h| h == hwnd), order.iter().position(|&h| h == app.hwnd().cast()),
                    order.iter().position(|&h| h == host), !GetPropW(hwnd, RETURN_TO_HOST).is_null(),
                    application_boundary(GetForegroundWindow()), WATCH.with(|w| (w.borrow().timer, w.borrow().pending)));
                assert!(above(app.hwnd().cast(), hwnd), "restored application must cover the pane without a click");
                assert_eq!(GetWindow(hwnd, GW_OWNER), expected_owner);
                assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
                if shell_owned { assert_backdrops(); }
            }
            // Exercise the separate EDIT surface in both desktop and revealed modes.
            SetWindowPos(app.hwnd().cast(), HWND_TOP, search_rect.right - 15, search_rect.top, 200, 160, SWP_NOACTIVATE);
            for reveal in [false, true] {
                focus(app.hwnd().cast());
                // Let Explorer consume the preceding foreground change before
                // moving focus back to a pane for the next scenario.
                settle(|| {});
                if reveal { super::super::raise_once(search_hwnd); }
                focus(search_hwnd);
                SetFocus(search_editor);
                assert_eq!(GetFocus(), search_editor);
                SetWindowTextW(search_editor, windows_sys::w!("search 输入"));
                assert_eq!(GetWindowTextLengthW(search_editor), 9);
                let mut editor_rect = RECT::default();
                GetWindowRect(search_editor, &raw mut editor_rect);
                assert!(editor_rect.left >= search_rect.left && editor_rect.right <= search_rect.right);
                for minimized in [true, false] {
                    let before = std::time::Instant::now();
                    toggle();
                    let mut missing = Vec::new();
                    settle(|| {
                        let dc = GetDC(std::ptr::null_mut());
                        let pixel = GetPixel(dc, search_point.x, search_point.y);
                        ReleaseDC(std::ptr::null_mut(), dc);
                        if !marker_visible(pixel) {
                            if missing.is_empty() {
                                eprintln!("missing search reveal={reveal} minimized={minimized} elapsed={:?} pixel={pixel:#x} pane={} hit={} opacity={} app={} foreground={}",
                                    before.elapsed(), super::super::desktop_diagnostics::facts(search_hwnd),
                                    super::super::desktop_diagnostics::facts(WindowFromPoint(search_point)),
                                    SendMessageW(search_hwnd, WM_APP + 199, 0, 0),
                                    super::super::desktop_diagnostics::facts(app.hwnd().cast()),
                                    super::super::desktop_diagnostics::facts(GetForegroundWindow()));
                            }
                            missing.push(pixel);
                        }
                        assert_ne!(IsWindowVisible(search_editor), 0, "search EDIT must remain visible");
                    });
                    assert!(missing.is_empty(), "focused search vanished during Win+D: {missing:x?}");
                    assert_eq!(IsIconic(app.hwnd()) != 0, minimized, "focused search must preserve Win+D restore");
                    assert_eq!(GetWindow(search_editor, GW_OWNER), search_hwnd);
                    if shell_owned { assert_backdrops(); }
                }
                assert!(above(app.hwnd().cast(), search_hwnd));
            }
            for peer in &extra_panes { super::super::prepare_close(peer.hwnd().cast()); }
            super::super::prepare_close(hwnd);
        }
    }

    #[test]
    fn crowded_desktop_recovers_and_lowers_without_crossing_apps() {
        let host = fixture();
        let app = fixture();
        let helpers: Vec<_> = (0..520).map(|_| fixture()).collect();
        let (pane, _) = managed(host.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            ShowWindow(host.hwnd().cast(), SW_SHOWNOACTIVATE);
            ShowWindow(app.hwnd().cast(), SW_SHOWNOACTIVATE);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            position(app.hwnd().cast(), HWND_TOP);
            position(host.hwnd().cast(), app.hwnd().cast());
            let mut previous = host.hwnd().cast();
            for helper in &helpers {
                ShowWindow(helper.hwnd().cast(), SW_HIDE);
                position(helper.hwnd().cast(), previous);
                previous = helper.hwnd().cast();
            }
            RemovePropW(hwnd, DESKTOP_LAYER);
            position(hwnd, previous);
            SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
            let foreground = GetForegroundWindow();
            repair_order(hwnd, host.hwnd().cast());
            assert!(above(hwnd, host.hwnd().cast()), "host beyond 512 hidden windows must still be found");
            assert!(above(app.hwnd().cast(), hwnd));

            // A real app deep in the same gap must still block upward repair.
            RemovePropW(hwnd, DESKTOP_LAYER);
            position(hwnd, previous);
            SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
            position(app.hwnd().cast(), host.hwnd().cast());
            repair_order(hwnd, host.hwnd().cast());
            assert_eq!(GetWindow(hwnd, GW_HWNDPREV), previous);

            // Follow the host downward through the same large hidden-window gap.
            RemovePropW(hwnd, DESKTOP_LAYER);
            position(hwnd, HWND_TOP);
            SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
            position(host.hwnd().cast(), previous);
            lower_in_order(hwnd, host.hwnd().cast(), &window_order().unwrap());
            assert_eq!(GetWindow(host.hwnd().cast(), GW_HWNDPREV), hwnd);
            assert_eq!(GetForegroundWindow(), foreground);
            assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        }
    }

    #[test]
    fn host_round_trip_crosses_offscreen_helper_without_covering_restored_apps() {
        let normal = fixture();
        let desktop = fixture();
        let view = fixture();
        let app = fixture();
        let helper = fixture();
        let (pane, _) = managed(normal.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            SetWindowLongPtrW(view.hwnd().cast(), GWL_STYLE, WS_CHILD as isize);
            for window in [normal.hwnd(), desktop.hwnd(), app.hwnd(), helper.hwnd(), pane.hwnd()] {
                ShowWindow(window.cast(), SW_SHOWNOACTIVATE);
            }
            // Match the nonempty NarratorHelperWindow rectangle in the Win10 log.
            assert_ne!(SetWindowPos(helper.hwnd().cast(), HWND_TOP, -48000, -48000, 24, 24,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER), 0);
            let foreground = GetForegroundWindow();
            for _ in 0..3 {
                ShowWindow(normal.hwnd().cast(), SW_HIDE);
                ShowWindow(app.hwnd().cast(), SW_HIDE);
                ShowWindow(desktop.hwnd().cast(), SW_SHOWNOACTIVATE);
                position(desktop.hwnd().cast(), HWND_TOP);
                position(helper.hwnd().cast(), desktop.hwnd().cast());
                RemovePropW(hwnd, DESKTOP_LAYER);
                position(hwnd, helper.hwnd().cast());
                SetPropW(hwnd, DESKTOP_LAYER, 1usize as _);
                SetParent(view.hwnd().cast(), desktop.hwnd().cast());
                synchronize(view.hwnd().cast());
                assert!(above(hwnd, desktop.hwnd().cast()), "offscreen helper must not block desktop recovery");

                ShowWindow(desktop.hwnd().cast(), SW_HIDE);
                ShowWindow(normal.hwnd().cast(), SW_SHOWNOACTIVATE);
                ShowWindow(app.hwnd().cast(), SW_SHOWNOACTIVATE);
                position(app.hwnd().cast(), hwnd);
                position(helper.hwnd().cast(), app.hwnd().cast());
                position(normal.hwnd().cast(), helper.hwnd().cast());
                SetParent(view.hwnd().cast(), normal.hwnd().cast());
                synchronize(view.hwnd().cast());
                assert!(above(app.hwnd().cast(), hwnd));
                assert!(above(hwnd, normal.hwnd().cast()));
                assert_eq!(GetForegroundWindow(), foreground);
                assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
            }
            // A partly offscreen ordinary window remains a real boundary.
            assert_ne!(SetWindowPos(helper.hwnd().cast(), HWND_TOP, -12, -12, 24, 24,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER), 0);
            assert!(application_boundary(helper.hwnd().cast()));
        }
    }

    #[test]
    fn foreground_desktop_crosses_covered_windows_but_not_apps_above_host() {
        let host = fixture();
        let covered = fixture();
        let app = fixture();
        let (pane, _) = managed(host.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            for window in [host.hwnd(), covered.hwnd(), app.hwnd(), pane.hwnd()] {
                ShowWindow(window.cast(), SW_SHOWNOACTIVATE);
            }
            position(app.hwnd().cast(), HWND_TOP);
            position(host.hwnd().cast(), app.hwnd().cast());
            position(covered.hwnd().cast(), host.hwnd().cast());
            position(hwnd, covered.hwnd().cast());
            assert!(application_boundary(covered.hwnd().cast()));
            let foreground = GetForegroundWindow();
            let order = window_order().unwrap();
            repair_in_order(hwnd, host.hwnd().cast(), &order, false);
            assert_eq!(GetWindow(hwnd, GW_HWNDPREV), covered.hwnd().cast());
            repair_in_order(hwnd, host.hwnd().cast(), &order, true);
            assert!(above(hwnd, host.hwnd().cast()));
            assert!(above(app.hwnd().cast(), hwnd));
            assert!(!GetPropW(hwnd, RETURN_TO_HOST).is_null());
            assert_eq!(GetForegroundWindow(), foreground);
        }
    }

    #[test]
    fn host_repair_does_not_cross_an_intervening_application() {
        let owner = fixture();
        let app = fixture();
        let (pane, _) = managed(owner.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            position(owner.hwnd().cast(), HWND_TOP);
            position(app.hwnd().cast(), owner.hwnd().cast());
            position(hwnd, app.hwnd().cast());
            assert!(above(owner.hwnd().cast(), app.hwnd().cast()));
            assert!(above(app.hwnd().cast(), hwnd));
            repair_order(hwnd, owner.hwnd().cast());
            assert!(above(app.hwnd().cast(), hwnd), "do not cross a visible app to chase the desktop");
        }
    }

    #[test]
    fn host_change_requests_a_frame_but_unchanged_owner_does_not() {
        let first = fixture();
        let second = fixture();
        let (pane, _) = managed(first.hwnd().cast());
        let hwnd = pane.hwnd().cast();
        unsafe {
            ValidateRect(hwnd, std::ptr::null());
            update_host(hwnd, std::ptr::null_mut(), first.hwnd().cast());
            assert_eq!(GetUpdateRect(hwnd, std::ptr::null_mut(), 0), 0);
            update_host(hwnd, std::ptr::null_mut(), second.hwnd().cast());
            assert_ne!(GetUpdateRect(hwnd, std::ptr::null_mut(), 0), 0);
            ValidateRect(hwnd, std::ptr::null());
            update_host(hwnd, std::ptr::null_mut(), second.hwnd().cast());
            assert_eq!(GetUpdateRect(hwnd, std::ptr::null_mut(), 0), 0);
        }
    }

    #[test]
    fn icon_host_takes_precedence_over_shell_owner() {
        let make = || Window::new("Desktop host fixture").size(80, 60).style(WS_POPUP)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        let shell = make();
        let worker = make();
        let view = make();
        unsafe {
            let view_hwnd = view.hwnd().cast();
            SetWindowLongPtrW(view_hwnd, GWL_STYLE, WS_CHILD as isize);
            SetParent(view_hwnd, worker.hwnd().cast());
            assert_eq!(host(view_hwnd, shell.hwnd().cast()), worker.hwnd().cast());
            // Explorer can move the existing view without replacing its handle.
            SetParent(view_hwnd, shell.hwnd().cast());
            assert_eq!(host(view_hwnd, worker.hwnd().cast()), shell.hwnd().cast());
            assert_eq!(host(std::ptr::null_mut(), shell.hwnd().cast()), shell.hwnd().cast());
        }
    }

    #[test]
    fn reveal_does_not_reassign_unmanaged_window_owner() {
        let owner = Window::new("Owner fixture").size(80, 60).style(WS_POPUP).create().unwrap();
        let pane = Window::new("Pane fixture").size(80, 60).style(WS_POPUP).create().unwrap();
        unsafe { SetWindowLongPtrW(pane.hwnd().cast(), GWLP_HWNDPARENT, owner.hwnd() as isize); }
        refresh(pane.hwnd().cast());
        assert_eq!(unsafe { GetWindow(pane.hwnd().cast(), GW_OWNER) }, owner.hwnd().cast());
    }
}
