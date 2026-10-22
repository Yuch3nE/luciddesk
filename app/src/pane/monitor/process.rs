//! Private same-build IPC. The parent plugin persists configuration; child EOF closes the UI.
//! Startup sends Config; subsequent messages are typed updates, including visibility-only updates.
use super::super::*;
use super::Preferences;
use luciddesk_core::{Backdrop, PanelTheme};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::{Child, ChildStdin, Command, Stdio};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    desktop_hidden: bool,
    panel: Panel,
    options: luciddesk_core::PaneOptions,
    preferences: Preferences,
    language: String,
    font: String,
    flags: [bool; 4],
    system_corners: interface_flags::SystemCorners,
    snap_gap: u32,
}
#[derive(Serialize, Deserialize)]
enum Update {
    Configure(Box<Config>),
    DesktopVisibility(bool),
}
#[derive(Clone, Serialize, Deserialize)]
struct WindowOptions {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    collapsed: bool,
    locked: bool,
    always_on_top: bool,
    auto_hide: bool,
}
impl WindowOptions {
    fn from_panel(p: &Panel) -> Self {
        let r = p.rect();
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
            collapsed: p.collapsed(),
            locked: p.locked(),
            always_on_top: p.always_on_top(),
            auto_hide: p.auto_hide(),
        }
    }
}
impl Config {
    fn capture(s: &PaneApp) -> Result<Self, String> {
        let value =
            crate::plugins::request(super::PLUGIN_ID, serde_json::json!({"op":"config_get"}))?;
        let window: WindowOptions =
            serde_json::from_value(value["window"].clone()).map_err(|e| e.to_string())?;
        // This ID exists only inside the child renderer, never in the user's workspace.
        let mut panel = Panel::new(
            PanelId::new(1),
            crate::i18n::text("monitor-title"),
            RectDip::from_bounds(window.x, window.y, window.width, window.height),
        );
        panel.set_monitor(true);
        panel.set_collapsed(window.collapsed);
        panel.set_locked(window.locked);
        panel.set_always_on_top(window.always_on_top);
        panel.set_auto_hide(window.auto_hide);
        let (theme, backdrop) = s
            .workspace
            .appearance()
            .unwrap_or((PanelTheme::System, Backdrop::Mica));
        panel.set_theme(theme);
        panel.set_backdrop(backdrop);
        Ok(Self {
            desktop_hidden: super::super::desktop_toggle::hidden(),
            panel,
            preferences: serde_json::from_value(value["options"].clone())
                .map_err(|e| e.to_string())?,
            options: s.workspace.pane_options(),
            language: crate::i18n::language().into(),
            font: fonts::family(),
            snap_gap: snap::gap(),
            system_corners: interface_flags::system_corners(),
            flags: [
                interface_flags::PANEL_SHADOW.get(),
                interface_flags::HEADER_DIVIDER.get(),
                interface_flags::COMPACT_MENU.get(),
                interface_flags::TITLE_EMOJI.get(),
            ],
        })
    }
}
type Devices = [Vec<(String, String)>; 3];
#[derive(Clone, Default, Serialize, Deserialize)]
struct Inventory {
    devices: Devices,
    gpu_aliases: HashMap<String, String>,
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Devices(Inventory),
    Ready(isize),
    Panel(Panel),
    Settings,
    Closed,
    Error(String),
}
fn write_json(output: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *output, value).map_err(|e| e.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}
fn emit(value: Reply) {
    let _ = write_json(&mut std::io::stdout().lock(), &value);
}
thread_local! { static PUBLISHED: RefCell<Option<Panel>> = const { RefCell::new(None) }; }
static CHILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(super) fn publish_devices() {
    if !CHILD.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    thread_local! { static LAST: RefCell<serde_json::Value> = const { RefCell::new(serde_json::Value::Null) }; }
    if let Ok(value) =
        crate::plugins::request(super::PLUGIN_ID, serde_json::json!({"op":"devices"}))
    {
        LAST.with(|last| {
            if *last.borrow() != value {
                if let Ok(devices) = Inventory::deserialize(&value) {
                    emit(Reply::Devices(devices));
                }
                *last.borrow_mut() = value;
            }
        });
    }
}
pub(in crate::pane) fn emit_settings() {
    emit(Reply::Settings);
}
pub(in crate::pane) fn publish(state: &Rc<RefCell<PaneApp>>, id: PanelId) {
    if !CHILD.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let state = state.borrow();
    if let Some(panel) = state.workspace.panel(id) {
        publish_panel(panel);
    } else {
        emit(Reply::Closed);
        windows_window::quit();
    }
}

