//! Independent bounded buffers retain distinct failures through recovery and storms.
use crate::{Level, format::Record};
use std::{
    collections::VecDeque,
    fmt::Write,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone)]
struct Retained {
    record: Arc<Record>,
    occurrences: u64,
    first_sequence: u64,
    first_timestamp: String,
}

#[derive(Clone, Default)]
struct History {
    entries: VecDeque<Retained>,
    bytes: usize,
    evicted: u64,
    coalesced: u64,
}
impl History {
    fn record(&mut self, record: Arc<Record>, count: usize, budget: usize) {
        let existing = self
            .entries
            .iter()
            .position(|entry| entry.record.same_event(&record));
        let entry = if let Some(index) = existing {
            let old = self.entries.remove(index).unwrap();
            self.bytes -= old.record.bytes();
            self.coalesced = self.coalesced.saturating_add(1);
            Retained {
                record,
                occurrences: old.occurrences.saturating_add(1),
                ..old
            }
        } else {
            Retained {
                first_sequence: record.sequence,
                first_timestamp: record.timestamp.clone(),
                record,
                occurrences: 1,
            }
        };
        self.bytes += entry.record.bytes();
        self.entries.push_back(entry);
        while self.entries.len() > count || self.bytes > budget {
            let old = self.entries.pop_front().unwrap();
            self.bytes -= old.record.bytes();
            self.evicted = self.evicted.saturating_add(old.occurrences);
        }
    }
    fn append(&self, output: &mut String, title: &str) {
        writeln!(
            output,
            "{title}: entries={} bytes={} evicted_events={} coalesced_events={}",
            self.entries.len(),
            self.bytes,
            self.evicted,
            self.coalesced
        )
        .unwrap();
        if self.entries.is_empty() {
            output.push_str("None recorded\n");
        }
        for entry in &self.entries {
            writeln!(
                output,
                "{} occurrences={} first_sequence={} first_time={:?}",
                entry.record.line, entry.occurrences, entry.first_sequence, entry.first_timestamp
            )
            .unwrap();
        }
    }
}

#[derive(Clone, Default)]
struct Buffer {
    events: History,
    errors: History,
}
impl Buffer {
    fn record(&mut self, record: Arc<Record>) {
        self.events.record(Arc::clone(&record), 128, 256 * 1024);
        if record.level == Level::Error {
            self.errors.record(record, 16, 128 * 1024);
        }
    }
    fn report(&self) -> String {
        let mut output = String::new();
        self.errors.append(
            &mut output,
            "Recent errors (this run, oldest last occurrence first)",
        );
        self.events.append(
            &mut output,
            "Recent log events (independent of file log level)",
        );
        output
    }
}
static BUFFER: OnceLock<Mutex<Buffer>> = OnceLock::new();
pub(super) fn record(record: Arc<Record>) {
    let mut buffer = BUFFER
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    buffer.record(record);
}
/// Clone only shared record references under the lock; format outside it.
pub fn report() -> String {
    let snapshot = BUFFER.get().map(|buffer| {
        buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    snapshot.map_or_else(
        || "No recent log events available\n".into(),
        |buffer| buffer.report(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(level: Level, component: &str, message: &str) -> Arc<Record> {
        Arc::new(Record::new(level, component, message, "test", "revision"))
    }
    #[test]
    fn repeated_errors_do_not_evict_distinct_failures_and_success_keeps_errors() {
        let mut buffer = Buffer::default();
        buffer.record(event(Level::Error, "hook.attach", "HRESULT=0x80070005"));
        for _ in 0..200 {
            buffer.record(event(Level::Error, "shell.read", "timeout"));
        }
        for _ in 0..200 {
            buffer.record(event(Level::Info, "worker", "recovered"));
        }
        assert_eq!(buffer.errors.entries.len(), 2);
        assert!(buffer.report().contains("occurrences=200"));
        assert!(buffer.report().contains("HRESULT=0x80070005"));
        assert!(buffer.report().contains("recovered"));
    }
    #[test]
    fn both_entry_count_and_byte_budgets_bound_memory_with_explicit_eviction() {
        let mut buffer = Buffer::default();
        for index in 0..200 {
            buffer.record(event(
                Level::Error,
                "failure",
                &format!("{index} {}", "界".repeat(16_384)),
            ));
        }
        assert!(buffer.events.entries.len() <= 128);
        assert!(buffer.events.bytes <= 256 * 1024);
        assert!(buffer.errors.entries.len() <= 16);
        assert!(buffer.errors.bytes <= 128 * 1024);
        assert!(buffer.events.evicted > 0);
        assert!(buffer.report().contains("truncated; omitted_bytes="));
        assert!(buffer.report().contains("evicted_events="));
    }
}
