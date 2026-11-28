//! Live folder sources are separate from Explorer desktop membership.
use super::*;
use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
pub(super) mod entry_mode;
mod images;
mod preferences;
pub(super) use entry_mode::EntryMode;
pub(super) use preferences::{Defaults, save_columns, saved_columns, toggle_column, visible_columns};
#[cfg(test)]
use std::time::{Duration, Instant};
use windows::Win32::{
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    UI::Shell::*,
};

// Distinguish complete snapshots even when paths/counts return to their old values.
fn next_content_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub(super) struct Source {
    pub path: PathBuf,
    root: PathBuf,
    history: Vec<PathBuf>,
    pub sort: (u8, bool),
    request: Arc<Commands>,
    cache: images::SharedCache,
    updates: mpsc::Receiver<Result<Vec<Item>, String>>,
    images: mpsc::Receiver<Patch>,
    viewport: Option<(i32, HashSet<ShellIdentity>)>,
    loaded_resolutions: HashMap<ShellIdentity, i32>,
    pub items: Vec<Item>,
    pub content_revision: u64,
    types_requested: Cell<bool>,
    types_queried: Cell<bool>,
    pub status: Option<String>,
    pub loading: bool,
    worker_stopped: bool,
    pending_rename: Option<PathBuf>,
}

pub(super) type ImageCache = images::SharedCache;

struct ImageRequest {
    generation: u64,
    size: i32,
    items: Vec<Item>,
}

enum Patch {
    Images(u64, Vec<Item>),
    Types(Vec<Item>),
}

impl Source {
    fn request_types(&self) {
        if self.loading || self.worker_stopped || self.types_requested.replace(true) {
            return;
        }
        let mut jobs = self.request.type_jobs.lock().unwrap();
        jobs.extend(self.items.iter().filter(|item| !item.details.folder && item.details.kind.is_empty())
            .map(|item| Item {
                identity: item.identity.clone(),
                label: String::new(),
                image: None,
                details: ItemDetails {
                    modified_time: item.details.modified_time,
                    size: item.details.size,
                    ..Default::default()
                },
            }));
        if !jobs.is_empty() {
            self.request.signal();
        }
    }

    pub(super) fn query_types(&self) {
        if self.loading {
            self.types_queried.set(true);
        } else {
            self.request_types();
        }
    }

    fn cancel_images(&mut self) {
        self.viewport = None;
        self.loaded_resolutions.clear();
        self.request.replace_images(0, Vec::new());
    }

    fn set_viewport(&mut self, viewport: Option<(i32, HashSet<ShellIdentity>)>) -> bool {
        if self.viewport == viewport { return false; }
        let mut jobs = Vec::new();
        let mut changed = false;
        let mut used = Vec::new();
        for item in &mut self.items {
            if let Some((size, _)) = viewport.as_ref().filter(|(_, wanted)| wanted.contains(&item.identity)) {
                if item.image.is_some() && self.loaded_resolutions.get(&item.identity) == Some(size) {
                    used.push(item.identity.persistent_key());
                } else if self.cache.lock().unwrap().restore(item, *size) {
                    self.loaded_resolutions.insert(item.identity.clone(), *size);
                    changed = true;
                } else {
                    jobs.push(Item { image: None, ..item.clone() });
                }
            } else {
                if item.image.take().is_some() { changed = true; }
                self.loaded_resolutions.remove(&item.identity);
            }
        }
        if let Some((size, _)) = &viewport && !used.is_empty() {
            self.cache.lock().unwrap().touch(&used, *size);
        }
        self.request.replace_images(viewport.as_ref().map_or(0, |(size, _)| *size), jobs);
        self.viewport = viewport;
        changed
    }

    fn poll_scan(&mut self) -> bool {
        if self.worker_stopped {
            return false;
        }
        let mut latest = None;
        loop {
            match self.updates.try_recv() {
                Ok(result) => latest = Some(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker_stopped = true;
                    self.request
                        .images_pending
                        .store(false, std::sync::atomic::Ordering::Release);
                    break;
                }
            }
        }
        let changed = latest.is_some() || self.worker_stopped;
        if let Some(result) = latest {
            self.apply_scan(result);
        }
        if self.worker_stopped {
            self.loading = false;
            if self.status.is_none() {
                self.apply_scan(Err(crate::i18n::format(
                    "ui-cannot-read-folder-check-the-path-and-permissions-n",
                    &[(
                        "error",
                        "Folder worker disconnected; reopen the panel".into(),
                    )],
                )));
            }
        }
        changed
    }

