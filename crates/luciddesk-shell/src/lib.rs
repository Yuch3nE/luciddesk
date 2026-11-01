//! Windows Shell enumeration, file operations, and UI integration.
mod activation;
mod apartment;
mod desktop;
mod drag_image;
mod error;
mod file_command;
mod folder_menu;
mod namespace;
mod native_layout;
mod native_menu;
mod notification;
mod rename;

pub use activation::{drag_shell_identities, open_shell_identity};
pub use apartment::ShellApartment;
pub use desktop::desktop_icons_hidden;
pub use drag_image::FileDragImage;
pub use luciddesk_core::ShellIdentity;
pub use error::ShellError;
pub use file_command::{
    FileCommand, copy_to_folder, drag_file_items, invoke_file_commands, paste_into_folder,
    show_file_items_menu,
};
pub use folder_menu::{FolderMenuResult, show_folder_menu};
pub use namespace::{
    ShellAttributes, ShellEntry, desktop_source_revision, enumerate_desktop_namespace,
    enumerate_desktop_source, enumerate_folder, local_app_data_path,
};
pub use native_layout::{
    NativeDesktopReader, NativeDesktopRevision, NativeDesktopSnapshot, native_desktop_snapshot,
};
pub use native_menu::{MenuInvocation, peek_desktop_item, show_isolated_item_menu};
#[cfg(feature = "desktop-menu-diagnostics")]
pub use native_menu::{show_desktop_item_menu, show_desktop_items_menu};
pub use notification::DesktopChangeSubscription;
pub use rename::{rename_shell_identity, rename_shell_item};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod menu_theme_tests;
