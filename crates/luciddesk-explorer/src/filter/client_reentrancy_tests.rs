//! Synthetic callbacks verify the single-request protocol boundary.
//! They do not establish that normal app actions bypass its menu/sync guards.
use super::*;

struct Context {
    session: std::mem::ManuallyDrop<FilterSession>,
    entered: Cell<bool>,
    rejected: Cell<bool>,
    requests: Cell<usize>,
    queued: bool,
}

const CALLBACK: u32 = WM_APP + 94;

fn reenter(context: &Context) {
    if !context.entered.replace(true) {
        let before = context.session.sequence.get();
        let clear = context.session.clear_selection();
        let submit = context.session.submit_hidden(&[]);
        let update = context.session.begin_update();
        let menu = context.session.prepare_menu(0, &[], 0, 0);
        let pause = context.session.pause(true);
        context.rejected.set(clear.is_err() && submit.is_err() && update.is_err()
            && menu.is_err() && pause.is_err()
            && context.session.update.get().is_none()
            && context.session.sequence.get() == before);
    }
}

unsafe extern "system" fn peer(hwnd: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe {
        if msg == CALLBACK {
            let context = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context);
            reenter(context);
            SetPropW(hwnd, ACK, wp as _);
            return 0;
        }
        if msg != WM_COPYDATA { return DefWindowProcW(hwnd, msg, wp, lp); }
        let context = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Context);
        context.requests.set(context.requests.get() + 1);
        let data = &*(lp as *const COPYDATASTRUCT);
        let bytes = std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize);
        let Some(request) = wire::decode(bytes) else { return 0; };
        if context.queued && !context.entered.get() {
            PostMessageW(hwnd, CALLBACK, request.sequence as usize, 0);
        } else {
            reenter(context);
            SetPropW(hwnd, ACK, request.sequence as usize as _);
        }
        MAGIC as isize
    }
}

#[test]
fn nested_requests_preserve_outer_request_and_session_health() {
    unsafe {
        let class = windows_sys::w!("LucidDeskReentrantClientTest");
        let instance = GetModuleHandleW(null_mut());
        assert_ne!(RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(peer), hInstance: instance,
            lpszClassName: class, ..Default::default() }), 0);
        for mode in 0..3 {
            let hwnd = CreateWindowExW(0, class, class, WS_POPUP, 0, 0, 1, 1,
                null_mut(), null_mut(), instance, null_mut());
            assert!(!hwnd.is_null());
            SetPropW(hwnd, OWNER, hwnd);
            let context = Box::new(Context {
                session: std::mem::ManuallyDrop::new(test_session(hwnd, hwnd, 0)),
                entered: Cell::new(false), rejected: Cell::new(false), requests: Cell::new(0),
                queued: mode == 2,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&raw const *context) as isize);
            let result = match mode {
                0 => context.session.clear_selection(),
                1 => context.session.submit_hidden(&[]),
                _ => context.session.cancel_menu(),
            };
            let rejected = context.rejected.get();
            let requests = context.requests.get();
            let completed = context.session.poll_hidden();
            let healthy = context.session.is_alive();
            // An early transport error must also release the request guard.
            RemovePropW(hwnd, OWNER);
            let disconnected = context.session.clear_selection();
            SetPropW(hwnd, OWNER, hwnd);
            let next = context.session.clear_selection();
            DestroyWindow(hwnd);
            assert!(rejected, "nested requests must fail before changing sequence or freeze token");
            assert_eq!(requests, 1, "only the outer request may reach Explorer");
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(completed, Ok(true));
            assert!(healthy, "rejecting reentry must not retire a healthy session");
            assert!(disconnected.is_err());
            assert!(next.is_ok(), "a completed or rejected send must release its guard");
        }
        UnregisterClassW(class, instance);
    }
}
