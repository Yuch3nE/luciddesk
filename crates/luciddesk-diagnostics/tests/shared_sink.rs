//! Separate test process exercises the public API and process-wide sink.
use luciddesk_diagnostics::{self as diagnostics, Level};
use std::sync::atomic::{AtomicUsize, Ordering};
static REPORTS: AtomicUsize = AtomicUsize::new(0);
fn report() -> String {
    REPORTS.fetch_add(1, Ordering::Relaxed);
    "host report".into()
}
fn filtered_argument() -> &'static str {
    panic!("filtered argument was evaluated")
}
#[test]
fn host_and_library_share_filter_destination_metadata_and_cache() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("workspace.db");
    diagnostics::initialize(&database, "test-version", "test-revision", report);
    assert!(!dir.path().join("logs").exists());
    diagnostics::emit!(Level::Trace, "library", "{}", filtered_argument());
    assert_eq!(REPORTS.load(Ordering::Relaxed), 0);
    diagnostics::emit!(Level::Error, "host", "startup failure");
    let path = diagnostics::path().unwrap();
    let before = std::fs::read(&path).unwrap();
    diagnostics::emit!(Level::Debug, "library", "suppressed event");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    diagnostics::set_level(Level::Trace);
    std::thread::spawn(|| diagnostics::emit!(Level::Trace, "library", "callback event"))
        .join()
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("version=\"test-version\" build=\"test-revision\""));
    assert!(text.contains("callback event"));
    assert!(text.contains("startup failure"));
    assert_eq!(REPORTS.load(Ordering::Relaxed), 1);
    diagnostics::set_level(Level::Error);
    let before = std::fs::read(&path).unwrap();
    for _ in 0..100 {
        diagnostics::emit!(Level::Debug, "library", "menu opened");
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!database.exists());
}
