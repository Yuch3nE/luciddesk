//! Membership and all COM calls stay on Explorer's desktop STA.
use super::*;
use std::{
    cell::{Cell, RefCell},
    ptr::null_mut,
};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider},
            Variant::VARIANT,
        },
        UI::Shell::*,
    },
    core::{Interface, Result},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND},
    System::{DataExchange::COPYDATASTRUCT, Threading::*},
    UI::{
        Controls::*,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    },
};

const TIMER: usize = MAGIC;
const RECOVERY_TIMER: usize = MAGIC + 1;
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
thread_local! { static GESTURE_INTERRUPTED: Cell<bool> = const { Cell::new(false) }; }
// COM calls can synchronously re-enter the subclass while STATE is borrowed.
// Keep the redraw gate separate so Explorer cannot unfreeze the restored items.
thread_local! { static FROZEN_VIEW: Cell<HWND> = const { Cell::new(null_mut()) }; }
struct State {
    desktop_visibility: super::desktop_visibility::DesktopVisibility,
    hwnd: HWND,
    owner: HWND,
    process: HANDLE,
    owner_watch: Option<owner::OwnerWatch>,
    view: IShellView,
    folder: IFolderView2,
    object_view: IShellFolderView,
    membership: items::Membership,
    menu_host: Option<menu::worker::Worker>,
    menu_prepared: bool,
    pending: Option<wire::Request>,
    paused: bool,
    redraw_paused: bool,
    updating: bool,
    update_sequence: u32,
    queued: bool,
    failed: bool,
    closing: bool,
    recovery: recovery::Recovery,
    read_retry: retry::ReadRetry,
    drag_start: Option<windows_sys::Win32::Foundation::POINT>,
    user_menu: bool,
    // Last field: release is signaled only after all COM objects are dropped.
    _library: library::StaLease,
}
impl Drop for State {
    fn drop(&mut self) {
        self.owner_watch.take();
        unsafe {
            CloseHandle(self.process);
        }
    }
}

pub fn attach(hwnd: HWND, owner: HWND) {
    diagnostics::reset(hwnd);
    diagnostics::stage(hwnd, diagnostics::Stage::Attach);
    let result = attach_inner(hwnd, owner);

    if let Err(error) = result {
        diagnostics::attach_failure(hwnd, error.code());
        unsafe {
            SetPropW(hwnd, ERROR, error.code().0 as u32 as usize as _);
        }
    }
}
// Bounded read-only sampling; never sleep or pump messages inside Explorer COM.
fn verify_counts(mut read: impl FnMut() -> Result<(u32, i32)>) -> Result<()> {
    let mut last = (0, 0);
    for _ in 0..3 {
        last = read()?;
        if i64::from(last.0) == i64::from(last.1) { return Ok(()); }
    }
    Err(windows::core::Error::new(
        windows::core::HRESULT::from_win32(windows_sys::Win32::Foundation::ERROR_RETRY),
        format!("桌面项目数量连续不一致: object_count={}, folder_count={}", last.0, last.1),
    ))
}

