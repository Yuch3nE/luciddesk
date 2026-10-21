//! Pane UI with Explorer-owned desktop membership, drawing, layout and input.
mod audit;
mod audit_schedule;
mod icon_changes;
mod icons;
mod image_retention;
mod shortcut_retention;
mod inventory;
mod rename_transaction;
use super::assets::RECYCLE_BIN_PARSING_NAME;
use super::search::{everything_settings, hotkey as search_hotkey};
use super::*;
use audit::DesktopAudit;
pub(super) use icons::refresh_icons;
use icons::{queue_pane_icons, retain_pane_images};
use inventory::Inventory;
use luciddesk_explorer::{
    filter::FilterSession,
    notifications::{DESKTOP_EXIT_MESSAGE, DESKTOP_INPUT_MESSAGE},
};
pub(super) use rename_transaction::commit as rename_item;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub(super) struct Session {
    wake: wake::Wake,
    // Drop the Hook before its owner HWND and before the pane windows.
    hook: Rc<FilterSession>,
    _icon_subscription: luciddesk_shell::DesktopChangeSubscription,
    recycle_subscription: Option<luciddesk_shell::DesktopChangeSubscription>,
    recycle_retry: Instant,
    _controller: windows_window::Window,
    icons_dirty: Rc<RefCell<icon_changes::Pending>>,
    icon_due: Option<Instant>,
    icon_reload: Option<icons::RefreshJob>,
    icon_refresh_failures: u32,
    view: isize,
    snapshot: Inventory,
    last_reconcile: Instant,
    audit: DesktopAudit,
    last_tick: Instant,
    tick_deferred: bool,
    last_icon_scan: Instant,
    published: RefCell<Option<Vec<String>>>,
    membership_pending: Option<Instant>,
    sender: mpsc::Sender<Loaded>,
    requested: std::collections::HashSet<String>,
    icon_failures: HashMap<String, (u32, Instant)>,
    image_retention: image_retention::ImageRetention,
    shortcut_retention: shortcut_retention::ShortcutRetention,
    initial_batches: usize,
    menu_active: Cell<bool>,
    last_pane_input: Cell<Option<u32>>,
    pending_desktop_input: Rc<Cell<Option<u32>>>,
    dirty: Rc<Cell<bool>>,
    retry_after: Option<Instant>,
    last_failure: Option<String>,
    pending_workspace_save: bool,
    save_due: Option<Instant>,
}

pub(super) fn visibility_hook(s: &PaneApp) -> Option<(Rc<FilterSession>, bool)> {
    s.session.as_ref().map(|h| (Rc::clone(&h.hook),
        h.membership_pending.is_none() && !h.menu_active.get() && is_alive(h)))
}

struct OleApartment;
/// Desired Explorer-hidden names, independent of image loading and submitted IPC.
pub(super) fn desired_membership(s: &PaneApp) -> Vec<String> {
    let mut names: Vec<_> = s
        .workspace
        .desktop_items()
        .iter()
        .filter(|item| matches!(item.placement(), DesktopPlacement::Pane { .. }))
        .map(|item| {
            item.identity()
                .activation_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
pub(super) fn membership_status(s: &PaneApp) -> &'static str {
    let Some(session) = &s.session else {
        return "disconnected";
    };
    if !session.hook.is_alive() {
        return "disconnected";
    }
    if session.menu_active.get() {
        return "deferred";
    }
    if session.membership_pending.is_some()
        || session.published.borrow().as_ref() != Some(&desired_membership(s))
    {
        "pending"
    } else {
        "applied"
    }
}
pub(super) fn is_alive(session: &Session) -> bool {
    session.hook.is_alive()
}
impl Drop for OleApartment {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Ole::OleUninitialize();
        }
    }
}

