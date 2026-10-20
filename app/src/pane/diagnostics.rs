//! On-demand reports retain desktop failures independently of recovery and log filtering.
use super::*;
use std::{
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

mod logs;
mod stages;
pub(super) use stages::{Operation, completed, failure};

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn report(s: &PaneApp) -> String {
    let mut report = crate::system_info::report();
    writeln!(report, "Report timestamp_unix_ms: {}", timestamp()).unwrap();
    writeln!(
        report,
        "Native desktop filter snapshot:\n{}",
        luciddesk_explorer::filter::desktop_filter_diagnostics()
            .as_deref()
            .unwrap_or("Unavailable: desktop view could not be discovered")
    )
    .unwrap();
    writeln!(report, "Process ID: {}", std::process::id()).unwrap();
    writeln!(
        report,
        "Diagnostic level: {}",
        luciddesk_diagnostics::level().label()
    )
    .unwrap();
    writeln!(
        report,
        "Desktop session: {}",
        if s.session.is_some() {
            "connected"
        } else {
            "disconnected"
        }
    )
    .unwrap();
    writeln!(
        report,
        "Desktop membership: {}",
        hybrid::membership_status(s)
    )
    .unwrap();
    if let Some(runtime) = &s.runtime {
        writeln!(report, "Reconnecting: {}\nConsecutive reconnect failures: {}\nLast connection attempt elapsed (ms): {}",
            runtime.reconnecting, runtime.reconnect_failures, runtime.last_attempt.elapsed().as_millis()).unwrap();
        writeln!(
            report,
            "\nCurrent desktop connection error:\n{}",
            runtime.desktop_error.as_deref().unwrap_or("None")
        )
        .unwrap();
        let log = luciddesk_diagnostics::path()
            .unwrap_or_else(|| runtime.path.with_file_name("logs").join("diagnostic.log"));
        writeln!(report, "Diagnostic log path: {}", log.display()).unwrap();
        logs::append(&mut report, &log.with_file_name("diagnostic.previous.log"));
        logs::append(&mut report, &log);
    } else {
        writeln!(report, "Desktop runtime: unavailable").unwrap();
    }
    report.push('\n');
    report.push_str(&luciddesk_diagnostics::recent_report());
    // CF_UNICODETEXT must not silently stop at a NUL in an original error.
    report
        .replace('\0', "␀")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
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
        let report = report(&app);
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
    fn missing_runtime_is_explicit() {
        let s = super::super::tests::test_state();
        assert!(report(&s).contains("Desktop runtime: unavailable"));
    }
}