fn attach_inner(hwnd: HWND, owner: HWND) -> Result<()> {
    unsafe {
        if IsWindow(owner) == 0
            || GetWindowThreadProcessId(hwnd, null_mut()) != GetCurrentThreadId()
        {
            return Err(windows::core::Error::from_thread());
        }
        if STATE.with(|slot| slot.borrow().is_some()) {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_UNEXPECTED,
            ));
        }
        diagnostics::stage(hwnd, diagnostics::Stage::ShellWindows);
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut raw = 0;
        diagnostics::stage(hwnd, diagnostics::Stage::DesktopDispatch);
        let dispatch = shell.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP.cast_signed()),
            &VARIANT::default(),
            SWC_DESKTOP,
            &raw mut raw,
            SWFO_NEEDDISPATCH,
        )?;
        diagnostics::stage(hwnd, diagnostics::Stage::ServiceProvider);
        let provider: IServiceProvider = dispatch.cast()?;
        diagnostics::stage(hwnd, diagnostics::Stage::Browser);
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        diagnostics::stage(hwnd, diagnostics::Stage::ActiveView);
        let view = browser.QueryActiveShellView()?;
        diagnostics::stage(hwnd, diagnostics::Stage::ViewWindow);
        let shell_hwnd = view.GetWindow()?.0;
        diagnostics::stage(hwnd, diagnostics::Stage::ViewIdentity);
        if GetParent(hwnd) != shell_hwnd {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        diagnostics::stage(hwnd, diagnostics::Stage::FolderView);
        let folder: IFolderView2 = view.cast()?;
        diagnostics::stage(hwnd, diagnostics::Stage::ObjectView);
        let object_view: IShellFolderView = view.cast()?;
        verify_counts(|| {
            diagnostics::stage(hwnd, diagnostics::Stage::ObjectCount);
            let objects = object_view.GetObjectCount()?;
            diagnostics::stage(hwnd, diagnostics::Stage::FolderCount);
            let items = folder.ItemCount(SVGIO_ALLVIEW)?;
            diagnostics::stage(hwnd, diagnostics::Stage::CountMatch);
            diagnostics::counts(hwnd, objects, items);
            Ok((objects, items))
        })?;
        let mut pid = 0;
        GetWindowThreadProcessId(owner, &raw mut pid);
        diagnostics::stage(hwnd, diagnostics::Stage::OwnerProcess);
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        diagnostics::stage(hwnd, diagnostics::Stage::Library);
        let library = match library::StaLease::new() {
            Ok(library) => library,
            Err(error) => { CloseHandle(process); return Err(error); }
        };
        let mut state = State {
            desktop_visibility: Default::default(),
            hwnd,
            owner,
            process,
            owner_watch: None,
            view,
            folder,
            object_view,
            membership: Default::default(),
            menu_host: None,
            menu_prepared: false,
            pending: None,
            paused: false,
            redraw_paused: false,
            updating: false,
            update_sequence: 0,
            queued: false,
            failed: false,
            closing: false,
            recovery: Default::default(),
            read_retry: Default::default(),
            drag_start: None,
            user_menu: false,
            _library: library,
        };
        diagnostics::stage(hwnd, diagnostics::Stage::OwnerWatch);
        state.owner_watch = Some(owner::OwnerWatch::new(process, owner, hwnd)?);
        // The caller may abandon a stale view while Shell COM initialization
        // is running. Never publish a late attachment to its destroyed owner.
        if IsWindow(owner) == 0 {
            return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_ABORT));
        }
        diagnostics::stage(hwnd, diagnostics::Stage::ListSubclass);
        if SetWindowSubclass(hwnd, Some(subclass), MAGIC, 0) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        diagnostics::stage(hwnd, diagnostics::Stage::ParentSubclass);
        if SetWindowSubclass(GetParent(hwnd), Some(subclass), MAGIC, 0) == 0 {
            RemoveWindowSubclass(hwnd, Some(subclass), MAGIC);
            return Err(windows::core::Error::from_thread());
        }
        // Last-resort lifecycle check if a wakeup is lost; never scans items.
        diagnostics::stage(hwnd, diagnostics::Stage::Timer);
        if SetTimer(hwnd, TIMER, 60_000, None) == 0 {
            RemoveWindowSubclass(GetParent(hwnd), Some(subclass), MAGIC);
            RemoveWindowSubclass(hwnd, Some(subclass), MAGIC);
            return Err(windows::core::Error::from_thread());
        }
        RemovePropW(hwnd, ERROR);
        RemovePropW(hwnd, ACK);
        STATE.with(|slot| *slot.borrow_mut() = Some(state));
        SetPropW(hwnd, OWNER, owner);
        diagnostics::stage(hwnd, diagnostics::Stage::Ready);
    }
    Ok(())
}

