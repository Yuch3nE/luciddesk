//! Hybrid pane UI and persisted group interaction. Native desktop synchronization lives in hybrid.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
// Application state and native desktop integration.
mod hybrid;
mod runtime;
mod control;
mod events;
mod recovery;
mod display_layout;
mod wake;

// Panel content and configuration.
mod folder;
mod folder_context;
mod search;
mod settings;
mod tabs;
mod sorting;
mod fixed_grid;
mod free_layout;
mod layout_defaults;

// Window interaction.
mod window;
mod drag_drop;
mod keyboard;
mod rename;
mod peek;
mod marquee;
mod columns;
mod layout;
mod scrollbar;
mod snap;
mod auto_hide;
mod visibility;
mod quick_reveal;
mod show_hotkey;

// Rendering and shared visual resources.
mod render;
pub(crate) use render::debug as render_debug;
mod assets;
mod image_pool;
mod scaled_icons;
mod canvas;
mod native_graphics;
mod composition;
mod acrylic;
mod animation;
mod theme;
mod fonts;
mod label;
mod title_emoji;
mod header_divider;
mod compact_menu;
mod shell_menu;

pub(crate) mod menu;
use events::handle;
pub use hybrid::run;

use luciddesk_core::{
    DesktopItem, DesktopPlacement, GridPosition, Panel, PanelId, RectDip, ShellIdentity, Workspace,
};
use luciddesk_shell::{ShellApartment, open_shell_identity};
use luciddesk_storage::WorkspaceStore;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsWindow, PostMessageW, WindowFromPoint,
};

#[derive(Clone)]
pub struct Item {
    pub details: ItemDetails,
    pub identity: ShellIdentity,
    pub label: String,
    pub image: Option<Arc<assets::Pixels>>,
}

#[derive(Clone, Default, PartialEq)]
pub struct ItemDetails {
    pub grid_position: Option<GridPosition>,
    pub free_position: Option<luciddesk_core::PointDip>,
    pub kind: String,
    pub modified: String,
    pub folder: bool,
    pub modified_time: Option<std::time::SystemTime>,
    pub size: Option<u64>,
}

impl ItemDetails {
    pub fn kind_text(&self) -> &str {
        // Shell type names use the Windows language and may be cached across
        // application language changes. Resolve folders when rendering instead.
        if self.folder { crate::i18n::text("ui-folder-type") } else { &self.kind }
    }
}

fn same_items(left: &[Item], right: &[Item]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            a.identity == b.identity
                && a.details == b.details
                && a.label == b.label
                && match (&a.image, &b.image) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        })
}

mod model;
use model::GroupModel;

struct View {
    id: PanelId,
    target: Rc<std::cell::Cell<PanelId>>,
    window: windows_window::Window,
    model: Rc<RefCell<GroupModel>>,
}

struct PaneApp {
    sorting: sorting::State,
    wake: wake::Wake,
    folders: HashMap<PanelId, folder::Source>,
    tab_models: HashMap<PanelId, GroupModel>,
    settings: Option<windows_window::Window>,
    // Desktop membership is suspended while Explorer/its compatible Hook is unavailable.
    session: Option<hybrid::Session>,
    drops: Vec<drag_drop::target::Registration>,
    runtime: Option<runtime::State>,
    workspace: Workspace,
    store: WorkspaceStore,
    views: Vec<View>,
    images: HashMap<String, Arc<assets::Pixels>>,
    receiver: mpsc::Receiver<Loaded>,
}

struct Loaded {
    requested: Vec<String>,
    images: Vec<(String, assets::Pixels)>,
}

