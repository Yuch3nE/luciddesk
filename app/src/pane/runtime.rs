//! Desktop integration can recover without taking independent panes down.
use super::search::{everything_settings, hotkey as search_hotkey};
use super::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub(super) struct State {
    pub path: PathBuf,
    pub desktop_error: Option<String>,
    notice: ConnectionNotice,
    last_attempt: Instant,
    reconnect_failures: u8,
    reconnecting: bool,
    last_maintenance: Instant,
    pub layouts: display_layout::Layouts,
    pub backup: recovery::Manager,
}

impl State {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            desktop_error: None,
            notice: ConnectionNotice::default(),
            last_attempt: Instant::now(),
            reconnect_failures: 0,
            reconnecting: false,
            last_maintenance: Instant::now(),
            layouts: Default::default(),
            backup: recovery::Manager::default(),
        }
    }
}

#[derive(Default)]
struct ConnectionNotice {
    unavailable: bool,
    generation: u64,
}
impl ConnectionNotice {
    fn transition(&mut self, unavailable: bool) -> bool {
        if self.unavailable == unavailable { return false; }
        self.unavailable = unavailable;
        self.generation += 1;
        true
    }
}

fn desktop_connection_log(database: &std::path::Path, error: Option<&str>) -> std::io::Result<PathBuf> {
    crate::init_logging(database);
    luciddesk_diagnostics::try_log(
        if error.is_some() {
            luciddesk_diagnostics::Level::Error
        } else {
            luciddesk_diagnostics::Level::Info
        },
        "desktop.connection",
        error.unwrap_or("connection recovered"),
    )?;
    luciddesk_diagnostics::path().ok_or_else(|| std::io::Error::other("diagnostics not initialized"))
}

fn notify_connection(state: &Rc<RefCell<PaneApp>>) {
    let (path, error, generation) = {
        let mut s = state.borrow_mut();
        let Some(runtime) = &mut s.runtime else { return; };
        if !runtime.notice.transition(runtime.desktop_error.is_some()) { return; }
        (runtime.path.clone(), runtime.desktop_error.clone(), runtime.notice.generation)
    };
    let logged = desktop_connection_log(&path, error.as_deref());
    let Some(error) = error else {
        if let Err(error) = logged { luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "desktop.connection", &format!("Desktop connection log: {error}")); }
        return;
    };
    let log = match logged {
        Ok(path) => crate::i18n::format("ui-desktop-log-saved", &[("path",path.display().to_string())]),
        Err(error) => crate::i18n::format("ui-desktop-log-failed", &[("error",error.to_string())]),
    };
    let message = crate::i18n::format("ui-desktop-connection-warning", &[("error",error),("log",log)]);
    let weak = Rc::downgrade(state);
    // Modal dialogs pump messages; release all app borrows before showing one.
    window::defer_action(move || {
        let still_unavailable = weak.upgrade().is_some_and(|state| {
            state.borrow().runtime.as_ref().is_some_and(|r| r.notice.unavailable && r.notice.generation == generation)
        });
        if still_unavailable { window::error(&message); }
    });
}

pub(super) fn status(s: &PaneApp) -> String {
    s.runtime
        .as_ref()
        .and_then(|r| r.desktop_error.as_ref())
        .map_or_else(
            || crate::i18n::text("ui-desktop-groups-connected").into(),
            |error| crate::i18n::format("ui-desktop-groups-unavailable-folder-panels-and-search-still-work-n", &[("error", format!("{}", error))]),
        )
}

