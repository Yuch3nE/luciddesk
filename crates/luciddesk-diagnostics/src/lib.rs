//! Lazy, bounded diagnostic files. No heartbeat or database writes.
mod format;
mod panic;
mod recent;
pub use panic::install_panic_hook;
mod time;
mod trace;
pub use recent::report as recent_report;
pub use time::local_timestamp;
pub use trace::TraceFile;

use std::{
    cell::Cell,
    collections::VecDeque,
    fmt::Write as _,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}
impl Level {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Error,
        }
    }
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}
struct Recent {
    record: Arc<format::Record>,
    at: Instant,
    suppressed: u64,
}
struct WriteFailure {
    kind: io::ErrorKind,
    os_error: Option<i32>,
    message: String,
}
impl WriteFailure {
    fn error(&self) -> io::Error {
        self.os_error.map_or_else(
            || io::Error::new(self.kind, self.message.clone()),
            io::Error::from_raw_os_error,
        )
    }
}
struct Logger {
    path: PathBuf,
    level: Level,
    recent: VecDeque<Recent>,
    recent_bytes: usize,
    window: Option<Instant>,
    written: [usize; 3],
    total_written: u64,
    total_suppressed: u64,
    total_dropped: u64,
    failures: u64,
    last_error: Option<WriteFailure>,
    dropped: u64,
    header: Option<String>,
    session_header_written: bool,
    retry_after: Option<Instant>,
    report: fn() -> String,
}
impl Logger {
    fn new(database: &Path, level: Level) -> Self {
        Self {
            path: database.with_file_name("logs").join("diagnostic.log"),
            level,
            recent: VecDeque::new(),
            recent_bytes: 0,
            window: None,
            written: [0; 3],
            total_written: 0,
            total_suppressed: 0,
            total_dropped: 0,
            failures: 0,
            last_error: None,
            dropped: 0,
            header: None,
            session_header_written: false,
            retry_after: None,
            report: String::new,
        }
    }
    #[cfg(test)]
    fn write(
        &mut self,
        level: Level,
        component: &str,
        message: &str,
        now: Instant,
    ) -> io::Result<()> {
        let record = Arc::new(format::Record::new(
            level, component, message, "unknown", "unknown",
        ));
        self.write_record(record, now)
    }
    fn check_backoff(&self, now: Instant) -> io::Result<()> {
        if self.retry_after.is_some_and(|deadline| now < deadline) {
            return Err(self.last_error.as_ref().map_or_else(
                || io::Error::other("diagnostic storage retry pending"),
                WriteFailure::error,
            ));
        }
        Ok(())
    }
    fn write_record(&mut self, record: Arc<format::Record>, now: Instant) -> io::Result<()> {
        if record.level > self.level {
            return Ok(());
        }
        self.check_backoff(now)?;
        let written = self.total_written;
        match self.write_ready(record, now) {
            Ok(()) => {
                if self.total_written > written {
                    self.retry_after = None;
                    self.last_error = None;
                }
                self.last_error
                    .as_ref()
                    .map_or(Ok(()), |error| Err(error.error()))
            }
            Err(error) => {
                self.retry_after = Some(now + Duration::from_secs(5));
                self.failures = self.failures.saturating_add(1);
                self.last_error = Some(WriteFailure {
                    kind: error.kind(),
                    os_error: error.raw_os_error(),
                    message: error.to_string(),
                });
                Err(error)
            }
        }
    }
    fn write_ready(&mut self, record: Arc<format::Record>, now: Instant) -> io::Result<()> {
        let index = self
            .recent
            .iter()
            .position(|r| r.record.same_event(&record));
        let mut suppressed = 0;
        if let Some(index) = index {
            let recent = &mut self.recent[index];
            if self.last_error.is_none()
                && now.saturating_duration_since(recent.at) < Duration::from_secs(30)
            {
                recent.suppressed = recent.suppressed.saturating_add(1);
                self.total_suppressed = self.total_suppressed.saturating_add(1);
                return Ok(());
            }
            suppressed = recent.suppressed;
        }
        // Bound storms of distinct errors too; no timer or background flush.
        if self
            .window
            .is_none_or(|start| now.saturating_duration_since(start) >= Duration::from_secs(30))
        {
            self.window = Some(now);
            self.written = [0; 3];
        }
        // Separate budgets prevent informational/warning storms starving errors.
        let priority = match record.level {
            Level::Error => 0,
            Level::Warn => 1,
            _ => 2,
        };
        if self.written[priority] >= 64 {
            self.dropped = self.dropped.saturating_add(1);
            self.total_dropped = self.total_dropped.saturating_add(1);
            return Ok(());
        }
        let mut entry = record.line.clone();
        if suppressed != 0 {
            let _ = write!(entry, " suppressed={suppressed}");
        }
        if self.dropped != 0 {
            let _ = write!(entry, " rate_limited={}", self.dropped);
        }
        entry.push('\n');
        let parent = self.path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let length = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        let header = self.header.get_or_insert_with(|| format::header(""));
        let rotate = length
            + entry.len() as u64
            + if length == 0 || !self.session_header_written {
                header.len() as u64
            } else {
                0
            }
            > 256 * 1024;
        if rotate && length > 0 {
            let previous = parent.join("diagnostic.previous.log");
            match std::fs::remove_file(&previous) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
            std::fs::rename(&self.path, previous)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        if length == 0 || rotate || !self.session_header_written {
            file.write_all(header.as_bytes())?;
        }
        file.write_all(entry.as_bytes())?;
        file.flush()?;
        self.session_header_written = true;
        self.written[priority] += 1;
        self.total_written = self.total_written.saturating_add(1);
        self.dropped = 0;
        if let Some(index) = index {
            self.recent_bytes -= self.recent.remove(index).unwrap().record.bytes();
        }
        self.recent_bytes += record.bytes();
        self.recent.push_back(Recent {
            record,
            at: now,
            suppressed: 0,
        });
        while self.recent.len() > 64 || self.recent_bytes > 256 * 1024 {
            self.recent_bytes -= self.recent.pop_front().unwrap().record.bytes();
        }
        Ok(())
    }
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static METADATA: OnceLock<(String, String)> = OnceLock::new();
static DESTINATION: OnceLock<PathBuf> = OnceLock::new();
static HEADER: OnceLock<String> = OnceLock::new();
static LAST_FALLBACK: Mutex<Option<Instant>> = Mutex::new(None);

fn lock_writer(logger: &Mutex<Logger>) -> io::Result<std::sync::MutexGuard<'_, Logger>> {
    if std::thread::panicking() {
        logger.try_lock().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "log writer unavailable during panic",
            )
        })
    } else {
        logger
            .lock()
            .map_err(|_| io::Error::other("diagnostic logger poisoned"))
    }
}

