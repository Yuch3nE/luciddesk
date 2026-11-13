//! One Explorer STA per filter session. Reuse idle menu hosts and keep command
//! dialogs alive when changing targets; all Shell objects stay on this thread.
use super::{super::wire::MenuContext, MenuHost, selection::ResolvedTargets};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows::{Win32::Foundation::E_FAIL, core::Result};
use windows_sys::Win32::{
    System::{Com::*, LibraryLoader::*, Threading::*},
    UI::WindowsAndMessaging::*,
};

type Reply<T> = mpsc::SyncSender<std::result::Result<T, i32>>;
enum Request {
    Prepare {
        names: Vec<String>,
        context: MenuContext,
        reply: Reply<isize>,
        cancelled: Arc<AtomicBool>,
    },
    Finish,
    Cancel {
        reply: Reply<()>,
    },
    Shutdown,
}
pub struct Worker {
    requests: mpsc::Sender<Request>,
    signal: Arc<RequestSignal>,
    thread: std::thread::JoinHandle<()>,
    active: RefCell<Option<ActiveInvocation>>,
}

// The worker owns a reference until its wait loop exits; closing a HANDLE
// while another thread is waiting on it would be undefined behavior.
struct RequestSignal(isize);
impl RequestSignal {
    fn new() -> Result<Arc<Self>> {
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(Arc::new(Self(handle as isize)))
    }
    fn notify(&self) {
        unsafe { SetEvent(self.0 as _); }
    }
}
impl Drop for RequestSignal {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _); }
    }
}

struct ActiveInvocation {
    cancelled: Arc<AtomicBool>,
    hwnd: isize,
}
impl ActiveInvocation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if self.hwnd != 0 {
            unsafe { PostMessageW(self.hwnd as _, WM_CANCELMODE, 0, 0); }
        }
    }
}
impl Worker {
    pub fn create(desktop: windows_sys::Win32::Foundation::HWND) -> Result<Self> {
        let desktop = desktop as isize;
        let (requests, receiver) = mpsc::channel();
        let signal = RequestSignal::new()?;
        let worker_signal = signal.clone();
        let thread = std::thread::Builder::new()
            .name("LucidDesk native menu".into())
            .spawn(move || unsafe {
                let initialized = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
                if initialized < 0 {
                    return;
                }
                let mut current: Option<MenuHost> = None;
                let mut retired: Vec<MenuHost> = Vec::new();
                let mut last_cleanup = Instant::now();
                'worker: loop {
                    loop {
                        match receiver.try_recv() {
                            Ok(Request::Prepare {
                                names,
                                context,
                                reply,
                                cancelled,
                            }) => {
                                if cancelled.load(Ordering::Acquire) {
                                    continue;
                                }
                                let result = prepare_host(
                                    desktop as _,
                                    &mut current,
                                    &mut retired,
                                    &names,
                                    context,
                                    &cancelled,
                                )
                                .map_err(|error| error.code().0);
                                if cancelled.load(Ordering::Acquire) {
                                    if let Some(host) = &current {
                                        let _ = host.finish(true);
                                    }
                                    continue;
                                }
                                let _ = reply.send(result);
                            }
                            Ok(Request::Finish) => {
                                if let Some(host) = &current {
                                    let _ = host.finish(false);
                                }
                            }
                            Ok(Request::Cancel { reply }) => {
                                let result =
                                    current.as_ref().map_or(Ok(()), |host| host.finish(true));
                                let _ = reply.send(result.map_err(|e| e.code().0));
                            }
                            Ok(Request::Shutdown) => break 'worker,
                            Err(mpsc::TryRecvError::Disconnected) => break 'worker,
                            Err(mpsc::TryRecvError::Empty) => break,
                        }
                    }
                    if !pump_messages() {
                        break;
                    }
                    if !retired.is_empty() && last_cleanup.elapsed() >= Duration::from_secs(1) {
                        retired.retain(MenuHost::has_owned_windows);
                        last_cleanup = Instant::now();
                    }
                    let timeout = if retired.is_empty() {
                        INFINITE
                    } else {
                        Duration::from_secs(1).saturating_sub(last_cleanup.elapsed())
                            .as_millis().max(1) as u32
                    };
                    let handles = [worker_signal.0 as _];
                    if MsgWaitForMultipleObjectsEx(1, handles.as_ptr(), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
                        == windows_sys::Win32::Foundation::WAIT_FAILED
                    {
                        break;
                    }
                }
                drop(current);
                drop(retired);
                CoUninitialize();
                // Also covers a class registered before a failed host creation.
                super::unregister_host_class();
            })
            .map_err(|error| windows::core::Error::new(E_FAIL, error.to_string()))?;
        super::super::library::keep_thread(&thread)?;
        Ok(Self {
            requests,
            signal,
            thread,
            active: RefCell::new(None),
        })
    }
    pub fn is_alive(&self) -> bool {
        !self.thread.is_finished()
    }
    fn send(&self, request: Request) -> Result<()> {
        self.requests.send(request).map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
        self.signal.notify();
        Ok(())
    }
    pub fn prepare(&self, names: &[String], context: MenuContext) -> Result<isize> {
        let (reply, result) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.active.borrow_mut() = Some(ActiveInvocation {
            cancelled: cancelled.clone(),
            hwnd: 0,
        });
        self.send(Request::Prepare {
                names: names.to_vec(),
                context,
                reply,
                cancelled: cancelled.clone(),
            })?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match result.try_recv() {
                Ok(Ok(hwnd)) => {
                    if let Some(active) = self.active.borrow_mut().as_mut() {
                        active.hwnd = hwnd;
                    }
                    return Ok(hwnd);
                }
                Ok(Err(code)) => return Err(windows::core::HRESULT(code).into()),
                Err(mpsc::TryRecvError::Disconnected) => return Err(E_FAIL.into()),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if Instant::now() >= deadline {
                cancelled.store(true, Ordering::Release);
                return Err(windows::core::Error::new(E_FAIL, "原生菜单准备超时"));
            }
            wait_for_shell_reply();
        }
    }
    pub fn finish(&self, cancel: bool) -> Result<()> {
        // Properties may still own a modal loop on the Shell STA. Normal
        // dismissal only queues cleanup and never allocates an unused reply.
        if !cancel {
            return self.send(Request::Finish);
        }
        if let Some(active) = self.active.borrow().as_ref() {
            active.cancel();
        }
        let (reply, result) = mpsc::sync_channel(1);
        self.send(Request::Cancel { reply })?;
        // Return only after the menu STA has dismissed/closed the presenter.
        // If an extension stalls, its atomic token still prevents later display
        // and command invocation; never forcibly terminate Explorer's thread.
        let start = Instant::now();
        loop {
            match result.try_recv() {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(code)) => return Err(windows::core::HRESULT(code).into()),
                Err(mpsc::TryRecvError::Disconnected) => return Err(E_FAIL.into()),
                _ => {}
            }
            if start.elapsed() >= Duration::from_secs(2) {
                return Err(windows::core::Error::new(
                    E_FAIL,
                    "菜单取消等待超时，已保留取消标记",
                ));
            }
            wait_for_shell_reply();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(active) = self.active.get_mut() {
            active.cancel();
        }
        let _ = self.send(Request::Shutdown);
    }
}