pub(super) fn reconnect(state: &Rc<RefCell<PaneApp>>) {
    let path = {
        let mut s = state.borrow_mut();
        if s.session.is_some() {
            return;
        }
        let Some(runtime) = &mut s.runtime else {
            return;
        };
        if runtime.reconnecting { return; }
        runtime.reconnecting = true;
        runtime.last_attempt = Instant::now();
        runtime.path.clone()
    };

    let error = hybrid::connect(state, &path).err();

    let mut s = state.borrow_mut();
    if let Some(runtime) = &mut s.runtime {
        runtime.reconnecting = false;
        runtime.last_attempt = Instant::now();
        runtime.reconnect_failures = if error.is_some() {
            runtime.reconnect_failures.saturating_add(1)
        } else { 0 };
        runtime.desktop_error = error;
    }
    if let Some(settings) = &s.settings {
        unsafe {
            InvalidateRect(settings.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    drop(s);
    notify_connection(state);
}

fn suspend(state: &Rc<RefCell<PaneApp>>) {
    let removed = {
        let mut s = state.borrow_mut();
        s.session.take();
        if let Some(runtime) = &mut s.runtime {
            runtime.desktop_error = Some(crate::i18n::text("ui-explorer-disconnected-waiting-to-reconnect").into());
            runtime.reconnect_failures = 0;
            runtime.last_attempt = Instant::now() - reconnect_delay(0);
        }
        let mut removed = Vec::new();
        let mut at = 0;
        while at < s.views.len() {
            let view = &s.views[at];
            if tabs::independent(&s.workspace, view.id)
            {
                at += 1;
            } else {
                let view = s.views.remove(at);
                window::prepare_close(view.window.hwnd().cast());
                hybrid::unregister_drop(&mut s, view.window.hwnd().cast());
                removed.push(view);
            }
        }
        removed
    };
    drop(removed);
    notify_connection(state);
}

pub(super) fn maintain(state: &Rc<RefCell<PaneApp>>, force: bool) -> Result<(), String> {
    if let Some(runtime) = &mut state.borrow_mut().runtime {
        runtime.last_maintenance = Instant::now();
    }
    display_layout::tick(state)?;
    recovery::maintain(state);
    if state
        .borrow()
        .session
        .as_ref()
        .is_some_and(|session| !hybrid::is_alive(session))
    {
        suspend(state);
    }
    let due = state
        .borrow()
        .runtime
        .as_ref()
        .is_some_and(|r| !r.reconnecting && r.last_attempt.elapsed() >= reconnect_delay(r.reconnect_failures));
    if force || due {
        reconnect(state);
    }
    {
        let removed = {
            let mut s = state.borrow_mut();
            let mut removed = Vec::new();
            let mut at = 0;
            while at < s.views.len() {
                if unsafe { IsWindow(s.views[at].window.hwnd().cast()) } == 0 {
                    let view = s.views.remove(at);
                    hybrid::unregister_drop(&mut s, view.window.hwnd().cast());
                    removed.push(view);
                } else {
                    at += 1;
                }
            }
            removed
        };
        drop(removed);
        let ids: Vec<_> = {
            let s = state.borrow();
            let search_enabled = s
                .workspace
                .panels()
                .iter()
                .any(|p| p.is_search() && !s.views.iter().any(|v| v.id == p.id()))
                && everything_settings::enabled(&s.store)?;
            s.workspace
                .panels()
                .iter()
                .filter(|p| {
                    s.workspace.tab_visible(p.id()) && (if p.is_search() {
                        search_enabled
                    } else {
                        tabs::independent(&s.workspace, p.id()) || s.session.is_some()
                    }) && !s.views.iter().any(|v| v.id == p.id())
                })
                .map(Panel::id)
                .collect()
        };
        for id in ids {
            create_view(state, id)?;
        }
    }
    Ok(())
}

fn reconnect_delay(failures: u8) -> Duration {
    Duration::from_millis((250u64 << failures.saturating_sub(1).min(6)).min(10_000))
}

fn next_work(s: &PaneApp, hotkey: &search_hotkey::Registration, reveal: &search_hotkey::Registration) -> Option<u32> {
    let now = Instant::now();
    let runtime = s.runtime.as_ref();
    // Slow safety check for missed window lifecycle notifications or a failed
    // view creation; ordinary work is driven by notifications and deadlines.
    let maintenance = runtime.map(|r| r.last_maintenance + Duration::from_secs(30));
    let reconnect = runtime.filter(|r| s.session.is_none() && !r.reconnecting)
        .map(|r| r.last_attempt + reconnect_delay(r.reconnect_failures));
    hybrid::next_work(s).map(u64::from).map(Duration::from_millis)
        .map(|delay| now + delay).into_iter()
        .chain(maintenance)
        .chain(reconnect)
        .chain(runtime.and_then(|r| r.layouts.deadline()))
        .chain(recovery::deadline(s))
        .chain(hotkey.retry_deadline())
        .chain(reveal.retry_deadline())
        .min()
        .map(|due| due.saturating_duration_since(now).as_millis().clamp(25, u32::MAX as u128) as u32)
}

pub(super) fn backup_status(s: &PaneApp) -> String {
    s.runtime.as_ref().map(|runtime| runtime.backup.view.status.clone()).unwrap_or_default()
}

pub(super) fn reload(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    load_log_level(&state.borrow().store)?;
    let views = {
        let mut s = state.borrow_mut();
        let workspace = s.store.load_workspace().map_err(|e| e.to_string())?;
        peek::load(&s.store)?;
    fonts::load(&s.store)?;
    header_divider::load(&s.store)?;
    compact_menu::load(&s.store)?;
    title_emoji::load(&s.store)?;
        search_hotkey::load(&s.store)?;
        everything_settings::load(&s.store)?;
        s.session.take();
        s.drops.clear();
        s.folders.clear();
        s.tab_models.clear();
        s.images.clear();
        for v in &s.views {
            window::prepare_close(v.window.hwnd().cast());
        }
        s.workspace = workspace;
        s.runtime.as_mut().unwrap().layouts = Default::default();
        display_layout::initialize(&mut s, luciddesk_window::enumerate_monitors())?;
        std::mem::take(&mut s.views)
    };
    drop(views);
    reconnect(state);
    let ids: Vec<_> = {
        let s = state.borrow();
        let search_enabled = everything_settings::enabled(&s.store)?;
        s.workspace
            .panels()
            .iter()
            .filter(|p| {
                if p.is_search() {
                    search_enabled
                } else {
                    tabs::independent(&s.workspace, p.id()) || s.session.is_some()
                }
            })
            .map(Panel::id)
            .collect()
    };
    for id in ids {
        create_view(state, id)?;
    }
    if let Some(settings) = &state.borrow().settings {
        unsafe {
            InvalidateRect(settings.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    Ok(())
}

pub(super) const REFRESH_HOTKEYS: u32 = WM_APP + 0x4c4;

pub(super) fn supervisor(state: &Rc<RefCell<PaneApp>>) -> Result<windows_window::Window, String> {
    let weak = Rc::downgrade(state);
    let wake = state.borrow().wake.clone();
    let received = wake.clone();
    let changed = wake.clone();
    state.borrow_mut().store.set_change_callback(move || changed.notify());
    let mut hotkey = search_hotkey::Registration::default();
    let mut reveal_hotkey = search_hotkey::Registration::with_id(show_hotkey::ID);
    let mut search_state = None;
    let mut search_enabled = false;
    let mut search_checked = Instant::now();
    let show_message = unsafe { RegisterWindowMessageW(windows_sys::w!("LucidDesk.ShowExisting")) };
    let taskbar_created = unsafe { RegisterWindowMessageW(windows_sys::w!("TaskbarCreated")) };
    let mut language_dirty = false;
    let mut layout_dirty = false;
    let mut reconnect_hint = false;
    let window = windows_window::Window::new("LucidDesk Runtime")
        .size(1, 1)
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
        .on_message(move |raw, msg, wp, lp| {
            if unsafe { crate::window_visibility::defer_show(msg, lp, false) } {
                return Some(0);
            }
            if msg == REFRESH_HOTKEYS {
                let Some(state) = weak.upgrade() else { return Some(0); };
                let s = state.borrow();
                let present = s.views.iter().any(|v| s.workspace.panel(v.id).is_some_and(Panel::is_search));
                let enabled = present && everything_settings::enabled(&s.store).unwrap_or(false);
                hotkey.update(raw as isize, enabled.then(search_hotkey::settings));
                reveal_hotkey.update(raw as isize, show_hotkey::enabled(&s.store).then(|| show_hotkey::settings(&s.store)));
                show_hotkey::update_status(reveal_hotkey.message());
                return Some(isize::from(hotkey.ready() && reveal_hotkey.ready()));
            }
            if msg == WM_DESTROY {
                received.unbind();
                hotkey.update(raw as isize, None);
                reveal_hotkey.update(raw as isize, None);
                return Some(0);
            }
            if msg == WM_HOTKEY && wp == show_hotkey::ID as usize {
                if let Some(state) = weak.upgrade() { show_hotkey::activate(&state); }
                return Some(0);
            }
            if msg == WM_HOTKEY && wp == search_hotkey::ID as usize {
                if let Some(state) = weak.upgrade() {
                    search_hotkey::activate(&state);
                }
                return Some(0);
            }
            if msg == show_message {
                if let Some(state) = weak.upgrade() {
                    let s = state.borrow();
                    for view in &s.views {
                        unsafe {
                            ShowWindow(view.window.hwnd().cast(), SW_SHOWNOACTIVATE);
                        }
                    }
                    if let Some(view) = s.views.first() {
                        unsafe {
                            SetForegroundWindow(view.window.hwnd().cast());
                        }
                    } else if let Some(settings) = &s.settings {
                        unsafe {
                            ShowWindow(settings.hwnd().cast(), SW_RESTORE);
                            SetForegroundWindow(settings.hwnd().cast());
                        }
                    }
                }
                return Some(0);
            }
            if matches!(msg, WM_SETTINGCHANGE | WM_FONTCHANGE | WM_THEMECHANGED) { super::assets::invalidate_font(); }
            language_dirty |= msg == WM_SETTINGCHANGE || msg == super::wake::READY;
            if matches!(msg, WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED | WM_POWERBROADCAST) {
                layout_dirty = true;
            } else if taskbar_created != 0 && msg == taskbar_created {
                reconnect_hint = true;
                layout_dirty = true;
            } else if msg != WM_TIMER && msg != super::wake::READY {
                return None;
            }
            if msg == super::wake::READY {
                layout_dirty |= received.received();
            }
            if let Some(state) = weak.upgrade() {
                if state.try_borrow_mut().is_ok() {
                    unsafe { KillTimer(raw.cast(), 2); }
                    if std::mem::take(&mut language_dirty) {
                        let result = (|| -> Result<(), String> {
                            let changed = {
                                let s = state.borrow();
                                let changed = crate::i18n::initialize(&s.store)?;
                                if changed {
                                    // A font preference read failure must not swallow the
                                    // notification after the active language has changed.
                                    if let Err(error) = fonts::load(&s.store) {
                                        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.runtime", &format!("Language font refresh: {error}"));
                                    }
                                }
                                changed
                            };
                            if changed {
                                // Text renderers refresh their language-dependent formats
                                // on repaint. Keep item snapshots, selection, scroll and
                                // any uncommitted rename intact; no data reload is needed.
                                // Notify after releasing PaneApp, including owned windows.
                                unsafe extern "system" fn notify(hwnd: windows_sys::Win32::Foundation::HWND, _: isize) -> i32 {
                                    unsafe { PostMessageW(hwnd, crate::i18n::CHANGED, 0, 0); InvalidateRect(hwnd, std::ptr::null(), 0); }
                                    1
                                }
                                unsafe { EnumThreadWindows(GetWindowThreadProcessId(raw.cast(), std::ptr::null_mut()), Some(notify), 0); }
                            }
                            Ok(())
                        })();
                        if let Err(error) = result { luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.runtime", &format!("Language refresh: {error}")); }
                    }

                    let folder_renames = {
                        let mut s = state.borrow_mut();
                        if let Some(runtime) = &mut s.runtime {
                            if std::mem::take(&mut layout_dirty) {
                                runtime.layouts.invalidate();
                            }
                            if std::mem::take(&mut reconnect_hint) {
                                runtime.reconnect_failures = 0;
                                runtime.last_attempt = Instant::now() - reconnect_delay(0);
                            }
                        }
                        sorting::poll(&mut s);
                        let folder_renames = folder::poll(&mut s);
                        if s.session.is_some() {
                            if let Err(error) = hybrid::tick(&mut s) {
                                luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.runtime", &format!("Desktop synchronization: {error}"));
                            }
                        }
                        folder_renames
                    };
                    for (id, identity) in folder_renames {
                        if let Err(message) = events::handle(&state, id, Event::RenameItem(identity)) {
                            luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.runtime", &format!("New folder item rename: {message}"));
                        }
                    }
                    let previous = {
                        let s = state.borrow();
                        (status(&s), backup_status(&s), search_hotkey::status(), show_hotkey::status())
                    };
                    let managed = state.borrow().runtime.is_some();
                    if managed && let Err(error) = maintain(&state, false) {
                        luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.runtime", &format!("Runtime recovery: {error}"));
                    }
                    let s = state.borrow();
                    let search_present = s.views.iter()
                        .any(|v| s.workspace.panel(v.id).is_some_and(Panel::is_search));
                    let signature = (search_present, s.store.change_count());
                    if search_state != Some(signature) || search_checked.elapsed() >= Duration::from_secs(30) {
                        search_enabled = search_present && everything_settings::enabled(&s.store).unwrap_or(false);
                        search_state = Some(signature);
                        search_checked = Instant::now();
                    }
                    hotkey.update(raw as isize, search_enabled.then(search_hotkey::settings));
                    reveal_hotkey.update(raw as isize, show_hotkey::enabled(&s.store).then(|| show_hotkey::settings(&s.store)));
                    show_hotkey::update_status(reveal_hotkey.message());
                    unsafe {
                        if let Some(delay) = next_work(&s, &hotkey, &reveal_hotkey) {
                            SetTimer(raw.cast(), 2, delay, None);
                        }
                    }
                    if previous != (status(&s), backup_status(&s), search_hotkey::status(), show_hotkey::status())
                        && let Some(settings) = &s.settings
                    {
                        unsafe {
                            InvalidateRect(settings.hwnd().cast(), std::ptr::null(), 0);
                        }
                    }
                } else {
                    // Nested COM/menu callbacks can temporarily hold PaneApp.
                    // Retry the coalesced notification instead of dropping it.
                    unsafe {
                        SetTimer(raw.cast(), 2, 25, None);
                    }
                }
            }
            Some(0)
        })
        .create()
        .map_err(|e| e.to_string())?;
    wake.bind(window.hwnd() as isize);
    Ok(window)
}

#[cfg(test)]
mod tests {
    #[test]
    fn saved_language_notifies_open_windows_without_restart() {
        let _apartment = crate::pane::test_support::apartment();
        crate::i18n::with_locale(0, || {
            let state = Rc::new(RefCell::new(super::super::tests::test_state()));
            let observed = Rc::new(std::cell::Cell::new(false));
            let flag = observed.clone();
            let observer = windows_window::Window::new("language observer")
                .size(1, 1).style(WS_POPUP)
                .on_message(move |_, msg, _, _| {
                    if msg == crate::i18n::CHANGED { flag.set(true); return Some(0); }
                    None
                }).create().unwrap();
            let runtime = supervisor(&state).unwrap();
            state.borrow().store.save_preference("language", "en-US").unwrap();
            unsafe { SendMessageW(runtime.hwnd().cast(), super::super::wake::READY, 0, 0); }
            assert_eq!(crate::i18n::language(), "en-US");
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&raw mut msg, observer.hwnd().cast(), crate::i18n::CHANGED, crate::i18n::CHANGED, PM_REMOVE) != 0 {
                    DispatchMessageW(&msg);
                }
            }
            assert!(observed.get());
            state.borrow().store.save_preference("language", "zh-CN").unwrap();
            unsafe { SendMessageW(runtime.hwnd().cast(), super::super::wake::READY, 0, 0); }
            assert_eq!(crate::i18n::language(), "zh-CN");
        });
    }

    use super::*;

    #[test]
    fn reconnect_retries_start_fast_and_remain_bounded() {
        let delays: Vec<_> = (1..=9).map(|n| reconnect_delay(n).as_millis()).collect();
        assert_eq!(delays, [250, 500, 1000, 2000, 4000, 8000, 10000, 10000, 10000]);
        assert_eq!(reconnect_delay(u8::MAX), Duration::from_secs(10));
        assert_eq!(reconnect_delay(0), Duration::from_millis(250));
    }

    #[test]
    fn nested_reconnect_does_not_start_another_shell_connection() {
        let mut app = super::super::tests::test_state();
        let mut runtime = State::new(std::path::PathBuf::from("unused-reconnect-test.db"));
        runtime.reconnecting = true;
        runtime.reconnect_failures = 3;
        let attempt = runtime.last_attempt;
        app.runtime = Some(runtime);
        let state = Rc::new(RefCell::new(app));
        reconnect(&state);
        let s = state.borrow();
        let runtime = s.runtime.as_ref().unwrap();
        assert!(runtime.reconnecting);
        assert_eq!(runtime.last_attempt, attempt);
        assert_eq!(runtime.reconnect_failures, 3);
        assert!(s.session.is_none());
    }

    #[test]
    fn store_changes_wake_runtime_without_a_fixed_heartbeat() {
        let root = tempfile::tempdir().unwrap();
        let mut app = super::super::tests::test_state();
        app.workspace = Workspace::default();
        app.runtime = Some(State::new(root.path().join("workspace.db")));
        let state = Rc::new(RefCell::new(app));
        let supervisor = supervisor(&state).unwrap();
        unsafe {
            let mut message = MSG::default();
            // Remove the initial bind notification so only the write can wake us.
            PeekMessageW(&raw mut message, supervisor.hwnd().cast(), super::super::wake::READY, super::super::wake::READY, PM_REMOVE);
        }
        state.borrow().wake.received();
        state.borrow().store.save_preference("event-test", "written").unwrap();
        unsafe {
            let mut message = MSG::default();
            assert_ne!(PeekMessageW(&raw mut message, supervisor.hwnd().cast(), super::super::wake::READY, super::super::wake::READY, PM_REMOVE), 0);
            assert_eq!(KillTimer(supervisor.hwnd().cast(), 1), 0, "no fixed heartbeat");
        }
    }
    #[test]
    fn folder_completion_reaches_supervisor_without_timer_polling() {
        let root = std::env::temp_dir().join(format!(
            "luciddesk-wake-folder-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        let id = PanelId::new(2);
        state
            .borrow_mut()
            .workspace
            .panel_mut(id)
            .unwrap()
            .set_folder(Some(root.clone()));
        folder::ensure(&mut state.borrow_mut(), id).unwrap();
        let supervisor = supervisor(&state).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.borrow().folders[&id].loading && Instant::now() < deadline {
            unsafe {
                let mut msg = MSG::default();
                // Deliberately do not dispatch WM_TIMER: the worker notification
                // must deliver the initial snapshot even if it finished pre-bind.
                while PeekMessageW(
                    &raw mut msg,
                    supervisor.hwnd().cast(),
                    super::super::wake::READY,
                    super::super::wake::READY,
                    PM_REMOVE,
                ) != 0
                {
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!state.borrow().folders[&id].loading);
        assert!(state.borrow().folders[&id].status.is_none());
        drop(supervisor);
        state.borrow_mut().folders.clear();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn suspended_desktop_preserves_membership_and_independent_panes() {
        let _sta = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(super::super::tests::test_state()));
        create_view(&state, PanelId::new(1)).unwrap();
        handle(
            &state,
            PanelId::new(0),
            Event::MapFolder(std::env::temp_dir()),
        )
        .unwrap();
        handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
        let before = state.borrow().workspace.clone();
        let independent: Vec<_> = state
            .borrow()
            .views
            .iter()
            .filter(|v| v.id != PanelId::new(1))
            .map(|v| (v.id, v.window.hwnd()))
            .collect();
        assert_eq!(
            state.borrow().drops.len(),
            1,
            "folder drops must register without a desktop session"
        );
        suspend(&state);
        assert_eq!(state.borrow().workspace, before);
        assert_eq!(state.borrow().views.len(), 2);
        for (id, hwnd) in independent {
            assert!(
                state
                    .borrow()
                    .views
                    .iter()
                    .any(|v| v.id == id && v.window.hwnd() == hwnd)
            );
            unsafe {
                SendMessageW(hwnd.cast(), WM_DISPLAYCHANGE, 0, 0);
            }
        }
        let mut msg = MSG::default();
        assert_eq!(
            unsafe {
                PeekMessageW(
                    &raw mut msg,
                    std::ptr::null_mut(),
                    WM_QUIT,
                    WM_QUIT,
                    PM_REMOVE,
                )
            },
            0
        );
        let mut s = state.borrow_mut();
        s.drops.clear();
        for view in &s.views {
            window::prepare_close(view.window.hwnd().cast());
        }
    }
}

#[cfg(test)]
mod connection_notice_tests {
    #[test]
    fn retries_do_not_repeat_notice_but_new_outages_do() {
        let mut notice = super::ConnectionNotice::default();
        assert!(!notice.transition(false));
        assert!(notice.transition(true));
        let first = notice.generation;
        for _ in 0..50 { assert!(!notice.transition(true)); }
        assert_eq!(notice.generation,first);
        assert!(notice.transition(false));
        assert!(notice.transition(true));
        assert_ne!(notice.generation,first);
    }
}
