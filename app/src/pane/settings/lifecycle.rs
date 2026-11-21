//! Deferred error reporting and startup operation scheduling.
use super::*;

// A closing window must finish destroying its native resources before a modal
// error starts another message loop. Keep the callback owned and borrow-free.
pub(super) fn report_close_errors(errors: Vec<String>, report: impl FnOnce(&str) + 'static) {
    if errors.is_empty() {
        return;
    }
    let message = errors.join("\n");
    let pending = message.clone();
    if !window::defer_action(move || report(&pending)) {
        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "settings.close", &message);
    }
}

pub(super) fn start_login_operation(
    controller: &mut crate::startup::Controller,
    hwnd: windows_sys::Win32::Foundation::HWND,
    enabled: Option<bool>,
) -> Result<(), String> {
    if controller.busy() {
        return Ok(());
    }
    if unsafe { SetTimer(hwnd, STARTUP_TIMER, 150, None) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = match enabled {
        Some(enabled) => controller.set_enabled(enabled),
        None => controller.refresh(),
    };
    if result.is_err() {
        unsafe {
            KillTimer(hwnd, STARTUP_TIMER);
        }
    }
    result
}

#[derive(Default)]
pub(super) struct PendingStyles {
    pub material: Option<Backdrop>,
    pub radius: Option<f32>,
    pub grid: Option<f32>,
}

/// Return false when a caller still owns app state; retry on a later message.
pub(super) fn close(
    state: &Rc<RefCell<PaneApp>>,
    hwnd: windows_sys::Win32::Foundation::HWND,
    pending: &mut PendingStyles,
) -> bool {
    let Ok(mut owner) = state.try_borrow_mut() else {
        return false;
    };
    let mut errors = Vec::new();
    if let Some(original) = pending.material.take() {
        if let Err(error) = events::commit_material(&mut owner, original) {
            errors.push(error);
        }
    }
    if let Some(original) = pending.radius.take() {
        if let Err(error) = events::commit_radius(&mut owner, original) {
            errors.push(error);
        }
    }
    if let Some(original) = pending.grid.take() {
        if let Err(error) = events::commit_grid(&mut owner, original) {
            errors.push(error);
        }
    }
    let window = if owner
        .settings
        .as_ref()
        .is_some_and(|window| window.hwnd().cast() == hwnd)
    {
        owner.settings.take()
    } else {
        None
    };
    drop(owner);
    unsafe {
        KillTimer(hwnd, CONTROL_MOTION_TIMER);
        KillTimer(hwnd, FONT_LOAD_TIMER);
        KillTimer(hwnd, UPDATE_TIMER);
        KillTimer(hwnd, STARTUP_TIMER);
    }
    // Drop outside the PaneApp borrow: native destruction can send
    // focus messages to other panes. The callback's render resources
    // are released when this invocation returns.
    drop(window);
    report_close_errors(errors, window::error);
    true
}