pub(in crate::pane) fn publish_panel(panel: &Panel) {
    if !CHILD.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    PUBLISHED.with(|last| {
        if last.borrow().as_ref() != Some(panel) {
            emit(Reply::Panel(panel.clone()));
            *last.borrow_mut() = Some(panel.clone());
        }
    });
}

struct Process {
    child: Option<Child>,
    job: Option<OwnedHandle>,
    input: Option<ChildStdin>,
    reader: Option<std::thread::JoinHandle<()>>,
    replies: mpsc::Receiver<Reply>,
    config: Config,
    hwnd: isize,
    devices: Inventory,
}
impl Drop for Process {
    fn drop(&mut self) {
        // EOF requests ordinary teardown, but waiting never blocks the UI.
        self.input.take();
        if let Some(child) = self.child.take() {
            let job = self.job.take();
            let reader = self.reader.take();
            if let Err(error) = reap(child, job, reader, std::time::Duration::from_secs(2)) {
                luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Error,
                    "monitor.process", "Start cleanup thread: {error}");
            }
        }
    }
}

fn own_renderer(child: &Child) -> Result<OwnedHandle, String> {
    use windows_sys::Win32::System::JobObjects::*;
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(raw, JobObjectExtendedLimitInformation,
            (&raw const limits).cast(), size_of_val(&limits) as u32) == 0
            || AssignProcessToJobObject(raw, child.as_raw_handle()) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(job)
    }
}

