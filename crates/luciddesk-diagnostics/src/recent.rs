//! In-memory logs survive recovery, level filtering and unavailable file storage.
use crate::{Level, format};
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

#[derive(Default)]
struct Buffer {
    events: VecDeque<String>,
    errors: VecDeque<String>,
    omitted: u64,
}

impl Buffer {
    fn record(&mut self, level: Level, component: &str, message: &str) {
        let entry = format::entry(level, component, message, "", "");
        if self.events.len() == 128 {
            self.events.pop_front();
            self.omitted = self.omitted.saturating_add(1);
        }
        self.events.push_back(entry.clone());
        // Keep failures separately so successful recovery does not evict them.
        if level == Level::Error {
            if self.errors.len() == 8 {
                self.errors.pop_front();
            }
            self.errors.push_back(entry);
        }
    }

    fn report(&self) -> String {
        let mut report = String::from("Recent errors (this run, oldest first):\n");
        for entry in &self.errors {
            report.push_str(entry);
            report.push('\n');
        }
        report.push_str(&format!(
            "\nRecent log events (independent of file log level; omitted={}):\n",
            self.omitted
        ));
        for entry in &self.events {
            report.push_str(entry);
            report.push('\n');
        }
        report
    }
}

static BUFFER: OnceLock<Mutex<Buffer>> = OnceLock::new();

pub(super) fn record(level: Level, component: &str, message: &str) {
    if let Ok(mut buffer) = BUFFER.get_or_init(Default::default).lock() {
        buffer.record(level, component, message);
    }
}

/// A read-only snapshot; never calls the host or takes its application-state lock.
pub fn report() -> String {
    BUFFER
        .get()
        .and_then(|buffer| buffer.lock().ok())
        .map_or_else(
            || "No recent log events available\n".into(),
            |buffer| buffer.report(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_recovery_does_not_evict_errors_and_memory_is_bounded() {
        let mut buffer = Buffer::default();
        for index in 0..10 {
            buffer.record(
                Level::Error,
                "hook.attach",
                &format!("HRESULT=0x80070005 error {index}"),
            );
        }
        for _ in 0..200 {
            buffer.record(Level::Info, "hook.attach", "recovered");
        }
        let report = buffer.report();
        assert_eq!(buffer.events.len(), 128);
        assert_eq!(buffer.errors.len(), 8);
        assert_eq!(buffer.omitted, 82);
        assert!(report.contains("HRESULT=0x80070005 error 2"));
        assert!(!report.contains("HRESULT=0x80070005 error 1\""));
        assert!(report.contains("recovered"));
    }
}
