//! Isolated process exercises reentrant callbacks, concurrent capture and panic hooks.
use luciddesk_diagnostics::{self as diagnostics, Level};
use std::sync::atomic::{AtomicUsize, Ordering};
static REPORTS: AtomicUsize = AtomicUsize::new(0);
static PANICS: AtomicUsize = AtomicUsize::new(0);
fn report() -> String {
    REPORTS.fetch_add(1, Ordering::Relaxed);
    assert!(diagnostics::path().is_some());
    assert!(diagnostics::storage_report().contains("written="));
    let nested = diagnostics::try_log(Level::Error, "header.nested", "nested callback captured");
    assert_eq!(nested.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    "reentrant host report".into()
}
#[test]
fn logging_is_reentrant_bounded_and_keeps_file_memory_identity_and_panics() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::initialize(
        &dir.path().join("workspace.db"),
        "test-version",
        "test-build",
        report,
    );
    diagnostics::set_level(Level::Trace);
    let source_line = line!() + 1;
    diagnostics::try_log(Level::Error, "outer", "first record").unwrap();
    let path = diagnostics::path().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let line = text
        .lines()
        .find(|line| line.contains("component=\"outer\""))
        .unwrap();
    let memory = diagnostics::recent_report();
    assert!(
        memory.contains(line),
        "file and memory must retain the same sequence/timestamp/source"
    );
    assert!(line.contains(&format!("resilience.rs:{source_line}:")));
    assert!(memory.contains("nested callback captured"));
    assert!(memory.contains("version=\"test-version\" build=\"test-build\""));
    assert_eq!(REPORTS.load(Ordering::Relaxed), 1);
    let threads: Vec<_> = (0..8)
        .map(|worker| {
            std::thread::spawn(move || {
                for index in 0..50 {
                    diagnostics::log(
                        Level::Info,
                        "parallel",
                        &format!("worker={worker} index={index}"),
                    );
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    diagnostics::try_log(Level::Error, "critical", "must survive info storm").unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("must survive info storm")
    );
    assert!(diagnostics::storage_report().contains("rate_limited="));
    assert_eq!(REPORTS.load(Ordering::Relaxed), 1);

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        PANICS.fetch_add(1, Ordering::Relaxed);
        previous(info);
    }));
    diagnostics::install_panic_hook();
    diagnostics::install_panic_hook();
    let panic_line = line!() + 1;
    assert!(std::panic::catch_unwind(|| panic!("original panic detail")).is_err());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("component=\"process.panic\""));
    assert!(text.contains("original panic detail"));
    assert!(text.contains(&format!("resilience.rs:{panic_line}:")));
    assert!(text.contains("Backtrace:"));
    assert_eq!(PANICS.load(Ordering::Relaxed), 1);
}
