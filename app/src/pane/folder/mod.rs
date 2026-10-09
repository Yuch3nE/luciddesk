//! Live folder sources are separate from Explorer desktop membership.
use super::*;
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

pub(super) struct Source {
    pub path: PathBuf,
    root: PathBuf,
    history: Vec<PathBuf>,
    pub sort: (u8, bool),
    request: Arc<Commands>,
    cache: images::SharedCache,
    updates: mpsc::Receiver<Result<Vec<Item>, String>>,
    images: mpsc::Receiver<Vec<Item>>,
    pub items: Vec<Item>,
    pub status: Option<String>,
    pub loading: bool,
    pending_rename: Option<PathBuf>,
}

struct Commands {
    event: isize,
    stop: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicBool,
    images_pending: std::sync::atomic::AtomicBool,
    priority: Mutex<Vec<String>>,
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
            images_pending: false.into(),
            priority: Mutex::new(Vec::new()),
        })
    }
    fn signal(&self) {
        unsafe {
            windows_sys::Win32::System::Threading::SetEvent(self.event as _);
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
        self.request.active.store(active, std::sync::atomic::Ordering::Release);
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
                let _apartment = match ShellApartment::initialize_sta() {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        wake.notify();
                        return;
                    }
                };
                let mut watch = Watch::new(&root);
                let cache = worker_cache;
                loop {
                    if commands.stop.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    if commands.active.load(std::sync::atomic::Ordering::Acquire) {
                        let mut jobs = Vec::new();
                        let result = luciddesk_shell::enumerate_folder(&root)
                            .map(|entries| {
                                let mut cache = cache.lock().unwrap();
                                let mut items: Vec<Item> = entries.into_iter().map(|entry| {
                                    let mut item = Item {
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
                                    };
                                    if !cache.restore(&mut item) { jobs.push(item.clone()); }
                                    item
                                }).collect();
                                cache.retain_folder(&root, &items.iter().map(|item| item.identity.persistent_key()).collect());
                                drop(cache);
                                sort_items(&mut items, sort);
                                sort_items(&mut jobs, sort);
                                items
                            })
                            .map_err(|error| {
                                crate::i18n::format("ui-cannot-read-folder-check-the-path-and-permissions-n", &[("error", format!("{}", error))])
                            });
                        if result.is_err() {
                            cache.lock().unwrap().retain_folder(&root, &Default::default());
                            watch = None;
                        }
                        commands.images_pending.store(!jobs.is_empty(), std::sync::atomic::Ordering::Release);
                        if sender.send(result.clone()).is_err() {
                            break;
                        }
                        wake.notify();
                        // Send only completed image/type changes while loading. A final
                        // snapshot also supports consumers waiting for a complete scan.
                        if let Ok(mut items) = result {
                            if !jobs.is_empty() {
                                images::enrich(&mut items, jobs, &commands, &cache, &image_sender, &wake);
                                commands.images_pending.store(false, std::sync::atomic::Ordering::Release);
                                if commands.stop.load(std::sync::atomic::Ordering::Acquire) { return; }
                                if sender.send(Ok(items)).is_err() { return; }
                                wake.notify();
                            }
                        }
                    }
                    use windows_sys::Win32::System::Threading::*;
                    let handles = [
                        commands.event as _,
                        watch.as_ref().map_or(std::ptr::null_mut(), |w| w.0),
                    ];
                    let result = wait_for_change(
                        &handles[..if watch.is_some() { 2 } else { 1 }],
                        if watch.is_some() || !commands.active.load(std::sync::atomic::Ordering::Acquire) { INFINITE } else { 2000 },
                    );
                    if commands.stop.load(std::sync::atomic::Ordering::Acquire) {
                        break;
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
            items: Vec::new(),
            status: None,
            loading: true,
            pending_rename: None,
        })
    }
    pub fn refresh(&self) {
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
            let source = Source::start_with(path, state.wake.clone(), Arc::default(), sort)?;
            state.folders.insert(id, source);
        }
    } else {
        state.folders.remove(&id);
    }
    Ok(())
}

