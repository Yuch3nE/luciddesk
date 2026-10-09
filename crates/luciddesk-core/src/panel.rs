//! Pane state and presentation.
use crate::{Backdrop, PanelId, PanelTheme, RectDip};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
enum PanelSource {
    Desktop,
    Folder(PathBuf),
    Search,
}

/// A pane with a mutually exclusive desktop, folder, or search content source.
#[derive(Clone, Debug, PartialEq)]
// These flags are independent UI preferences, not mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
pub struct Panel {
    source: PanelSource,
    list_view: bool,
    fixed_grid: bool,
    free_layout: bool,
    theme: PanelTheme,
    always_on_top: bool,
    auto_hide: bool,
    id: PanelId,
    title: String,
    rect: RectDip,
    collapsed: bool,
    locked: bool,
    backdrop: Backdrop,
}

impl Panel {
    /// Creates a desktop pane, using the initial pane minimum size.
    #[must_use]
    pub fn new(id: PanelId, title: impl Into<String>, rect: RectDip) -> Self {
        Self {
            id,
            source: PanelSource::Desktop,
            list_view: false,
            fixed_grid: false,
            free_layout: false,
            title: title.into(),
            rect: RectDip::new(rect.x, rect.y, rect.width, rect.height),
            collapsed: false,
            auto_hide: false,
            theme: PanelTheme::System,
            always_on_top: false,
            locked: false,
            backdrop: Backdrop::DEFAULT,
        }
    }

    #[must_use]
    pub const fn id(&self) -> PanelId {
        self.id
    }

    /// A folder pane displays live children, independently of desktop membership.
    #[must_use]
    pub fn folder(&self) -> Option<&Path> {
        match &self.source {
            PanelSource::Folder(path) => Some(path),
            PanelSource::Desktop | PanelSource::Search => None,
        }
    }

    /// Selects a folder, replacing search content when a path is supplied.
    /// Clearing a folder returns to desktop content; search content is unaffected.
    pub fn set_folder(&mut self, path: Option<PathBuf>) {
        match path {
            Some(path) => {
                if self.folder().is_none() {
                    self.list_view = true;
                }
                self.fixed_grid = false;
                self.free_layout = false;
                self.source = PanelSource::Folder(path);
            },
            None if matches!(self.source, PanelSource::Folder(_)) => {
                self.source = PanelSource::Desktop;
            }
            None => {}
        }
    }

    /// Only ordinary desktop panels can share a tabbed window.
    #[must_use]
    pub const fn supports_tabs(&self) -> bool {
        matches!(self.source, PanelSource::Desktop)
    }

    #[must_use]
    pub const fn is_search(&self) -> bool {
        matches!(self.source, PanelSource::Search)
    }

    /// Enables search and clears the folder, list view, collapsed state, and auto-hide flag.
    /// Disabling search returns to desktop content without changing a folder pane.
    pub fn set_search(&mut self, enabled: bool) {
        if enabled {
            self.fixed_grid = false;
            self.free_layout = false;
            self.source = PanelSource::Search;
            self.list_view = false;
            self.collapsed = false;
            self.auto_hide = false;
        } else if self.is_search() {
            self.source = PanelSource::Desktop;
        }
    }

    /// Preserve ordinary pane icon cells, including empty cells.
    #[must_use]
    pub const fn fixed_grid(&self) -> bool { self.supports_tabs() && self.fixed_grid }
    pub const fn free_layout(&self) -> bool { self.fixed_grid() && self.free_layout }
    pub const fn set_free_layout(&mut self, enabled: bool) { self.free_layout = enabled; }
    pub const fn set_fixed_grid(&mut self, enabled: bool) { self.fixed_grid = enabled; if !enabled { self.free_layout = false; } }

    /// Whether desktop or folder items use rows instead of an icon grid.
    #[must_use]
    pub const fn list_view(&self) -> bool {
        self.list_view
    }
    pub const fn set_list_view(&mut self, enabled: bool) {
        self.list_view = enabled;
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn rect(&self) -> RectDip {
        self.rect
    }

    #[must_use]
    pub const fn collapsed(&self) -> bool {
        self.collapsed
    }

    #[must_use]
    pub const fn auto_hide(&self) -> bool {
        self.auto_hide
    }

    #[must_use]
    pub const fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    #[must_use]
    pub const fn theme(&self) -> PanelTheme {
        self.theme
    }
    pub const fn set_theme(&mut self, theme: PanelTheme) {
        self.theme = theme;
    }

    pub const fn set_always_on_top(&mut self, enabled: bool) {
        self.always_on_top = enabled;
    }

    pub const fn set_auto_hide(&mut self, enabled: bool) {
        self.auto_hide = enabled;
    }

    #[must_use]
    pub const fn locked(&self) -> bool {
        self.locked
    }

    #[must_use]
    pub const fn backdrop(&self) -> Backdrop {
        self.backdrop
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    pub fn set_rect(&mut self, rect: RectDip) {
        self.rect = RectDip::from_bounds(rect.x, rect.y, rect.width, rect.height);
    }

    pub const fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed;
    }

    pub const fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    pub const fn set_backdrop(&mut self, backdrop: Backdrop) {
        self.backdrop = backdrop;
    }
}
