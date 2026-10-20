//! Lazy, bounded diagnostic files. No heartbeat or database writes.
mod format;
mod recent;
mod trace;
pub use recent::report as recent_report;
pub use trace::TraceFile;

use std::{
    collections::VecDeque,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
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
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Error,
        }
    }
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
    level: Level,
    component: String,
    message: String,
    at: Instant,
    suppressed: u64,
}
struct Logger {
    path: PathBuf,
    level: Level,
    recent: VecDeque<Recent>,
    window: Option<Instant>,
    written: usize,
    dropped: u64,
    header: Option<String>,
    retry_after: Option<Instant>,
    version: String,
    build: String,
    report: fn() -> String,
}
impl Logger {
    fn new(database: &Path, level: Level) -> Self {
        Self {
            path: database.with_file_name("logs").join("diagnostic.log"),
            level,
            recent: VecDeque::new(),
            window: None,
            written: 0,
            dropped: 0,
            header: None,
            retry_after: None,
            version: "unknown".into(),
            build: "unknown".into(),
            report: String::new,
        }
    }
    fn write(
        &mut self,
        level: Level,
        component: &str,
        message: &str,
        now: Instant,
    ) -> io::Result<()> {
        if level > self.level || self.retry_after.is_some_and(|deadline| now < deadline) {
            return Ok(());
        }
        match self.write_ready(level, component, message, now) {
            Ok(()) => {
                self.retry_after = None;
                Ok(())
            }
            Err(error) => {
                self.retry_after = Some(now + Duration::from_secs(5));
                Err(error)
            }
        }
    }
    fn write_ready(
        &mut self,
        level: Level,
        component: &str,
        message: &str,
        now: Instant,
    ) -> io::Result<()> {
        if level > self.level {
            return Ok(());
        }
        let message: String = message.chars().take(16_384).collect();
        let component: String = component.chars().take(128).collect();
        let index = self
            .recent
            .iter()
            .position(|r| r.level == level && r.component == component && r.message == message);
        let mut suppressed = 0;
        if let Some(index) = index {
            let recent = &mut self.recent[index];
            if now.saturating_duration_since(recent.at) < Duration::from_secs(30) {
                recent.suppressed += 1;
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
            self.written = 0;
        }
        if self.written >= 64 {
            self.dropped = self.dropped.saturating_add(1);
            return Ok(());
        }
        let mut entry = format::entry(level, &component, &message, &self.version, &self.build);
        if suppressed != 0 {
            entry.push_str(&format!(" suppressed={suppressed}"));
        }
        if self.dropped != 0 {
            entry.push_str(&format!(" rate_limited={}", self.dropped));
        }
        entry.push('\n');
        let parent = self.path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let length = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        let header = self
            .header
            .get_or_insert_with(|| format::header(&(self.report)()));
        let rotate =
            length + entry.len() as u64 + if length == 0 { header.len() as u64 } else { 0 }
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
        if length == 0 || rotate {
            file.write_all(header.as_bytes())?;
        }
        file.write_all(entry.as_bytes())?;
        file.flush()?;
        self.written += 1;
        self.dropped = 0;
        if let Some(index) = index {
            self.recent.remove(index);
        }
        if self.recent.len() == 64 {
            self.recent.pop_front();
        }
        self.recent.push_back(Recent {
            level,
            component,
            message,
            at: now,
            suppressed: 0,
        });
        Ok(())
    }
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();
static LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(Level::Error as u8);
/// Called once by the process host. Initialization does not open/create files.
/// Libraries only emit events; they never choose a path or initialize a second sink.
pub fn initialize(database: &Path, version: &str, build: &str, report: fn() -> String) {
    LOGGER.get_or_init(|| {
        let mut logger = Logger::new(database, Level::Error);
        logger.version = version.into();
        logger.build = build.into();
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
pub fn enabled(level: Level) -> bool {
    level <= self::level()
}
pub fn set_level(level: Level) {
    if let Some(logger) = LOGGER.get() {
        if let Ok(mut logger) = logger.lock() {
            if logger.level != level {
                logger.level = level;
                logger.recent.clear();
            }
            LEVEL.store(level as u8, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
pub fn path() -> Option<PathBuf> {
    LOGGER.get()?.lock().ok().map(|logger| logger.path.clone())
}
/// Fallible output for hosts that need to report unavailable diagnostic storage.
pub fn try_log(level: Level, component: &str, message: &str) -> io::Result<()> {
    recent::record(level, component, message);
    if !enabled(level) {
        return Ok(());
    }
    let logger = LOGGER
        .get()
        .ok_or_else(|| io::Error::other("diagnostics not initialized"))?;
    logger
        .lock()
        .map_err(|_| io::Error::other("diagnostic logger poisoned"))?
        .write(level, component, message, Instant::now())
}
/// Non-panicking fallback is safe at native callback boundaries. Never writes stdout.
pub fn log(level: Level, component: &str, message: &str) {
    if let Err(error) = try_log(level, component, message) {
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
        logger
            .write(
                Level::Error,
                "test",
                "failure",
                now + Duration::from_secs(1),
            )
            .unwrap();
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
        assert_eq!(
            text.lines().filter(|l| l.starts_with("timestamp_")).count(),
            64
        );
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
}