impl State {
    fn arm_recovery(&self) {
        unsafe {
            KillTimer(self.hwnd, RECOVERY_TIMER);
            if let Some(delay) = schedule::recovery_delay(
                if self.failed { self.recovery.deadline() } else { self.read_retry.deadline() },
                self.updating, self.pending.is_some() || (self.queued && !self.failed), std::time::Instant::now(),
            ) {
                SetTimer(self.hwnd, RECOVERY_TIMER, delay, None);
            }
        }
    }
    fn queue(&mut self) {
        if !self.queued {
            self.queued = true;
            if unsafe { PostMessageW(self.hwnd, work_message(), 0, 0) } == 0 {
                self.arm_recovery();
            }
        }
    }
    fn redraw(&mut self, enabled: bool) {
        set_redraw(self.hwnd, &mut self.redraw_paused, enabled);
    }
    fn apply_membership(&mut self) -> std::result::Result<(), items::ApplyError> {
        let mut redraw = MembershipRedraw::new(
            self.hwnd,
            &mut self.redraw_paused,
            self.paused || self.updating,
        );
        self.membership.apply_before_write(
            &self.folder,
            &self.object_view,
            self.paused,
            || redraw.before_write(),
        )
    }
    fn restore(&mut self) -> bool {
        self.failed = true;
        self.paused = false;
        self.updating = false;
        let report = self.recovery.attempt(std::time::Instant::now(),
            || {
                let members = self.membership.restore(&self.folder, &self.object_view);
                // Icon visibility must be released even when restoring a
                // particular membership fails and requires another retry.
                let icons = self.desktop_visibility.restore_from_shell(self.hwnd, &self.folder).map_err(Into::into);
                members.and(icons).map_err(|e| {
                diagnostics::restore_failure(self.hwnd, e.stage, e.code());
                    e
                })
            },
            || unsafe { self.view.Refresh() });
        diagnostics::recovery(self.hwnd, if report.complete { 2 } else { 1 }, report.restore_error, report.refresh_error);
        if !report.complete {
            unsafe {
                if GetPropW(self.hwnd, ERROR).is_null() {
                    SetPropW(self.hwnd, ERROR, report.restore_error.unwrap_or(windows::core::HRESULT::from_win32(windows_sys::Win32::Foundation::ERROR_RETRY)).0 as u32 as usize as _);
                }
            }
        }
        self.redraw(true);
        self.arm_recovery();
        report.complete
    }
    fn apply_request(
        &mut self,
        request: &wire::Request,
    ) -> std::result::Result<(), items::ApplyError> {
        match request.op {
            wire::REPLACE_IDENTITY => {
                if !self.updating {
                    return Err(windows::Win32::Foundation::E_UNEXPECTED.into());
                }
                self.membership
                    .replace_identity(&request.names[0], &request.names[1])?;
                // Metadata handoff is atomic; the subsequent SET/UPDATE_END
                // applies membership with its own read/write error boundary.
                return Ok(());
            }
            wire::UPDATE_BEGIN => {
                if self.updating || self.paused || self.menu_prepared {
                    return Err(windows::Win32::Foundation::E_UNEXPECTED.into());
                }
                self.update_sequence = request.sequence;
                if unsafe { GetPropW(self.hwnd, UPDATE_RELEASE) as usize }
                    == request.sequence as usize
                {
                    unsafe {
                        SetPropW(self.hwnd, UPDATE_RELEASED, request.sequence as usize as _);
                    }
                    return Err(windows::Win32::Foundation::E_ABORT.into());
                }
                self.updating = true;
                self.redraw(false);
                return Ok(());
            }
            wire::UPDATE_END => {
                if !self.updating {
                    return Ok(());
                }
                self.updating = false;
                let result = self.apply_membership();
                unsafe {
                    SetPropW(
                        self.hwnd,
                        UPDATE_RELEASED,
                        self.update_sequence as usize as _,
                    );
                }
                return result;
            }
            wire::MENU_PREPARE => {
                unsafe {
                    let context = request
                        .menu
                        .ok_or(windows::Win32::Foundation::E_INVALIDARG)?;
                    let mut requested_pid = 0;
                    let mut owner_pid = 0;
                    GetWindowThreadProcessId(context.owner as _, &raw mut requested_pid);
                    GetWindowThreadProcessId(self.owner, &raw mut owner_pid);
                    if requested_pid == 0 || requested_pid != owner_pid || self.menu_prepared {
                        return Err(windows::Win32::Foundation::E_INVALIDARG.into());
                    }
                    RemovePropW(self.hwnd, RENAME);
                    RemovePropW(self.hwnd, MENU_HOST);
                    if !self
                        .menu_host
                        .as_ref()
                        .is_some_and(menu::worker::Worker::is_alive)
                    {
                        self.menu_host = Some(menu::worker::Worker::create(self.hwnd)?);
                    }
                    let hwnd = self
                        .menu_host
                        .as_ref()
                        .ok_or(windows::Win32::Foundation::E_FAIL)?
                        .prepare(&request.names, context)?;
                    self.menu_prepared = true;
                    SetPropW(self.hwnd, MENU_HOST, hwnd as _);
                }
                return Ok(());
            }
            wire::MENU_FINISH | wire::MENU_CANCEL => {
                self.menu_prepared = false;
                unsafe {
                    RemovePropW(self.hwnd, MENU_HOST);
                }
                if request.op == wire::MENU_CANCEL {
                    // Cancellation may follow a stuck WinUI popup. Retire its
                    // STA as well as the presenter; never reuse that UI state.
                    // Worker drop signals shutdown without joining Explorer.
                    if let Some(host) = self.menu_host.take() {
                        host.finish(true)?;
                    }
                } else if let Some(host) = &self.menu_host {
                    host.finish(false)?;
                }
                return Ok(());
            }
            wire::SET => {
                self.membership.desired =
                    request.names.iter().map(|name| items::key(name)).collect();
            }
            wire::CLEAR_SELECTION => {
                unsafe {
                    let value = LVITEMW {
                        state: 0,
                        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                        ..Default::default()
                    };
                    SendMessageW(
                        self.hwnd,
                        LVM_SETITEMSTATE,
                        usize::MAX,
                        (&raw const value) as isize,
                    );
                }
                return Ok(());
            }
            wire::PAUSE => {
                unsafe {
                    RemovePropW(self.hwnd, RENAME);
                }
                self.redraw(false);
                self.paused = true;
            }
            wire::RESUME => {
                self.paused = false;
            }
            _ => {}
        }
        self.apply_membership()
    }
    fn apply_result(
        &mut self,
        result: std::result::Result<(), items::ApplyError>,
    ) -> Option<items::ApplyError> {
        let error = match result {
            Ok(()) => {
                self.read_retry.recovered();
                None
            }
            Err(error) => {
                diagnostics::failure(self.hwnd, error.stage, error.code());
                if !self
                    .read_retry
                    .failed(error.retryable(), std::time::Instant::now())
                {
                    self.restore();
                    unsafe {
                        SetPropW(self.hwnd, ERROR, error.code().0 as u32 as usize as _);
                    }
                }
                Some(error)
            }
        };
        self.arm_recovery();
        error
    }
    fn cleanup(&mut self) -> bool {
        self.closing = true;
        self.menu_host.take();
        if !self.restore() { return false; }
        remove_registration(self.hwnd);
        true
    }
}