    fn apply_scan(&mut self, result: Result<Vec<Item>, String>) {
        self.content_revision = next_content_revision();
        self.loading = false;
        match result {
            Ok(items) => {
                self.cancel_images();
                self.request.type_jobs.lock().unwrap().clear();
                self.types_requested.set(false);
                self.items = items;
                sort_items(&mut self.items, self.sort);
                self.status = None;
            }
            Err(error) => self.status = Some(error),
        }
    }
}

struct Commands {
    event: isize,
    stop: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicBool,
    rescan: std::sync::atomic::AtomicBool,
    images_pending: std::sync::atomic::AtomicBool,
    image_generation: std::sync::atomic::AtomicU64,
    image_request: Mutex<Option<ImageRequest>>,
    type_jobs: Mutex<VecDeque<Item>>,
}
impl Commands {
    fn new() -> Result<Self, String> {
        let event = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
            )
        };
        if event.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            event: event as isize,
            stop: false.into(),
            active: true.into(),
            rescan: true.into(),
            images_pending: false.into(),
            image_generation: 0.into(),
            image_request: Mutex::new(None),
            type_jobs: Mutex::default(),
        })
    }
    fn signal(&self) {
        unsafe {
            windows_sys::Win32::System::Threading::SetEvent(self.event as _);
        }
    }

    fn replace_images(&self, size: i32, items: Vec<Item>) {
        use std::sync::atomic::Ordering;
        let mut pending = self.image_request.lock().unwrap();
        let generation = self.image_generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        *pending = (!items.is_empty()).then_some(ImageRequest { generation, size, items });
        self.images_pending.store(pending.is_some(), Ordering::Release);
        self.signal();
    }

    fn finish_images(&self, generation: u64) {
        use std::sync::atomic::Ordering;
        let _pending = self.image_request.lock().unwrap();
        if self.image_generation.load(Ordering::Acquire) == generation {
            self.images_pending.store(false, Ordering::Release);
        }
    }
}
impl Drop for Commands {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.event as _);
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.request
            .stop
            .store(true, std::sync::atomic::Ordering::Release);
        self.request.signal();
    }
}

struct Watch(windows_sys::Win32::Foundation::HANDLE);