pub fn run(path: &Path, title: Option<String>) -> Result<(), String> {
    unsafe {
        windows::Win32::System::Ole::OleInitialize(None).map_err(|e| e.to_string())?;
    }
    let _ole = OleApartment;
    let _graphics = super::native_graphics::GraphicsLifetime;
    let first_run = !path.exists();
    let mut store = WorkspaceStore::open(path).map_err(|e| e.to_string())?;
    load_log_level(&store)?;
    crate::plugins::initialize(&store)?;
    crate::i18n::initialize(&store)?;
    super::peek::load(&store)?;
    fonts::load(&store)?;
    interface_flags::HEADER_DIVIDER.load(&store)?;
    interface_flags::COMPACT_MENU.load(&store)?;
    interface_flags::TITLE_EMOJI.load(&store)?;
    search_hotkey::load(&store)?;
    everything_settings::load(&store)?;
    let mut workspace = store.load_workspace().map_err(|e| e.to_string())?;
    // Migrate older workspaces that allowed multiple search panes.
    let duplicates: Vec<_> = workspace
        .panels()
        .iter()
        .filter(|p| p.is_search())
        .skip(1)
        .map(Panel::id)
        .collect();
    if !duplicates.is_empty() {
        for id in duplicates {
            remove_panel(&mut workspace, id);
        }
        store
            .save_workspace(&workspace)
            .map_err(|e| e.to_string())?;
    }
    let monitors = luciddesk_window::enumerate_monitors();
    if workspace.panels().is_empty() && first_run {
        let mut pane = Panel::new(
            PanelId::new(1),
            title
                .clone()
                .unwrap_or_else(|| crate::i18n::text("ui-new-group").into()),
            display_layout::new_pane(&workspace, false),
        );
        pane.set_backdrop(luciddesk_core::Backdrop::Acrylic);
        layout_defaults::Mode::load(&store)?.apply(&mut pane);
        workspace.add_panel(pane).map_err(|e| e.to_string())?;
    } else if let Some(title) = title.filter(|_| !workspace.panels().is_empty()) {
        let id = workspace.panels()[0].id();
        workspace.panel_mut(id).unwrap().set_title(title);
    }
    let (_, receiver) = mpsc::channel();
    let state = Rc::new(RefCell::new(PaneApp {
        monitors: Default::default(),
        monitor_child: false,
        sorting: Default::default(),
        wake: Default::default(),
        folders: HashMap::new(),
        tab_models: HashMap::new(),
        settings: None,
        session: None,
        drops: Vec::new(),
        runtime: Some(runtime::State::new(path.to_path_buf())),
        workspace,
        store,
        views: Vec::new(),
        images: HashMap::new(),
        receiver,
    }));
    display_layout::initialize(&mut state.borrow_mut(), monitors)?;
    {
        let mut s = state.borrow_mut();
        let workspace = s.workspace.clone();
        s.store
            .save_workspace(&workspace)
            .map_err(|e| e.to_string())?;
    }
    runtime::reconnect(&state);
    let search_enabled = everything_settings::enabled(&state.borrow().store)?;
    let ids: Vec<_> = state
        .borrow()
        .workspace
        .panels()
        .iter()
        .filter(|p| {
            (!p.is_search() || search_enabled)
                && (tabs::independent(&state.borrow().workspace, p.id())
                    || state.borrow().session.is_some())
        })
        .map(Panel::id)
        .collect();
    for id in ids {
        create_view(&state, id)?;
    }
    monitor::process::configure(&state)?;
    let tray_state = Rc::downgrade(&state);
    let appearance_state = Rc::downgrade(&state);
    let tray = crate::tray::Tray::new(
        move || {
            appearance_state
                .upgrade()
                .and_then(|state| {
                    let s = state.borrow();
                    let appearance = s.workspace.appearance().or_else(|| {
                        s.workspace
                            .panels()
                            .first()
                            .map(|p| (p.theme(), p.backdrop()))
                    });
                    let (theme, backdrop) = appearance.unwrap_or((
                        luciddesk_core::PanelTheme::System,
                        luciddesk_core::Backdrop::Mica,
                    ));
                    Some((
                        theme,
                        backdrop,
                        search::everything_settings::enabled(&s.store).unwrap_or(false),
                    ))
                })
                .unwrap_or((
                    luciddesk_core::PanelTheme::System,
                    luciddesk_core::Backdrop::Mica,
                    false,
                ))
        },
        move |action| {
            let Some(state) = tray_state.upgrade() else {
                return;
            };
            match action {
                crate::tray::Action::Search => search::hotkey::activate(&state),
                crate::tray::Action::OpenConfig => {
                    recovery::request(&state, &Event::OpenConfigDirectory)
                }
                crate::tray::Action::Refresh => {
                    let mut s = state.borrow_mut();
                    refresh_icons(&mut s);
                    for source in s.folders.values() {
                        source.refresh();
                    }
                    for view in &s.views {
                        if s.workspace.panel(view.id).is_some_and(Panel::is_search) {
                            search::refresh(view.window.hwnd().cast());
                        }
                    }
                    s.wake.notify();
                }
                crate::tray::Action::NewFolder => {
                    if let Err(error) = handle(&state, PanelId::new(0), Event::NewFolder) {
                        window::error(&error);
                    }
                }
                crate::tray::Action::Settings => {
                    if let Err(error) = handle(&state, PanelId::new(0), Event::Settings) {
                        window::error(&error);
                    }
                }
                crate::tray::Action::Exit => windows_window::quit(),
                crate::tray::Action::Show => quick_reveal::show_all(&state),
                crate::tray::Action::New => {
                    if let Err(error) = handle(&state, PanelId::new(0), Event::New) {
                        window::error(&error);
                    }
                }
            }
        },
    )?;
    display_layout::record(&mut state.borrow_mut())?;
    let supervisor = runtime::supervisor(&state)?;
    let control = control::start(&state)?;
    // Settings are opened only by an explicit user action, including offline startup.
    windows_window::run();
    if let Err(error) = desktop_toggle::restore(&state) {
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Error, "desktop.visibility", "Shutdown restore: {error}");
    }
    crate::pane::render_debug::render_trace(format_args!("shutdown: stop supervisor"));
    state.borrow_mut().monitors = Default::default();
    drop(control);
    drop(supervisor);
    drop(tray);
    // DETACH restores Explorer's Shell view and can synchronously call back
    // into our windows. Do not hold PaneApp's RefMut across native teardown.
    let session = state.borrow_mut().session.take();
    crate::pane::render_debug::render_trace(format_args!("shutdown: detach Explorer begin"));
    drop(session);
    crate::pane::render_debug::render_trace(format_args!("shutdown: detach Explorer returned"));
    Ok(())
}

