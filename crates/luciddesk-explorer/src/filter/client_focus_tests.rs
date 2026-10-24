//! Reproduce a sent focus callback whose sender also services filter requests.
use super::*;

const START: u32 = WM_APP + 91;
const COMPLETE: u32 = WM_APP + 92;
const DEFERRED: u32 = WM_APP + 93;

struct Context {
    session: std::mem::ManuallyDrop<FilterSession>,
    deferred: bool,
    result: std::cell::RefCell<Option<Result<(), String>>>,
}

unsafe extern "system" fn controller(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        if matches!(msg, WM_SETFOCUS | DEFERRED) {
            let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context;
            if let Some(context) = context.as_ref() {
                if msg == WM_SETFOCUS && context.deferred {
                    PostMessageW(hwnd, DEFERRED, 0, 0);
                } else {
                    let result = context.session.clear_selection();
                    *context.result.borrow_mut() = Some(result);
                }
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn explorer(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        match msg {
            START => {
                // While waiting for the focus callback, Windows can dispatch
                // sent WM_COPYDATA, but not our posted COMPLETE message.
                let mut result = 0;
                SendMessageTimeoutW(lp as HWND, WM_SETFOCUS, 0, 0,
                    SMTO_ABORTIFHUNG, 5000, &raw mut result);
                0
            }
            WM_COPYDATA => {
                let data = &*(lp as *const COPYDATASTRUCT);
                let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
                let request = wire::decode(bytes).unwrap();
                PostMessageW(hwnd, COMPLETE, request.sequence as usize, 0);
                MAGIC as isize
            }
            COMPLETE => { SetPropW(hwnd, ACK, wp as _); 0 }
            WM_CLOSE => { DestroyWindow(hwnd); PostQuitMessage(0); 0 }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

#[test]
fn sent_focus_callback_times_out_inline_but_succeeds_when_deferred() {
    unsafe {
        let class = windows_sys::w!("LucidDeskFocusControllerTest");
        let instance = GetModuleHandleW(null_mut());
        assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(controller), hInstance: instance,
            lpszClassName: class, ..Default::default() }), 0);
        let mut outcomes = Vec::new();
        for deferred in [false, true] {
            let owner = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                null_mut(), null_mut(), instance, null_mut());
            assert!(!owner.is_null());
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let class = windows_sys::w!("LucidDeskFocusExplorerTest");
                let instance = GetModuleHandleW(null_mut());
                assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(explorer), hInstance: instance,
                    lpszClassName: class, ..Default::default() }), 0);
                let view = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                    null_mut(), null_mut(), instance, null_mut());
                assert!(!view.is_null());
                sender.send(view as usize).unwrap();
                let mut message = MSG::default();
                while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 {
                    DispatchMessageW(&message);
                }
                UnregisterClassW(class, instance);
            });
            let view = receiver.recv_timeout(Duration::from_secs(2)).unwrap() as HWND;
            SetPropW(view, OWNER, owner);
            let context = Box::new(Context {
                session: std::mem::ManuallyDrop::new(FilterSession {
                    view, owner, hook: null_mut(), module: null_mut(),
                    sequence: Cell::new(0), timed_out: Cell::new(false), in_flight: Cell::new(false),
                    pending_hidden: Cell::new(None), hidden_started: Cell::new(None), update: Cell::new(None), explorer_watch: None,
                }),
                deferred, result: std::cell::RefCell::new(None),
            });
            SetWindowLongPtrW(owner, GWLP_USERDATA, (&raw const *context) as isize);
            let started = Instant::now();
            PostMessageW(view, START, 0, owner as isize);
            while context.result.borrow().is_none() && started.elapsed() < Duration::from_secs(6) {
                let mut message = MSG::default();
                if PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    DispatchMessageW(&message);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            outcomes.push((context.result.borrow_mut().take(), started.elapsed()));
            PostMessageW(view, WM_CLOSE, 0, 0);
            worker.join().unwrap();
            SetWindowLongPtrW(owner, GWLP_USERDATA, 0);
            DestroyWindow(owner);
        }
        UnregisterClassW(class, instance);
        let error = outcomes[0].0.as_ref().unwrap().as_ref().unwrap_err();
        assert!(error.contains("桌面过滤响应超时"));
        assert!(error.contains("in_send=0x1"), "must reproduce inside a sent callback: {error}");
        assert!(outcomes[1].0.as_ref().unwrap().is_ok(), "{:?}", outcomes[1]);
        assert!(outcomes[1].1 < Duration::from_secs(1), "deferred request must not rely on timeout recovery");
    }
}
