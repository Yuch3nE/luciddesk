//! Exercise the real MessageBox modal pump, without synchronous key injection.
use super::*;
use std::cell::Cell;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

struct Probe {
    callback: Rc<RefCell<Box<dyn FnMut()>>>,
    pane: HWND,
    observed: Cell<bool>,
    callable: Cell<bool>,
    enabled: Cell<bool>,
    dialog: Cell<bool>,
}

thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

unsafe extern "system" fn inspect(_: HWND, _: u32, timer: usize, _: u32) {
    let dialog = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() };
    let mut class = [0u16; 32];
    let length = unsafe { GetClassNameW(dialog, class.as_mut_ptr(), class.len() as i32) };
    let is_dialog = length > 0 && String::from_utf16_lossy(&class[..length as usize]) == "#32770";
    // Creating a dialog may itself pump timers before activation. Keep the
    // repeating probe alive until the modal dialog is actually ready.
    if !is_dialog {
        return;
    }
    unsafe { KillTimer(std::ptr::null_mut(), timer); }
    PROBE.with(|slot| {
        let slot = slot.borrow();
        let probe = slot.as_ref().unwrap();
        probe.enabled.set(unsafe { IsWindowEnabled(probe.pane) } != 0);
        probe.dialog.set(is_dialog);
        if let Ok(mut callback) = probe.callback.try_borrow_mut() {
            callback();
            probe.callable.set(true);
        }
        probe.observed.set(true);
    });
    // The dialog belongs to this test thread; never locate another app's dialog.
    unsafe { PostMessageW(dialog, WM_CLOSE, 0, 0); }
}

#[test]
fn error_dialog_runs_after_event_borrow_and_keeps_pane_input_callable() {
    // The inline case demonstrates the old failure condition using Windows'
    // own modal timer dispatch. The deferred case exercises production code.
    let pane = Window::new("Event error regression").style(WS_POPUP).create().unwrap();
    for deferred in [false, true] {
        let calls = Rc::new(Cell::new(0));
        let called = calls.clone();
        let callback: Rc<RefCell<Box<dyn FnMut()>>> = Rc::new(RefCell::new(Box::new(move || {
            called.set(called.get() + 1);
        })));
        PROBE.with(|slot| *slot.borrow_mut() = Some(Probe {
            callback: callback.clone(), pane: pane.hwnd().cast(),
            observed: Cell::new(false), callable: Cell::new(false), enabled: Cell::new(false),
            dialog: Cell::new(false),
        }));
        let timer = unsafe { SetTimer(std::ptr::null_mut(), 0, 100, Some(inspect)) };
        assert_ne!(timer, 0);
        {
            let _event = callback.borrow_mut();
            if deferred {
                defer_error("Deferred event error regression".into());
                assert!(!PROBE.with(|slot| slot.borrow().as_ref().unwrap().observed.get()));
            } else {
                error("Inline event error regression");
            }
        }
        while !PROBE.with(|slot| slot.borrow().as_ref().unwrap().observed.get()) {
            let mut message = MSG::default();
            unsafe {
                assert!(GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) > 0);
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let probe = PROBE.with(|slot| slot.borrow_mut().take().unwrap());
        assert!(probe.dialog.get(), "probe must execute inside the real MessageBox loop");
        assert!(probe.enabled.get(), "ownerless MessageBox leaves the pane enabled");
        assert_eq!(probe.callable.get(), deferred);
        assert_eq!(calls.get(), usize::from(deferred));
    }
}
