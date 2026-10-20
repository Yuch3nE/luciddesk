//! Explicit support traces are separate from the process-wide severity filter.
use crate::{Level, format};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
    time::Instant,
};

/// Bounded opt-in trace. The caller selects a path only after explicit enablement.
/// No background writes, automatic retries or interaction with the global logger.
pub struct TraceFile {
    file: File,
    started: Instant,
    remaining: usize,
    version: String,
    build: String,
}

impl TraceFile {
    /// Creates/truncates a support trace and writes a bounded, escaped report header.
    ///
    /// # Errors
    /// Returns file creation or header-write failures.
    pub fn create(
        path: &Path,
        version: &str,
        build: &str,
        report: &str,
        max_events: usize,
    ) -> io::Result<Self> {
        let mut file = File::create(path)?;
        file.write_all(format::header(report).as_bytes())?;
        Ok(Self {
            file,
            started: Instant::now(),
            remaining: max_events,
            version: version.chars().take(128).collect(),
            build: build.chars().take(128).collect(),
        })
    }

    /// Each attempt consumes one slot, including failed writes, to bound driver storms.
    ///
    /// # Errors
    /// Returns the file-write failure; a failed attempt still consumes its slot.
    #[track_caller]
    pub fn write(&mut self, component: &str, event: std::fmt::Arguments<'_>) -> io::Result<()> {
        if self.remaining == 0 {
            return Ok(());
        }
        self.remaining -= 1;
        let entry = format::entry(
            Level::Trace,
            component,
            &event.to_string(),
            &self.version,
            &self.build,
        );
        writeln!(
            self.file,
            "{entry} elapsed_ms={}",
            self.started.elapsed().as_millis()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_is_escaped_bounded_and_independent_of_global_logger() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.log");
        let mut trace = TraceFile::create(&path, "test", "build", "report\nline", 2).unwrap();
        trace
            .write("pane.render", format_args!("first\nline"))
            .unwrap();
        trace.write("pane.render", format_args!("second")).unwrap();
        let before = std::fs::read(&path).unwrap();
        trace
            .write("pane.render", format_args!("discarded"))
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let text = String::from_utf8(before).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains("message=\"first\\nline\""));
        assert!(text.contains("level=TRACE"));
        assert!(text.contains("elapsed_ms="));
        assert!(!dir.path().join("logs").exists());
    }
}