#[derive(Clone)]
enum Event {
    NewTab(bool),
    SelectTab(PanelId),
    CloseTab,
    CloseTabId(PanelId),
    MoveTab(i8),
    MoveTabId(PanelId, i8),
    RenameTab(PanelId),
    DetachTab(PanelId),
    FinishPaneMove(bool),
    PreviewPaneMove,
    ToggleHeaderDivider,
    ToggleCompactMenu,
    SetTitleEmojiColor(bool),
    PaneItemFocus,
    BeginItemMenu(Rc<RefCell<Option<Result<Rc<luciddesk_explorer::filter::FilterSession>, String>>>>),
    EndItemMenu,
    RenameItem(ShellIdentity),
    RenameTitle,
    SetTitle(String),
    ClosePane,
    Settings,
    Refresh,
    RetryDesktop,
    ExportBackup,
    CreateBackup,
    RestoreBackupPath(std::path::PathBuf),
    ExportBackupPath(std::path::PathBuf),
    DeleteBackup(std::path::PathBuf),
    RestoreBackup,
    OpenBackups,
    OpenConfigDirectory,
    ReloadConfig,
    ToggleAutoHide,
    ToggleTopmost,
    ToggleLocked,
    SetCornerRadius(f32),
    SetIconGrid(f32),
    SetPanelText(luciddesk_core::PanelText),
    ToggleTextProtection,
    ToggleBorder,
    ToggleSnap,
    ResetPaneOptions,
    Theme(luciddesk_core::PanelTheme),
    // Hover-driven window visibility; never persisted as the manual fold preference.
    AutoHideCollapsed(bool),
    Moving(*mut RECT),
    Sizing(*mut RECT, RECT, u32),
    Material(luciddesk_core::Backdrop),
    New,
    EnableSearch,
    ToggleSearch,
    NewFolder,
    MapFolder(std::path::PathBuf),
    ChangeFolder,
    SetFolder(std::path::PathBuf),
    OpenFolder,
    SortFolder(u8),
    SortFolderMenu(u8),
    SortPane(PanelId, bool),
    SortPaneColumn(PanelId, u8),
    ToggleFixedGrid,
    ToggleGridAlignment,
    FolderItemCreated(std::path::PathBuf),
    SetFolderColumns([f32; 4]),
    ToggleFolderColumn(u8),
    FolderBack,
    FolderHome,
    Activate(usize),
    ActivateSelection,
    Peek,
    FileCommand(luciddesk_shell::FileCommand),
    FileDrag(Option<luciddesk_shell::FileDragImage>),
    ToggleListView,
    Drop { index: usize, point: POINT, offset: luciddesk_core::PointDip },
    Geometry(RectDip),
    Collapse,
    Exit,
}

fn items_for(state: &PaneApp, id: PanelId) -> Vec<Item> {
    if state.workspace.panel(id).is_some_and(Panel::is_search) {
        return Vec::new();
    }
    if state
        .workspace
        .panel(id)
        .is_some_and(|p| p.folder().is_some())
    {
        return state
            .folders
            .get(&id)
            .map(|source| source.items.clone())
            .unwrap_or_default();
    }
    desktop_items_for(state, id, None).unwrap()
}

/// Compare borrowed desktop data before copying unchanged names and identities.
fn desktop_items_for(state: &PaneApp, id: PanelId, current: Option<&[Item]>) -> Option<Vec<Item>> {
    let panel = state.workspace.panel(id);
    let free_grid = panel.filter(|p| p.free_layout()).map(|_| free_layout::grid(&state.workspace));
    let fixed = panel.is_some_and(Panel::fixed_grid);
    let details = |item: &luciddesk_core::DesktopItem| ItemDetails {
        free_position: free_grid.map(|grid| free_layout::point(item, grid)),
        grid_position: if fixed { match item.placement() {
            DesktopPlacement::Pane { position, .. } => Some(*position), _ => None,
        }} else { None },
        ..Default::default()
    };
    let items: Vec<_> = ordered_desktop_items(&state.workspace, id).into_iter()
        .map(|item| (item, details(item), state.images.get(&item.identity().persistent_key())))
        .collect();
    if current.is_some_and(|current| current.len() == items.len()
        && current.iter().zip(&items).all(|(old, (item, details, image))| {
            old.identity == *item.identity() && old.label == item.display_name() && old.details == *details
                && match (old.image.as_ref(), *image) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        })) { return None; }
    Some(items.into_iter().map(|(item, details, image)| Item {
        details,
        identity: item.identity().clone(),
        label: item.display_name().to_string(),
        image: image.cloned(),
    }).collect())
}