thread_local! { static WRITING: Cell<bool> = const { Cell::new(false) }; }
struct Writing;
impl Writing {
    fn enter() -> io::Result<Self> {
        WRITING.with(|writing| {
            if writing.replace(true) {
                Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "recursive log retained in memory only",
                ))
            } else {
                Ok(Self)
            }
        })
    }
}
impl Drop for Writing {
    fn drop(&mut self) {
        WRITING.set(false);
    }
}

static LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(Level::Error as u8);
/// Called once by the process host. Initialization does not open/create files.
/// Libraries only emit events; they never choose a path or initialize a second sink.
pub fn initialize(database: &Path, version: &str, build: &str, report: fn() -> String) {
    LOGGER.get_or_init(|| {
        let _ = METADATA.set((format::bounded(version, 128), format::bounded(build, 128)));
        let mut logger = Logger::new(database, Level::Error);
        let _ = DESTINATION.set(logger.path.clone());
        logger.report = report;
        Mutex::new(logger)
    });
}
pub fn level() -> Level {
    match LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Level::Warn,
        2 => Level::Info,
        3 => Level::Debug,
        4 => Level::Trace,
        _ => Level::Error,
    }
}
#[must_use]
pub fn enabled(level: Level) -> bool {
    level <= self::level()
}
pub fn set_level(level: Level) {
    if let Some(logger) = LOGGER.get()
        && let Ok(mut logger) = logger.lock()
    {
        if logger.level != level {
            logger.level = level;
            logger.recent.clear();
            logger.recent_bytes = 0;
        }
        LEVEL.store(level as u8, std::sync::atomic::Ordering::Relaxed);
    }
}
pub fn path() -> Option<PathBuf> {
    DESTINATION.get().cloned()
}
/// A read-only storage-health snapshot; a busy writer never blocks report generation.
pub fn storage_report() -> String {
    let Some(logger) = LOGGER.get() else {
        return "Log storage: not initialized\n".into();
    };
    match logger.try_lock() {
        Ok(logger) => format!(
            "Log storage: {}\nlevel={} written={} suppressed={} rate_limited={} io_failures={} retry_in_ms={} last_io_error={}\n",
            logger.path.display(),
            logger.level.label(),
            logger.total_written,
            logger.total_suppressed,
            logger.total_dropped,
            logger.failures,
            logger.retry_after.map_or(0, |deadline| deadline
                .saturating_duration_since(Instant::now())
                .as_millis()),
            logger.last_error.as_ref().map_or_else(
                || "None".into(),
                |error| format!(
                    "{}; kind={:?}; os_error={:?}",
                    error.message, error.kind, error.os_error
                )
            ),
        ),
        Err(error) => format!("Log storage: unavailable ({error})\n"),
    }
}

