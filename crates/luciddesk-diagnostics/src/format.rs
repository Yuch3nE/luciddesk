//! Bounded, escaped records shared by memory, files and explicit traces.
use crate::Level;
use std::{
    fmt::Write,
    panic::Location,
    sync::LazyLock,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

pub(crate) fn bounded(value: &str, limit: usize) -> String {
    let mut characters = value.chars();
    let mut text: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        let omitted = value.len() - text.len();
        let _ = write!(text, " [truncated; omitted_bytes={omitted}]");
    }
    text
}

pub(crate) fn header(report: &str) -> String {
    let report = bounded(report, 16_384);
    let written_at = crate::local_timestamp();
    format!(
        "# format=luciddesk-log-v2 written_at={written_at:?} time=computer-local report={report:?}\n"
    )
}

pub(crate) struct Record {
    pub level: Level,
    pub component: String,
    pub message: String,
    pub location: &'static Location<'static>,
    pub line: String,
    pub sequence: u64,
    pub timestamp: String,
}

impl Record {
    #[track_caller]
    pub fn new(level: Level, component: &str, message: &str, version: &str, build: &str) -> Self {
        let elapsed_ms = STARTED.elapsed().as_millis();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = crate::local_timestamp();
        let location = Location::caller();
        let component = bounded(component, 128);
        let message = bounded(message, 16_384);
        let version = bounded(version, 128);
        let build = bounded(build, 128);
        let thread = std::thread::current();
        let thread_name = bounded(thread.name().unwrap_or("unnamed"), 128);
        let source = format!(
            "{}:{}:{}",
            bounded(location.file(), 1024),
            location.line(),
            location.column()
        );
        let line = format!(
            "[{timestamp}] level={} component={component:?} message={message:?} | source={source:?} | sequence={sequence} elapsed_ms={elapsed_ms} pid={} thread={:?} thread_name={thread_name:?} | version={version:?} build={build:?}",
            level.label(),
            std::process::id(),
            thread.id()
        );
        Self {
            level,
            component,
            message,
            location,
            line,
            sequence,
            timestamp,
        }
    }

    pub fn bytes(&self) -> usize {
        self.component.len() + self.message.len() + self.line.len() + self.timestamp.len()
    }

    pub fn same_event(&self, other: &Self) -> bool {
        self.level == other.level
            && self.component == other.component
            && self.location == other.location
            && self.message == other.message
    }
}

#[track_caller]
pub(crate) fn entry(
    level: Level,
    component: &str,
    message: &str,
    version: &str,
    build: &str,
) -> String {
    Record::new(level, component, message, version, build).line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fields_stay_on_one_line_and_preserve_unicode() {
        let text = entry(
            Level::Error,
            "a\nb",
            "错误\r\n\"quoted\"\t",
            "v\n1",
            "rev\r2",
        );
        assert_eq!(text.lines().count(), 1);
        assert!(text.starts_with('['));
        assert_eq!(&text[31..33], "] ");
        assert!(text.contains(" | source="));
        assert!(text.contains(" elapsed_ms="));
        assert!(!text.contains("timestamp_unix_ms="));
        assert!(text.contains("错误\\r\\n\\\"quoted\\\"\\t"));
        assert!(text.contains("version=\"v\\n1\" build=\"rev\\r2\""));
        assert_eq!(header("a\nb").lines().count(), 1);
    }

    #[test]
    fn records_and_reports_have_bounded_fields() {
        let huge = "界".repeat(100_000);
        assert!(entry(Level::Trace, &huge, &huge, &huge, &huge).len() < 60_000);
        assert!(header(&huge).len() < 50_000);
    }
}
