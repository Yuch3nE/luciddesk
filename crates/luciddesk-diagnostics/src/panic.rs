//! Opt-in process-host panic logging. Preserve the previous panic hook and unwind policy.
use crate::{Level, log};
use std::{backtrace::Backtrace, sync::Once};
static INSTALL: Once = Once::new();

/// Install once in the executable, never implicitly from a library or injected DLL.
pub fn install_panic_hook() {
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let cause = info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            log(
                Level::Error,
                "process.panic",
                &format!(
                    "location={} cause={cause}\nBacktrace:\n{}",
                    info.location()
                        .map_or_else(|| "unknown".into(), ToString::to_string),
                    Backtrace::force_capture()
                ),
            );
            previous(info);
        }));
    });
}
