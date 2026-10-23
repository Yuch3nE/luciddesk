//! Platform-independent domain model for `LucidDesk`.
//!
//! A [`Workspace`] owns panels and desktop membership; Windows integration and
//! persistence are handled by separate crates.
//!
//! ```
//! use luciddesk_core::{Panel, PanelId, RectDip, Workspace};
//!
//! let mut workspace = Workspace::new();
//! workspace.add_panel(Panel::new(PanelId::new(1), "Work", RectDip::default()))?;
//! assert_eq!(workspace.panels().len(), 1);
//! # Ok::<(), luciddesk_core::WorkspaceError>(())
//! ```

mod appearance;
mod geometry;
mod identity;
mod item;
mod panel;
mod workspace;

pub use appearance::{Backdrop, BackdropKind, PaneOptions, PanelText, PanelTheme, TunableMaterial};
pub use geometry::{GridPosition, PointDip, RectDip};
pub use identity::{MonitorId, PanelId, ShellIdentity};
pub use item::{DesktopItem, DesktopPlacement};
pub use panel::{Panel, IconDisplay, IconSize};
pub use workspace::{PaneTabs, Workspace, WorkspaceError};

#[cfg(test)]
mod tests;
