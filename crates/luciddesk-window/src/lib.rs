//! Display enumeration and application error reporting.
mod monitors;
mod message;
pub use monitors::{MonitorDescriptor, PixelRect, enumerate_monitors};
pub use message::{MessageKind, show_message, confirm_message, set_message_labels, set_message_theme, message_is_dark};

pub fn show_error(error: &str) {
    show_message(0, "LucidDesk", error, MessageKind::Error);
}
