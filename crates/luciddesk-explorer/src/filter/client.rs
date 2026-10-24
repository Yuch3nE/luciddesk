use super::*;
use std::{
    cell::Cell,
    path::Path,
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE, HWND},
    System::{DataExchange::COPYDATASTRUCT, LibraryLoader::*},
};

#[cfg(test)]
#[path = "client_focus_tests.rs"]
mod focus_tests;

#[cfg(test)]
#[path = "client_reentrancy_tests.rs"]
mod reentrancy_tests;

const ATTACH_TIMEOUT: Duration = Duration::from_secs(3);
const ACK_TIMEOUT: Duration = Duration::from_secs(3);
const MENU_ACK_TIMEOUT: Duration = Duration::from_secs(5);
const SEND_TIMEOUT_MS: u32 = 1500;

#[derive(Clone, Copy)]
struct PendingHidden {
    sequence: u32,
    started: Instant,
}

/// Owns one desktop membership session. Dropping it requests restoration of removed view items.
pub struct FilterSession {
    view: HWND,
    owner: HWND,
    hook: HHOOK,
    module: HMODULE,
    sequence: Cell<u32>,
    retired: Cell<bool>,
    in_flight: Cell<bool>,
    pending_hidden: Cell<Option<PendingHidden>>,
    update: Cell<Option<u32>>,
    explorer_watch: Option<explorer::ExplorerWatch>,
}

// Both SendMessageTimeout and ACK waits can dispatch callbacks on this STA.
// The protocol has one ACK slot, so a nested request must not replace it.
// This is a protocol precondition, not a connection failure or retry mechanism.
struct RequestGuard<'a>(&'a Cell<bool>);
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) { self.0.set(false); }
}

struct AttachTarget {
    view: HWND,
    parent: HWND,
    process: u32,
    thread: u32,
}
impl AttachTarget {
    fn capture(view: HWND) -> Result<Self, String> {
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(view, &raw mut process) };
        if thread == 0 || process == 0 { return Err("桌面窗口已失效".into()); }
        Ok(Self { view, parent: unsafe { GetParent(view) }, process, thread })
    }
    fn alive(&self) -> bool {
        let mut process = 0;
        unsafe {
            IsWindow(self.view) != 0
                && GetWindowThreadProcessId(self.view, &raw mut process) == self.thread
                && process == self.process
                && GetParent(self.view) == self.parent
        }
    }
    fn current(&self, discovered: Option<isize>) -> bool {
        self.alive() && discovered == Some(self.view as isize)
    }
}

// Capture the native error before cleanup or diagnostic calls can overwrite it.
fn attach_os_error(view: HWND, stage: diagnostics::Stage) -> String {
    let error = std::io::Error::last_os_error();
    let code = error.raw_os_error().unwrap_or(0) as u32;
    let hr = if code == 0 { windows::Win32::Foundation::E_FAIL } else { windows::core::HRESULT::from_win32(code) };
    diagnostics::failure(view, stage, hr);
    format!("{error}; {}", diagnostics::summary(view))
}

fn wait_for_attach(target: &AttachTarget, owner: HWND) -> Result<(), String> {
    wait_for_attach_with(target, owner, ATTACH_TIMEOUT, || crate::desktop_view().ok())
}