/// Memory capture precedes filtering and I/O. A failed write/backoff returns an error.
///
/// # Errors
/// Returns the original storage failure during I/O/backoff, or an unavailable-writer error.
#[track_caller]
pub fn try_log(level: Level, component: &str, message: &str) -> io::Result<()> {
    let (version, build) = METADATA
        .get()
        .map_or(("unknown", "unknown"), |(version, build)| {
            (version.as_str(), build.as_str())
        });
    let record = Arc::new(format::Record::new(
        level, component, message, version, build,
    ));
    recent::record(Arc::clone(&record));
    if !enabled(level) {
        return Ok(());
    }
    let _writing = Writing::enter()?;
    let logger = LOGGER
        .get()
        .ok_or_else(|| io::Error::other("diagnostics not initialized"))?;
    // Host callbacks may query/log diagnostics. Never invoke one under a logger lock.
    let report = {
        let logger = lock_writer(logger)?;
        logger.check_backoff(Instant::now())?;
        logger.header.is_none().then_some(logger.report)
    };
    let header = report.map(|report| {
        HEADER
            .get_or_init(|| {
                let text = std::panic::catch_unwind(report).unwrap_or_else(|_| {
                    "Host diagnostic report unavailable: callback panicked".into()
                });
                format::header(&text)
            })
            .clone()
    });
    let mut logger = lock_writer(logger)?;
    if let Some(header) = header {
        logger.header.get_or_insert(header);
    }
    logger.write_record(record, Instant::now())
}
/// Non-panicking fallback is safe at native callback boundaries. Never writes stdout.
#[track_caller]
pub fn log(level: Level, component: &str, message: &str) {
    if let Err(error) = try_log(level, component, message) {
        // A broken disk must not turn repeated logging into a stderr storm.
        let Ok(mut last) = LAST_FALLBACK.try_lock() else {
            return;
        };
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(5)) {
            return;
        }
        *last = Some(Instant::now());
        drop(last);
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            format::entry(
                Level::Error,
                "diagnostics",
                &format!("{error}; component={component:?} original={message:?}"),
                "unknown",
                "unknown"
            )
        );
    }
}
/// Filter before evaluating formatting arguments or collecting expensive diagnostics.
#[macro_export]
macro_rules! emit {
    ($level:expr,$component:expr,$($arg:tt)*) => {{
        let level=$level;
        if $crate::enabled(level) {$crate::log(level,$component,&format!($($arg)*));}
    }};
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_error_filters_before_any_file_io() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::parse(""));
        for level in [Level::Warn, Level::Info, Level::Debug, Level::Trace] {
            logger
                .write(level, "test", "filtered", Instant::now())
                .unwrap();
        }
        assert!(!dir.path().join("logs").exists());
        logger
            .write(Level::Error, "test", "failure", Instant::now())
            .unwrap();
        assert!(
            std::fs::read_to_string(&logger.path)
                .unwrap()
                .contains("level=ERROR")
        );
        assert!(!dir.path().join("workspace.db").exists());
        assert_eq!(Level::parse("typo"), Level::Error);
        assert_eq!(Level::parse(" DEBUG "), Level::Debug);
    }
    #[test]
    fn failed_output_backs_off_then_recovers_on_next_event() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        std::fs::write(&logs, "blocked").unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Error);
        let now = Instant::now();
        assert!(logger.write(Level::Error, "test", "failure", now).is_err());
        std::fs::remove_file(&logs).unwrap();
        assert!(
            logger
                .write(
                    Level::Error,
                    "test",
                    "failure",
                    now + Duration::from_secs(1)
                )
                .is_err()
        );
        assert!(!logs.exists());
        logger
            .write(
                Level::Error,
                "test",
                "failure",
                now + Duration::from_secs(6),
            )
            .unwrap();
        assert!(logger.path.exists());
    }
    #[test]
    fn distinct_error_storm_is_bounded_without_background_flush() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Error);
        let now = Instant::now();
        for i in 0..200 {
            logger
                .write(Level::Error, "storm", &format!("failure {i}"), now)
                .unwrap();
        }
        let text = std::fs::read_to_string(&logger.path).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with('[')).count(), 64);
        assert_eq!(logger.dropped, 136);
        logger
            .write(
                Level::Error,
                "storm",
                "recovered window",
                now + Duration::from_secs(31),
            )
            .unwrap();
        assert!(
            std::fs::read_to_string(&logger.path)
                .unwrap()
                .contains("rate_limited=136")
        );
    }
    #[test]
    fn duplicate_errors_are_suppressed_and_files_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Info);
        let now = Instant::now();
        logger
            .write(Level::Error, "test", "failure\nnext", now)
            .unwrap();
        let before = std::fs::read(&logger.path).unwrap();
        for _ in 0..20 {
            logger
                .write(Level::Error, "test", "failure\nnext", now)
                .unwrap();
        }
        assert_eq!(std::fs::read(&logger.path).unwrap(), before);
        logger
            .write(
                Level::Error,
                "test",
                "failure\nnext",
                now + Duration::from_secs(31),
            )
            .unwrap();
        assert!(
            std::fs::read_to_string(&logger.path)
                .unwrap()
                .contains("suppressed=20")
        );
        std::fs::write(&logger.path, vec![b'x'; 256 * 1024]).unwrap();
        logger
            .write(Level::Info, "desktop.connection", "recovered", now)
            .unwrap();
        assert!(
            logger
                .path
                .with_file_name("diagnostic.previous.log")
                .exists()
        );
        assert!(
            std::fs::read_to_string(&logger.path)
                .unwrap()
                .contains("level=INFO")
        );
    }
    #[test]
    fn info_and_warning_storms_cannot_exhaust_the_error_budget() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Trace);
        let now = Instant::now();
        for level in [Level::Info, Level::Warn] {
            for index in 0..100 {
                logger
                    .write(level, "storm", &format!("entry {index}"), now)
                    .unwrap();
            }
        }
        logger
            .write(Level::Error, "critical", "original HRESULT=0x80004005", now)
            .unwrap();
        let text = std::fs::read_to_string(&logger.path).unwrap();
        assert!(text.contains("original HRESULT=0x80004005"));
        assert_eq!(logger.written, [1, 64, 64]);
        assert_eq!(logger.total_dropped, 72);
    }

    #[test]
    fn file_output_does_not_silently_pretruncate_original_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Error);
        logger
            .write(
                Level::Error,
                "long.error",
                &"错误".repeat(20_000),
                Instant::now(),
            )
            .unwrap();
        let text = std::fs::read_to_string(&logger.path).unwrap();
        assert!(text.contains("truncated; omitted_bytes="));
        assert!(text.contains("sequence="));
        assert!(text.contains("thread_name="));
        assert!(text.contains("source="));
        assert!(text.len() < 256 * 1024);
    }

    #[test]
    fn backoff_reports_original_os_error_and_recovers_only_after_a_real_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = Logger::new(&dir.path().join("workspace.db"), Level::Error);
        let now = Instant::now();
        logger
            .write(Level::Error, "test", "first entry", now)
            .unwrap();
        // A deliberately invalid destination simulates the directory disappearing.
        let original = logger.path.clone();
        logger.path = dir.path().join("directory-blocker").join("log");
        std::fs::write(dir.path().join("directory-blocker"), "not a directory").unwrap();
        let first = logger
            .write(
                Level::Error,
                "test",
                "first entry",
                now + Duration::from_secs(31),
            )
            .unwrap_err();
        let again = logger
            .write(
                Level::Error,
                "test",
                "first entry",
                now + Duration::from_secs(32),
            )
            .unwrap_err();
        assert_eq!(first.kind(), again.kind());
        assert_eq!(first.raw_os_error(), again.raw_os_error());
        assert_eq!(logger.failures, 1);
        logger.path = original;
        logger
            .write(
                Level::Error,
                "test",
                "first entry",
                now + Duration::from_secs(37),
            )
            .unwrap();
        assert_eq!(logger.total_written, 2);
        assert!(logger.last_error.is_none());
    }
    #[test]
    fn a_new_session_marks_readable_format_without_rewriting_old_log_records() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("workspace.db");
        let mut logger = Logger::new(&database, Level::Error);
        std::fs::create_dir_all(logger.path.parent().unwrap()).unwrap();
        let old = "# format=luciddesk-log-v1\ntimestamp_unix_ms=123 old record\n";
        std::fs::write(&logger.path, old).unwrap();
        let now = Instant::now();
        logger
            .write(Level::Error, "new", "first event", now)
            .unwrap();
        logger
            .write(Level::Error, "new", "second event", now)
            .unwrap();
        let text = std::fs::read_to_string(&logger.path).unwrap();
        assert!(text.starts_with(old));
        assert_eq!(text.matches("format=luciddesk-log-v2").count(), 1);
        assert!(text.contains("written_at=\""));
        assert!(text.contains("first event"));
        assert!(text.contains("second event"));
    }
}
