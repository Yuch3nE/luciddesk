//! Capture app state briefly, then collect native/file diagnostics without an app borrow.
use super::*;
use std::{fmt::Write, path::PathBuf};
mod logs;
mod stages;
pub(super) use stages::{Operation, completed, failure};

pub(super) struct Snapshot {
    state: String,
    log: Option<PathBuf>,
}
impl Snapshot {
    /// No file I/O, Shell discovery or callbacks; the caller can release PaneApp immediately.
    pub fn capture(s: &PaneApp) -> Self {
        let mut state = format!(
            "Desktop session: {}\nDesktop membership: {}\n",
            if s.session.is_some() {
                "connected"
            } else {
                "disconnected"
            },
            hybrid::membership_status(s)
        );
        let log = if let Some(runtime) = &s.runtime {
            writeln!(state, "Reconnecting: {}\nConsecutive reconnect failures: {}\nLast connection attempt elapsed (ms): {}\n\nCurrent desktop connection error:\n{}",
                runtime.reconnecting, runtime.reconnect_failures, runtime.last_attempt.elapsed().as_millis(),
                runtime.desktop_error.as_deref().unwrap_or("None")).unwrap();
            Some(runtime.path.with_file_name("logs").join("diagnostic.log"))
        } else {
            state.push_str("Desktop runtime: unavailable\n");
            None
        };
        Self { state, log }
    }

    pub fn report(self) -> String {
        let mut report = crate::system_info::report();
        writeln!(
            report,
            "Report generated at (computer local time): {}\nProcess ID: {}\nDiagnostic level: {}",
            luciddesk_diagnostics::local_timestamp(),
            std::process::id(),
            luciddesk_diagnostics::level().label()
        )
        .unwrap();
        report.push_str(&self.state);
        writeln!(
            report,
            "\nNative desktop filter snapshot:\n{}",
            luciddesk_explorer::filter::desktop_filter_diagnostics()
                .as_deref()
                .unwrap_or("Unavailable: desktop view could not be discovered")
        )
        .unwrap();
        report.push_str(&luciddesk_diagnostics::storage_report());
        // Capture recent records before file I/O; event sequences correlate the two sources.
        report.push_str(&luciddesk_diagnostics::recent_report());
        if let Some(log) = luciddesk_diagnostics::path().or(self.log) {
            writeln!(report, "Diagnostic log path: {}", log.display()).unwrap();
            logs::append(&mut report, &log.with_file_name("diagnostic.previous.log"));
            logs::append(&mut report, &log);
        }
        // CF_UNICODETEXT must not silently stop at a NUL in an original error.
        report
            .replace('\0', "␀")
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_keeps_errors_after_recovery_and_logging_never_borrows_app_state() {
        let mut s = super::super::tests::test_state();
        s.runtime = Some(runtime::State::new("diagnostics-test/workspace.db".into()));
        let s = Rc::new(RefCell::new(s));
        let app = s.borrow_mut();
        let revision = app.store.change_count();
        let workspace = app.workspace.clone();
        let error = "LoadLibraryExW: HRESULT=0x80070005\nC:\\测试\\hook.dll\0remaining details";
        let result = Err::<(), _>(error).inspect_err(|cause| failure(Operation::AttachHook, cause));
        assert_eq!(
            result.unwrap_err(),
            error,
            "logging preserves the original error"
        );
        std::thread::spawn(|| completed(Operation::AttachHook, "recovered"))
            .join()
            .unwrap();
        let report = Snapshot::capture(&app).report();
        assert!(report.contains("category=hook-ipc"));
        assert!(report.contains("location=app"));
        assert!(report.contains("HRESULT=0x80070005"));
        assert!(report.contains("remaining details"));
        assert!(report.contains("recovered"));
        assert!(report.contains("Diagnostic log path:"));
        assert!(!report.contains('\0'));
        assert_eq!(app.store.change_count(), revision);
        assert_eq!(app.workspace, workspace);
    }

    #[test]
    fn captured_snapshot_outlives_the_app_borrow_and_keeps_original_state() {
        let app = Rc::new(RefCell::new(super::super::tests::test_state()));
        let snapshot = Snapshot::capture(&app.borrow());
        assert!(app.try_borrow_mut().is_ok());
        drop(app);
        assert!(snapshot.report().contains("Desktop runtime: unavailable"));
    }

    #[test]
    fn missing_runtime_is_explicit() {
        let s = super::super::tests::test_state();
        assert!(
            Snapshot::capture(&s)
                .report()
                .contains("Desktop runtime: unavailable")
        );
    }
}