fn remove_registration(hwnd: HWND) {
    diagnostics::clear(hwnd);
    unsafe {
        KillTimer(hwnd, TIMER);
        KillTimer(hwnd, RECOVERY_TIMER);
        RemoveWindowSubclass(GetParent(hwnd), Some(subclass), MAGIC);
        RemoveWindowSubclass(hwnd, Some(subclass), MAGIC);
        for property in [
            OWNER,
            ACK,
            ERROR,
            RENAME,
            MENU_HOST,
            MENU_ERROR,
            UPDATE_RELEASE,
            UPDATE_RELEASED,
            REQUEST_ERROR,
        ] {
            RemovePropW(hwnd, property);
        }
    }
}

// Every filtering entry point gates COM-reentrant painting, even during reads.
// Only actual writes invalidate the view on release. The subclass suppresses
// painting without WM_SETREDRAW, which would change native window visibility.
// Peek/rename transactions retain their gate until their explicit end request.
struct MembershipRedraw<'a> {
    hwnd: HWND,
    paused: &'a mut bool,
    retain: bool,
}
impl<'a> MembershipRedraw<'a> {
    fn new(hwnd: HWND, paused: &'a mut bool, retain: bool) -> Self {
        FROZEN_VIEW.with(|view| view.set(hwnd));
        Self { hwnd, paused, retain }
    }
    fn before_write(&mut self) {
        set_redraw(self.hwnd, self.paused, false);
    }
}
impl Drop for MembershipRedraw<'_> {
    fn drop(&mut self) {
        if !self.retain {
            set_redraw(self.hwnd, self.paused, true);
            FROZEN_VIEW.with(|view| view.set(null_mut()));
        }
    }
}