pub(super) fn connect(state: &Rc<RefCell<PaneApp>>, _path: &Path) -> Result<(), String> {
    desktop_toggle::connection_changed();
    use diagnostics::{Operation, failure};
    if luciddesk_explorer::conflicting_desktop_extension() {
        let error = crate::i18n::text("ui-close-other-desktop-organizers-first");
        failure(Operation::Conflict, error);
        return Err(error.into());
    }
    // Hidden icons are a display preference, not evidence of an unusable Shell
    // view. Validate discovery, enumeration and COM capabilities below.
    let view = luciddesk_explorer::desktop_view()
        .inspect_err(|error| failure(Operation::DiscoverView, error))?;
    let dirty = Rc::new(Cell::new(false));
    let notify = Rc::clone(&dirty);
    let icons_dirty = Rc::new(RefCell::new(icon_changes::Pending::default()));
    let icon_notify = Rc::clone(&icons_dirty);
    let input_receiver = Rc::downgrade(state);
    let pending_desktop_input = Rc::new(Cell::new(None));
    let pending_input = Rc::clone(&pending_desktop_input);
    let work_ready = state.borrow().wake.clone();
    let controller = windows_window::Window::new("LucidDesk Hybrid Controller")
        .size(1, 1)
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
        .on_message(move |_, message, wparam, lparam| {
            if unsafe { crate::window_visibility::defer_show(message, lparam, false) } {
                return Some(0);
            }
            if message == WM_DESTROY {
                // Releasing a failed/stale connection must not quit independent panes.
                Some(0)
            } else if message == DESKTOP_EXIT_MESSAGE {
                work_ready.notify();
                Some(0)
            } else if message == ICON_CHANGE_MESSAGE {
                notify.set(true);
                work_ready.notify();
                if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Debug) {
                    luciddesk_diagnostics::log(
                        luciddesk_diagnostics::Level::Debug,
                        "pane.hybrid",
                        &format!("icon-notify event={:x}", lparam),
                    );
                }
                icon_notify
                    .borrow_mut()
                    .add(unsafe { icon_changes::capture(wparam, lparam as u32) });
                Some(0)
            } else if message == RECYCLE_CHANGE_MESSAGE {
                work_ready.notify();
                if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Debug) {
                    luciddesk_diagnostics::log(
                        luciddesk_diagnostics::Level::Debug,
                        "pane.hybrid",
                        &format!("recycle-notify event={:x}", lparam),
                    );
                }
                icon_notify
                    .borrow_mut()
                    .add([icon_changes::Change::Name(RECYCLE_BIN_PARSING_NAME.into())]);
                Some(0)
            } else if message == luciddesk_explorer::notifications::DESKTOP_TOGGLE_MESSAGE {
                if wparam as isize == view {
                    desktop_toggle::request(lparam as u32);
                    work_ready.notify();
                }
                Some(0)
            } else if message == DESKTOP_INPUT_MESSAGE {
                work_ready.notify();
                if wparam as isize == view {
                    pending_input.set(Some(lparam as u32));
                    if let Some(state) = input_receiver.upgrade() {
                        if let Ok(s) = state.try_borrow() {
                            clear_pane_selection_on_desktop_input(&s);
                        }
                    }
                }
                Some(0)
            } else {
                None
            }
        })
        .create()
        .inspect_err(|error| failure(Operation::Controller, error))
        .map_err(|e| e.to_string())?;
    // Subscribe before reading so changes during initialization are not lost.
    let icon_subscription = luciddesk_shell::DesktopChangeSubscription::register(
        controller.hwnd() as isize,
        ICON_CHANGE_MESSAGE,
    )
    .inspect_err(|error| failure(Operation::SubscribeDesktop, error))
    .map_err(|error| error.to_string())?;
    // Warm and validate the Shell view from the client before injecting work
    // into Explorer's STA. During startup, the HWND can exist before ShellWindows
    // is ready; asking Explorer to initialize it from its own callback can stall.
    // Reuse this snapshot below instead of performing a second enumeration.
    // End the app borrow before Shell enumeration can dispatch COM callbacks.
    let managed = managed_identities(&state.borrow());
    let snapshot = inventory::capture(&managed)
        .inspect_err(|error| failure(Operation::InitialInventory, error))
        .map_err(|error| format!("桌面连接准备失败 (Inventory): {error}"))?;
    if luciddesk_explorer::desktop_view()
        .inspect_err(|error| failure(Operation::ValidateView, error))?
        != view
    {
        let error = crate::i18n::text("ui-desktop-view-changed-while-reading-reconnecting");
        failure(Operation::ValidateView, error);
        return Err(error.into());
    }
    let hook_dll = crate::desktop_component::prepare()
        .inspect_err(|error| failure(Operation::Component, error))
        .map_err(|error| format!("桌面连接准备失败 (Component): {error}"))?;
    let hook = FilterSession::connect(view, controller.hwnd() as isize, &hook_dll.path)
        .inspect_err(|error| failure(Operation::AttachHook, error))?;
    diagnostics::completed(
        Operation::AttachHook,
        format_args!(
            "view=0x{view:x} owner=0x{:x} dll={}",
            controller.hwnd() as usize,
            hook_dll.path.display()
        ),
    );
    let wake = state.borrow().wake.clone();
    let audit = DesktopAudit::start(wake.clone())?;
    let (sender, receiver) = mpsc::channel();
    let session = Session {
        wake,
        hook: Rc::new(hook),
        _icon_subscription: icon_subscription,
        recycle_subscription: None,
        recycle_retry: Instant::now(),
        _controller: controller,
        icons_dirty,
        icon_due: None,
        icon_reload: None,
        icon_refresh_failures: 0,
        view,
        snapshot,
        last_reconcile: Instant::now(),
        audit,
        last_tick: Instant::now() - Duration::from_millis(20),
        tick_deferred: false,
        last_icon_scan: Instant::now() - Duration::from_secs(1),
        published: RefCell::new(Some(Vec::new())),
        membership_pending: None,
        sender,
        requested: Default::default(),
        icon_failures: HashMap::new(),
        image_retention: Default::default(),
        shortcut_retention: Default::default(),
        initial_batches: 0,
        menu_active: Cell::new(false),
        last_pane_input: Cell::new(None),
        pending_desktop_input,
        dirty,
        retry_after: None,
        last_failure: None,
        pending_workspace_save: false,
        save_due: None,
    };
    let mut s = state.borrow_mut();
    s.receiver = receiver;
    s.session = Some(session);
    // A fresh hook has no desired membership. Cached pane images may produce
    // no loading-completion event, so publish now rather than waiting for one.
    if let Err(error) = reconcile_inventory(&mut s)
        .inspect_err(|error| failure(Operation::Reconcile, error))
        .and_then(|()| sync(&mut s))
    {
        let session = s.session.take();
        drop(s);
        // Detach and subscription teardown can synchronously call our windows.
        drop(session);
        return Err(error);
    }
    Ok(())
}

