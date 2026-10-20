//! A host's optional system-report callback cannot take logging or business code down.
use luciddesk_diagnostics::{self as diagnostics, Level};
fn broken_report() -> String {
    panic!("injected host report failure")
}
#[test]
fn report_callback_panic_is_contained_and_original_event_is_still_written() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::initialize(
        &dir.path().join("workspace.db"),
        "test",
        "test",
        broken_report,
    );
    diagnostics::try_log(
        Level::Error,
        "original",
        "original error HRESULT=0x80070005",
    )
    .unwrap();
    let text = std::fs::read_to_string(diagnostics::path().unwrap()).unwrap();
    assert!(text.contains("callback panicked"));
    assert!(text.contains("original error HRESULT=0x80070005"));
    diagnostics::try_log(Level::Error, "next", "still usable").unwrap();
    assert!(diagnostics::recent_report().contains("still usable"));
}
