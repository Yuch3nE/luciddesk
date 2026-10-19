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

/// Owns one desktop membership session. Dropping it restores removed view items.
pub struct FilterSession {
    view: HWND,
    owner: HWND,
    hook: HHOOK,
    module: HMODULE,
    sequence: Cell<u32>,
    timed_out: Cell<bool>,
    in_flight: Cell<bool>,
    pending_hidden: Cell<Option<u32>>,
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
    let started = Instant::now();
    let mut next_discovery = Duration::ZERO;
    while started.elapsed() < Duration::from_secs(3) {
        if !target.alive() || unsafe { IsWindow(owner) } == 0 {
            return Err("握手期间桌面或控制窗口已失效，将重新定位".into());
        }
        if started.elapsed() >= next_discovery {
            if !target.current(crate::desktop_view().ok()) {
                return Err("握手期间桌面视图已被替换，将重新定位".into());
            }
            next_discovery = started.elapsed() + Duration::from_millis(50);
        }
        unsafe {
            if GetPropW(target.view, OWNER) == owner { return Ok(()); }
            let error = GetPropW(target.view, ERROR) as usize as u32;
            if error != 0 { return Err(format!("桌面过滤连接失败 (0x{error:08x}); {}", diagnostics::summary(target.view))); }
        }
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
                timed_out: Cell::new(false),
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
            !self.timed_out.get()
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
        if self.timed_out.get() {
            return Err("桌面过滤连接已超时，等待重新连接".into());
        }
        if self.pending_hidden.get().is_some() {
            return Err("桌面分组同步尚未完成".into());
        }
        let sequence = self.next_sequence();
        let bytes = wire::encode(wire::SET, sequence, names)?;
        self.transmit(&bytes)?;
        self.pending_hidden.set(Some(sequence));
        Ok(())
    }