fn wait_for_change(handles: &[windows_sys::Win32::Foundation::HANDLE], timeout: u32) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let started = std::time::Instant::now();
    loop {
        let remaining = if timeout == u32::MAX {
            timeout
        } else {
            timeout.saturating_sub(started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32)
        };
        let result = unsafe {
            MsgWaitForMultipleObjectsEx(
                handles.len() as u32,
                handles.as_ptr(),
                remaining,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if result != handles.len() as u32 {
            return result;
        }
        // Shell can create hidden windows on this STA. Keep servicing their
        // messages while waiting indefinitely for directory or shutdown events.
        unsafe {
            let mut msg = windows_sys::Win32::UI::WindowsAndMessaging::MSG::default();
            for _ in 0..64 {
                if PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
impl Watch {
    fn new(path: &Path) -> Option<Self> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            FindFirstChangeNotificationW(
                path.as_ptr(),
                0,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
            )
        };
        (handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE).then_some(Self(handle))
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Storage::FileSystem::FindCloseChangeNotification(self.0);
        }
    }
}

impl Source {
    pub(super) fn set_active(&self, active: bool) {
        use std::sync::atomic::Ordering;
        if self.request.active.swap(active, Ordering::AcqRel) != active && active {
            self.request.rescan.store(true, Ordering::Release);
        }
        self.request.signal();
    }
    pub(super) fn navigation_context(&self) -> String {format!("{:?}:{:?}:{:?}",self.root,self.path,self.history)}
    pub(super) fn navigation(&self) -> [bool; 2] {
        [!self.history.is_empty(), self.path != self.root]
    }

    #[cfg(test)]
    fn start(path: PathBuf, wake: wake::Wake) -> Result<Self, String> {
        Self::start_with(path, wake, Arc::default(), (0, false))
    }

    fn start_with(path: PathBuf, wake: wake::Wake, cache: images::SharedCache, sort: (u8, bool)) -> Result<Self, String> {
        let request = Arc::new(Commands::new()?);
        let commands = Arc::clone(&request);
        let (sender, updates) = mpsc::sync_channel(1);
        let (image_sender, image_updates) = mpsc::sync_channel(2);
        let worker_cache = Arc::clone(&cache);
        let root = path.clone();
        std::thread::Builder::new()
            .name("folder-pane".into())
            .spawn(move || {
                let _exit = wake.on_drop();
                let sender = sender;
                let image_sender = image_sender;
                let _apartment = match ShellApartment::initialize_sta() {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        wake.notify();
                        return;
                    }
                };
                use std::sync::atomic::Ordering;
                let mut watch = Watch::new(&root);
                let cache = worker_cache;
                let mut scan_failed = false;
                loop {
                    if commands.stop.load(Ordering::Acquire) { break; }
                    if commands.active.load(Ordering::Acquire) {
                        if commands.rescan.swap(false, Ordering::AcqRel) {
                            let result = luciddesk_shell::enumerate_folder(&root)
                                .map(|entries| {
                                    let mut items: Vec<Item> = entries.into_iter().map(|entry| Item {
                                        details: ItemDetails {
                                            modified: modified_text(entry.modified),
                                            folder: entry.attributes.folder,
                                            modified_time: entry.modified,
                                            size: entry.size,
                                            ..Default::default()
                                        },
                                        identity: entry.identity,
                                        label: entry.display_name,
                                        image: None,
                                    }).collect();
                                    let live = items.iter().map(|item| (item.identity.persistent_key(), item)).collect();
                                    cache.lock().unwrap().retain_folder(&root, &live);
                                    sort_items(&mut items, sort);
                                    items
                                })
                                .map_err(|error| {
                                    crate::i18n::format("ui-cannot-read-folder-check-the-path-and-permissions-n", &[("error", error.to_string())])
                                });
                            scan_failed = result.is_err();
                            if sender.send(result).is_err() { break; }
                            wake.notify();
                        }
                        let pending_images = commands.image_request.lock().unwrap().take();
                        if let Some(request) = pending_images {
                            let generation = request.generation;
                            images::enrich(request, &commands, &cache, &image_sender, &wake);
                            commands.finish_images(generation);
                            wake.notify();
                            continue;
                        }
                        let mut batch: Vec<_> = {
                            let mut jobs = commands.type_jobs.lock().unwrap();
                            let count = jobs.len().min(32);
                            jobs.drain(..count).collect()
                        };
                        if !batch.is_empty() {
                            // Yield between batches so new viewports can load thumbnails first.
                            for item in &mut batch {
                                item.details.kind = file_type(&item.identity);
                            }
                            if image_sender.send(Patch::Types(batch)).is_err() { break; }
                            wake.notify();
                            continue;
                        }
                    }
                    use windows_sys::Win32::System::Threading::*;
                    let handles = [
                        commands.event as _,
                        watch.as_ref().map_or(std::ptr::null_mut(), |w| w.0),
                    ];
                    let result = wait_for_change(
                        &handles[..if watch.is_some() { 2 } else { 1 }],
                        if (watch.is_some() && !scan_failed) || !commands.active.load(std::sync::atomic::Ordering::Acquire) { INFINITE } else { 2000 },
                    );
                    if commands.stop.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    if result == 1 || result == windows_sys::Win32::Foundation::WAIT_TIMEOUT {
                        commands.rescan.store(true, Ordering::Release);
                    }
                    if result == 1 {
                        if unsafe {
                            windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification(
                                handles[1],
                            )
                        } == 0
                        {
                            watch = None;
                        }
                        // Merge a burst without indefinitely postponing a visible update.
                        unsafe {
                            WaitForSingleObject(commands.event as _, 100);
                        }
                    } else if result == u32::MAX {
                        let _ = sender.send(Err(std::io::Error::last_os_error().to_string()));
                        wake.notify();
                        break;
                    }
                    if watch.is_none() {
                        watch = Watch::new(&root);
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            root: path.clone(),
            history: Vec::new(),
            sort,
            path,
            request,
            cache,
            updates,
            images: image_updates,
            viewport: None,
            loaded_resolutions: HashMap::new(),
            items: Vec::new(),
            content_revision: next_content_revision(),
            types_requested: Cell::new(false),
            types_queried: Cell::new(false),
            status: None,
            loading: true,
            worker_stopped: false,
            pending_rename: None,
        })
    }
    pub fn refresh(&self) {
        self.request.rescan.store(true, std::sync::atomic::Ordering::Release);
        self.request.signal();
    }
}

pub(super) fn ensure(state: &mut PaneApp, id: PanelId) -> Result<(), String> {
    let path = state
        .workspace
        .panel(id)
        .and_then(Panel::folder)
        .map(Path::to_path_buf);
    if let Some(path) = path {
        if state
            .folders
            .get(&id)
            .is_none_or(|source| source.root != path)
        {
            let sort = state
                .store
                .preference(&format!("panel_folder_sort:{}", id.get()))
                .map_err(|e| e.to_string())?
                .and_then(|v| {
                    let (column, direction) = v.split_once(':')?;
                    Some((column.parse::<u8>().ok()?.min(3), direction == "desc"))
                })
                .unwrap_or((0, false));
            let source = Source::start_with(path, state.wake.clone(), Arc::clone(&state.folder_images), sort)?;
            state.folders.insert(id, source);
        }
    } else {
        state.folders.remove(&id);
    }
    Ok(())
}

#[derive(Default)]
struct ItemPatch {
    kind: Option<String>,
    image: Option<Arc<assets::Pixels>>,
}

pub(super) fn poll(state: &mut PaneApp) -> Vec<(PanelId, ShellIdentity)> {
    let mut refresh = HashMap::new();
    let mut repaint = HashMap::<PanelId, HashMap<ShellIdentity, ItemPatch>>::new();
    for (&id, source) in &mut state.folders {
        // Each update is a complete snapshot. Sort and publish only the newest
        // one when the UI was busy while several scans completed.
        let scanned = source.poll_scan();
        let queried = !source.loading && source.types_queried.replace(false);
        if !source.types_requested.get()
            && (source.sort.0 == 1 || queried
                || state.workspace.panel(id).is_some_and(Panel::list_view))
        {
            source.request_types();
        }
        let generation = source.request.image_generation.load(std::sync::atomic::Ordering::Acquire);
        let mut image_updates = HashMap::new();
        let mut type_updates = HashMap::new();
        while let Ok(batch) = source.images.try_recv() {
            match batch {
                Patch::Images(request, batch) if request == generation => {
                    image_updates.extend(batch.into_iter().map(|item| (item.identity.clone(), item)));
                }
                Patch::Images(..) => {}
                Patch::Types(batch) => {
                    type_updates.extend(batch.into_iter().map(|item| (item.identity.clone(), item)));
                }
            }
        }
        let mut reordered = false;
        if !image_updates.is_empty() || !type_updates.is_empty() {
            let mut kind_changed = false;
            for item in &mut source.items {
                if let Some(update) = type_updates.remove(&item.identity)
                    && same_revision(item, &update) && item.details.kind != update.details.kind
                {
                    item.details.kind = update.details.kind;
                    if !scanned {
                        repaint.entry(id).or_default().entry(item.identity.clone()).or_default()
                            .kind = Some(item.details.kind.clone());
                    }
                    kind_changed = true;
                }
                if let Some(update) = image_updates.remove(&item.identity)
                    && let Some((size, _)) = source.viewport.as_ref().filter(|(_, wanted)| wanted.contains(&item.identity))
                    && same_revision(item, &update)
                    && let Some(image) = update.image
                {
                    source.loaded_resolutions.insert(item.identity.clone(), *size);
                    if replace_image(item, image) && !scanned {
                        repaint.entry(id).or_default().entry(item.identity.clone()).or_default()
                            .image = item.image.clone();
                    }
                }
                if image_updates.is_empty() && type_updates.is_empty() { break; }
            }
            if kind_changed {
                source.content_revision = next_content_revision();
                if source.sort.0 == 1 {
                    reordered = sort_items(&mut source.items, source.sort);
                }
            }
        }
        if scanned || reordered {
            refresh.insert(id, reordered && !scanned);
        }
    }
    for view in &state.views {
        let hwnd = view.window.hwnd().cast();
        if let Some(&reordered) = refresh.get(&view.id) {
            refresh_view(state, view, false);
            if reordered {
                unsafe { PostMessageW(hwnd, window::SYNC_POINTER, 0, 0); }
            }
            continue;
        }
        let Some(mut patches) = repaint.remove(&view.id) else { continue; };
        for item in &mut view.model.borrow_mut().items {
            if let Some(patch) = patches.remove(&item.identity) {
                if let Some(kind) = patch.kind { item.details.kind = kind; }
                if let Some(image) = patch.image { item.image = Some(image); }
            }
            if patches.is_empty() { break; }
        }
        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
    }
    let mut renames = Vec::new();
    for view in &state.views {
        let Some(source) = state.folders.get_mut(&view.id) else { continue; };
        let Some(path) = &source.pending_rename else { continue; };
        let mut model = view.model.borrow_mut();
        if let Some(index) = model.items.iter().position(|item| item.identity.file_system_path() == Some(path.as_path())) {
            let identity = model.items[index].identity.clone();
            model.select_item(index, false, false);
            let mut bounds = RECT::default();
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(view.window.hwnd().cast(), &raw mut bounds); }
            let scale = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(view.window.hwnd().cast()) }.max(96) as f32 / 96.0;
            let grid = model.grid(bounds.right as f32 / scale, bounds.bottom as f32 / scale);
            model.scroll = (index / grid.columns.max(1)).min(grid.max_scroll(model.items.len()));
            source.pending_rename = None;
            renames.push((view.id, identity));
        }
    }
    for view in &state.views {
        if (refresh.contains_key(&view.id) || renames.iter().any(|(id, _)| *id == view.id))
            && let Some(source) = state.folders.get_mut(&view.id)
        {
            sync_viewport(source, view);
        }
    }
    renames
}

pub(super) fn update_viewport(state: &mut PaneApp, id: PanelId) {
    if let Some(source) = state.folders.get_mut(&id)
        && let Some(view) = state.views.iter().find(|view| view.id == id)
    {
        sync_viewport(source, view);
    }
}

pub(super) fn deactivate(state: &mut PaneApp, id: PanelId) {
    if let Some(source) = state.folders.get_mut(&id) {
        source.set_active(false);
        source.set_viewport(None);
        if let Some(model) = state.tab_models.get_mut(&id) {
            for item in &mut model.items { item.image = None; }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewportKey {
    tab: PanelId,
    visible: Option<VisibleViewport>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct VisibleViewport {
    width: i32,
    height: i32,
    dpi: u32,
    items_revision: u64,
    scroll: usize,
    scroll_x: u32,
    list: bool,
    fixed_grid: bool,
    free_layout: bool,
    icon_display: luciddesk_core::IconDisplay,
    icon_size: u32,
    grid_scale: u32,
    font_revision: u64,
    asset_font_revision: u64,
}

impl ViewportKey {
    pub(super) fn read(hwnd: windows_sys::Win32::Foundation::HWND, model: &GroupModel) -> Self {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut bounds = RECT::default();
        let visible = !model.collapsed && !model.merge_occluded && unsafe {
            IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0
                && GetClientRect(hwnd, &raw mut bounds) != 0
        };
        Self {
            tab: model.active_tab,
            visible: visible.then(|| VisibleViewport {
                width: bounds.right,
                height: bounds.bottom,
                dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
                items_revision: model.items_revision,
                scroll: model.scroll,
                scroll_x: model.scroll_x.to_bits(),
                list: model.is_list(),
                fixed_grid: model.fixed_grid,
                free_layout: model.free_layout,
                icon_display: model.icon_display,
                icon_size: model.icon_size.to_bits(),
                grid_scale: model.options.grid_scale.to_bits(),
                font_revision: fonts::revision(),
                asset_font_revision: assets::font_revision(),
            }),
        }
    }
}

fn sync_viewport(source: &mut Source, view: &super::View) {
    let hwnd = view.window.hwnd().cast();
    let mut model = view.model.borrow_mut();
    let demand = ViewportKey::read(hwnd, &model).visible.and_then(|viewport| {
        let scale = viewport.dpi as f32 / 96.0;
        let width = viewport.width as f32 / scale;
        let height = viewport.height as f32 / scale;
        let grid = model.grid(width, height);
        if height - model.horizontal_inset(width) <= grid.content_top + 1.0 {
            return None;
        }
        let wanted = model.visible_indices(grid, grid.content_top - grid.cell_height, height + grid.cell_height)
            .filter(|&index| {
                let (x, y) = model.cell(grid, index);
                x + grid.cell_width > 6.0 && x < width - 6.0
                    && y + grid.cell_height * 2.0 > grid.content_top
            })
            .map(|index| model.items[index].identity.clone())
            .collect::<HashSet<_>>();
        (!wanted.is_empty()).then_some((assets::icon_resolution(grid.icon_size * scale), wanted))
    });
    if source.set_viewport(demand) {
        let images: HashMap<_, _> = source.items.iter().filter_map(|item| {
            item.image.as_ref().map(|image| (&item.identity, image))
        }).collect();
        for item in &mut model.items {
            item.image = images.get(&item.identity).map(|image| Arc::clone(image));
        }
        drop(model);
        unsafe { windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0); }
    }
}

pub(super) fn item_created(state: &mut PaneApp, id: PanelId, path: PathBuf) {
    if let Some(source) = state.folders.get_mut(&id) {
        // A delayed Shell callback must not rename an item in a newly navigated directory.
        if path.parent() == Some(source.path.as_path()) {
            source.pending_rename = Some(path);
            source.refresh();
        }
    }
}

fn same_revision(item: &Item, update: &Item) -> bool {
    item.identity == update.identity
        && item.details.modified_time == update.details.modified_time
        && item.details.folder == update.details.folder
        && item.details.size == update.details.size
}

fn replace_image(item: &mut Item, image: Arc<assets::Pixels>) -> bool {
    if item.image.as_ref().is_some_and(|old| Arc::ptr_eq(old, &image)
        || (old.width == image.width && old.height == image.height && old.data == image.data)) {
        return false;
    }
    item.image = Some(image);
    true
}

pub(super) fn sort_items(items: &mut Vec<Item>, sort: (u8, bool)) -> bool {
    let mut sorted: Vec<_> = std::mem::take(items)
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            // Reuse the worker's directory snapshot; sorting must not touch disk
            // on the UI thread (especially for network folders).
            let folder = item.details.folder;
            let modified = item.details.modified_time;
            let name = item.label.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let kind = if sort.0 == 1 {
                item.details.kind.encode_utf16().chain(Some(0)).collect::<Vec<_>>()
            } else { Vec::new() };
            (folder, name, kind, modified, item, index)
        })
        .collect();
    let text_order = |a: &[u16], b: &[u16]| unsafe {
        windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.as_ptr(), b.as_ptr()).cmp(&0)
    };
    let direction = |order: std::cmp::Ordering| if sort.1 { order.reverse() } else { order };
    // The identity tie-breaker gives deterministic order without stable-sort scratch storage.
    sorted.sort_unstable_by(|(af, an, ak, at, a, _), (bf, bn, bk, bt, b, _)| {
        let name = || text_order(an, bn).then_with(|| an.cmp(bn)).then_with(|| a.identity.persistent_key().cmp(&b.identity.persistent_key()));
        match sort.0 {
            // Explorer reverses the complete name order, including folder grouping.
            0 => direction(bf.cmp(af).then_with(name)),
            // Type keeps folders first and names ascending in either direction.
            1 => bf.cmp(af).then_with(|| {
                if *af { name() } else {
                    let unknown = |kind: &str| kind.trim().is_empty() || kind == "—";
                    unknown(&a.details.kind).cmp(&unknown(&b.details.kind))
                        .then_with(|| if unknown(&a.details.kind) { std::cmp::Ordering::Equal } else { direction(text_order(ak, bk)) }).then_with(name)
                }
            }),
            // Preserve the existing mixed timeline for modified-date sorting.
            2 => {
                if at.is_none() || bt.is_none() {
                    at.is_none().cmp(&bt.is_none()).then_with(name)
                } else {
                    direction(at.cmp(bt)).then_with(name)
                }
            }
            // Empty sizes precede files ascending and follow them descending;
            // equal sizes (including folders) always use ascending names.
            3 => direction(bf.cmp(af)).then_with(|| {
                if *af { name() } else {
                    a.details.size.is_none().cmp(&b.details.size.is_none())
                        .then_with(|| direction(a.details.size.cmp(&b.details.size))).then_with(name)
                }
            }),
            _ => name(),
        }
    });
    let reordered = sorted.iter().enumerate().any(|(index, item)| index != item.5);
    *items = sorted.into_iter().map(|(_, _, _, _, item, _)| item).collect();
    reordered
}

/// Refresh persisted view preferences without saving or restarting an unchanged folder source.
pub(super) fn apply_saved_preferences(state: &mut PaneApp, id: PanelId) -> Result<(), String> {
    ensure(state,id)?;
    let preferences=state.store.folder_preferences(id).map_err(|e|e.to_string())?;
    let order=(preferences.sort_column,preferences.descending);
    let source=state.folders.get_mut(&id).ok_or("folder source unavailable")?;
    let changed=source.sort!=order;
    if changed {source.sort=order;sort_items(&mut source.items,order);}
    if let Some(view)=state.views.iter().find(|v|v.id==id) {
        let mut model=view.model.borrow_mut();
        let path_changed=model.folder.as_ref()!=Some(&source.path);
        model.folder=Some(source.path.clone());
        model.list_view=state.workspace.panel(id).unwrap().list_view();
        model.folder_sort=order;
        model.folder_columns=preferences.column_widths;
        model.folder_visible_columns=preferences.visible_columns;
        if changed || path_changed {model.scroll=0;model.clear_selection();}
    }
    Ok(())
}

pub(super) fn sort(state: &mut PaneApp, id: PanelId, column: u8) -> Result<(), String> {
    let Some(source) = state.folders.get_mut(&id) else {
        return Ok(());
    };
    let column = column.min(3);
    let order = super::sorting::next_order(Some(source.sort), column);
    set_sort(state, id, order)
}

pub(super) fn set_sort(state: &mut PaneApp, id: PanelId, order: (u8, bool)) -> Result<(), String> {
    let Some(source) = state.folders.get_mut(&id) else { return Ok(()); };
    state
        .store
        .save_preference(
            &format!("panel_folder_sort:{}", id.get()),
            &format!("{}:{}", order.0, if order.1 { "desc" } else { "asc" }),
        )
        .map_err(|e| e.to_string())?;
    source.sort = order;
    state.sorting.orders.borrow_mut().insert(id, order);
    sort_items(&mut source.items, order);
    if order.0 == 1 { source.request_types(); }
    if let Some(view) = state.views.iter().find(|view| view.id == id) {
        view.model.borrow_mut().scroll = 0;
    }
    refresh_changed_views(state, true);
    Ok(())
}

pub(super) fn navigate(
    state: &mut PaneApp,
    id: PanelId,
    path: Option<PathBuf>,
) -> Result<(), String> {
    let Some(old) = state.folders.get(&id) else {
        return Ok(());
    };
    let mut history = old.history.clone();
    let path = if let Some(path) = path {
        if !path.is_dir() {
            return Err(crate::i18n::text("ui-folder-inaccessible").into());
        }
        history.push(old.path.clone());
        path
    } else {
        let Some(path) = history.pop() else {
            return Ok(());
        };
        path
    };
    let mut source = Source::start_with(path.clone(), state.wake.clone(), Arc::clone(&old.cache), old.sort)?;
    source.root = old.root.clone();
    source.history = history;
    state.folders.insert(id, source);
    if let Some(view) = state.views.iter().find(|v| v.id == id) {
        let mut model = view.model.borrow_mut();
        model.folder = Some(path);
        model.clear_selection();
        model.scroll = 0;
    }
    refresh_changed_views(state, true);
    Ok(())
}

pub(super) fn home(state: &mut PaneApp, id: PanelId) -> Result<(), String> {
    let Some(source) = state.folders.get(&id) else { return Ok(()); };
    if source.path == source.root { return Ok(()); }
    let root = source.root.clone();
    navigate(state, id, Some(root))?;
    state.folders.get_mut(&id).unwrap().history.clear();
    refresh_changed_views(state, true);
    Ok(())
}

pub(super) fn choose(owner: isize) -> Result<Option<PathBuf>, String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.to_string())?;
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)
            .map_err(|e| e.to_string())?;
        dialog
            .SetTitle(windows::core::PCWSTR(crate::i18n::wide("ui-select-a-folder-to-display")))
            .map_err(|e| e.to_string())?;
        if let Err(error) = dialog.Show(Some(windows::Win32::Foundation::HWND(owner as _))) {
            if error.code().0 as u32 == 0x800704c7 {
                return Ok(None);
            }
            return Err(error.to_string());
        }
        let item = dialog.GetResult().map_err(|e| e.to_string())?;
        let raw = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| e.to_string())?;
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_wide(raw.as_wide()));
        windows::Win32::System::Com::CoTaskMemFree(Some(raw.0.cast()));
        Ok(Some(path))
    }
}