pub(super) fn register_drop(state: &Rc<RefCell<PaneApp>>, id: PanelId) -> Result<(), String> {
    if state
        .borrow()
        .workspace
        .panel(id)
        .is_some_and(|p| p.is_search() || p.is_monitor())
    {
        return Ok(());
    }
    let hwnd = state
        .borrow()
        .views
        .iter()
        .find(|v| v.id == id)
        .unwrap()
        .window
        .hwnd();
    let weak = Rc::downgrade(state);
    let registration = super::drag_drop::target::Registration::new(
        windows::Win32::Foundation::HWND(hwnd.cast()),
        if state
            .borrow()
            .workspace
            .panel(id)
            .is_some_and(|p| p.folder().is_some())
        {
            windows::Win32::System::Ole::DROPEFFECT_COPY
        } else {
            windows::Win32::System::Ole::DROPEFFECT_LINK
        },
        move |identities, commit| {
            let Some(state) = weak.upgrade() else {
                return false;
            };
            let Ok(s) = state.try_borrow_mut() else {
                return false;
            };
            if s.views
                .iter()
                .find(|v| v.id == id)
                .is_none_or(|v| v.model.borrow().collapsed)
            {
                return false;
            }
            if let Some(path) = s.folders.get(&id).map(|source| source.path.clone()) {
                if !folder::accepts_copy(identities, &path) {
                    return false;
                }
                if !commit {
                    return true;
                }
                let items = identities.to_vec();
                drop(s);
                return window::post_action(hwnd.cast(), move || {
                    if let Err(error) = luciddesk_shell::copy_to_folder(
                        windows::Win32::Foundation::HWND(hwnd.cast()),
                        &items,
                        &path,
                    ) {
                        window::error_for(hwnd.cast(), &error.to_string());
                    }
                });
            }
            let keys: Option<Vec<_>> = identities
                .iter()
                .map(|identity| {
                    s.workspace
                        .desktop_items()
                        .iter()
                        .find(|i| i.identity().equivalent_to(identity))
                        .map(|i| i.identity().clone())
                })
                .collect();
            let Some(keys) = keys else {
                return false;
            };
            if !commit {
                return true;
            }
            // Explorer is the OLE caller. Apply membership only after returning
            // from Drop, so its desktop STA can process our asynchronous request.
            drop(s);
            window::post_action(hwnd.cast(), move || {
                let mut s = state.borrow_mut();
                if s.workspace.panel(id).is_none()
                    || keys
                        .iter()
                        .any(|identity| s.workspace.desktop_item(identity).is_none())
                {
                    return;
                }
                let old = s.workspace.clone();
                normalize_pane_orders(&mut s);
                if s.workspace.panel(id).is_some_and(Panel::fixed_grid) {
                    let keys: Vec<_> = keys.iter().filter(|key| !matches!(s.workspace.desktop_item(key).unwrap().placement(), DesktopPlacement::Pane { pane_id, .. } if *pane_id == id))
                        .map(ShellIdentity::persistent_key).collect();
                    fixed_grid::place(&mut s.workspace, id, &keys, GridPosition::new(0, 0));
                } else {
                    let mut at = items_for(&s, id).len();
                    for identity in keys {
                        s.workspace
                            .desktop_item_mut(&identity)
                            .unwrap()
                            .set_placement(DesktopPlacement::Pane {
                                pane_id: id,
                                position: GridPosition::new(at as u32, 0),
                            });
                        at += 1;
                    }
                }
                if let Err(error) = save_placement(&mut s) {
                    s.workspace = old;
                    let restored = sync(&mut s);
                    crate::pane::render_debug::render_trace(format_args!(
                        "collection rollback: save={error}; restored={}", restored.is_ok()
                    ));
                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.hybrid", &format!("Desktop collection rejected: {error}"));
                }
                refresh_views(&mut s);
            })
        },
    )
    .map_err(|e| e.to_string())?;
    state.borrow_mut().drops.push(registration);
    Ok(())
}

pub(super) fn unregister_drop(state: &mut PaneApp, hwnd: windows_sys::Win32::Foundation::HWND) {
    state
        .drops
        .retain(|registration| registration.window().0 != hwnd);
}

fn managed_identities(s: &PaneApp) -> Vec<ShellIdentity> {
    s.workspace
        .desktop_items()
        .iter()
        .filter(|item| matches!(item.placement(), DesktopPlacement::Pane { .. }))
        .map(|item| item.identity().clone())
        .collect()
}
// Apply the captured inventory without another synchronous Shell enumeration.
fn reconcile_inventory(s: &mut PaneApp) -> Result<(), String> {
    s.session
        .as_mut()
        .unwrap()
        .image_retention
        .invalidate(&mut s.images);
    normalize_pane_orders(s);
    let h = s.session.as_mut().unwrap();
    let snapshot = &h.snapshot;
    h.shortcut_retention.reconcile(
        &mut s.workspace,
        snapshot
            .items
            .iter()
            .map(|item| DesktopItem::new(item.identity.clone(), item.display_name.clone())),
        Instant::now(),
    );
    let valid: Vec<_> = s.workspace.panels().iter().map(Panel::id).collect();
    for item in s.workspace.desktop_items_mut() {
        if matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if !valid.contains(pane_id))
        {
            item.set_placement(DesktopPlacement::default());
        }
    }
    let live: std::collections::HashSet<_> = snapshot
        .items
        .iter()
        .map(|item| item.identity.persistent_key())
        .collect();
    s.images.retain(|key, _| live.contains(key));
    let h = s.session.as_mut().unwrap();
    h.requested.retain(|key| live.contains(key));
    h.icon_failures.retain(|key, _| live.contains(key));
    h.last_reconcile = Instant::now();
    queue_pane_icons(s, true);
    s.session.as_mut().unwrap().pending_workspace_save = true;
    // Persistence is retried by tick before any new membership is published.
    refresh_views(s);
    Ok(())
}

fn persist_pending(
    pending: &mut bool,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    if !*pending {
        return Ok(false);
    }
    save()?;
    *pending = false;
    Ok(true)
}

fn retry_save(
    pending: &mut bool,
    due: &mut Option<Instant>,
    now: Instant,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    if due.is_some_and(|deadline| now < deadline) {
        return Ok(false);
    }
    let result = persist_pending(pending, save);
    *due = result.is_err().then_some(now + Duration::from_secs(5));
    result
}