fn ordered_desktop_items(workspace: &Workspace, id: PanelId) -> Vec<&luciddesk_core::DesktopItem> {
    let mut items: Vec<_> = workspace
        .desktop_items()
        .iter()
        .filter_map(|item| {
            if let DesktopPlacement::Pane { pane_id, position } = item.placement() {
                (*pane_id == id).then_some(((position.row, position.column), item))
            } else {
                None
            }
        })
        .collect();
    items.sort_by(|(a, left), (b, right)| a.cmp(b).then_with(|| left.identity().persistent_key().cmp(&right.identity().persistent_key())));
    items.into_iter().map(|(_, item)| item).collect()
}

fn create_model(state: &PaneApp, id: PanelId) -> Result<GroupModel, String> {
    let panel = state.workspace.panel(id).ok_or(crate::i18n::text("ui-tab-closed"))?;
    let items = items_for(state, id);
    Ok(GroupModel {
        merge_preview: Vec::new(),
        merge_occluded: false,
        tabs: Vec::new(),
        active_tab: id,
        sort_orders: state.sorting.orders.clone(),
        folder_sort: (0, false),
        folder_columns: folder::saved_columns(&state.store, id)?,
        folder_visible_columns: folder::visible_columns(&state.store, id)?,
        folder_navigation: [false; 2],
        list_view: panel.list_view(),
        fixed_grid: panel.fixed_grid(),
        free_layout: panel.free_layout(),
        folder: panel.folder().map(Path::to_path_buf),
        folder_status: None,
        options: state.workspace.pane_options(),
        theme: panel.theme(),
        dark: theme::is_dark(panel.theme()),

        hovered_item: None,
        scrollbar: Default::default(),
        focused: false,
        auto_hide: panel.auto_hide(),
        locked: panel.locked(),
        reveal: if panel.collapsed() { 0.0 } else { 1.0 },
        hovered_tab: None,
        hovered_button: None,
        pressed_button: None,
        backdrop: panel.backdrop(),
        native_material: false,
        title: panel.title().to_string(),
        items,
        icon_size: layout::DESKTOP_ICON_SIZE,
        selected: None,
        selection: Default::default(),
        selection_anchor: None,
        renaming: None,
        scroll: 0,
        scroll_x: 0.0,
        geometry_cache: Default::default(),
        collapsed: panel.collapsed(),
        // Desktop membership is available before creating the view. Only folder
        // sources have an asynchronous inventory to wait for; icons load separately.
        loading: panel.folder().is_some(),
    })
}

fn create_view(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<(), String> {
    if !state.borrow().workspace.tab_visible(id) { return Ok(()); }
    folder::ensure(&mut state.borrow_mut(), id)?;
    let panel = state.borrow().workspace.panel(id).unwrap().clone();
    let mut initial = create_model(&state.borrow(), id)?;
    tabs::decorate(&state.borrow().workspace, id, &mut initial);
    let model = Rc::new(RefCell::new(initial));
    let target = Rc::new(std::cell::Cell::new(id));
    let event_target = target.clone();
    let weak = Rc::downgrade(state);
    let callback = move |event| {
        let Some(state) = weak.upgrade() else {
            return false;
        };
        if matches!(event, Event::ClosePane) {
            let hwnd = state.borrow().views.iter().find(|v| v.id == event_target.get())
                .map(|v| v.window.hwnd().cast());
            if hwnd.is_some_and(visibility::request_close) { return false; }
        }
        let closing = matches!(event, Event::ClosePane);
        let wake_needed = !matches!(&event, Event::Moving(_) | Event::Sizing(..));
        let result = handle(&state, event_target.get(), event);
        if wake_needed {
            state.borrow().wake.notify();
        }
        match result {
            Ok(done) => done,
            Err(error) => {
                if closing {
                    let hwnd = state.borrow().views.iter().find(|v| v.id == event_target.get())
                        .map(|v| v.window.hwnd().cast());
                    if let Some(hwnd) = hwnd {
                        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, visibility::RESTORE, 0, 0); }
                    }
                }
                luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.mod", &format!("{error}"));
                window::error(&error);
                false
            }
        }
    };
    let window = if panel.is_search() {
        search::create(panel.rect(), Rc::clone(&model), callback)?
    } else {
        window::create(panel.rect(), Rc::clone(&model), callback)?
    };
    window::set_layer(window.hwnd().cast(), panel.always_on_top());
    state.borrow().wake.watch_window(window.hwnd() as isize)?;
    state.borrow_mut().views.push(View { id, target, window, model });
    display_layout::place(&state.borrow(), id);
    if state.borrow().session.is_some() || panel.folder().is_some() {
        hybrid::register_drop(state, id)?;
    }
    Ok(())
}

