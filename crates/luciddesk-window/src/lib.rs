//! Display enumeration and application error reporting.
mod monitors;
mod message;
pub use monitors::{MonitorDescriptor, PixelRect, enumerate_monitors};
pub use message::{MessageKind, show_message};

pub fn show_error(error: &str) {
    show_message(0, "LucidDesk", error, MessageKind::Error);
}