/// Runs only on the menu STA; resolve before replacing a useful host and keep
/// hosts with outstanding command dialogs alive until the periodic cleanup.
fn prepare_host(
    desktop: windows_sys::Win32::Foundation::HWND,
    current: &mut Option<MenuHost>,
    retired: &mut Vec<MenuHost>,
    names: &[String],
    context: MenuContext,
    cancelled: &Arc<AtomicBool>,
) -> Result<isize> {
    if current.as_ref().is_some_and(MenuHost::is_busy) {
        return Err(
            windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_BUSY.0).into(),
        );
    }
    let targets = ResolvedTargets::resolve(names)?;
    if let Some(host) = current.as_mut() {
        // A transient presenter failure must not pin the session to classic
        // menus forever. Keep the fallback for this invocation, then retry
        // creation on the next request. Healthy compact hosts remain cached.
        if host.presenter.is_some() && host.reprepare(&targets, context, cancelled.clone())? {
            return host.view_hwnd();
        }
    }
    // Tear down an idle presenter before initializing its replacement.
    if let Some(previous) = current.take() {
        if previous.has_owned_windows() {
            retired.push(previous);
        }
    }
    let next = MenuHost::create(desktop, &targets, context, cancelled.clone())?;
    let hwnd = next.view_hwnd()?;
    *current = Some(next);
    Ok(hwnd)
}

/// Wait on the desktop thread while filter state is borrowed. Service only
/// synchronous COM calls; dispatching posted filter requests would reenter it.
fn wait_for_shell_reply() {
    unsafe {
        let mut msg = MSG::default();
        PeekMessageW(
            &raw mut msg,
            std::ptr::null_mut(),
            WM_NULL,
            WM_NULL,
            PM_NOREMOVE,
        );
        MsgWaitForMultipleObjectsEx(0, std::ptr::null(), 5, QS_SENDMESSAGE, MWMO_INPUTAVAILABLE);
    }
}