    /// Returns false while Explorer is still processing the submitted set.
    pub fn poll_hidden(&self) -> Result<bool, String> {
        let Some(sequence) = self.pending_hidden.get() else { return Ok(true); };
        unsafe {
            if !self.is_alive() {
                self.pending_hidden.set(None);
                return Err(format!("桌面过滤连接已断开; {}", diagnostics::summary(self.view)));
            }
            if GetPropW(self.view, ACK) as usize != sequence as usize {
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
        if self.timed_out.get() {
            return Err("桌面过滤连接已超时，等待重新连接".into());
        }
        if self.pending_hidden.get().is_some() {
            return Err("桌面分组同步尚未完成，请稍后重试".into());
        }
        if self.update.get().is_some() {
            if unsafe { GetPropW(self.view, UPDATE_RELEASE) as usize }
                != self.update.get().unwrap() as usize
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
            if SendMessageTimeoutW(
                self.view,
                WM_COPYDATA,
                self.owner as usize,
                (&raw const data) as isize,
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                1500,
                &raw mut accepted,
            ) == 0
                || accepted != MAGIC
            {
                return Err("Explorer 拒绝桌面过滤请求".into());
            }
        }
        Ok(())
    }
    fn send(&self, op: u32, sequence: u32, bytes: &[u8]) -> Result<(), String> {
        let _request = self.enter_request()?;
        // An accepted request without an ACK has an unknown outcome. Retire
        // this session instead of treating a live HWND as a healthy protocol.
        // Cleanup must remain possible before the supervisor reconnects.
        if self.timed_out.get() && !matches!(op,
            wire::DETACH | wire::UPDATE_END | wire::MENU_FINISH | wire::MENU_CANCEL)
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
            let timeout = Duration::from_secs(if wire::is_menu_transaction(op) { 5 } else { 3 });
            while started.elapsed() < timeout {
                if op == wire::DETACH && GetPropW(self.view, OWNER).is_null() {
                    return Ok(());
                }
                if IsWindow(self.view) == 0 || GetPropW(self.view, OWNER) != self.owner {
                    return Err("等待确认期间桌面过滤连接已断开".into());
                }
                if GetPropW(self.view, ACK) as usize == sequence as usize {
                    if wire::is_menu_transaction(op) {
                        let error = GetPropW(self.view, MENU_ERROR) as usize as u32;
                        return if error == 0 {
                            Ok(())
                        } else {
                            Err(format!("独立原生菜单不可用 (0x{error:08x})"))
                        };
                    }
                    let error = GetPropW(self.view, ERROR) as usize as u32;
                    return if error == 0 {
                        let retry = GetPropW(self.view, REQUEST_ERROR) as usize as u32;
                        if retry == 0 {
                            Ok(())
                        } else {
                            Err(format!(
                                "桌面过滤请求未完成 (0x{retry:08x}); {}", diagnostics::summary(self.view)
                            ))
                        }
                    } else {
                        Err(format!("桌面过滤失败，已请求恢复原生项目 (0x{error:08x}); {}", diagnostics::summary(self.view)))
                    };
                }
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
            let description = if wire::is_menu_transaction(op) {
                "Explorer 菜单准备或释放超时"
            } else {
                self.timed_out.set(true);
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
#[cfg(test)]
mod attach_tests {
    use super::*;
    struct Window(HWND);
    impl Window {
        fn new() -> Self {
            let hwnd = unsafe { CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("attach target test"),
                WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), null_mut(), null_mut()) };
            assert!(!hwnd.is_null());
            Self(hwnd)
        }
    }
    impl Drop for Window { fn drop(&mut self) { unsafe { DestroyWindow(self.0); } } }
    #[test]
    fn native_attach_errors_keep_stage_and_hresult() {
        let view = Window::new();
        for stage in [diagnostics::Stage::LoadLibrary, diagnostics::Stage::Export,
            diagnostics::Stage::Hook, diagnostics::Stage::PostAttach] {
            unsafe { windows_sys::Win32::Foundation::SetLastError(5); }
            let error = attach_os_error(view.0, stage);
            assert!(error.contains("HRESULT=0x80070005"));
            assert!(error.contains(&format!("({})", stage as usize)));
        }
        unsafe { windows_sys::Win32::Foundation::SetLastError(0); }
        assert!(attach_os_error(view.0, diagnostics::Stage::Hook).contains("HRESULT=0x80004005"));
    }
    #[test]
    fn destroyed_attach_target_aborts_without_waiting_for_timeout() {
        let view = Window::new();
        let owner = Window::new();
        let target = AttachTarget::capture(view.0).unwrap();
        drop(view);
        let started = Instant::now();
        assert!(wait_for_attach(&target, owner.0).unwrap_err().contains("已失效"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn current_target_rejects_replacement_missing_view_and_changed_parent() {
        let view = Window::new();
        let replacement = Window::new();
        let target = AttachTarget::capture(view.0).unwrap();
        assert!(target.current(Some(view.0 as isize)));
        assert!(!target.current(Some(replacement.0 as isize)));
        assert!(!target.current(None));
        unsafe {
            SetWindowLongPtrW(view.0, GWL_STYLE, WS_CHILD as isize);
            SetParent(view.0, replacement.0);
            assert_eq!(GetParent(view.0), replacement.0);
        }
        assert!(!target.alive());
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
mod update_tests {
    use super::*;
    #[test]
    fn accepted_timeout_retires_session_even_with_live_owner_and_late_ack() {
        unsafe extern "system" fn accept(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            unsafe {
                if msg == WM_COPYDATA {
                    let data = &*(lp as *const COPYDATASTRUCT);
                    let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                    if wire::decode(bytes).unwrap().op == wire::DETACH {
                        RemovePropW(hwnd, OWNER);
                    }
                    MAGIC as isize
                } else { DefWindowProcW(hwnd, msg, wp, lp) }
            }
        }
        unsafe {
            let class = windows_sys::w!("LucidDeskAcceptedTimeoutTest");
            let instance = GetModuleHandleW(null_mut());
            assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(accept), hInstance: instance,
                lpszClassName: class, ..Default::default() }), 0);
            let hwnd = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                null_mut(), null_mut(), instance, null_mut());
            assert!(!hwnd.is_null());
            SetPropW(hwnd, OWNER, hwnd);
            let session = std::mem::ManuallyDrop::new(FilterSession {
                view: hwnd, owner: hwnd, hook: null_mut(), module: null_mut(),
                sequence: Cell::new(0), timed_out: Cell::new(false), in_flight: Cell::new(false), pending_hidden: Cell::new(None),
                update: Cell::new(None), explorer_watch: None,
            });
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
            DestroyWindow(hwnd);
            UnregisterClassW(class, instance);
        }
    }

    #[test]
    fn asynchronous_membership_waits_without_blocking_or_overwriting_ack() {
        unsafe extern "system" fn accept(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
            if msg == WM_COPYDATA { MAGIC as isize }
            else { unsafe { DefWindowProcW(hwnd, msg, wp, lp) } }
        }
        unsafe {
            let class = windows_sys::w!("LucidDeskAsyncMembershipTest");
            let instance = GetModuleHandleW(null_mut());
            assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(accept), hInstance: instance,
                lpszClassName: class, ..Default::default() }), 0);
            let hwnd = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                null_mut(), null_mut(), instance, null_mut());
            assert!(!hwnd.is_null());
            SetPropW(hwnd, OWNER, hwnd);
            let session = std::mem::ManuallyDrop::new(FilterSession { timed_out: Cell::new(false), in_flight: Cell::new(false),
                view: hwnd, owner: hwnd, hook: null_mut(), module: null_mut(),
                sequence: Cell::new(0), pending_hidden: Cell::new(None),
                update: Cell::new(None), explorer_watch: None,
            });
            let names: Vec<_> = (0..100).map(|i| format!("test:{i}")).collect();
            session.submit_hidden(&names).unwrap();
            let sequence = session.pending_hidden.get().unwrap();
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
            let sequence = session.pending_hidden.get().unwrap();
            SetPropW(hwnd, REQUEST_ERROR, 123usize as _);
            SetPropW(hwnd, ACK, sequence as usize as _);
            assert!(session.poll_hidden().is_err());
            assert!(session.pending_hidden.get().is_none(), "errors must permit retry");
            RemovePropW(hwnd, REQUEST_ERROR);
            session.submit_hidden(&names).unwrap();
            RemovePropW(hwnd, OWNER);
            assert!(session.poll_hidden().is_err());
            DestroyWindow(hwnd);
            UnregisterClassW(class, instance);
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
                        let data = &*(lp as *const COPYDATASTRUCT);
                        let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                        let request = wire::decode(bytes).unwrap();
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
            let owner = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!(""),
                WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), null_mut(), null_mut());
            assert!(!owner.is_null());
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let class = windows_sys::w!("LucidDeskMembershipCallbackTest");
                let instance = GetModuleHandleW(null_mut());
                assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(peer), hInstance: instance,
                    lpszClassName: class, ..Default::default() }), 0);
                let hwnd = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                    null_mut(), null_mut(), instance, null_mut());
                sender.send(hwnd as usize).unwrap();
                let mut message = MSG::default();
                while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 {
                    DispatchMessageW(&message);
                }
                UnregisterClassW(class, instance);
            });
            let view = receiver.recv_timeout(Duration::from_secs(2)).unwrap() as HWND;
            SetPropW(view, OWNER, owner);
            PostMessageW(owner, ACTION, 0, 0);
            let session = std::mem::ManuallyDrop::new(FilterSession { timed_out: Cell::new(false), in_flight: Cell::new(false),
                view, owner, hook: null_mut(), module: null_mut(), sequence: Cell::new(0), pending_hidden: Cell::new(None),
                update: Cell::new(None), explorer_watch: None,
            });
            let result = session.set_hidden(&["test:item".into()]);
            let mut message = MSG::default();
            let queued = PeekMessageW(&raw mut message, owner, ACTION, ACTION, PM_REMOVE);
            PostMessageW(view, WM_CLOSE, 0, 0);
            worker.join().unwrap();
            DestroyWindow(owner);
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
        unsafe {
            let class = windows_sys::w!("LucidDeskDisconnectBeforeAckTest");
            let instance = GetModuleHandleW(null_mut());
            assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(disconnected), hInstance: instance,
                lpszClassName: class, ..Default::default() }), 0);
            let hwnd = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                null_mut(), null_mut(), instance, null_mut());
            assert!(!hwnd.is_null());
            let session = std::mem::ManuallyDrop::new(FilterSession { timed_out: Cell::new(false), in_flight: Cell::new(false),
                view: hwnd, owner: hwnd, hook: null_mut(), module: null_mut(),
                sequence: Cell::new(0), pending_hidden: Cell::new(None), update: Cell::new(None), explorer_watch: None,
            });
            SetPropW(hwnd, OWNER, hwnd);
            let started = Instant::now();
            assert!(session.set_hidden(&[]).unwrap_err().contains("已断开"));
            assert!(started.elapsed() < Duration::from_secs(1));
            SetPropW(hwnd, OWNER, hwnd);
            assert!(session.request(wire::DETACH, &[]).is_ok(), "detaching is successful once ownership is removed");
            DestroyWindow(hwnd);
            UnregisterClassW(class, instance);
        }
    }
    #[test]
    fn rejected_release_keeps_token_and_can_be_acknowledged_outside_ipc() {
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            SetPropW(hwnd, OWNER, hwnd);
            // STATIC rejects WM_COPYDATA, so this exercises a real transport
            // rejection without injecting into the user's Explorer.
            let session = std::mem::ManuallyDrop::new(FilterSession { timed_out: Cell::new(false), in_flight: Cell::new(false),
                view: hwnd,
                owner: hwnd,
                hook: null_mut(),
                module: null_mut(),
                sequence: Cell::new(42), pending_hidden: Cell::new(None),
                update: Cell::new(Some(42)),
                explorer_watch: None,
            });
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
            DestroyWindow(hwnd);
        }
    }
}