fn refresh_views(state: &mut PaneApp) {
    refresh_changed_views(state, false);
}

fn refresh_changed_views(state: &mut PaneApp, force: bool) {
    sorting::maintain(state);
    for view in &state.views {
        if state.workspace.panel(view.id).is_some_and(Panel::is_search) {
            continue;
        }
        let mut model = view.model.borrow_mut();
        tabs::decorate(&state.workspace, view.id, &mut model);
        if model.folder.is_some() {
            model.folder_sort = state
                .folders
                .get(&view.id)
                .map_or((0, false), |source| source.sort);
            let navigation = state
                .folders
                .get(&view.id)
                .map_or([false; 2], folder::Source::navigation);
            let status = state
                .folders
                .get(&view.id)
                .and_then(|source| source.status.clone());
            let loading = state
                .folders
                .get(&view.id)
                .is_none_or(|source| source.loading);
            if model.folder_status != status
                || model.loading != loading
                || model.folder_navigation != navigation
            {
                model.folder_navigation = navigation;
                model.folder_status = status;
                model.loading = loading;
                unsafe {
                    InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
                }
            }
        }
        let items = if let Some(source) = state.folders.get(&view.id) {
            // Compare the borrowed snapshot before cloning all names, metadata
            // and pixel Arcs when another pane triggered this refresh.
            if !force && same_items(&model.items, &source.items) && !model.loading {
                continue;
            }
            source.items.clone()
        } else {
            let current = (!force && !model.loading).then_some(model.items.as_slice());
            let Some(items) = desktop_items_for(state, view.id, current) else { continue; };
            items
        };
        model.replace_items(items);
        model.hovered_item = None;
        let hwnd = view.window.hwnd().cast();
        let mut bounds = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &raw mut bounds);
        }
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        model.scroll_x = model.horizontal_offset((bounds.right - bounds.left) as f32 / scale);
        model.scroll = model.scroll.min(
            model
                .grid(
                    (bounds.right - bounds.left) as f32 / scale,
                    (bounds.bottom - bounds.top) as f32 / scale,
                )
                .max_scroll(model.items.len()),
        );
        if model.selected.is_some_and(|i| i >= model.items.len()) {
            model.selected = None;
        }
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
}

fn save(state: &mut PaneApp) -> Result<(), String> {
    save_state(state, false)
}

fn save_state(state: &mut PaneApp, record_layout: bool) -> Result<(), String> {
    state.workspace.sync_tab_windows();
    hybrid::sync(state)?;
    let layout = record_layout.then(|| display_layout::capture(state)).flatten();
    state
        .store
        .save_workspace_with_layout(
            &state.workspace,
            layout.as_ref().map(|(topology, bounds)| (topology.as_str(), bounds.as_slice())),
        )
        .map_err(|e| crate::i18n::format("ui-could-not-save-group", &[("e", format!("{}", e))]))
}

fn remove_panel(workspace: &mut Workspace, id: PanelId) {
    workspace.remove_panel(id);
    for item in workspace.desktop_items_mut() {
        if matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == id) {
            item.set_placement(DesktopPlacement::default());
        }
    }
}