pub(super) fn poll(state: &mut PaneApp) -> Vec<(PanelId, ShellIdentity)> {
    let mut changed = false;
    for source in state.folders.values_mut() {
        let mut latest = None;
        while let Ok(result) = source.updates.try_recv() {
            latest = Some(result);
        }
        // Each update is a complete snapshot. Sort and publish only the newest
        // one when the UI was busy while several scans completed.
        if let Some(result) = latest {
            source.loading = false;
            match result {
                Ok(items) => {
                    source.items = items;
                    sort_items(&mut source.items, source.sort);
                    source.status = None;
                }
                Err(error) => {
                    source.items.clear();
                    source.status = Some(error);
                }
            }
            changed = true;
        }
        let mut patches = HashMap::new();
        while let Ok(batch) = source.images.try_recv() {
            for item in batch { patches.insert(item.identity.clone(), item); }
        }
        if !patches.is_empty() {
            let mut patched = false;
            let mut kind_changed = false;
            for item in &mut source.items {
                if patches.is_empty() { break; }
                if let Some(update) = patches.remove(&item.identity) {
                    let different_kind = item.details.kind != update.details.kind;
                    let applied = apply_image_patch(item, update);
                    kind_changed |= applied && different_kind;
                    patched |= applied;
                }
            }
            if kind_changed && source.sort.0 == 1 { sort_items(&mut source.items, source.sort); }
            changed |= patched;
        }
    }
    if changed {
        refresh_views(state);
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
    // Re-evaluate the viewport while work arrives, including after scrolling or
    // changing the sort order. Unseen files remain queued behind these entries.
    for view in &state.views {
        let Some(source) = state.folders.get(&view.id) else { continue; };
        if !changed && !source.request.images_pending.load(std::sync::atomic::Ordering::Acquire) { continue; }
        let model = view.model.borrow();
        let mut bounds = RECT::default();
        let hwnd = view.window.hwnd().cast();
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &raw mut bounds); }
        let scale = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        let grid = model.grid(bounds.right as f32 / scale, bounds.bottom as f32 / scale);
        let start = model.scroll.saturating_mul(grid.columns).min(model.items.len());
        let count = (grid.visible_rows + 2).saturating_mul(grid.columns);
        let priority: Vec<_> = model.items.iter().skip(start).take(count).map(|item| item.identity.persistent_key()).collect();
        source.cache.lock().unwrap().touch(&priority);
        *source.request.priority.lock().unwrap() = priority;
    }
    renames
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

fn apply_image_patch(item: &mut Item, update: Item) -> bool {
    // Unknown timestamps cannot establish that a delayed result still belongs
    // to this scan. Such items receive images through the full final snapshot.
    if item.identity != update.identity
        || item.details.modified_time.is_none()
        || item.details.modified_time != update.details.modified_time
        || item.details.folder != update.details.folder
        || item.details.size != update.details.size
    {
        return false;
    }
    let image_changed = match (&item.image, &update.image) {
        (Some(a), Some(b)) => !Arc::ptr_eq(a, b)
            && (a.width != b.width || a.height != b.height || a.data != b.data),
        (None, None) => false,
        _ => true,
    };
    let changed = image_changed || item.details.kind != update.details.kind;
    if image_changed { item.image = update.image; }
    item.details.kind = update.details.kind;
    changed
}

pub(super) fn sort_items(items: &mut Vec<Item>, sort: (u8, bool)) {
    let mut sorted: Vec<_> = std::mem::take(items)
        .into_iter()
        .map(|item| {
            // Reuse the worker's directory snapshot; sorting must not touch disk
            // on the UI thread (especially for network folders).
            let folder = item.details.folder;
            let modified = item.details.modified_time;
            let name = item.label.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let kind = if sort.0 == 1 {
                item.details.kind.encode_utf16().chain(Some(0)).collect::<Vec<_>>()
            } else { Vec::new() };
            (folder, name, kind, modified, item)
        })
        .collect();
    let text_order = |a: &[u16], b: &[u16]| unsafe {
        windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.as_ptr(), b.as_ptr()).cmp(&0)
    };
    let direction = |order: std::cmp::Ordering| if sort.1 { order.reverse() } else { order };
    // The identity tie-breaker gives deterministic order without stable-sort scratch storage.
    sorted.sort_unstable_by(|(af, an, ak, at, a), (bf, bn, bk, bt, b)| {
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
    *items = sorted.into_iter().map(|(_, _, _, _, item)| item).collect();
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
            window::error(&error);
        }
    }) {
        return Err(crate::i18n::text("ui-could-not-open-folder-picker").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
