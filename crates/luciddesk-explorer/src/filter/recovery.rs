//! Restore-only recovery stays on Explorer's STA, even after the owner exits.
use std::time::{Duration, Instant};
use windows::core::{HRESULT, Result};

#[derive(Default)]
pub(super) struct Recovery {
    due: Option<Instant>,
    attempts: u32,
    refreshed: bool,
    complete: bool,
}
pub(super) struct Report {
    pub complete: bool,
    pub restore_error: Option<HRESULT>,
    pub refresh_error: Option<HRESULT>,
}
impl Recovery {
    pub fn deadline(&self) -> Option<Instant> { self.due }
    pub fn attempt(&mut self, now: Instant, mut restore: impl FnMut() -> Result<()>, mut refresh: impl FnMut() -> Result<()>) -> Report {
        let mut report = Report { complete: self.complete, restore_error: None, refresh_error: None };
        if self.complete || self.due.is_some_and(|due| now < due) { return report; }
        match restore() {
            Ok(()) => {
                self.complete = true;
                self.due = None;
                report.complete = true;
                return report;
            }
            Err(error) => report.restore_error = Some(error.code()),
        }
        // Refresh success is not recovery success. Verify on a later STA turn
        // so asynchronous enumeration can finish; never pump/sleep inside COM.
        if !self.refreshed {
            match refresh() {
                Ok(()) => self.refreshed = true,
                Err(error) => report.refresh_error = Some(error.code()),
            }
        }
        let delay = Duration::from_millis((250u64 << self.attempts.min(8)).min(60_000));
        self.attempts = self.attempts.saturating_add(1);
        self.due = Some(now + delay);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::E_FAIL;
    #[test]
    fn refresh_success_requires_later_verified_restore() {
        let mut recovery = Recovery::default();
        let now = Instant::now();
        let first = recovery.attempt(now, || Err(E_FAIL.into()), || Ok(()));
        assert!(!first.complete);
        assert_eq!(first.restore_error, Some(E_FAIL));
        assert!(!recovery.attempt(now, || panic!("early retry"), || panic!("early refresh")).complete);
        assert!(!recovery.attempt(recovery.deadline().unwrap(), || Err(E_FAIL.into()), || panic!("duplicate refresh")).complete);
        assert!(recovery.attempt(recovery.deadline().unwrap(), || Ok(()), || panic!("unneeded refresh")).complete);
        assert!(recovery.deadline().is_none());
    }
    #[test]
    fn failed_refresh_retains_recovery_and_backs_off_without_abandoning_it() {
        let mut recovery = Recovery::default();
        let mut now = Instant::now();
        for attempt in 0..20 {
            let report = recovery.attempt(now, || Err(E_FAIL.into()), || Err(E_FAIL.into()));
            assert!(!report.complete);
            assert_eq!(report.refresh_error, Some(E_FAIL));
            let due = recovery.deadline().unwrap();
            assert_eq!((due - now).as_millis(), (250u128 << attempt.min(8)).min(60_000));
            now = due;
        }
        assert!(recovery.attempt(now, || Ok(()), || panic!("unneeded refresh")).complete);
    }
}