fn wait_for_attach_with(
    target: &AttachTarget,
    owner: HWND,
    timeout: Duration,
    mut discover: impl FnMut() -> Option<isize>,
) -> Result<(), String> {
    let started = Instant::now();
    let mut next_discovery = Duration::ZERO;
    loop {
        if !target.alive() || unsafe { IsWindow(owner) } == 0 {
            return Err("握手期间桌面或控制窗口已失效，将重新定位".into());
        }
        if started.elapsed() >= next_discovery {
            if !target.current(discover()) {
                return Err("握手期间桌面视图已被替换，将重新定位".into());
            }
            next_discovery = started.elapsed() + Duration::from_millis(50);
        }
        unsafe {
            if GetPropW(target.view, OWNER) == owner { return Ok(()); }
            let error = GetPropW(target.view, ERROR) as usize as u32;
            if error != 0 { return Err(format!("桌面过滤连接失败 (0x{error:08x}); {}", diagnostics::summary(target.view))); }
        }
        // A dispatched callback (including a modal disconnect warning) can run
        // past the deadline while Explorer completes attachment. Check identity
        // and the published result before declaring timeout on return.
        if started.elapsed() >= timeout { break; }
        // Explorer's Shell initialization can synchronously query our windows.
        // Keep this STA responsive rather than making the handshake wait for
        // its own UI thread. The app guards reentrant reconnect attempts.
        unsafe {
            let mut message = MSG::default();
            for _ in 0..32 {
                if PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) == 0 { break; }
                if message.message == WM_QUIT {
                    PostQuitMessage(message.wParam as i32);
                    return Err("桌面握手被退出请求中断".into());
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 10, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
    diagnostics::attach_failure(target.view, windows::core::HRESULT::from_win32(
        windows_sys::Win32::Foundation::ERROR_TIMEOUT,
    ));
    Err(format!("Explorer 在 3 秒内未确认桌面过滤连接; {}", diagnostics::summary(target.view)))
}
impl FilterSession {
    /// # Errors
    /// Fails if Explorer does not expose the required COM interface on its UI thread.
    pub fn connect(view: isize, owner: isize, dll: &Path) -> Result<Self, String> {
        unsafe {
            let view = view as HWND;
            let owner = owner as HWND;
            if IsWindow(view) == 0 || IsWindow(owner) == 0 {
                return Err("桌面或控制窗口无效".into());
            }
            if !GetPropW(view, OWNER).is_null() {
                return Err(format!("桌面过滤已有活动连接或恢复尚未完成; {}", diagnostics::summary(view)));
            }
            let target = AttachTarget::capture(view)?;
            // SetWindowsHookEx translates the entry RVA into Explorer's module.
            // A legacy image with the same name/path can have a different RVA;
            // never inject while that image (or an old session) is still mapped.
            if !crate::discovery::component_released_in(target.process)? {
                return Err("Explorer 中的旧桌面组件尚未释放。请稍后重试；若持续出现，请重启资源管理器后再启动 LucidDesk。".into());
            }
            let path = std::fs::canonicalize(dll).map_err(|e| format!("桌面组件路径解析失败 ({}): {e}", dll.display()))?;
            let path: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            diagnostics::reset(view);
            diagnostics::stage(view, diagnostics::Stage::LoadLibrary);
            let module = LoadLibraryExW(
                path.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if module.is_null() {
                let error = attach_os_error(view, diagnostics::Stage::LoadLibrary);
                return Err(error);
            }
            diagnostics::stage(view, diagnostics::Stage::Export);
            let Some(proc) = GetProcAddress(module, windows_sys::s!("LucidDeskFilterHook")) else {
                let error = attach_os_error(view, diagnostics::Stage::Export);
                FreeLibrary(module);
                return Err(error);
            };
            let callback = std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(i32, usize, isize) -> isize,
            >(proc);
            diagnostics::stage(view, diagnostics::Stage::Hook);
            let hook = SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(callback),
                module,
                target.thread,
            );
            if hook.is_null() {
                let error = attach_os_error(view, diagnostics::Stage::Hook);
                FreeLibrary(module);
                return Err(error);
            }
            let mut session = Self {
                view,
                owner,
                hook,
                module,
                sequence: Cell::new(0),
                retired: Cell::new(false),
                in_flight: Cell::new(false),
                pending_hidden: Cell::new(None),
                update: Cell::new(None),
                explorer_watch: None,
            };
            RemovePropW(view, ERROR);
            diagnostics::stage(view, diagnostics::Stage::PostAttach);
            if PostMessageW(view, message(), owner as usize, MAGIC as isize) == 0 {
                return Err(attach_os_error(view, diagnostics::Stage::PostAttach));
            }
            wait_for_attach(&target, owner)?;
            session.explorer_watch = Some(explorer::ExplorerWatch::new(target.process, owner)?);
            Ok(session)
        }
    }
    #[must_use]
    pub fn is_alive(&self) -> bool {
        unsafe {
            !self.retired.get()
                && self.explorer_watch.as_ref().is_none_or(|watch| watch.alive())
                && IsWindow(self.view) != 0
                && GetPropW(self.view, OWNER) == self.owner
                && GetPropW(self.view, ERROR).is_null()
        }
    }
    /// # Errors
    /// Returns an error if Explorer cannot apply and acknowledge the complete set.
    pub fn set_hidden(&self, names: &[String]) -> Result<(), String> {
        self.request(wire::SET, names)
    }
    /// Submit membership without waiting for Shell enumeration and updates.
    /// Call poll_hidden until complete before submitting another transaction.
    pub fn submit_hidden(&self, names: &[String]) -> Result<(), String> {
        let _request = self.enter_request()?;
        if self.retired.get() {
            return Err("桌面过滤连接已超时，等待重新连接".into());
        }
        if self.pending_hidden.get().is_some() {
            return Err("桌面分组同步尚未完成".into());
        }
        let sequence = self.next_sequence();
        let bytes = wire::encode(wire::SET, sequence, names)?;
        self.transmit(&bytes)?;
        self.pending_hidden.set(Some(PendingHidden { sequence, started: Instant::now() }));
        Ok(())
    }

    /// Returns false while Explorer is still processing the submitted set.
    pub fn poll_hidden(&self) -> Result<bool, String> {
        let Some(PendingHidden { sequence, started }) = self.pending_hidden.get() else { return Ok(true); };
        unsafe {
            if !self.is_alive() {
                self.pending_hidden.set(None);
                return Err(format!("桌面过滤连接已断开; {}", diagnostics::summary(self.view)));
            }
            if GetPropW(self.view, ACK) as usize != sequence as usize {
                if started.elapsed() >= ACK_TIMEOUT {
                    self.retired.set(true);
                    self.pending_hidden.set(None);
                    return Err(format!("桌面分组同步确认超时; sequence={sequence}; {}", diagnostics::summary(self.view)));
                }
                return Ok(false);
            }
            self.pending_hidden.set(None);
            let error = GetPropW(self.view, REQUEST_ERROR) as usize as u32;
            if error != 0 {
                return Err(format!("桌面过滤请求未完成 (0x{error:08x}); {}", diagnostics::summary(self.view)));
            }
        }
        Ok(true)
    }
    /// Request fallback repair after the app's shared desktop audit.
    pub fn repair(&self) -> Result<(), String> {
        if !self.is_alive() { return Err("桌面过滤连接已断开".into()); }
        if unsafe { PostMessageW(self.view, work_message(), 0, 0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    /// # Errors
    /// Fails when the desktop connection is unavailable.
    pub fn clear_selection(&self) -> Result<(), String> {
        self.request(wire::CLEAR_SELECTION, &[])
    }
    /// Freeze presentation while a managed Shell identity changes. Membership
    /// remains filtered; the caller must finish even if the operation fails.
    pub fn begin_update(&self) -> Result<(), String> {
        self.ensure_idle()?;
        if self.retired.get() {
            return Err("桌面过滤连接已超时，等待重新连接".into());
        }
        if self.pending_hidden.get().is_some() {
            return Err("桌面分组同步尚未完成，请稍后重试".into());
        }
        if let Some(sequence) = self.update.get() {
            if unsafe { GetPropW(self.view, UPDATE_RELEASE) as usize } != sequence as usize
            {
                return Err("已有活动身份更新".into());
            }
            self.finish_update()?;
        }
        let sequence = self.next_sequence();
        self.update.set(Some(sequence));
        let bytes = wire::encode(wire::UPDATE_BEGIN, sequence, &[])?;
        self.send(wire::UPDATE_BEGIN, sequence, &bytes)
    }
    pub fn finish_update(&self) -> Result<(), String> {
        let Some(sequence) = self.update.get() else {
            return Ok(());
        };
        unsafe {
            if GetPropW(self.view, OWNER) != self.owner {
                return Err("桌面过滤连接已断开".into());
            }
            // This marker survives a rejected/timed-out WM_COPYDATA. Explorer's
            // timer services it even when the client remains alive. A sequence
            // makes a late release harmless to a subsequent transaction.
            if SetPropW(self.view, UPDATE_RELEASE, sequence as usize as _) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            PostMessageW(self.view, work_message(), 0, 0);
            if GetPropW(self.view, UPDATE_RELEASED) as usize == sequence as usize {
                self.update.set(None);
                return Ok(());
            }
        }
        let result = self.request(wire::UPDATE_END, &[]);
        if result.is_ok() {
            self.update.set(None);
        }
        result
    }
    pub fn replace_identity(&self, old: &str, new: &str) -> Result<(), String> {
        self.request(wire::REPLACE_IDENTITY, &[old.into(), new.into()])
    }
    /// Temporarily restores Shell membership for Explorer preview integrations.
    /// # Errors
    /// Fails when the desktop cannot enter/leave the suspended filtering scope.
    pub fn pause(&self, pause: bool) -> Result<bool, String> {
        self.request(if pause { wire::PAUSE } else { wire::RESUME }, &[])?;
        Ok(!pause && unsafe { !GetPropW(self.view, RENAME).is_null() })
    }
    /// Enable blank-desktop gestures on the existing desktop thread hook.
    /// # Errors
    /// Returns the native request/acknowledgement failure.
    pub fn desktop_gesture(&self, enabled: bool) -> Result<(), String> {
        self.request(if enabled { wire::DESKTOP_GESTURE_ON } else { wire::DESKTOP_GESTURE_OFF }, &[])
    }
    /// Hide native icons temporarily, or restore the state saved by this session.
    /// # Errors
    /// Returns the Shell visibility or acknowledgement failure.
    pub fn desktop_hidden(&self, hidden: bool) -> Result<(), String> {
        self.request(if hidden { wire::DESKTOP_HIDE } else { wire::DESKTOP_RESTORE }, &[])
    }
    /// Prepare an independent Shell view. Desktop membership remains filtered.
    /// # Errors
    /// Fails if Explorer cannot provide an isolated native menu host.
    /// Pumps caller UI messages; callers must release app and event borrows first.
    pub fn prepare_menu(
        &self,
        owner: isize,
        names: &[String],
        x: i32,
        y: i32,
    ) -> Result<isize, String> {
        self.ensure_idle()?;
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(self.view, &raw mut pid);
            AllowSetForegroundWindow(pid);
            let sequence = self.next_sequence();
            let bytes = wire::encode_menu(
                sequence,
                names,
                wire::MenuContext {
                    owner: owner as u64,
                    x,
                    y,
                },
            )?;
            self.send(wire::MENU_PREPARE, sequence, &bytes)?;
            let host = GetPropW(self.view, MENU_HOST);
            if IsWindow(host) == 0 {
                return Err("Explorer 未创建独立菜单窗口".into());
            }
            Ok(host as isize)
        }
    }
    /// Release the menu host and return whether its rename command was chosen.
    /// # Errors
    /// Fails when Explorer does not acknowledge cleanup. Pumps caller UI messages.
    pub fn finish_menu(&self) -> Result<bool, String> {
        self.request(wire::MENU_FINISH, &[])?;
        Ok(unsafe { !GetPropW(self.view, RENAME).is_null() })
    }
    pub fn cancel_menu(&self) -> Result<(), String> {
        self.request(wire::MENU_CANCEL, &[])
    }
    fn next_sequence(&self) -> u32 {
        let sequence = self.sequence.get().wrapping_add(1).max(1);
        self.sequence.set(sequence);
        sequence
    }
    fn request(&self, op: u32, names: &[String]) -> Result<(), String> {
        self.ensure_idle()?;
        let sequence = self.next_sequence();
        let bytes = wire::encode(op, sequence, names)?;
        self.send(op, sequence, &bytes)
    }
    fn ensure_idle(&self) -> Result<(), String> {
        if self.in_flight.get() {
            Err("桌面过滤请求正在执行，不能重入提交".into())
        } else { Ok(()) }
    }
    fn enter_request(&self) -> Result<RequestGuard<'_>, String> {
        self.ensure_idle()?;
        self.in_flight.set(true);
        Ok(RequestGuard(&self.in_flight))
    }
    fn transmit(&self, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            if GetPropW(self.view, OWNER) != self.owner {
                return Err("桌面过滤连接已断开".into());
            }
            let data = COPYDATASTRUCT {
                dwData: MAGIC,
                cbData: bytes.len() as u32,
                lpData: bytes.as_ptr().cast_mut().cast(),
            };
            let mut accepted = 0;
            windows_sys::Win32::Foundation::SetLastError(0);
            if SendMessageTimeoutW(
                self.view,
                WM_COPYDATA,
                self.owner as usize,
                (&raw const data) as isize,
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                SEND_TIMEOUT_MS,
                &raw mut accepted,
            ) == 0
            {
                // Delivery failure can leave the operation executing in Explorer.
                // Retire the session rather than reuse its single ACK slot.
                let error = std::io::Error::last_os_error();
                self.retired.set(true);
                return Err(format!("桌面过滤请求发送失败，执行结果不确定: {error}"));
            }
            if accepted != MAGIC {
                return Err("Explorer 拒绝桌面过滤请求".into());
            }
        }
        Ok(())
    }
    fn ack_result(&self, op: u32) -> Result<(), String> {
        unsafe {
            if wire::is_desktop_visibility(op) {
                let error = GetPropW(self.view, REQUEST_ERROR) as usize as u32;
                return if error == 0 { Ok(()) } else {
                    Err(format!("桌面显隐请求失败 (0x{error:08x}); op={op}"))
                };
            }
            if wire::is_menu_transaction(op) {
                let error = GetPropW(self.view, MENU_ERROR) as usize as u32;
                return if error == 0 {
                    Ok(())
                } else {
                    Err(format!("独立原生菜单不可用 (0x{error:08x})"))
                };
            }
            let error = GetPropW(self.view, ERROR) as usize as u32;
            if error != 0 {
                return Err(format!("桌面过滤失败，已请求恢复原生项目 (0x{error:08x}); {}", diagnostics::summary(self.view)));
            }
            let retry = GetPropW(self.view, REQUEST_ERROR) as usize as u32;
            if retry != 0 {
                return Err(format!("桌面过滤请求未完成 (0x{retry:08x}); {}", diagnostics::summary(self.view)));
            }
            Ok(())
        }
    }
    fn pump_request_messages(op: u32) -> Result<(), String> {
        unsafe {
            if wire::is_menu_transaction(op) {
                // Explorer can synchronously query our windows while creating
                // its Shell view. Menu callers must hold no UI model borrows.
                let mut message = MSG::default();
                for _ in 0..32 {
                    if PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) == 0 {
                        break;
                    }
                    if message.message == WM_QUIT {
                        PostQuitMessage(message.wParam as i32);
                        return Err("菜单操作被退出请求中断".into());
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                MsgWaitForMultipleObjectsEx(
                    0,
                    std::ptr::null(),
                    5,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
            } else {
                // Shell view updates may synchronously query an owner HWND.
                // Service sent messages without dispatching queued app actions
                // while the caller still owns its workspace/model borrow.
                let mut message = MSG::default();
                PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
                MsgWaitForMultipleObjectsEx(
                    0, std::ptr::null(), 5, QS_SENDMESSAGE, MWMO_INPUTAVAILABLE,
                );
            }
        }
        Ok(())
    }
    fn send(&self, op: u32, sequence: u32, bytes: &[u8]) -> Result<(), String> {
        let _request = self.enter_request()?;
        // An accepted request without an ACK has an unknown outcome. Retire
        // this session instead of treating a live HWND as a healthy protocol.
        // Cleanup must remain possible before the supervisor reconnects.
        if self.retired.get() && !matches!(op,
            wire::DETACH | wire::UPDATE_END | wire::MENU_FINISH | wire::MENU_CANCEL
            | wire::DESKTOP_RESTORE | wire::DESKTOP_GESTURE_OFF)
        {
            return Err("桌面过滤连接已超时，等待重新连接".into());
        }
        // Do not overwrite the ACK/error of an asynchronous SET before its
        // caller has consumed it. Detach remains available during shutdown.
        if op != wire::DETACH && self.pending_hidden.get().is_some() {
            return Err("桌面分组同步尚未完成，请稍后重试".into());
        }
        self.transmit(bytes)?;
        unsafe {
            let started = Instant::now();
            // The menu worker itself allows three seconds to prepare. Give
            // its reply time to reach this outer IPC boundary before timing out.
            let timeout = if wire::is_menu_transaction(op) { MENU_ACK_TIMEOUT } else { ACK_TIMEOUT };
            while started.elapsed() < timeout {
                if op == wire::DETACH && GetPropW(self.view, OWNER).is_null() {
                    return Ok(());
                }
                if IsWindow(self.view) == 0 || GetPropW(self.view, OWNER) != self.owner {
                    return Err("等待确认期间桌面过滤连接已断开".into());
                }
                if GetPropW(self.view, ACK) as usize == sequence as usize {
                    return self.ack_result(op);
                }
                Self::pump_request_messages(op)?;
            }
            let description = if wire::is_menu_transaction(op) {
                "Explorer 菜单准备或释放超时"
            } else {
                self.retired.set(true);
                "桌面过滤响应超时"
            };
            Err(format!(
                "{description}; op={op}, sequence={sequence}, ack={}, elapsed_ms={}, owner_match={}, request_error=0x{:08x}, in_send=0x{:x}; {}",
                GetPropW(self.view, ACK) as usize, started.elapsed().as_millis(),
                GetPropW(self.view, OWNER) == self.owner,
                GetPropW(self.view, REQUEST_ERROR) as usize as u32,
                InSendMessageEx(std::ptr::null()),
                diagnostics::summary(self.view),
            ))
        }
    }
}
impl Drop for FilterSession {
    fn drop(&mut self) {
        self.explorer_watch.take();
        let _ = self.request(wire::DETACH, &[]);
        unsafe {
            UnhookWindowsHookEx(self.hook);
            FreeLibrary(self.module);
        }
    }
}

#[cfg(test)]
fn test_session(view: HWND, owner: HWND, sequence: u32) -> FilterSession {
    FilterSession {
        view, owner, hook: null_mut(), module: null_mut(), sequence: Cell::new(sequence),
        retired: Cell::new(false), in_flight: Cell::new(false), pending_hidden: Cell::new(None),
        update: Cell::new(None), explorer_watch: None,
    }
}

#[cfg(test)]
mod test_windows {
    use super::*;

    // Own the window and its optional class on the creating thread, including
    // assertion-unwind paths. System classes must never be unregistered.
    pub(super) struct Window {
        pub hwnd: HWND,
        class: Option<windows_sys::core::PCWSTR>,
        instance: HMODULE,
    }
    impl Window {
        pub fn new() -> Self { Self::create(windows_sys::w!("STATIC"), None) }
        pub fn with_proc(class: windows_sys::core::PCWSTR, proc: WNDPROC) -> Self {
            Self::create(class, proc)
        }
        fn create(class: windows_sys::core::PCWSTR, proc: WNDPROC) -> Self {
            unsafe {
                let instance = if proc.is_some() { GetModuleHandleW(null_mut()) } else { null_mut() };
                if proc.is_some() {
                    assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: proc, hInstance: instance,
                        lpszClassName: class, ..Default::default() }), 0);
                }
                let mut window = Self { hwnd: null_mut(), class: proc.map(|_| class), instance };
                window.hwnd = CreateWindowExW(0, class, windows_sys::w!("client test"), WS_POPUP,
                    0, 0, 1, 1, null_mut(), null_mut(), instance, null_mut());
                assert!(!window.hwnd.is_null());
                window
            }
        }
        pub fn owner(&self, owner: HWND) {
            unsafe { assert_ne!(SetPropW(self.hwnd, OWNER, owner), 0); }
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                // A WM_CLOSE callback or parent destruction may already have closed it.
                if IsWindow(self.hwnd) != 0 { DestroyWindow(self.hwnd); }
                if let Some(class) = self.class { UnregisterClassW(class, self.instance); }
            }
        }
    }
}

#[cfg(test)]
mod attach_tests {
    use super::*;
    use super::test_windows::Window;
    #[test]
    fn attach_observes_result_after_a_callback_crosses_the_deadline() {
        unsafe extern "system" fn delayed(hwnd: HWND, _: u32, id: usize, _: u32) {
            unsafe {
                KillTimer(hwnd, id);
                std::thread::sleep(Duration::from_millis(120));
                let view = GetPropW(hwnd, windows_sys::w!("LucidDesk.Test.AttachTarget"));
                if id == 1 { SetPropW(view, OWNER, hwnd); }
                else { SetPropW(view, ERROR, 0x80004005usize as _); }
            }
        }
        for id in [1, 2] {
            let view = Window::new();
            let owner = Window::new();
            let target = AttachTarget::capture(view.hwnd).unwrap();
            unsafe {
                assert_ne!(SetPropW(owner.hwnd, windows_sys::w!("LucidDesk.Test.AttachTarget"), view.hwnd), 0);
                assert_ne!(SetTimer(owner.hwnd, id, 1, Some(delayed)), 0);
            }
            let result = wait_for_attach_with(&target, owner.hwnd, Duration::from_millis(100), || Some(view.hwnd as isize));
            if id == 1 { assert!(result.is_ok(), "{result:?}"); }
            else { assert!(result.unwrap_err().contains("0x80004005")); }
        }
        // A late success on a replaced desktop must still be rejected.
        let view = Window::new();
        let owner = Window::new();
        let target = AttachTarget::capture(view.hwnd).unwrap();
        unsafe {
            assert_ne!(SetPropW(owner.hwnd, windows_sys::w!("LucidDesk.Test.AttachTarget"), view.hwnd), 0);
            assert_ne!(SetTimer(owner.hwnd, 1, 1, Some(delayed)), 0);
        }
        let mut discoveries = 0;
        let error = wait_for_attach_with(&target, owner.hwnd, Duration::from_millis(100), || {
            discoveries += 1;
            (discoveries == 1).then_some(view.hwnd as isize)
        }).unwrap_err();
        assert!(error.contains("桌面视图已被替换"), "{error}");
        let view = Window::new();
        let owner = Window::new();
        let target = AttachTarget::capture(view.hwnd).unwrap();
        let error = wait_for_attach_with(&target, owner.hwnd, Duration::from_millis(20), || Some(view.hwnd as isize)).unwrap_err();
        assert!(error.contains("未确认桌面过滤连接"), "{error}");
    }
    #[test]
    fn native_attach_errors_keep_stage_and_hresult() {
        let view = Window::new();
        for stage in [diagnostics::Stage::LoadLibrary, diagnostics::Stage::Export,
            diagnostics::Stage::Hook, diagnostics::Stage::PostAttach] {
            unsafe { windows_sys::Win32::Foundation::SetLastError(5); }
            let error = attach_os_error(view.hwnd, stage);
            assert!(error.contains("HRESULT=0x80070005"));
            assert!(error.contains(&format!("({})", stage as usize)));
        }
        unsafe { windows_sys::Win32::Foundation::SetLastError(0); }
        assert!(attach_os_error(view.hwnd, diagnostics::Stage::Hook).contains("HRESULT=0x80004005"));
    }
    #[test]
    fn destroyed_attach_target_aborts_without_waiting_for_timeout() {
        let view = Window::new();
        let owner = Window::new();
        let target = AttachTarget::capture(view.hwnd).unwrap();
        drop(view);
        let started = Instant::now();
        assert!(wait_for_attach(&target, owner.hwnd).unwrap_err().contains("已失效"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn current_target_rejects_replacement_missing_view_and_changed_parent() {
        let view = Window::new();
        let replacement = Window::new();
        let target = AttachTarget::capture(view.hwnd).unwrap();
        assert!(target.current(Some(view.hwnd as isize)));
        assert!(!target.current(Some(replacement.hwnd as isize)));
        assert!(!target.current(None));
        unsafe {
            SetWindowLongPtrW(view.hwnd, GWL_STYLE, WS_CHILD as isize);
            SetParent(view.hwnd, replacement.hwnd);
            assert_eq!(GetParent(view.hwnd), replacement.hwnd);
        }
        assert!(!target.alive());
    }
}


#[cfg(test)]
mod update_tests {
    use super::*;
    use super::test_windows::Window;

    unsafe fn received_request(lp: isize) -> wire::Request {
        unsafe {
            let data = &*(lp as *const COPYDATASTRUCT);
            let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
            wire::decode(bytes).unwrap()
        }
    }

    #[test]
    fn transport_timeout_retires_session_but_explicit_rejection_does_not() {
        use std::sync::mpsc;
        unsafe extern "system" fn receiver(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            unsafe {
                if msg == WM_COPYDATA {
                    let request = received_request(lp);
                    if request.op == wire::DETACH {
                        RemovePropW(hwnd, OWNER);
                        return MAGIC as isize;
                    }
                    if request.sequence == 1 { return 0; }
                    std::thread::sleep(Duration::from_millis(1800));
                    SetPropW(hwnd, ACK, request.sequence as usize as _);
                    return MAGIC as isize;
                }
                if msg == WM_CLOSE { DestroyWindow(hwnd); PostQuitMessage(0); return 0; }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        let (sender, receiver_channel) = mpsc::channel();
        let worker = std::thread::spawn(move || unsafe {
            let window = Window::with_proc(windows_sys::w!("LucidDeskTransportTimeoutTest"), Some(receiver));
            let hwnd = window.hwnd;
            window.owner(hwnd);
            sender.send(hwnd as usize).unwrap();
            let mut message = MSG::default();
            while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 { DispatchMessageW(&message); }

        });
        unsafe {
            let hwnd = receiver_channel.recv_timeout(Duration::from_secs(5)).unwrap() as HWND;
            let session = std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 0));
            let rejected = session.clear_selection();
            let healthy_after_rejection = session.is_alive();
            let timeout = session.clear_selection();
            let healthy_after_timeout = session.is_alive();
            // Cleanup is still allowed and waits for the delayed receiver to return.
            let detached = session.request(wire::DETACH, &[]);
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            worker.join().unwrap();
            assert!(rejected.unwrap_err().contains("拒绝"));
            assert!(healthy_after_rejection);
            assert!(timeout.unwrap_err().contains("执行结果不确定"));
            assert!(!healthy_after_timeout);
            assert!(detached.is_ok(), "{detached:?}");
        }
    }
    #[test]
    fn accepted_timeout_retires_session_even_with_live_owner_and_late_ack() {
        unsafe extern "system" fn accept(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            unsafe {
                if msg == WM_COPYDATA {
                    if received_request(lp).op == wire::DETACH {
                        RemovePropW(hwnd, OWNER);
                    }
                    MAGIC as isize
                } else { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        unsafe {
            let window = Window::with_proc(windows_sys::w!("LucidDeskAcceptedTimeoutTest"), Some(accept));
            let hwnd = window.hwnd;
            window.owner(hwnd);
            let session = std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 0));
            assert!(session.is_alive());
            let error = session.clear_selection().unwrap_err();
            assert!(error.contains("桌面过滤响应超时; op=2, sequence=1, ack=0"), "{error}");
            assert!(error.contains("owner_match=true"), "{error}");
            assert_eq!(GetPropW(hwnd, OWNER), hwnd, "native ownership alone is not protocol health");
            assert!(!session.is_alive());
            SetPropW(hwnd, ACK, 1usize as _);
            assert!(!session.is_alive(), "a late ACK must not resurrect an ambiguous session");
            let started = Instant::now();
            assert!(session.clear_selection().unwrap_err().contains("等待重新连接"));
            assert!(session.submit_hidden(&[]).is_err());
            let sequence = session.sequence.get();
            assert!(session.begin_update().is_err());
            assert!(session.update.get().is_none(), "rejected update must not create a freeze token");
            assert_eq!(session.sequence.get(), sequence);
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(session.request(wire::DETACH, &[]).is_ok(), "cleanup must still reach Explorer");
            assert!(GetPropW(hwnd, OWNER).is_null());
        }
    }

    #[test]
    fn asynchronous_membership_waits_without_blocking_or_overwriting_ack() {
        unsafe extern "system" fn accept(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            if msg == WM_COPYDATA { MAGIC as isize }
            else { unsafe { DefWindowProcW(hwnd, msg, wp, lp) } }
        }
        unsafe {
            let window = Window::with_proc(windows_sys::w!("LucidDeskAsyncMembershipTest"), Some(accept));
            let hwnd = window.hwnd;
            window.owner(hwnd);
            let session = std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 0));
            let names: Vec<_> = (0..100).map(|i| format!("test:{i}")).collect();
            session.submit_hidden(&names).unwrap();
            let sequence = session.pending_hidden.get().unwrap().sequence;
            for _ in 0..100 { assert!(!session.poll_hidden().unwrap()); }
            assert!(session.submit_hidden(&[]).is_err());
            assert!(session.clear_selection().is_err(), "must preserve outstanding ACK");
            let next_sequence = session.sequence.get();
            assert!(session.begin_update().is_err());
            assert!(session.update.get().is_none(), "busy rejection must not create a freeze token");
            assert_eq!(session.sequence.get(), next_sequence);
            assert!(GetPropW(hwnd, UPDATE_RELEASE).is_null());
            SetPropW(hwnd, ACK, sequence as usize as _);
            assert!(session.poll_hidden().unwrap());
            session.submit_hidden(&[]).unwrap();
            let sequence = session.pending_hidden.get().unwrap().sequence;
            SetPropW(hwnd, REQUEST_ERROR, 123usize as _);
            SetPropW(hwnd, ACK, sequence as usize as _);
            assert!(session.poll_hidden().is_err());
            assert!(session.pending_hidden.get().is_none(), "errors must permit retry");
            RemovePropW(hwnd, REQUEST_ERROR);
            session.submit_hidden(&names).unwrap();
            RemovePropW(hwnd, OWNER);
            assert!(session.poll_hidden().is_err());
            window.owner(hwnd);
            session.submit_hidden(&names).unwrap();
            let sequence = session.pending_hidden.get().unwrap().sequence;
            session.pending_hidden.set(Some(PendingHidden { sequence, started: Instant::now() - Duration::from_secs(4) }));
            SetPropW(hwnd, ACK, sequence as usize as _);
            assert_eq!(session.poll_hidden(), Ok(true), "an available ACK wins over delayed polling");
            session.submit_hidden(&names).unwrap();
            let sequence = session.pending_hidden.get().unwrap().sequence;
            session.pending_hidden.set(Some(PendingHidden { sequence, started: Instant::now() - Duration::from_secs(4) }));
            assert!(session.poll_hidden().unwrap_err().contains("确认超时"));
            assert!(!session.is_alive());
            SetPropW(hwnd, ACK, sequence as usize as _);
            assert!(!session.is_alive(), "late ACK cannot revive a timed-out session");
            assert!(session.submit_hidden(&names).is_err());
        }
    }

    #[test]
    fn membership_wait_services_sent_callbacks_without_running_posted_actions() {
        const WORK: u32 = WM_APP + 77;
        const ACTION: u32 = WM_APP + 78;
        unsafe extern "system" fn peer(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            unsafe {
                match msg {
                    WM_COPYDATA => {
                        let request = received_request(lp);
                        PostMessageW(hwnd, WORK, request.sequence as usize, wp as isize);
                        MAGIC as isize
                    }
                    WORK => {
                        // Ensure the original SendMessageTimeout has returned.
                        std::thread::sleep(Duration::from_millis(30));
                        let mut answer = 0;
                        if SendMessageTimeoutW(lp as HWND, WM_NULL, 0, 0, SMTO_ABORTIFHUNG,
                            1000, &raw mut answer) != 0 {
                            SetPropW(hwnd, ACK, wp as _);
                        }
                        0
                    }
                    WM_CLOSE => { DestroyWindow(hwnd); PostQuitMessage(0); 0 }
                    _ => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
        }
        unsafe {
            let owner_window = Window::new();
            let owner = owner_window.hwnd;
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let window = Window::with_proc(windows_sys::w!("LucidDeskMembershipCallbackTest"), Some(peer));
                let hwnd = window.hwnd;
                sender.send(hwnd as usize).unwrap();
                let mut message = MSG::default();
                while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 {
                    DispatchMessageW(&message);
                }
            });
            let view = receiver.recv_timeout(Duration::from_secs(2)).unwrap() as HWND;
            SetPropW(view, OWNER, owner);
            PostMessageW(owner, ACTION, 0, 0);
            let session = std::mem::ManuallyDrop::new(test_session(view, owner, 0));
            let result = session.set_hidden(&["test:item".into()]);
            let mut message = MSG::default();
            let queued = PeekMessageW(&raw mut message, owner, ACTION, ACTION, PM_REMOVE);
            PostMessageW(view, WM_CLOSE, 0, 0);
            worker.join().unwrap();
            drop(owner_window);
            assert!(result.is_ok(), "sent callback must not deadlock ACK: {result:?}");
            assert_ne!(queued, 0, "posted application actions must remain queued");
        }
    }

    #[test]
    fn accepted_request_aborts_when_connection_disappears_before_ack() {
        unsafe extern "system" fn disconnected(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            if msg == WM_COPYDATA {
                unsafe { RemovePropW(hwnd, OWNER); }
                MAGIC as isize
            } else { unsafe { DefWindowProcW(hwnd, msg, wp, lp) } }
        }
        let window = Window::with_proc(windows_sys::w!("LucidDeskDisconnectBeforeAckTest"), Some(disconnected));
        let hwnd = window.hwnd;
        let session = std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 0));
        window.owner(hwnd);
        let started = Instant::now();
        assert!(session.set_hidden(&[]).unwrap_err().contains("已断开"));
        assert!(started.elapsed() < Duration::from_secs(1));
        window.owner(hwnd);
        assert!(session.request(wire::DETACH, &[]).is_ok(), "detaching is successful once ownership is removed");
    }
    #[test]
    fn rejected_release_keeps_token_and_can_be_acknowledged_outside_ipc() {
        unsafe {
            let window = Window::new();
            let hwnd = window.hwnd;
            window.owner(hwnd);
            // STATIC rejects WM_COPYDATA, so this exercises a real transport
            // rejection without injecting into the user's Explorer.
            let session = std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 42));
            session.update.set(Some(42));
            assert!(session.finish_update().is_err());
            assert_eq!(GetPropW(hwnd, UPDATE_RELEASE) as usize, 42);
            assert_eq!(session.update.get(), Some(42));
            assert!(session.begin_update().is_err());
            assert_eq!(
                session.update.get(),
                Some(42),
                "must not start a new freeze before release"
            );
            SetPropW(hwnd, UPDATE_RELEASED, 42usize as _);
            session.finish_update().unwrap();
            assert_eq!(session.update.get(), None);
        }
    }
}
