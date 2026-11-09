//! Read-only desktop visibility sampling. Never activate, show, or reorder a window.
use super::*;
use luciddesk_diagnostics::{Level, enabled};
use std::collections::HashMap;

const TIMER: usize = 0x4c44;
thread_local! {
    static LAST: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
    static CONTENT: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
}

pub(super) fn content(hwnd: HWND, message: u32, surface: Option<f32>, transition: f32) {
    if !enabled(Level::Info) { return; }
    let state = format!("surface_opacity={surface:?} transition_opacity={transition}");
    let changed = CONTENT.with(|states| {
        let mut states = states.borrow_mut();
        if states.get(&(hwnd as usize)) == Some(&state) { return false; }
        states.insert(hwnd as usize, state.clone());
        true
    });
    if changed || matches!(message, WM_SHOWWINDOW | WM_WINDOWPOSCHANGED) {
        luciddesk_diagnostics::emit!(Level::Info, "pane.desktop_content",
            "pane={hwnd:?} message={message:#x} {state}");
    }
}

pub(super) fn facts(hwnd: HWND) -> String {
    if hwnd.is_null() { return "none".into(); }
    unsafe {
        let mut class = [0u16; 128];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let mut cloak = 0u32;
        let result = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            windows::Win32::Foundation::HWND(hwnd),
            windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
            (&raw mut cloak).cast(), 4,
        );
        let cloak = match result {
            Ok(()) => format!("{cloak:#x}"),
            Err(error) => format!("error:{:#x}", error.code().0),
        };
        let mut rect = RECT::default();
        let rect_ok = GetWindowRect(hwnd, &raw mut rect);
        format!("hwnd={hwnd:?},class={},visible={},iconic={},cloak={},owner={:?},style={:#x},exstyle={:#x},rect=({},{},{},{}),rect_ok={rect_ok}",
            String::from_utf16_lossy(&class[..len]), IsWindowVisible(hwnd), IsIconic(hwnd), cloak,
            GetWindow(hwnd, GW_OWNER), GetWindowLongW(hwnd, GWL_STYLE) as u32,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32, rect.left, rect.top, rect.right, rect.bottom)
    }
}

fn capture(hwnd: HWND) -> String {
    unsafe {
        let view = luciddesk_explorer::desktop_view().unwrap_or_default() as HWND;
        let host = GetAncestor(view, GA_ROOT);
        let shell = GetShellWindow();
        let owner = GetWindow(hwnd, GW_OWNER);
        // Full enumeration keeps desktop ranks meaningful on crowded sessions;
        // only the detail list is capped. No foreign window titles.
        let order = super::desktop_owner::window_order();
        let mut ranks = [None; 4];
        let mut above = Vec::new();
        let mut seen_pane = false;
        for (rank, &current) in order.iter().flatten().enumerate() {
            for (index, target) in [hwnd, host, shell, owner].iter().enumerate() {
                if current == *target { ranks[index] = Some(rank); }
            }
            if current == hwnd { seen_pane = true; }
            else if !seen_pane && above.len() < 12 && IsWindowVisible(current) != 0 {
                above.push(facts(current));
            }
        }
        let content = CONTENT.with(|states| states.borrow().get(&(hwnd as usize)).cloned());
        format!("pane=[{}] content={content:?} parent=[{}] owner=[{}] icon_view={view:?} icon_host=[{}] shell=[{}] foreground=[{}] \
            ranks(pane,host,shell,owner)={ranks:?} window_count={} enumeration_ok={} app_desktop_hidden={} above={above:?}",
            facts(hwnd), facts(GetParent(hwnd)), facts(owner), facts(host), facts(shell), facts(GetForegroundWindow()),
            order.as_ref().map_or(0, Vec::len), order.is_some(), super::super::desktop_toggle::hidden())
    }
}

pub(super) fn sample(hwnd: HWND, reason: &str, force: bool) {
    if !enabled(Level::Info) { return; }
    let snapshot = capture(hwnd);
    let changed = LAST.with(|last| {
        let mut last = last.borrow_mut();
        if last.get(&(hwnd as usize)) == Some(&snapshot) { return false; }
        last.insert(hwnd as usize, snapshot.clone());
        true
    });
    if force || changed {
        luciddesk_diagnostics::emit!(Level::Info, "pane.desktop_state", "reason={reason}; {snapshot}");
    }
}

pub(in crate::pane) fn start(hwnd: HWND) {
    // Opt in at window creation; enabling diagnostics requires an app restart.
    // Normal error-only logging must not add idle timer wakeups.
    if !enabled(Level::Info) { return; }
    unsafe extern "system" fn tick(hwnd: HWND, _: u32, _: usize, _: u32) {
        if !enabled(Level::Info) { stop(hwnd); return; }
        // A hidden/cloaked pane may receive no useful position messages when
        // Explorer moves its own desktop. Sampling catches that case as well.
        sample(hwnd, "poll-500ms", false);
    }
    if unsafe { SetTimer(hwnd, TIMER, 500, Some(tick)) } == 0 {
        luciddesk_diagnostics::emit!(Level::Warn, "pane.desktop_state", "sampling timer failed; hwnd={hwnd:?}");
    }
    sample(hwnd, "created", true);
}

pub(super) fn stop(hwnd: HWND) {
    unsafe { KillTimer(hwnd, TIMER); }
    LAST.with(|last| last.borrow_mut().remove(&(hwnd as usize)));
    CONTENT.with(|states| states.borrow_mut().remove(&(hwnd as usize)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_snapshot_distinguishes_hidden_and_visible_without_changing_window() {
        let pane = Window::new("Diagnostics fixture").size(80, 60).style(WS_POPUP)
            .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
            .create().unwrap();
        let hwnd = pane.hwnd().cast();
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let previous = GetWindow(hwnd, GW_HWNDPREV);
            let visible = facts(hwnd);
            assert!(visible.contains("visible=1,iconic=0,cloak="), "{visible}");
            assert_eq!(GetWindow(hwnd, GW_HWNDPREV), previous);
            ShowWindow(hwnd, SW_HIDE);
            let hidden = facts(hwnd);
            assert!(hidden.contains("visible=0,iconic=0,cloak="), "{hidden}");
            assert_ne!(visible, hidden);
            let snapshot = capture(hwnd);
            assert!(snapshot.contains("ranks(pane,host,shell,owner)="));
            assert_eq!(IsWindowVisible(hwnd), 0);
            assert!(!snapshot.contains("Diagnostics fixture"), "do not collect window titles");
        }
        LAST.with(|last| last.borrow_mut().insert(hwnd as usize, "test".into()));
        stop(hwnd);
        LAST.with(|last| assert!(!last.borrow().contains_key(&(hwnd as usize))));
    }
}