fn set_redraw(hwnd: HWND, paused: &mut bool, enabled: bool) {
    unsafe {
        if enabled && *paused {
            FROZEN_VIEW.with(|view| view.set(null_mut()));
            windows_sys::Win32::Graphics::Gdi::RedrawWindow(
                hwnd,
                std::ptr::null(),
                null_mut(),
                windows_sys::Win32::Graphics::Gdi::RDW_INVALIDATE
                    | windows_sys::Win32::Graphics::Gdi::RDW_ALLCHILDREN,
            );
            *paused = false;
        } else if !enabled && !*paused {
            FROZEN_VIEW.with(|view| view.set(hwnd));
            *paused = true;
        }
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    _: usize,
) -> isize {
    let _callback = library::Callback::enter();
    let frozen = FROZEN_VIEW.with(Cell::get);
    if !frozen.is_null() && msg == WM_NOTIFY && lp != 0 {
        let header = unsafe { &*(lp as *const NMHDR) };
        if header.hwndFrom == frozen && header.code == NM_CUSTOMDRAW {
            let draw = unsafe { &*(lp as *const NMCUSTOMDRAW) };
            if draw.dwDrawStage == CDDS_PREPAINT {
                return CDRF_SKIPDEFAULT as isize;
            }
        }
    }
    // Shell calls can toggle redraw reentrantly. While our paint gate owns the
    // transaction, do not forward either value: DefWindowProc changes WS_VISIBLE.
    // Run before the RefCell guard, including during those nested COM calls.
    if frozen == hwnd {
        match msg {
            WM_SETREDRAW => return 0,
            WM_PAINT => {
                unsafe {
                    windows_sys::Win32::Graphics::Gdi::ValidateRect(hwnd, std::ptr::null());
                }
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_PRINTCLIENT => return 0,
            WM_NCDESTROY => FROZEN_VIEW.with(|view| view.set(null_mut())),
            _ => {}
        }
    }
    let handled = std::panic::catch_unwind(|| {
        STATE.with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                // A Shell/COM modal loop can consume our posted wake while an
                // outer callback owns STATE. Do not lose that wake or repost it
                // into the same nested loop: retry after a short timer instead.
                if msg == work_message() || (msg == WM_TIMER && matches!(wp, TIMER | RECOVERY_TIMER)) {
                    unsafe { SetTimer(hwnd, RECOVERY_TIMER, 25, None); }
                    return Some(0);
                }
                return None;
            };
            let state = slot.as_mut()?;
            if hwnd != state.hwnd {
                if msg == WM_NOTIFY && lp != 0 {
                    let header = unsafe { &*(lp as *const NMHDR) };
                    if header.hwndFrom == state.hwnd
                        && matches!(header.code, LVN_INSERTITEM | LVN_DELETEITEM | LVN_DELETEALLITEMS)
                    {
                        // Explorer can update its view without sending the public
                        // LVM_* messages through the child subclass. Observe the
                        // resulting notifications too; never mutate inside them.

                        state.queue();
                    }
                }
                if msg == WM_COMMAND && !state.paused && state.user_menu {
                    state.membership.user_changed_layout();
                    state.user_menu = false;
                }
                if msg == WM_NOTIFY && lp != 0 && state.paused {
                    let header = unsafe { &*(lp as *const NMHDR) };
                    if header.hwndFrom == state.hwnd
                        && matches!(header.code, LVN_BEGINLABELEDITA | LVN_BEGINLABELEDITW)
                    {
                        // Explorer's rename command belongs in the pane editor, not
                        // in the temporarily restored native view underneath it.
                        unsafe {
                            SetPropW(state.hwnd, RENAME, 1usize as _);
                        }
                        return Some(1);
                    }
                }
                return None;
            }
            if msg == WM_NCDESTROY {
                // The dying view cannot accept COM updates; its replacement enumerates
                // the original data source. No custom membership survives that rebuild.
                remove_registration(hwnd);
                slot.take();
                return None;
            }
            // Startup refreshes can bypass both LVM_* and LVN_* notifications.
            // A retained hide list therefore needs checking before every native
            // paint, even when no work was queued. No timer or repaint is added
            // for an unchanged view, and membership is never mutated in custom draw.
            if filter_before_paint(msg, !state.membership.desired.is_empty(),
                state.queued || state.read_retry.pending(), state.paused, state.updating, state.failed) {
                if state.read_retry.waiting() {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::ValidateRect(hwnd, std::ptr::null());
                    }
                    return Some(0);
                }
                let result = state.apply_membership();
                if state.apply_result(result).is_some() && !state.failed {
                    unsafe {
                        windows_sys::Win32::Graphics::Gdi::ValidateRect(hwnd, std::ptr::null());
                    }
                    return Some(0);
                }
            }
            if msg == WM_COPYDATA && wp as HWND == state.owner && lp != 0 {
                let data = unsafe { &*(lp as *const COPYDATASTRUCT) };
                if data.dwData != MAGIC {
                    return None;
                }
                if data.lpData.is_null()
                    || data.cbData as usize > wire::MAX_BYTES
                    || state.pending.is_some()
                {
                    return Some(0);
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize)
                };
                let Some(request) = wire::decode(bytes) else {
                    return Some(0);
                };
                state.pending = Some(request);
                state.queue();
                return Some(MAGIC as isize);
            }
            if msg == work_message() || (msg == WM_TIMER && matches!(wp, TIMER | RECOVERY_TIMER)) {
                let urgent = msg == work_message()
                    || state.queued
                    || state.pending.is_some()
                    || (!state.failed && state.read_retry.pending());
                state.queued = false;
                if unsafe {
                    IsWindow(state.owner) == 0 || WaitForSingleObject(state.process, 0) == 0
                } {
                    if state.cleanup() { slot.take(); }
                    return Some(0);
                }
                if state.closing {
                    if state.cleanup() { slot.take(); }
                    return Some(0);
                }
                if state.failed {
                    state.restore();
                }
                if state.updating
                    && unsafe { GetPropW(hwnd, UPDATE_RELEASE) as usize }
                        == state.update_sequence as usize
                {
                    // Do not restore all members on a cleanup/read error. Normal
                    // filtering will retry; presentation must always be released.
                    state.updating = false;
                    let result = state.apply_membership();
                    state.apply_result(result);
                    state.redraw(true);
                    unsafe {
                        SetPropW(hwnd, UPDATE_RELEASED, state.update_sequence as usize as _);
                    }
                }
                // Lifecycle/release timers do not enumerate unchanged membership.
                if !urgent {
                    return Some(0);
                }
                let request = state.pending.take();
                if request.is_some() {
                    unsafe {
                        RemovePropW(hwnd, REQUEST_ERROR);
                    }
                }
                if request.is_none() && state.read_retry.waiting() {
                    return Some(0);
                }
                if request
                    .as_ref()
                    .is_some_and(|request| request.op == wire::DETACH)
                {
                    if state.cleanup() { slot.take(); }
                    return Some(0);
                }
                if let Some(request) = request.as_ref().filter(|r| wire::is_desktop_visibility(r.op)) {
                    let result = match request.op {
                        wire::DESKTOP_HIDE => state.desktop_visibility.hide(state.hwnd),
                        wire::DESKTOP_RESTORE => state.desktop_visibility.restore_from_shell(state.hwnd, &state.folder),
                        _ => {
                            state.desktop_visibility.enable(request.op == wire::DESKTOP_GESTURE_ON);
                            Ok(())
                        }
                    };
                    unsafe {
                        if let Err(error) = result {
                            SetPropW(hwnd, REQUEST_ERROR, error.code().0 as u32 as usize as _);
                        }
                        SetPropW(hwnd, ACK, request.sequence as usize as _);
                    }
                    return Some(0);
                }
                if !state.failed {
                    if let Some(request) = &request {
                        if wire::is_menu_transaction(request.op) {
                            unsafe {
                                RemovePropW(hwnd, MENU_ERROR);
                            }
                            if let Err(error) = state.apply_request(request) {
                                unsafe {
                                    SetPropW(hwnd, MENU_ERROR, error.code().0 as u32 as usize as _);
                                }
                            }
                            unsafe {
                                SetPropW(hwnd, ACK, request.sequence as usize as _);
                            }
                            // A menu error must never restore desktop membership.
                            return Some(0);
                        }
                    }
                    let result = if let Some(request) = &request {
                        state.apply_request(request)
                    } else if state.paused {
                        Ok(())
                    } else {
                        let _sample = diagnostics::Sample::start(state.hwnd);
                        state.apply_membership()
                    };
                    if let Some(error) = state.apply_result(result) {
                        if request.is_some() {
                            unsafe {
                                SetPropW(hwnd, REQUEST_ERROR, error.code().0 as u32 as usize as _);
                            }
                        }
                    }
                }
                if let Some(request) = request {
                    unsafe {
                        SetPropW(hwnd, ACK, request.sequence as usize as _);
                    }
                }
                return Some(0);
            }
            if !state.paused
                && matches!(
                    msg,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_KEYDOWN
                )
            {
                unsafe {
                    PostMessageW(
                        state.owner,
                        crate::notifications::DESKTOP_INPUT_MESSAGE,
                        hwnd as usize,
                        GetMessageTime() as isize,
                    );
                }
            }
            if !state.paused && msg == WM_RBUTTONDOWN {
                state.user_menu = true;
            }
            if !state.paused && msg == WM_LBUTTONDOWN {
                let mut point = windows_sys::Win32::Foundation::POINT::default();
                unsafe {
                    GetCursorPos(&raw mut point);
                }
                state.drag_start = Some(point);
            }
            if !state.paused && matches!(msg, WM_LBUTTONUP | WM_CAPTURECHANGED) {
                if let Some(start) = state.drag_start.take() {
                    let mut end = windows_sys::Win32::Foundation::POINT::default();
                    unsafe {
                        GetCursorPos(&raw mut end);
                    }
                    if start.x.abs_diff(end.x) > 4 || start.y.abs_diff(end.y) > 4 {
                        state.membership.user_changed_layout();
                    }
                }
            }
            if matches!(msg, WM_DISPLAYCHANGE | WM_DPICHANGED) {
                state.membership.user_changed_layout();
            }
            if matches!(
                msg,
                LVM_SETITEMCOUNT
                    | LVM_INSERTITEMW
                    | LVM_DELETEITEM
                    | LVM_DELETEALLITEMS
                    | LVM_SORTITEMS
                    | LVM_SORTITEMSEX
                    | WM_DISPLAYCHANGE
                    | WM_SETTINGCHANGE
            ) {
                state.queue();
            }
            None
        })
    })
    .ok()
    .flatten();
    if msg == work_message() || (msg == WM_TIMER && matches!(wp, TIMER | RECOVERY_TIMER)) {
        STATE.with(|slot| {
            if let Ok(slot) = slot.try_borrow() {
                if let Some(state) = slot.as_ref() { state.arm_recovery(); }
            }
        });
    }
    handled.unwrap_or_else(|| unsafe { DefSubclassProc(hwnd, msg, wp, lp) })
}