fn persist_workspace(s: &mut PaneApp) -> Result<bool, String> {
    let h = s.session.as_mut().unwrap();
    retry_save(
        &mut h.pending_workspace_save,
        &mut h.save_due,
        Instant::now(),
        || {
            s.store
                .save_workspace(&s.workspace)
                .map_err(|error| error.to_string())
        },
    )
}
/// Hide a managed item only after its Pane image is available. Both normal
/// synchronization and committed renames must publish the same complete set.
fn hidden_names(s: &PaneApp) -> Vec<String> {
    let mut names: Vec<_> = s
        .workspace
        .desktop_items()
        .iter()
        .filter(|item| {
            matches!(item.placement(), DesktopPlacement::Pane { .. })
                && s.images.contains_key(&item.identity().persistent_key())
        })
        .map(|item| {
            item.identity()
                .activation_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

// An IPC error does not cancel a request already accepted by Explorer.
// Invalidate before sending so even a rollback to the previous (possibly empty)
// set must be published again after a timeout or partial native update.
fn publish_membership(
    published: &RefCell<Option<Vec<String>>>,
    names: Vec<String>,
    send: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if published.borrow().as_ref() == Some(&names) {
        return Ok(());
    }
    published.replace(None);
    send(&names)?;
    published.replace(Some(names));
    Ok(())
}

pub(super) fn sync(s: &mut PaneApp) -> Result<(), String> {
    let Some(h) = s.session.as_ref() else {
        return Ok(());
    };
    if h.menu_active.get() {
        return Ok(());
    }
    if !h.hook.is_alive() {
        let error = crate::i18n::text("ui-explorer-view-filter-disconnected");
        diagnostics::failure(diagnostics::Operation::ValidateHook, error);
        return Err(error.into());
    }
    let h = s.session.as_mut().unwrap();
    if let Some(started) = h.membership_pending {
        match h.hook.poll_hidden().inspect_err(|error| {
            diagnostics::failure(diagnostics::Operation::PollMembership, error)
        }) {
            Ok(false) => return Ok(()),
            Ok(true) => {
                h.membership_pending = None;
                h.audit.invalidate();
                crate::pane::render_debug::render_trace(format_args!(
                    "membership confirmed elapsed={}ms",
                    started.elapsed().as_millis()
                ));
            }
            Err(error) => {
                h.membership_pending = None;
                h.published.replace(None);
                h.dirty.set(true);
                return Err(error);
            }
        }
    }
    if h.pending_workspace_save {
        return Ok(());
    }
    if h.dirty.get() || !h.icons_dirty.borrow().is_empty() {
        h.image_retention.invalidate(&mut s.images);
    }
    let live = retain_pane_images(
        &s.workspace,
        &mut s.images,
        &mut h.image_retention,
        Instant::now(),
    );
    let names = hidden_names(s);
    let h = s.session.as_mut().unwrap();
    if let Err(error) = publish_membership(&h.published, names, |names| {
        h.hook.submit_hidden(names)?;
        h.audit.membership_changed();
        h.membership_pending = Some(Instant::now());
        crate::pane::render_debug::render_trace(format_args!(
            "membership submitted count={}",
            names.len()
        ));
        Ok(())
    })
    .inspect_err(|error| diagnostics::failure(diagnostics::Operation::PublishMembership, error))
    {
        // A failed immediate rollback may meet the same busy Explorer. Ensure
        // the supervisor retries the authoritative workspace after Drop returns.
        h.dirty.set(true);
        h.wake.notify();
        return Err(error);
    }
    h.icon_failures.retain(|key, _| live.contains(key));
    queue_pane_icons(s, true);
    Ok(())
}

pub(super) fn clear_desktop_selection(s: &PaneApp) -> Result<(), String> {
    if let Some(h) = &s.session {
        if !h.hook.is_alive() {
            // Focus can return again when the first error dialog closes. Let
            // the supervisor retire the session instead of opening another one.
            h.wake.notify();
            return Ok(());
        }
        h.last_pane_input
            .set(Some(unsafe { GetMessageTime() } as u32));
        if !h.menu_active.get() && h.membership_pending.is_none() {
            h.hook.clear_selection()?;
        }
    }
    Ok(())
}

fn desktop_input_is_newer(input: u32, pane_input: Option<u32>) -> bool {
    // GetMessageTime wraps every 49.7 days. Equal ticks conservatively preserve
    // the pane's choice; compare event times, never foreground activation order.
    pane_input.is_none_or(|pane| input.wrapping_sub(pane) as i32 > 0)
}

fn clear_pane_selection_on_desktop_input(s: &PaneApp) {
    let Some(h) = &s.session else {
        return;
    };
    let Some(input) = h.pending_desktop_input.take() else {
        return;
    };
    // The first native press can arrive before Explorer activates its desktop.
    // Ordering protects a newer pane selection without discarding that press.
    if h.menu_active.get() || !desktop_input_is_newer(input, h.last_pane_input.get()) {
        return;
    }
    for view in &s.views {
        let Ok(mut model) = view.model.try_borrow_mut() else {
            // A nested paint/COM callback may still borrow the model. Keep the
            // event for the existing UI tick instead of silently losing it.
            h.pending_desktop_input.set(Some(input));
            continue;
        };
        let changed = model.selected.is_some() || !model.selection.is_empty() || model.focused;
        model.clear_selection();
        model.focused = false;
        if changed {
            unsafe {
                InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
            }
        }
    }
}

// Called by the queued preview action, not when its shortcut is received.
// False means another menu/preview owns this session; do not open or resume it.
pub(super) fn pause_for_preview(s: &PaneApp, allow: bool) -> Result<bool, String> {
    if let Some(h) = &s.session {
        if allow {
            if h.menu_active.replace(true) {
                return Ok(false);
            }
            h.last_pane_input
                .set(Some(unsafe { GetMessageTime() } as u32));
        }
        if let Err(error) = h.hook.pause(allow) {
            if allow {
                h.menu_active.set(false);
            }
            return Err(error);
        }
        h.menu_active.set(allow);
    }
    Ok(true)
}

pub(super) fn begin_item_menu(s: &PaneApp) -> Result<Rc<FilterSession>, String> {
    let h = s
        .session
        .as_ref()
        .ok_or(crate::i18n::text("ui-desktop-filter-is-not-ready"))?;
    if h.membership_pending.is_some() {
        return Err(crate::i18n::text("ui-desktop-groups-are-syncing-try-again-shortly").into());
    }
    if h.menu_active.replace(true) {
        return Err(crate::i18n::text("ui-a-menu-or-preview-is-already-active").into());
    }
    h.last_pane_input
        .set(Some(unsafe { GetMessageTime() } as u32));
    Ok(Rc::clone(&h.hook))
}
pub(super) fn end_item_menu(s: &PaneApp) {
    if let Some(h) = &s.session {
        h.menu_active.set(false);
    }
}

pub(super) fn tick(s: &mut PaneApp) -> Result<(), String> {
    clear_pane_selection_on_desktop_input(s);
    let h = s.session.as_ref().unwrap();
    if h.retry_after
        .is_some_and(|deadline| Instant::now() < deadline)
    {
        return Ok(());
    }
    match tick_once(s) {
        Ok(()) => {
            let h = s.session.as_mut().unwrap();
            h.retry_after = None;
            h.last_failure = None;
        }
        Err(error) => {
            let h = s.session.as_mut().unwrap();
            if h.last_failure.as_ref() != Some(&error) {
                luciddesk_diagnostics::log(
                    luciddesk_diagnostics::Level::Warn,
                    "pane.hybrid",
                    &format!("Hybrid synchronization deferred: {error}"),
                );
                h.last_failure = Some(error);
            }
            // Sorting can invalidate the inventory while it is being read. Leave
            // the pane and its membership alive; re-read before the next publish.
            h.dirty.set(true);
            h.retry_after = Some(Instant::now() + Duration::from_millis(250));
        }
    }
    Ok(())
}

// Failed optional subscriptions retry on a bounded deadline. Callers also refresh
// the affected icon after each attempt, covering the notification gap.
fn retry_subscription<T>(
    slot: &mut Option<T>,
    due: &mut Instant,
    now: Instant,
    register: impl FnOnce() -> Result<T, String>,
) -> Result<bool, String> {
    if slot.is_some() || now < *due {
        return Ok(false);
    }
    *due = now + Duration::from_secs(30);
    *slot = Some(register()?);
    Ok(true)
}

pub(super) fn workspace_save_pending(s: &PaneApp) -> bool {
    s.session.as_ref().is_some_and(|h| h.pending_workspace_save)
}

fn tick_once(s: &mut PaneApp) -> Result<(), String> {
    s.session
        .as_mut()
        .unwrap()
        .shortcut_retention
        .expire(&s.workspace, Instant::now());
    if s.session.as_ref().unwrap().menu_active.get() {
        return Ok(());
    }
    {
        let h = s.session.as_mut().unwrap();
        let owner = h._controller.hwnd() as isize;
        let attempted = retry_subscription(
            &mut h.recycle_subscription,
            &mut h.recycle_retry,
            Instant::now(),
            || {
                luciddesk_shell::DesktopChangeSubscription::register_recycle_bin(
                    owner,
                    RECYCLE_CHANGE_MESSAGE,
                )
                .map_err(|error| error.to_string())
            },
        )
        .inspect_err(|error| diagnostics::failure(diagnostics::Operation::SubscribeRecycle, error));
        if !matches!(attempted, Ok(false)) {
            h.icons_dirty
                .borrow_mut()
                .add([icon_changes::Change::Name(RECYCLE_BIN_PARSING_NAME.into())]);
        }
        if let Err(error) = attempted {
            luciddesk_diagnostics::log(
                luciddesk_diagnostics::Level::Warn,
                "pane.hybrid.recycle",
                &format!("Recycle notification unavailable; retrying in 30s: {error}"),
            );
        }
    }
    match persist_workspace(s)
        .inspect_err(|error| diagnostics::failure(diagnostics::Operation::SaveWorkspace, error))
    {
        Ok(true) => {
            s.session.as_ref().unwrap().dirty.set(true);
            refresh_views(s);
        }
        Err(error) => luciddesk_diagnostics::log(
            luciddesk_diagnostics::Level::Warn,
            "pane.hybrid.save",
            &format!("Workspace save pending; retrying in 5s: {error}"),
        ),
        Ok(false) => {}
    }
    let h = s.session.as_mut().unwrap();
    if h.last_tick.elapsed() < Duration::from_millis(20) {
        h.tick_deferred = true;
        return Ok(());
    }
    h.tick_deferred = false;
    h.last_tick = Instant::now();
    let mut urgent = h.dirty.replace(false);
    if urgent {
        h.audit.invalidate();
    }
    urgent |= icons::tick(s, urgent);
    urgent |= poll_inventory(s, urgent)?;
    urgent |= s.session.as_ref().unwrap().membership_pending.is_some();
    // The Hook repairs its retained desired set. Publish only when application
    // state changed, rather than duplicating its watchdog with periodic SETs.
    if urgent { sync(s) } else { Ok(()) }
}

fn poll_inventory(s: &mut PaneApp, urgent: bool) -> Result<bool, String> {
    let h = s.session.as_ref().unwrap();
    if h.pending_workspace_save || h.membership_pending.is_some() || h.published.borrow().is_none()
    {
        return Ok(false);
    }
    if h.audit.due(h.last_reconcile.elapsed()) {
        let managed = managed_identities(s);
        s.session
            .as_mut()
            .unwrap()
            .audit
            .submit(managed, urgent)
            .inspect_err(|error| {
                diagnostics::failure(diagnostics::Operation::SubmitAudit, error)
            })?;
    }
    let Some(result) = s
        .session
        .as_mut()
        .unwrap()
        .audit
        .poll()
        .inspect_err(|error| diagnostics::failure(diagnostics::Operation::PollAudit, error))?
    else {
        return Ok(false);
    };
    let managed_keys = inventory::revision_keys(&managed_identities(s));
    let h = s.session.as_mut().unwrap();
    if result.member_keys != managed_keys {
        h.dirty.set(true);
        return Ok(false);
    }
    h.last_reconcile = Instant::now();
    let snapshot = result
        .inventory
        .inspect_err(|error| diagnostics::failure(diagnostics::Operation::ReadAudit, error))?;
    let changed = snapshot
        .as_ref()
        .is_some_and(|next| !inventory::same(&h.snapshot, next));
    h.audit.complete(changed);
    // One scheduler owns fallback audits and hook repair. Changed inventories
    // publish membership through sync(); unchanged ones still repair missed events.
    if !changed {
        h.hook
            .repair()
            .inspect_err(|error| diagnostics::failure(diagnostics::Operation::Repair, error))?;
    }
    if let Some(snapshot) = snapshot.filter(|_| changed) {
        h.snapshot = snapshot;
        reconcile_inventory(s)
            .inspect_err(|error| diagnostics::failure(diagnostics::Operation::Reconcile, error))?;
    }
    Ok(changed)
}

// Schedule the next input, retry, audit, cache expiry or repair deadline.
// Worker completions and Shell changes wake the supervisor independently.
pub(super) fn next_work(s: &PaneApp) -> Option<u32> {
    let h = s.session.as_ref()?;
    let now = Instant::now();
    if let Some(retry) = h.retry_after {
        return Some(
            retry
                .saturating_duration_since(now)
                .as_millis()
                .clamp(25, u32::MAX as u128) as u32,
        );
    }
    if h.menu_active.get() {
        return None;
    }
    if h.tick_deferred
        || h.membership_pending.is_some()
        || h.dirty.get()
        || h.pending_desktop_input.get().is_some()
        || (!h.pending_workspace_save && h.audit.due(Duration::ZERO))
    {
        return Some(25);
    }
    let icon_due = h
        .icon_due
        .filter(|_| h.initial_batches == 0 && h.icon_reload.is_none());
    icon_due
        .into_iter()
        .chain(
            (!h.pending_workspace_save)
                .then(|| h.audit.remaining(h.last_reconcile.elapsed()))
                .flatten()
                .map(|delay| now + delay),
        )
        .chain(
            h.pending_workspace_save
                .then_some(h.save_due.unwrap_or(now)),
        )
        .chain(
            h.image_retention
                .deadline()
                .filter(|_| !h.pending_workspace_save),
        )
        .chain(h.shortcut_retention.deadline())
        .chain(h.recycle_subscription.is_none().then_some(h.recycle_retry))
        .chain(icons::retry_deadline(
            h.initial_batches != 0 || h.icon_reload.is_some(),
            h.last_icon_scan,
            &s.images,
            &h.requested,
            &h.icon_failures,
        ))
        .min()
        .map(|due| {
            due.saturating_duration_since(now)
                .as_millis()
                .clamp(25, u32::MAX as u128) as u32
        })
}

// Persist drag intent and present it before asking Explorer to update its view.
// A slow Shell operation must not roll back a successfully saved user action.
fn save_placement(s: &mut PaneApp) -> Result<(), String> {
    s.workspace.sync_tab_windows();
    s.store
        .save_workspace(&s.workspace)
        .map_err(|error| error.to_string())?;
    refresh_views(s);
    if let Some(h) = &mut s.session {
        h.audit.membership_changed();
        h.dirty.set(true);
        h.wake.notify();
    }
    Ok(())
}

pub(super) fn release(
    s: &mut PaneApp,
    id: PanelId,
    indices: &[usize],
    point: POINT,
) -> Result<bool, String> {
    let Some(h) = &s.session else {
        return Ok(false);
    };
    let surface = unsafe { WindowFromPoint(point) };
    if surface != h.view as _ && surface != unsafe { GetParent(h.view as _) } {
        return Ok(false);
    }
    let items = items_for(s, id);
    if indices.is_empty() || indices.iter().any(|index| *index >= items.len()) {
        return Ok(false);
    }
    let old = s.workspace.clone();
    for index in indices {
        let Some(item) = items.get(*index) else {
            return Ok(false);
        };
        if let Some(entry) = s.workspace.desktop_item_mut(&item.identity) {
            entry.set_placement(DesktopPlacement::default());
        }
    }
    if let Err(error) = save_placement(s) {
        s.workspace = old;
        return Err(error);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_subscription_failure_retries_without_busy_loop_or_replacing_success() {
        let now = Instant::now();
        let mut due = now;
        let mut slot = None;
        assert!(
            retry_subscription(&mut slot, &mut due, now, || Err::<u8, _>(
                "unavailable".into()
            ))
            .is_err()
        );
        assert!(slot.is_none());
        assert_eq!(due, now + Duration::from_secs(30));
        assert!(!retry_subscription(&mut slot, &mut due, now, || panic!("early retry")).unwrap());
        let next = due;
        assert!(retry_subscription(&mut slot, &mut due, next, || Ok(7)).unwrap());
        assert_eq!(slot, Some(7));
        let next = due;
        assert!(
            !retry_subscription(&mut slot, &mut due, next, || panic!("already subscribed"))
                .unwrap()
        );
    }
    #[test]
    fn drag_round_trip_saves_all_placements_without_shell_confirmation() {
        let mut s = crate::pane::tests::test_state();
        let id = PanelId::new(1);
        let identities: Vec<_> = s
            .workspace
            .desktop_items()
            .iter()
            .map(|item| item.identity().clone())
            .collect();
        for identity in &identities {
            s.workspace
                .desktop_item_mut(identity)
                .unwrap()
                .set_placement(DesktopPlacement::default());
        }
        save_placement(&mut s).unwrap();
        assert!(
            items_for(&s, id).is_empty(),
            "released items must leave the pane immediately"
        );
        assert_eq!(s.store.load_workspace().unwrap(), s.workspace);
        for (index, identity) in identities.iter().enumerate() {
            s.workspace
                .desktop_item_mut(identity)
                .unwrap()
                .set_placement(DesktopPlacement::Pane {
                    pane_id: id,
                    position: GridPosition::new(index as u32, 0),
                });
        }
        save_placement(&mut s).unwrap();
        assert_eq!(items_for(&s, id).len(), identities.len());
        assert_eq!(s.store.load_workspace().unwrap(), s.workspace);
    }

    #[test]
    fn timed_out_collection_republishes_rollback_after_partial_or_late_hiding() {
        for original in [Vec::new(), vec!["existing".to_string()]] {
            for applied in [0, 2, 3] {
                let published = RefCell::new(Some(original.clone()));
                let requested = vec!["A".to_string(), "B".to_string(), "C".to_string()];
                let mut native = original.clone();
                assert!(
                    publish_membership(&published, requested.clone(), |names| {
                        native = names[..applied].to_vec();
                        Err("injected timeout after accepting SET".into())
                    })
                    .is_err()
                );
                assert!(published.borrow().is_none());
                // The first rollback can be rejected while that SET is pending.
                assert!(
                    publish_membership(&published, original.clone(), |_| {
                        Err("Explorer still busy".into())
                    })
                    .is_err()
                );
                // The timed-out SET completes later, after the app rolled back.
                native = requested;
                let mut sent = false;
                publish_membership(&published, original.clone(), |names| {
                    sent = true;
                    native = names.to_vec();
                    Ok(())
                })
                .unwrap();
                assert!(sent, "rollback must not be skipped by the old cache");
                assert_eq!(native, original);
                publish_membership(&published, original.clone(), |_| {
                    panic!("an acknowledged unchanged set must not be resent")
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn reconnect_membership_uses_cached_images_and_waits_only_for_missing_ones() {
        let mut s = crate::pane::tests::test_state();
        s.images.clear();
        assert!(hidden_names(&s).is_empty());
        let identities: Vec<_> = s
            .workspace
            .desktop_items()
            .iter()
            .filter(|item| matches!(item.placement(), DesktopPlacement::Pane { .. }))
            .map(|item| item.identity().clone())
            .collect();
        assert!(identities.len() > 1);
        let image = Arc::new(assets::Pixels {
            width: 1,
            height: 1,
            data: vec![0; 4],
        });
        s.images
            .insert(identities[0].persistent_key(), image.clone());
        assert_eq!(
            hidden_names(&s),
            vec![
                identities[0]
                    .activation_name()
                    .to_string_lossy()
                    .into_owned()
            ]
        );
        for identity in &identities {
            s.images.insert(identity.persistent_key(), image.clone());
        }
        assert_eq!(hidden_names(&s).len(), identities.len());
        s.workspace
            .desktop_item_mut(&identities[0])
            .unwrap()
            .set_placement(DesktopPlacement::default());
        assert_eq!(
            hidden_names(&s).len(),
            identities.len() - 1,
            "cached pixels must not cause an item released to Explorer to be hidden"
        );
    }

    #[test]
    fn failed_inventory_save_remains_pending_until_persisted_and_presented() {
        let mut pending = true;
        let mut presented = 0;
        for fails in [true, true, false, false] {
            let result = persist_pending(&mut pending, || {
                if fails {
                    Err("injected database busy".into())
                } else {
                    Ok(())
                }
            });
            if fails {
                assert!(result.is_err());
                assert!(pending);
            } else if result.unwrap() {
                presented += 1;
            }
        }
        assert_eq!(presented, 1);
        assert!(!pending);
        assert!(!persist_pending(&mut pending, || panic!("already saved")).unwrap());
    }
    #[test]
    fn failed_save_has_its_own_deadline_and_does_not_clear_pending_state() {
        let now = Instant::now();
        let mut pending = true;
        let mut due = None;
        assert!(retry_save(&mut pending, &mut due, now, || Err("disk full".into())).is_err());
        assert!(pending);
        assert_eq!(due, Some(now + Duration::from_secs(5)));
        assert!(!retry_save(&mut pending, &mut due, now, || panic!("early write")).unwrap());
        assert!(
            retry_save(&mut pending, &mut due, now + Duration::from_secs(5), || Ok(
                ()
            ))
            .unwrap()
        );
        assert!(!pending);
        assert!(due.is_none());
    }
    #[test]
    fn database_failure_preserves_published_members_until_save_recovers() {
        let mut pending = true;
        let published = RefCell::new(Some(vec!["existing".into()]));
        let mut native = vec!["existing".to_string()];
        let desired = vec!["existing".to_string(), "new".to_string()];
        for fails in [true, true, false] {
            let saved = persist_pending(&mut pending, || {
                if fails {
                    Err("injected disk write failure".into())
                } else {
                    Ok(())
                }
            });
            if saved.is_err() {
                assert!(pending);
                assert_eq!(native, vec!["existing"]);
                assert_eq!(*published.borrow(), Some(native.clone()));
                continue;
            }
            publish_membership(&published, desired.clone(), |names| {
                native = names.to_vec();
                Ok(())
            })
            .unwrap();
        }
        assert!(!pending);
        assert_eq!(native, desired);
    }
    #[test]
    fn desktop_input_order_preserves_newer_pane_selection() {
        assert!(desktop_input_is_newer(120, Some(100)));
        assert!(!desktop_input_is_newer(120, Some(140)));
        assert!(!desktop_input_is_newer(120, Some(120)));
        assert!(desktop_input_is_newer(10, Some(u32::MAX - 10)));
    }
}

const ICON_CHANGE_MESSAGE: u32 = WM_APP + 0x352;

const RECYCLE_CHANGE_MESSAGE: u32 = WM_APP + 0x353;
