//! COM and graphics lifetimes for in-process native UI tests.
use luciddesk_shell::ShellApartment;
use std::sync::OnceLock;
use super::native_graphics::GraphicsLifetime;

/// Declare before windows, models and renderers so they drop before this guard.
/// Fields drop in declaration order: graphics caches first, then the thread STA.
#[must_use]
pub(crate) struct Apartment {
    _graphics: GraphicsLifetime,
    _sta: ShellApartment,
}

pub(crate) fn apartment() -> Apartment {
    // windows-core caches agile WinRT factories for the process. If the last
    // COM apartment exits between Rust test threads, Windows.UI.dll can unload
    // while UISettings' cached vtable still points into it. Keep exactly one MTA
    // usage reference until test-process exit, matching main's long-lived COM
    // lifetime. This reference is deliberately not decremented between tests;
    // the OS reclaims it at process exit. This module is compiled only for tests.
    static PROCESS_COM: OnceLock<usize> = OnceLock::new();
    PROCESS_COM.get_or_init(|| {
        // SAFETY: the API maintains a process-wide MTA reference and does not
        // change the calling thread's apartment. OLE below still creates an STA.
        unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() }
            .expect("initialize test-process COM lifetime").0 as usize
    });
    Apartment {
        _sta: ShellApartment::initialize_sta().expect("initialize test UI STA"),
        _graphics: GraphicsLifetime,
    }
}

#[test]
fn winrt_factories_survive_sequential_sta_threads() {
    // Without the process anchor, activation on the second thread raises an
    // access violation after Windows.UI.dll unloads at the first STA shutdown.
    for _ in 0..3 {
        std::thread::spawn(|| {
            let _apartment = apartment();
            use windows::Win32::System::Com::{CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER, APTTYPE_STA, APTTYPE_MAINSTA};
            let mut kind = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) }.unwrap();
            assert!(kind == APTTYPE_STA || kind == APTTYPE_MAINSTA);
            let settings = windows::UI::ViewManagement::UISettings::new().unwrap();
            settings.GetColorValue(windows::UI::ViewManagement::UIColorType::Foreground).unwrap();
        }).join().unwrap();
    }
}