fn reap(mut child: Child, job: Option<OwnedHandle>, reader: Option<std::thread::JoinHandle<()>>,
    grace: std::time::Duration) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("monitor-cleanup".into()).spawn(move || {
        let deadline = std::time::Instant::now() + grace;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Error,
                            "monitor.process", "Monitor process exited: {status}");
                    }
                    break;
                }
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                result => {
                    luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn,
                        "monitor.process", "Monitor shutdown exceeded grace period or failed: {result:?}");
                    // Only this owned, stateless renderer is terminated.
                    let _ = child.kill();
                    break;
                }
            }
        }
        // Also guarantees cleanup if the parent exits before this thread runs.
        drop(job);
        if let Some(reader) = reader {
            // A broken pipe or blocked decoder must not extend the deadline.
            if reader.is_finished() { let _ = reader.join(); }
        }
    })
}
#[derive(Default)]
pub(in crate::pane) struct Manager {
    process: Option<Process>,
}
impl Manager {
    pub(in crate::pane) fn windows(&self) -> Vec<windows_sys::Win32::Foundation::HWND> {
        self.process
            .iter()
            .filter(|p| p.hwnd != 0)
            .map(|p| p.hwnd as _)
            .collect()
    }
    fn publish_inventory(&self) {
        let devices = self
            .process
            .as_ref()
            .map(|p| p.devices.clone())
            .unwrap_or_default();
        let _ = crate::plugins::request(
            super::PLUGIN_ID,
            serde_json::json!({"op":"inventory", "devices":devices.devices, "gpu_aliases":devices.gpu_aliases}),
        );
    }
}
fn open(state: &Rc<RefCell<PaneApp>>, config: Config) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(test)]
    let executable = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("luciddesk.exe");
    let mut child = Command::new(executable)
        .arg("--monitor-child")
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Start monitor process: {e}"))?;
    let job = match own_renderer(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            return Err(format!("Track monitor process: {error}"));
        }
    };
    let input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (sender, replies) = mpsc::channel();
    let wake = state.borrow().wake.clone();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let reply = line
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
            match reply {
                Ok(reply) => {
                    if sender.send(reply).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Reply::Error(error));
                    break;
                }
            }
            wake.notify();
        }
        drop(sender);
        wake.notify();
    });
    let mut process = Process {
        child: Some(child),
        job: Some(job),
        input: Some(input),
        reader: Some(reader),
        replies,
        config,
        hwnd: 0,
        devices: Default::default(),
    };
    write_json(process.input.as_mut().unwrap(), &process.config)?;
    state.borrow_mut().monitors.process = Some(process);
    Ok(())
}
pub(in crate::pane) fn configure(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    let config = if super::available() {
        Some(Config::capture(&state.borrow())?)
    } else {
        None
    };
    if config.as_ref().is_none_or(|c| !c.preferences.enabled) {
        let process = state.borrow_mut().monitors.process.take();
        drop(process);
        if super::available() {
            state.borrow().monitors.publish_inventory();
        }
        return Ok(());
    }
    let config = config.unwrap();
    if state.borrow().monitors.process.is_none() {
        return open(state, config);
    }
    let mut s = state.borrow_mut();
    let process = s.monitors.process.as_mut().unwrap();
    if process.config != config {
        write_json(process.input.as_mut().unwrap(), &Update::Configure(Box::new(config.clone())))?;
        process.config = config;
    }
    Ok(())
}
/// Presentation-only changes reuse the live renderer and its cached settings.
/// Do not query plugins, launch processes or reconfigure monitoring backends.
pub(in crate::pane) fn desktop_visibility(state: &Rc<RefCell<PaneApp>>, hidden: bool) -> Result<(), String> {
    let mut s = state.borrow_mut();
    if let Some(process) = s.monitors.process.as_mut()
        && process.config.desktop_hidden != hidden
    {
        write_json(process.input.as_mut().ok_or("Monitor input closed")?, &Update::DesktopVisibility(hidden))?;
        process.config.desktop_hidden = hidden;
    }
    Ok(())
}
pub(in crate::pane) fn poll(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    if state.borrow().monitor_child || state.borrow().monitors.process.is_none() {
        return Ok(());
    }
    let mut show_settings = false;
    let mut settings_changed = false;
    let mut closed = false;
    {
        let mut s = state.borrow_mut();
        loop {
            match s.monitors.process.as_ref().unwrap().replies.try_recv() {
                Ok(Reply::Devices(devices)) => {
                    settings_changed = true;
                    s.monitors.process.as_mut().unwrap().devices = devices;
                    s.monitors.publish_inventory();
                }
                Ok(Reply::Ready(hwnd)) => s.monitors.process.as_mut().unwrap().hwnd = hwnd,
                Ok(Reply::Settings) => show_settings = true,
                Ok(Reply::Panel(panel)) => {
                    crate::plugins::request(
                        super::PLUGIN_ID,
                        serde_json::json!({"op":"save_window", "window":WindowOptions::from_panel(&panel)}),
                    )?;
                    s.monitors.process.as_mut().unwrap().config.panel = panel;
                }
                Ok(Reply::Closed) => closed = true,
                Ok(Reply::Error(error)) => {
                    luciddesk_diagnostics::log(
                        luciddesk_diagnostics::Level::Error,
                        "monitor.process",
                        &error,
                    );
                    closed = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if settings_changed || closed {
            if let Some(window) = &s.settings {
                unsafe {
                    InvalidateRect(window.hwnd().cast(), std::ptr::null(), 0);
                }
            }
        }
    }
    if closed {
        let process = state.borrow_mut().monitors.process.take();
        drop(process);
        let mut preferences = Preferences::load()?;
        preferences.enabled = false;
        preferences.save()?;
    }
    configure(state)?;
    if show_settings {
        settings::show_monitor(state)?;
    }
    Ok(())
}

fn apply(state: &Rc<RefCell<PaneApp>>, config: Config) -> Result<(), String> {
    apply_visibility(state, config.desktop_hidden);
    apply_settings(state, &config)
}

fn apply_visibility(state: &Rc<RefCell<PaneApp>>, hidden: bool) {
    let windows: Vec<_> = state.borrow().views.iter().map(|v| v.window.hwnd().cast()).collect();
    super::super::desktop_toggle::child_visibility(hidden, &windows);
}

fn apply_update(state: &Rc<RefCell<PaneApp>>, update: Update) -> Result<(), String> {
    match update {
        Update::Configure(config) => apply(state, *config),
        Update::DesktopVisibility(hidden) => {
            apply_visibility(state, hidden);
            Ok(())
        }
    }
}

fn apply_settings(state: &Rc<RefCell<PaneApp>>, config: &Config) -> Result<(), String> {
    let id = config.panel.id();
    let initial = state.borrow().views.is_empty();
    let rect_changed;
    {
        let mut s = state.borrow_mut();
        rect_changed = s
            .workspace
            .panel(id)
            .is_none_or(|p| p.rect() != config.panel.rect());
        s.store
            .save_preference("language", &config.language)
            .map_err(|e| e.to_string())?;
        s.store
            .save_preference("ui_font_family", &config.font)
            .map_err(|e| e.to_string())?;
        crate::i18n::initialize(&s.store)?;
        fonts::load(&s.store)?;
        for (flag, value) in [
            &interface_flags::PANEL_SHADOW,
            &interface_flags::HEADER_DIVIDER,
            &interface_flags::COMPACT_MENU,
            &interface_flags::TITLE_EMOJI,
        ]
        .into_iter()
        .zip(config.flags)
        {
            flag.save(&s.store, value)?;
        }
        snap::save_gap(&s.store, config.snap_gap)?;
        interface_flags::save_system_corners(&s.store, config.system_corners)?;
        s.workspace.set_pane_options(config.options);
        if let Some(panel) = s.workspace.panel_mut(id) {
            *panel = config.panel.clone();
        } else {
            s.workspace
                .add_panel(config.panel.clone())
                .map_err(|e| e.to_string())?;
        }
        config.preferences.save()?;
    }
    if initial {
        create_view(state, id)?;
    } else {
        handle(state, id, Event::MonitorOptions(config.preferences.clone()))?;
    }
    let hwnd = {
        let s = state.borrow();
        s.views.first().map(|view| {
            let mut model = view.model.borrow_mut();
            model.title = config.panel.title().into();
            model.theme = config.panel.theme();
            model.dark = theme::is_dark(model.theme);
            model.backdrop = config.panel.backdrop();
            model.options = config.options;
            model.locked = config.panel.locked();
            model.auto_hide = config.panel.auto_hide();
            view.window.hwnd().cast()
        })
    };
    // Native window updates may reenter callbacks; all state/model borrows end above.
    if let Some(hwnd) = hwnd {
        window::set_layer(hwnd, config.panel.always_on_top());
        window::update_auto_hide(hwnd, config.panel.auto_hide());
        unsafe {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
    events::show_collapsed(&state.borrow(), id, config.panel.collapsed());
    if rect_changed && !initial {
        if let Some(hwnd) = hwnd {
            let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            let rect = config.panel.rect();
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    (rect.x * scale) as i32,
                    (rect.y * scale) as i32,
                    (rect.width * scale) as i32,
                    (rect.height * scale) as i32,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
    }
    PUBLISHED.with(|last| *last.borrow_mut() = Some(config.panel.clone()));
    Ok(())
}
fn create_controller(
    state: &Rc<RefCell<PaneApp>>,
    commands: mpsc::Receiver<Result<Update, String>>,
) -> Result<windows_window::Window, String> {
    let weak = Rc::downgrade(state);
    windows_window::Window::new("LucidDesk monitor controller")
        .size(1, 1)
        .style(WS_POPUP)
        .ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
        .on_message(move |_, msg, _, lp| {
            if unsafe { crate::window_visibility::defer_show(msg, lp, false) } {
                return Some(0);
            }
            if msg == WM_DESTROY {
                return Some(0);
            }
            if msg != wake::READY {
                return None;
            }
            let Some(state) = weak.upgrade() else {
                return Some(0);
            };
            state.borrow().wake.received();
            loop {
                match commands.try_recv() {
                    Ok(Ok(update)) => {
                        if let Err(error) = apply_update(&state, update) {
                            emit(Reply::Error(error));
                            windows_window::quit();
                        }
                    }
                    Ok(Err(error)) => {
                        emit(Reply::Error(error));
                        windows_window::quit();
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        windows_window::quit();
                        break;
                    }
                }
            }
            Some(0)
        })
        .create()
        .map_err(|e| e.to_string())
}

pub(crate) fn run() -> Result<(), String> {
    run_inner().inspect_err(|error| emit(Reply::Error(error.clone())))
}
fn run_inner() -> Result<(), String> {
    CHILD.store(true, std::sync::atomic::Ordering::Relaxed);
    let _graphics = native_graphics::GraphicsLifetime;
    let mut input = BufReader::new(std::io::stdin());
    let mut line = String::new();
    if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
        return Ok(());
    }
    let config: Config = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if !config.panel.is_monitor() || !config.preferences.enabled {
        return Err("Invalid monitor startup configuration".into());
    }
    let (_, receiver) = mpsc::channel();
    let state = Rc::new(RefCell::new(PaneApp {
        monitors: Default::default(),
        monitor_child: true,
        sorting: Default::default(),
        wake: Default::default(),
        folders: HashMap::new(),
        tab_models: HashMap::new(),
        settings: None,
        session: None,
        drops: Vec::new(),
        runtime: None,
        workspace: Workspace::new(),
        store: WorkspaceStore::open_in_memory().map_err(|e| e.to_string())?,
        views: Vec::new(),
        images: HashMap::new(),
        receiver,
    }));
    apply(&state, config)?;
    let (sender, commands) = mpsc::channel();
    let controller = create_controller(&state, commands)?;
    let wake = state.borrow().wake.clone();
    wake.bind(controller.hwnd() as isize);
    std::thread::spawn(move || {
        for line in input.lines() {
            let config = line
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()));
            if sender.send(config).is_err() {
                break;
            }
            wake.notify();
        }
        drop(sender);
        wake.notify();
    });
    emit(Reply::Ready(
        state
            .borrow()
            .views
            .first()
            .ok_or("Monitor plugin unavailable")?
            .window
            .hwnd() as isize,
    ));
    windows_window::run();
    state.borrow().wake.unbind();
    drop(controller);
    let views = std::mem::take(&mut state.borrow_mut().views);
    for view in &views {
        window::prepare_close(view.window.hwnd().cast());
    }
    drop(views);
    drop(state);
    Ok(())
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