pub(super) fn identity(path: PathBuf) -> ShellIdentity {
    ShellIdentity::FileSystem {
        path,
        volume_id: None,
        file_id: None,
    }
}

fn file_type(identity: &ShellIdentity) -> String {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_TYPENAME, SHGetFileInfoW};
    let path: Vec<_> = identity
        .activation_name()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut info = SHFILEINFOW::default();
    if unsafe {
        SHGetFileInfoW(
            path.as_ptr(),
            0,
            &raw mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME,
        )
    } == 0
    {
        return "—".into();
    }
    let name = info.szTypeName;
    String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())])
}

pub(super) fn size_text(bytes: Option<u64>, folder: bool) -> String {
    if folder { return String::new(); }
    let Some(bytes) = bytes else { return "—".into(); };
    if bytes < 1024 { return format!("{bytes} B"); }
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB", "TB", "PB", "EB"] {
        value /= 1024.0;
        unit = next;
        if value < 1024.0 { break; }
    }
    format!("{value:.1} {unit}")
}

pub(super) fn modified_text(value: Option<std::time::SystemTime>) -> String {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx},
    };
    let Some(time) = value else {
        return "—".into();
    };
    let epoch = 116_444_736_000_000_000u128;
    let ticks = match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => epoch.checked_add(duration.as_nanos() / 100),
        Err(error) => epoch.checked_sub(error.duration().as_nanos() / 100),
    }
    .and_then(|ticks| u64::try_from(ticks).ok());
    let Some(ticks) = ticks else {
        return "—".into();
    };
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    if unsafe { FileTimeToSystemTime(&file, &raw mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTimeEx(std::ptr::null(), &utc, &raw mut local) } == 0
    {
        return "—".into();
    }
    format!(
        "{:04}/{:02}/{:02} {:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    )
}

pub(super) fn accepts_copy(items: &[ShellIdentity], destination: &Path) -> bool {
    let destination = destination.to_string_lossy().to_lowercase();
    !items.is_empty()
        && items.iter().all(|item| {
            item.file_system_path().is_some_and(|path| {
                let source = path.to_string_lossy().to_lowercase();
                path.parent()
                    .is_some_and(|parent| parent.to_string_lossy().to_lowercase() != destination)
                    && source != destination
                    && !destination.starts_with(&format!("{source}\\"))
            })
        })
}

pub(super) fn request_picker(
    state: &Rc<RefCell<PaneApp>>,
    id: PanelId,
    change: bool,
) -> Result<(), String> {
    let owner = state
        .borrow()
        .views
        .iter()
        .find(|v| v.id == id)
        .map_or(0, |v| v.window.hwnd() as isize);
    let weak = Rc::downgrade(state);
    if !window::defer_action(move || {
        let result = (|| -> Result<(), String> {
            let Some(path) = choose(owner)? else {
                return Ok(());
            };
            let Some(state) = weak.upgrade() else {
                return Ok(());
            };
            handle(
                &state,
                id,
                if change {
                    Event::SetFolder(path)
                } else {
                    Event::MapFolder(path)
                },
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            window::error_for(owner as _, &error);
        }
    }) {
        return Err(crate::i18n::text("ui-could-not-open-folder-picker").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod memory_bench;
