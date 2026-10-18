use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct ReadRetry {
    failures: u8,
    due: Option<Instant>,
}
impl ReadRetry {
    pub fn failed(&mut self, retryable: bool, now: Instant) -> bool {
        if !retryable || self.failures >= 3 {
            return false;
        }
        self.failures += 1;
        self.due = Some(now + Duration::from_millis(250 << (self.failures - 1)));
        true
    }
    pub fn waiting(&self) -> bool {
        self.due.is_some_and(|due| Instant::now() < due)
    }
    pub fn pending(&self) -> bool {
        self.due.is_some()
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.due
    }
    pub fn recovered(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_back_off_and_success_discards_the_old_deadline() {
        let now = Instant::now();
        let mut retry = ReadRetry::default();
        for delay in [250, 500, 1000] {
            assert!(retry.failed(true, now));
            assert_eq!(retry.due, Some(now + Duration::from_millis(delay)));
        }
        assert!(!retry.failed(true, now));
        retry.recovered();
        assert!(!retry.pending());
        assert!(!retry.waiting());
        assert!(retry.failed(true, now));
        assert_eq!(retry.due, Some(now + Duration::from_millis(250)));
    }
    #[test]
    fn read_recovery_is_bounded_and_never_retries_a_partial_write() {
        let mut retry = ReadRetry::default();
        let now = Instant::now();
        assert!(retry.failed(true, now));
        assert!(retry.waiting());
        assert!(
            !retry.failed(false, now),
            "a mutation error must not be treated as a safe read retry"
        );
        retry.recovered();
        for _ in 0..3 {
            assert!(retry.failed(true, now));
        }
        assert!(
            !retry.failed(true, now),
            "persistent read failure must eventually enter recovery"
        );
        retry.recovered();
        assert!(!retry.pending());
        assert!(
            retry.failed(true, now),
            "a later independent transient read gets its own budget"
        );
    }
}
