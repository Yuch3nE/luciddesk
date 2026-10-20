//! File filtering and I/O failure must never hide diagnostics from copied reports.
use luciddesk_diagnostics::{self as diagnostics, Level};

#[test]
fn memory_survives_missing_logger_failed_file_output_and_worker_recovery() {
    assert!(diagnostics::try_log(Level::Error, "before.init", "HRESULT=0x80004005").is_err());
    assert!(diagnostics::recent_report().contains("HRESULT=0x80004005"));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("logs"), "blocked directory").unwrap();
    diagnostics::initialize(
        &dir.path().join("workspace.db"),
        "test",
        "test",
        String::new,
    );
    diagnostics::log(Level::Info, "worker.start", "filtered from file");
    assert!(diagnostics::try_log(Level::Error, "hook.attach", "HRESULT=0x80070005").is_err());
    std::thread::spawn(|| diagnostics::log(Level::Info, "hook.attach", "recovered"))
        .join()
        .unwrap();
    let report = diagnostics::recent_report();
    assert!(report.contains("filtered from file"));
    assert!(report.contains("HRESULT=0x80070005"));
    assert!(report.contains("recovered"));
    assert!(!dir.path().join("workspace.db").exists());
}