/// Called only for queued messages removed by our existing desktop-thread hook.
pub(super) fn desktop_gesture(message: &MSG) -> bool {
    if !super::desktop_visibility::observes(message.message) { return false; }
    STATE.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            if let Some(state) = slot.as_mut() {
                if GESTURE_INTERRUPTED.with(|interrupted| interrupted.replace(false)) {
                    state.desktop_visibility.cancel_gesture();
                }
                let blocked = state.paused || state.updating || state.menu_prepared || state.closing || state.failed;
                if unsafe { state.desktop_visibility.message(state.hwnd, message, blocked) } {
                    unsafe {
                        PostMessageW(state.owner, crate::notifications::DESKTOP_TOGGLE_MESSAGE,
                            state.hwnd as usize, message.time as isize);
                    }
                }
                let consume = state.desktop_visibility.consumes_double_click(message.message);
                #[cfg(debug_assertions)]
                if consume { unsafe {
                    let key = windows_sys::w!("LucidDesk.Desktop.DoubleClick.Consumed");
                    let count = GetPropW(state.hwnd, key) as usize;
                    SetPropW(state.hwnd, key, count.saturating_add(1) as _);
                } }
                return consume;
            }
        } else { GESTURE_INTERRUPTED.with(|interrupted| interrupted.set(true)); }
        false
    })
}

