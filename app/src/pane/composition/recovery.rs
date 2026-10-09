//! Bounded window-local recovery, independent of a discarded surface's timers.
use std::time::{Duration, Instant};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::{KillTimer, SetTimer}};

const RECOVERY_RETRY: usize = 0x4c50_4752;
const DELAYS: [u32; 3] = [100, 250, 1000];

#[derive(Default)]
pub(crate) struct PaintRecovery {
    failures: usize,
    retry_at: Option<Instant>,
}

impl PaintRecovery {
    /// `false` means rasterization was deferred; it must not reset recovery.
    pub fn paint<T, E: std::fmt::Display>(
        &mut self,
        hwnd: HWND,
        resource: &mut Option<T>,
        component: &str,
        paint: impl FnOnce(&mut Option<T>) -> Result<bool, E>,
    ) {
        if let Some(at) = self.retry_at {
            let remaining = at.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                unsafe { SetTimer(hwnd, RECOVERY_RETRY, remaining.as_millis() as u32 + 1, Some(super::retry_present)); }
                return;
            }
        }
        match paint(resource) {
            Ok(true) if self.failures != 0 => {
                *self = Self::default();
                unsafe { KillTimer(hwnd, RECOVERY_RETRY); }
            }
            Ok(_) => {}
            Err(error) => {
                // Surface::drop cancels presentation retries. Release it before
                // scheduling recovery, and keep the two timer IDs separate.
                *resource = None;
                if self.failures == 0 {
                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, component,
                        &format!("Rendering failed; recreating graphics resources: {error}"));
                }
                self.retry_at = None;
                if let Some(&delay) = DELAYS.get(self.failures) {
                    self.retry_at = Some(Instant::now() + Duration::from_millis(u64::from(delay)));
                    // The callback is one-shot and only invalidates this window.
                    if unsafe { SetTimer(hwnd, RECOVERY_RETRY, delay, Some(super::retry_present)) } == 0 {
                        self.retry_at = None;
                        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, component,
                            "Could not schedule graphics recovery");
                    }
                    self.failures += 1;
                } else {
                    // Stop automatic retries; a later user interaction may retry.
                    unsafe { KillTimer(hwnd, RECOVERY_RETRY); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    #[test]
    fn failed_resources_are_dropped_and_retries_stop_until_external_redraw() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Recovery test").size(96, 64)
            .style(WS_POPUP).create().unwrap();
        let hwnd = window.hwnd().cast();
        let mut recovery = PaintRecovery::default();
        let mut resource = Some(std::sync::Arc::new(()));
        let released = std::sync::Arc::downgrade(resource.as_ref().unwrap());
        for attempt in 0..4 {
            recovery.retry_at = None; // Advance to the next scheduled attempt.
            recovery.paint(hwnd, &mut resource, "test.render", |_| Err::<bool, _>("device lost"));
            assert!(resource.is_none());
            assert!(released.upgrade().is_none());
            assert_eq!(unsafe { KillTimer(hwnd, RECOVERY_RETRY) } != 0, attempt < 3);
        }
        recovery.paint(hwnd, &mut resource, "test.render", |slot| {
            *slot = Some(std::sync::Arc::new(()));
            Ok::<_, &str>(true)
        });
        assert!(resource.is_some());
        assert_eq!(recovery.failures, 0);
        recovery.paint(hwnd, &mut resource, "test.render", |_| Err::<bool, _>("device lost again"));
        assert_ne!(unsafe { KillTimer(hwnd, RECOVERY_RETRY) }, 0);
    }

    #[test]
    fn early_callback_preserves_wakeup_and_deferred_frames_do_not_reset_recovery() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Recovery wakeup test").size(96, 64)
            .style(WS_POPUP).create().unwrap();
        let hwnd = window.hwnd().cast();
        let mut resource = Some(());
        let mut recovery = PaintRecovery::default();
        recovery.paint(hwnd, &mut resource, "test.render", |_| Err::<bool, _>("device lost"));
        // Simulate an early native timer callback which cancels its own timer.
        unsafe { super::super::retry_present(hwnd, WM_TIMER, RECOVERY_RETRY, 0); }
        recovery.paint(hwnd, &mut resource, "test.render", |_| -> Result<bool, &str> {
            panic!("must wait before recreating resources");
        });
        assert_ne!(unsafe { KillTimer(hwnd, RECOVERY_RETRY) }, 0);
        recovery.retry_at = None;
        recovery.paint(hwnd, &mut resource, "test.render", |_| Ok::<_, &str>(false));
        assert_eq!(recovery.failures, 1);
        recovery.paint(hwnd, &mut resource, "test.render", |_| Ok::<_, &str>(true));
        assert_eq!(recovery.failures, 0);
    }
}