fn set_order(workspace: &mut Workspace, id: PanelId, items: &[Item]) {
    if workspace
        .panel(id)
        .is_some_and(|panel| panel.folder().is_some() || panel.is_search())
    {
        return;
    }
    // Monitor surfaces are not panes. Their remaining icons retain absolute positions.
    if workspace.panel(id).is_none() {
        return;
    }
    let columns = fixed_grid::visible_columns(workspace, id);
    let free = workspace.panel(id).is_some_and(Panel::free_layout);
    let metrics = free_layout::grid(workspace);
    let fixed = workspace.panel(id).is_some_and(Panel::fixed_grid);
    for (position, item) in items.iter().enumerate() {
        if let Some(entry) = workspace.desktop_item_mut(&item.identity) {
            fixed_grid::set_position(entry, id, if fixed { fixed_grid::position(position, columns) } else { GridPosition::new(position as u32, 0) }, free, metrics);
        }
    }
}

// Persist a unique pane-local order before Shell replaces its inventory order.
// Also repair legacy grid coordinates and gaps left by items dragged out.
fn normalize_pane_orders(state: &mut PaneApp) {
    let ids: Vec<_> = state.workspace.panels().iter().map(Panel::id).collect();
    for id in ids {
        if state.workspace.panel(id).is_some_and(Panel::fixed_grid) { continue; }
        let items = items_for(state, id);
        set_order(&mut state.workspace, id, &items);
    }
}

#[cfg(test)]
fn transfer(
    state: &mut PaneApp,
    source: PanelId,
    index: usize,
    target: PanelId,
    at: usize,
) -> Result<(), String> {
    transfer_many(state, source, &[index], target, at)
}

fn transfer_many(
    state: &mut PaneApp,
    source: PanelId,
    indices: &[usize],
    target: PanelId,
    at: usize,
) -> Result<(), String> {
    if state.workspace.panel(target).is_none() {
        return Err(crate::i18n::text("ui-target-panel-unavailable").into());
    }
    let items = items_for(state, source);
    let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
    if selected.is_empty() || selected.iter().any(|i| *i >= items.len()) {
        return Err(crate::i18n::text("ui-selection-changed-drag-again").into());
    }
    let old = state.workspace.clone();
    if state.workspace.panel(target).is_some_and(Panel::fixed_grid) {
        let keys: Vec<_> = selected.iter().map(|i| items[*i].identity.persistent_key()).collect();
        let cell = fixed_grid::position(at, fixed_grid::columns(&state.workspace, target));
        fixed_grid::place(&mut state.workspace, target, &keys, cell);
        if source != target && !state.workspace.panel(source).is_some_and(Panel::fixed_grid) {
            let remaining = items_for(state, source);
            set_order(&mut state.workspace, source, &remaining);
        }
        return sorting::finish_move(state, old, source, target);
    }
    let moving: Vec<_> = items
        .iter()
        .enumerate()
        .filter(|(i, _)| selected.contains(i))
        .map(|(_, item)| item.clone())
        .collect();
    let remaining: Vec<_> = items
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !selected.contains(i))
        .map(|(_, item)| item)
        .collect();
    let insertion = if source == target {
        at.saturating_sub(selected.iter().filter(|i| **i < at).count())
    } else {
        at
    };
    if !state.workspace.panel(source).is_some_and(Panel::fixed_grid) {
        set_order(&mut state.workspace, source, &remaining);
    }
    let mut destination = if source == target {
        remaining
    } else {
        items_for(state, target)
    };
    destination.splice(
        insertion.min(destination.len())..insertion.min(destination.len()),
        moving,
    );
    set_order(&mut state.workspace, target, &destination);
    sorting::finish_move(state, old, source, target)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod test_support;


fn load_log_level(store: &WorkspaceStore) -> Result<(), String> {
    let value = store.preference("log_level").map_err(|e| e.to_string())?.unwrap_or_else(|| "error".into());
    luciddesk_diagnostics::set_level(luciddesk_diagnostics::Level::parse(&value));
    Ok(())
}