fn filter_before_paint(msg: u32, has_members: bool, pending: bool, paused: bool, updating: bool, failed: bool) -> bool {
    matches!(msg, WM_PAINT | WM_PRINTCLIENT)
        && (has_members || pending)
        && !paused && !updating && !failed
}

#[cfg(test)]
mod tests {
    #[test]
    fn count_mismatch_retries_but_preserves_interface_errors() {
        let mut reads = 0;
        super::verify_counts(|| { reads += 1; Ok((2, if reads == 1 { 1 } else { 2 })) }).unwrap();
        assert_eq!(reads, 2);
        reads = 0;
        let error = super::verify_counts(|| { reads += 1; Ok((2, 1)) }).unwrap_err();
        assert_eq!(reads, 3);
        assert_eq!(error.code(), windows::core::HRESULT::from_win32(1237));
        reads = 0;
        let error = super::verify_counts(|| { reads += 1; Err(windows::Win32::Foundation::E_NOINTERFACE.into()) }).unwrap_err();
        assert_eq!(reads, 1);
        assert_eq!(error.code(), windows::Win32::Foundation::E_NOINTERFACE);
    }

    use super::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    #[test]
    fn reentrant_work_wake_is_retried_after_state_borrow_releases() {
        unsafe {
            let hwnd = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!(""),
                WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), null_mut(), null_mut());
            assert!(!hwnd.is_null());
            assert_ne!(SetWindowSubclass(hwnd, Some(subclass), MAGIC, 0), 0);
            let mut retries = 0;
            STATE.with(|slot| {
                // Model a Shell nested message loop while the outer filter
                // callback still owns STATE, without touching the real desktop.
                let borrow = slot.borrow_mut();
                assert!(borrow.is_none());
                PostMessageW(hwnd, work_message(), 0, 0);
                let mut message = MSG::default();
                assert_ne!(PeekMessageW(&raw mut message, hwnd, work_message(), work_message(), PM_REMOVE), 0);
                DispatchMessageW(&message);
                for _ in 0..2 {
                    let started = std::time::Instant::now();
                    while started.elapsed() < std::time::Duration::from_secs(1) {
                        if PeekMessageW(&raw mut message, hwnd, WM_TIMER, WM_TIMER, PM_REMOVE) != 0 {
                            assert_eq!(message.wParam, RECOVERY_TIMER);
                            retries += 1;
                            if retries == 1 {
                                // A timer consumed by that same nested loop
                                // must also leave another chance to run.
                                DispatchMessageW(&message);
                            }
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                drop(borrow);
                if retries == 2 { DispatchMessageW(&message); }
            });
            KillTimer(hwnd, RECOVERY_TIMER);
            RemoveWindowSubclass(hwnd, Some(subclass), MAGIC);
            DestroyWindow(hwnd);
            assert_eq!(retries, 2, "nested message loops must not swallow the only work wake");
        }
    }

    #[test]
    fn silent_refresh_is_filtered_before_paint_without_a_queued_notification() {
        for msg in [WM_PAINT, WM_PRINTCLIENT] {
            assert!(filter_before_paint(msg, true, false, false, false, false),
                "a refresh can reinsert managed rows without any observed list notification");
            assert!(filter_before_paint(msg, false, true, false, false, false));
            assert!(!filter_before_paint(msg, false, false, false, false, false));
            assert!(!filter_before_paint(msg, true, true, true, false, false));
            assert!(!filter_before_paint(msg, true, true, false, true, false));
            assert!(!filter_before_paint(msg, true, true, false, false, true));
        }
        assert!(!filter_before_paint(WM_TIMER, true, false, false, false, false));
        assert!(!filter_before_paint(WM_NOTIFY, true, false, false, false, false));
    }

    #[test]
    fn menu_redraw_gate_survives_reentrant_calls_and_releases() {
        unsafe {
            // DefWindowProc exposes accidental native redraw toggles through
            // SysSetRedraw. Our paint gate must never disable native redraw.
            let class = windows_sys::w!("LucidDeskRedrawGateTest");
            let instance = GetModuleHandleW(std::ptr::null());
            let definition = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&definition), 0);
            let hwnd = CreateWindowExW(
                0,
                class,
                windows_sys::w!(""),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            assert_ne!(SetWindowSubclass(hwnd, Some(subclass), MAGIC, 0), 0);
            windows_sys::Win32::Graphics::Gdi::ValidateRect(hwnd, std::ptr::null());
            let mut paused = false;
            let disabled = || !GetPropW(hwnd, windows_sys::w!("SysSetRedraw")).is_null();
            let draw = NMCUSTOMDRAW {
                hdr: NMHDR {
                    hwndFrom: hwnd,
                    code: NM_CUSTOMDRAW,
                    ..Default::default()
                },
                dwDrawStage: CDDS_PREPAINT,
                ..Default::default()
            };
            // The queued-work path also reads Shell metadata while STATE is
            // borrowed. Reentrant custom drawing must be blocked before writes.
            STATE.with(|slot| {
                let _borrow = slot.borrow_mut();
                let _redraw = MembershipRedraw::new(hwnd, &mut paused, false);
                assert!(!disabled());
                assert_eq!(
                    SendMessageW(hwnd, WM_NOTIFY, 0, (&raw const draw) as isize),
                    CDRF_SKIPDEFAULT as isize
                );
                assert_eq!(SendMessageW(hwnd, WM_ERASEBKGND, 0, 0), 1);
                assert_eq!(SendMessageW(hwnd, WM_PRINTCLIENT, 0, 0), 0);
            });
            assert!(FROZEN_VIEW.with(Cell::get).is_null());
            set_redraw(hwnd, &mut paused, true);
            assert_eq!(
                windows_sys::Win32::Graphics::Gdi::GetUpdateRect(hwnd, std::ptr::null_mut(), 0),
                0,
                "a read-only membership check must not invalidate the desktop"
            );
            // Errors and unwinding must release a non-transactional write gate.
            let error: Result<()> = (|| {
                let mut redraw = MembershipRedraw::new(hwnd, &mut paused, false);
                redraw.before_write();
                assert!(!disabled());
                Err(windows::Win32::Foundation::E_FAIL.into())
            })();
            assert!(error.is_err());
            assert!(!paused);
            assert!(!disabled());
            assert!(FROZEN_VIEW.with(Cell::get).is_null());
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut redraw = MembershipRedraw::new(hwnd, &mut paused, false);
                redraw.before_write();
                panic!("simulated filtering unwind");
            }));
            assert!(panic.is_err());
            assert!(!paused);
            assert!(!disabled());
            assert!(FROZEN_VIEW.with(Cell::get).is_null());
            // A successful pass inside Peek/rename must retain the outer gate.
            {
                let mut redraw = MembershipRedraw::new(hwnd, &mut paused, true);
                redraw.before_write();
            }
            assert!(paused);
            assert!(!disabled());
            assert_eq!(FROZEN_VIEW.with(Cell::get), hwnd);
            // Match synchronous callbacks made while State::apply_request owns
            // the mutable state borrow, then the later menu message loop.
            STATE.with(|slot| {
                let _borrow = slot.borrow_mut();
                SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
                assert!(!disabled());
                let draw = NMCUSTOMDRAW {
                    hdr: NMHDR {
                        hwndFrom: hwnd,
                        code: NM_CUSTOMDRAW,
                        ..Default::default()
                    },
                    dwDrawStage: CDDS_PREPAINT,
                    ..Default::default()
                };
                assert_eq!(
                    SendMessageW(hwnd, WM_NOTIFY, 0, (&raw const draw) as isize),
                    CDRF_SKIPDEFAULT as isize
                );
            });
            SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
            assert!(!disabled());
            set_redraw(hwnd, &mut paused, true);
            assert!(!disabled());
            assert!(!paused);
            assert_ne!(
                windows_sys::Win32::Graphics::Gdi::GetUpdateRect(hwnd, std::ptr::null_mut(), 0),
                0,
                "membership writes still require a repaint when released"
            );
            // Both initial visibility states and changes while frozen must survive.
            for initially_visible in [false, true] {
                for final_visible in [false, true] {
                    ShowWindow(hwnd, if initially_visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
                    {
                        let mut redraw = MembershipRedraw::new(hwnd, &mut paused, false);
                        redraw.before_write();
                        assert_eq!(IsWindowVisible(hwnd) != 0, initially_visible);
                        SendMessageW(hwnd, WM_SETREDRAW, 0, 0);
                        SendMessageW(hwnd, WM_SETREDRAW, 1, 0);
                        assert_eq!(IsWindowVisible(hwnd) != 0, initially_visible);
                        ShowWindow(hwnd, if final_visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
                    }
                    assert_eq!(IsWindowVisible(hwnd) != 0, final_visible);
                    assert!(!disabled());
                    assert!(!paused);
                }
            }
            // A retained gate (Peek/rename) must also preserve a hidden window.
            ShowWindow(hwnd, SW_HIDE);
            {
                let mut redraw = MembershipRedraw::new(hwnd, &mut paused, true);
                redraw.before_write();
            }
            set_redraw(hwnd, &mut paused, true);
            assert_eq!(IsWindowVisible(hwnd), 0);
            RemoveWindowSubclass(hwnd, Some(subclass), MAGIC);
            DestroyWindow(hwnd);
            UnregisterClassW(class, instance);
        }
    }
}
