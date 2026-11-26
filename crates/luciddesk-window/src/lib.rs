//! Display enumeration and application error reporting.
pub mod control_style;
pub mod controls;
mod monitors;
mod message;
pub use monitors::{MonitorDescriptor, PixelRect, enumerate_monitors};
pub use message::{
    MessageKind, confirm_action, confirm_message, message_is_dark, set_message_font, set_message_labels,
    set_message_theme, show_message,
};

pub fn show_error(error: &str) {
    show_message(0, "LucidDesk", error, MessageKind::Error);
}