// Runs on the independent menu STA, where WinUI and posted input must dispatch.
fn pump_messages() -> bool {
    unsafe {
        let mut msg = MSG::default();
        for _ in 0..32 {
            if PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            if msg.message == WM_QUIT {
                return false;
            }
            let module = GetModuleHandleW(windows_sys::w!("Microsoft.UI.Windowing.Core.dll"));
            let consumed = GetProcAddress(module, windows_sys::s!("ContentPreTranslateMessage"))
                .is_some_and(|proc| {
                    let translate: unsafe extern "system" fn(*const MSG) -> i32 =
                        std::mem::transmute(proc);
                    translate(&msg) != 0
                });
            if !consumed {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "creates a native Shell view; run separately outside the restricted test sandbox"]
    fn fallback_host_is_replaced_on_the_next_request() {
        unsafe {
            windows::Win32::System::Ole::OleInitialize(None).unwrap();
            let owner = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!(""),
                WS_POPUP, 0, 0, 640, 480, std::ptr::null_mut(), std::ptr::null_mut(),
                std::ptr::null_mut(), std::ptr::null());
            assert!(!owner.is_null());
            let path = std::env::temp_dir().join(format!("luciddesk-menu-retry-{}.txt", std::process::id()));
            std::fs::write(&path, b"owned menu retry fixture").unwrap();
            {
                let names = vec![path.to_string_lossy().into_owned()];
                let targets = ResolvedTargets::resolve(&names).unwrap();
                let context = MenuContext { owner: owner as u64, x: 20, y: 20 };
                let cancelled = Arc::new(AtomicBool::new(false));
                let fallback = MenuHost::create_impl(owner, &targets, context, cancelled.clone(), false).unwrap();
                let marker = windows_sys::w!("LucidDesk.Test.FallbackHost");
                assert_ne!(SetPropW(fallback.view_hwnd().unwrap() as _, marker, 1usize as _), 0);
                let mut current = Some(fallback);
                let mut retired = Vec::new();
                let hwnd = prepare_host(owner, &mut current, &mut retired, &names, context, &cancelled).unwrap();
                assert!(GetPropW(hwnd as _, marker).is_null(), "a fallback host must not remain cached");
                assert!(retired.is_empty());
                assert_eq!(current.as_ref().unwrap().names, names);
            }
            std::fs::remove_file(path).unwrap();
            DestroyWindow(owner);
            windows::Win32::System::Ole::OleUninitialize();
        }
    }

    #[test]
    fn idle_worker_wakes_for_requests_and_shutdown() {
        let worker = Worker::create(std::ptr::null_mut()).unwrap();
        worker.finish(true).unwrap();
        worker.send(Request::Shutdown).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.is_alive() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!worker.is_alive());
    }

    #[test]
    fn dropping_worker_signals_shutdown_without_waiting_for_a_timeout() {
        let (requests, receiver) = mpsc::channel();
        let signal = RequestSignal::new().unwrap();
        let waiting = signal.clone();
        let (done, completion) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            assert_eq!(unsafe { WaitForSingleObject(waiting.0 as _, 3000) }, 0);
            assert!(matches!(receiver.recv().unwrap(), Request::Shutdown));
            done.send(()).unwrap();
        });
        drop(Worker { requests, signal, thread, active: RefCell::new(None) });
        completion.recv_timeout(Duration::from_secs(3)).unwrap();
    }

    #[test]
    fn modal_worker_does_not_block_normal_finish_and_cancel_propagates_close_failure() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (requests, receiver) = mpsc::channel();
        let (unblock, modal) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            // Simulate a Properties dialog keeping the Shell worker occupied.
            modal.recv().unwrap();
            assert!(matches!(receiver.recv().unwrap(), Request::Finish));
            let Request::Cancel { reply } = receiver.recv().unwrap() else {
                panic!("Expected a cancellation acknowledgement request");
            };
            reply
                .send(Err(windows::Win32::Foundation::E_ACCESSDENIED.0))
                .unwrap();
        });
        let worker = Worker {
            requests,
            signal: RequestSignal::new().unwrap(),
            thread,
            active: RefCell::new(Some(ActiveInvocation { cancelled: cancelled.clone(), hwnd: 0 })),
        };
        worker.finish(false).unwrap();
        unblock.send(()).unwrap();
        assert_eq!(
            worker.finish(true).unwrap_err().code(),
            windows::Win32::Foundation::E_ACCESSDENIED
        );
        assert!(cancelled.load(Ordering::Acquire));
        drop(worker);
        assert!(cancelled.load(Ordering::Acquire), "retiring a failed worker must not revive late commands");
    }
}
