//! Isolated Shell selection for native menus. Never adds items to the desktop.
mod callback;
mod classic;
mod commands;
mod cursor;
mod lifecycle;
mod presenter;
mod selection;

pub mod worker;
use super::{RENAME, wire::MenuContext};
use selection::ResolvedTargets;
use std::{
    cell::{Cell, RefCell},
    ptr::null_mut,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, HWND, RECT},
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
            Ole::IObjectWithSite,
        },
        UI::Shell::*,
    },
    core::{IUnknown, Interface, Result},
};
use windows_sys::Win32::UI::{
    Controls::*,
    Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    WindowsAndMessaging::*,
};
const SUBCLASS: usize = 0x4c504d48;

pub struct MenuHost {
    browser: IExplorerBrowser,
    hwnd: windows_sys::Win32::Foundation::HWND,
    view: Option<IShellView>,
    presenter: Option<presenter::NativePresenter>,
    callbacks: Option<Rc<MenuCallbacks>>,
    names: Vec<String>,
}
struct MenuCallbacks {
    first: Rc<Cell<Option<u32>>>,
    busy: Cell<bool>,
    view: IShellView,
    desktop: windows_sys::Win32::Foundation::HWND,
    context: Cell<MenuContext>,
    presenter: Option<presenter::NativePresenter>,
    invocation: lifecycle::Invocation,
    retired: Cell<bool>,
    classic: RefCell<Option<IContextMenu>>,
}
impl MenuHost {
    fn create(
        desktop: windows_sys::Win32::Foundation::HWND,
        targets: &ResolvedTargets,
        context: MenuContext,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        #[cfg(not(feature = "menu-diagnostics"))]
        let compact_available = true;
        #[cfg(feature = "menu-diagnostics")]
        let compact_available = unsafe {
            GetPropW(context.owner as _, windows_sys::w!("LucidDesk.Test.ForceClassic")).is_null()
        };
        Self::create_impl(desktop, targets, context, cancelled, compact_available)
    }
    fn create_impl(
        desktop: windows_sys::Win32::Foundation::HWND,
        targets: &ResolvedTargets,
        context: MenuContext,
        cancelled: Arc<AtomicBool>,
        compact_available: bool,
    ) -> Result<Self> {
        let names = &targets.names;
        let items = &targets.items;
        unsafe {
            if names.is_empty() || IsWindow(context.owner as _) == 0 {
                return Err(E_INVALIDARG.into());
            }
            let browser: IExplorerBrowser =
                CoCreateInstance(&ExplorerBrowser, None, CLSCTX_INPROC_SERVER)?;
            // A clipped native host supplies DPI/focus/selection context. No desktop
            // icons are drawn here; only the separate system popup is presented.
            let mut instance = null_mut();
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(
                windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                (host_messages as *const ()).cast(), &raw mut instance);
            let class = windows_sys::w!("LucidDesk.IsolatedShellHost.v1");
            let definition = WNDCLASSW {
                lpfnWndProc: Some(host_messages),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&definition);
            // LUCIDDESK_INSPECT marks the test Pane as APPWINDOW. Carry that
            // inspectability to its menu host without changing its empty region.
            let inspect =
                GetWindowLongPtrW(context.owner as _, GWL_EXSTYLE) & WS_EX_APPWINDOW as isize != 0;
            let hwnd = CreateWindowExW(
                (if inspect {
                    WS_EX_APPWINDOW
                } else {
                    WS_EX_TOOLWINDOW
                }) | WS_EX_NOACTIVATE,
                class,
                windows_sys::w!("LucidDesk Menu Host"),
                WS_POPUP,
                context.x.saturating_sub(32),
                context.y.saturating_sub(32),
                640,
                480,
                null_mut(),
                null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return Err(windows::core::Error::from_thread());
            }
            let region = windows_sys::Win32::Graphics::Gdi::CreateRectRgn(0, 0, 0, 0);
            if windows_sys::Win32::Graphics::Gdi::SetWindowRgn(hwnd, region, 0) == 0 {
                windows_sys::Win32::Graphics::Gdi::DeleteObject(region);
                DestroyWindow(hwnd);
                return Err(windows::core::Error::from_thread());
            }
            let mut host = Self {
                browser,
                hwnd,
                view: None,
                presenter: None,
                callbacks: None,
                names: names.to_vec(),
            };
            host.browser
                .Initialize(
                    HWND(hwnd),
                    &RECT {
                        left: 0,
                        top: 0,
                        right: 640,
                        bottom: 480,
                    },
                    Some(&FOLDERSETTINGS {
                        ViewMode: FVM_ICON.0 as u32,
                        fFlags: FWF_AUTOARRANGE.0 as u32,
                    }),
                )
                .map_err(|e| {
                    windows::core::Error::new(e.code(), format!("独立视图初始化失败：{e}"))
                })?;
            host.browser
                .SetOptions(EBO_NAVIGATEONCE | EBO_NOTRAVELLOG)?;
            host.browser
                .FillFromObject(None::<&IUnknown>, EBF_NODROPTARGET)
                .map_err(|e| {
                    windows::core::Error::new(e.code(), format!("独立集合初始化失败：{e}"))
                })?;
            let folder: IFolderView2 = host.browser.GetCurrentView().map_err(|e| {
                windows::core::Error::new(e.code(), format!("获取独立视图失败：{e}"))
            })?;
            let results: IResultsFolder = folder.GetFolder().map_err(|e| {
                windows::core::Error::new(e.code(), format!("获取独立集合失败：{e}"))
            })?;
            for item in items {
                results.AddItem(item)?;
            }
            selection::wait_for_targets(&folder, items)?;
            // Select the isolated result set, including mixed folders and namespace
            // items. Validate identities before any menu can invoke a command.
            selection::select_all(&folder, items.len())?;
            let selected: IShellItemArray = folder.Items(SVGIO_SELECTION)?;
            if selected.GetCount()? as usize != items.len() {
                return Err(windows::core::Error::new(E_FAIL, "独立视图选择数不匹配"));
            }
            if !selection::contains_all(&selected, items)? {
                return Err(windows::core::Error::new(E_FAIL, "独立菜单目标校验失败"));
            }
            let view: IShellView = folder.cast()?;
            let first = Rc::new(Cell::new(None));
            let invocation = lifecycle::Invocation::new(cancelled);

            let presenter = if compact_available {
                presenter::NativePresenter::create(
                    &view,
                    HWND(hwnd),
                    HWND(desktop),
                    first.clone(),
                    invocation.check_fn(),
                )
                .and_then(|presenter| {
                    let site: IObjectWithSite = host.browser.cast()?;
                    site.SetSite(&presenter.service())?;
                    Ok(presenter)
                })
                .inspect_err(|_error| {
                    if let Ok(hwnd) = view.GetWindow() {
                        SetPropW(hwnd.0, windows_sys::w!("LucidDesk.Menu.PresenterError"), _error.code().0 as u32 as usize as _);
                    }
                })
                .ok()
            } else {
                None
            };
            // A missing private capability affects only the compact presenter.
            // The validated Shell selection still supplies a public classic menu.
            host.presenter = presenter.clone();
            let view_hwnd = view.GetWindow()?.0;
            SetPropW(view_hwnd, windows_sys::w!("LucidDesk.Menu.Presenter"),
                if presenter.is_some() { 1usize } else { 2usize } as _);
            let callbacks = Rc::new(MenuCallbacks {
                first,
                busy: Cell::new(false),
                view: view.clone(),
                desktop,
                context: Cell::new(context),
                presenter,
                invocation,
                retired: Cell::new(false),
                classic: RefCell::new(None),
            });
            if SetWindowSubclass(
                view_hwnd,
                Some(menu_messages),
                SUBCLASS,
                (&raw const *callbacks) as usize,
            ) == 0
            {
                return Err(windows::core::Error::from_thread());
            }
            host.callbacks = Some(callbacks);
            host.view = Some(view);
            Ok(host)
        }
    }
    pub fn is_busy(&self) -> bool {
        self.callbacks
            .as_ref()
            .is_some_and(|callbacks| callbacks.busy.get())
    }
    /// Reuse only an idle host whose complete live identity set still matches.
    /// A disappeared/renamed target must never invoke a cached, stale selection.
    fn reprepare(
        &mut self,
        targets: &ResolvedTargets,
        context: MenuContext,
        cancelled: Arc<AtomicBool>,
    ) -> Result<bool> {
        if self.is_busy() || self.has_owned_windows() {
            return Ok(false);
        }
        unsafe {
            let callbacks = self.callbacks.as_ref().ok_or(E_FAIL)?;
            if callbacks.retired.get() {
                return Ok(false);
            }
            if callbacks.context.get().owner != context.owner || IsWindow(context.owner as _) == 0 {
                return Ok(false);
            }
            let items = &targets.items;
            let folder: IFolderView2 = callbacks.view.cast()?;
            if self.names != targets.names {
                // Preserve the native presenter and Shell view across target changes.
                // A partially updated collection must never be used for commands.
                callbacks.retired.set(true);
                let results: IResultsFolder = folder.GetFolder()?;
                results.RemoveAll().map_err(|e| windows::core::Error::new(e.code(), format!("清空菜单集合失败：{e}")))?;
                for item in items { results.AddItem(item).map_err(|e| windows::core::Error::new(e.code(), format!("更新菜单集合失败：{e}")))?; }
                selection::wait_for_targets(&folder, items).map_err(|e| windows::core::Error::new(e.code(), format!("等待替换菜单集合失败：{e}")))?;
            }
            let live: IShellItemArray = folder.Items(SVGIO_ALLVIEW)?;
            if live.GetCount()? as usize != items.len() {
                return Ok(false);
            }
            if !selection::contains_all(&live, items)? {
                return Ok(false);
            }
            selection::select_all(&folder, items.len()).map_err(|e| windows::core::Error::new(e.code(), format!("选中新菜单集合失败：{e}")))?;
            let selected: IShellItemArray = folder.Items(SVGIO_SELECTION).map_err(|e| windows::core::Error::new(e.code(), format!("读取新菜单选择失败：{e}")))?;
            if selected.GetCount()? as usize != items.len() || !selection::contains_all(&selected, items)? {
                return Err(windows::core::Error::new(E_FAIL, "切换菜单后选择目标不一致"));
            }
            self.names.clone_from(&targets.names);
            callbacks.retired.set(false);
            callbacks.context.set(context);
            callbacks.invocation.reset(cancelled);
            SetWindowPos(
                self.hwnd,
                null_mut(),
                context.x.saturating_sub(32),
                context.y.saturating_sub(32),
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
            Ok(true)
        }
    }
    pub fn finish(&self, cancel: bool) -> Result<()> {
        if let Some(callbacks) = &self.callbacks {
            if cancel {
                callbacks.retired.set(true);
                if let Some(presenter) = &callbacks.presenter {
                    presenter.close()?;
                }
            }
            callbacks.busy.set(false);
        }
        Ok(())
    }
    pub fn has_owned_windows(&self) -> bool {
        struct Search {
            root: windows_sys::Win32::Foundation::HWND,
            found: bool,
        }
        unsafe extern "system" fn visit(
            hwnd: windows_sys::Win32::Foundation::HWND,
            data: isize,
        ) -> i32 {
            unsafe {
                let search = &mut *(data as *mut Search);
                if hwnd != search.root
                    && IsWindowVisible(hwnd) != 0
                    && GetAncestor(hwnd, GA_ROOTOWNER) == search.root
                {
                    search.found = true;
                    return 0;
                }
                1
            }
        }
        let mut search = Search {
            root: self.hwnd,
            found: false,
        };
        unsafe {
            EnumWindows(Some(visit), (&raw mut search) as isize);
        }
        search.found
    }
    pub fn view_hwnd(&self) -> Result<isize> {
        unsafe { Ok(self.view.as_ref().ok_or(E_FAIL)?.GetWindow()?.0 as isize) }
    }
}
impl Drop for MenuHost {
    fn drop(&mut self) {
        unsafe {
            // Close XAML while its Shell callback and windows are still alive.
            if let Some(presenter) = &self.presenter {
                let _ = presenter.close();
            }
            if let Some(view) = &self.view {
                if let Ok(hwnd) = view.GetWindow() {
                    RemoveWindowSubclass(hwnd.0, Some(menu_messages), SUBCLASS);
                }
            }
            if let Ok(site) = self.browser.cast::<IObjectWithSite>() {
                let _ = site.SetSite(None::<&IUnknown>);
            }
            let _ = self.browser.Destroy();
            DestroyWindow(self.hwnd);
            // DLL-owned classes are not automatically removed when a DLL unloads.
            // Only the last destroyed host can unregister the shared class.
            unregister_host_class();
        }
    }
}
fn unregister_host_class() {
    unsafe {
        let mut instance = null_mut();
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(
            windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | windows_sys::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            (host_messages as *const ()).cast(), &raw mut instance);
        UnregisterClassW(windows_sys::w!("LucidDesk.IsolatedShellHost.v1"), instance);
    }
}
unsafe extern "system" fn menu_messages(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    _: usize,
    data: usize,
) -> isize {
    unsafe {
        let pointer = data as *const MenuCallbacks;
        Rc::increment_strong_count(pointer);
        let callbacks = Rc::from_raw(pointer);
        if callbacks
            .presenter
            .as_ref()
            .is_some_and(|p| p.handle_command_message(msg, wp))
        {
            return 0;
        }
        if msg == WM_CANCELMODE && callbacks.invocation.cancelled() {
            if let Some(presenter) = &callbacks.presenter {
                presenter.dismiss();
            }
            EndMenu();
            return 0;
        }
        if let Some(menu) = callbacks.classic.borrow().clone() {
            if let Some(result) = classic::message(&menu, msg, wp, lp) {
                return result;
            }
        }
        if msg == WM_NOTIFY && lp != 0 {
            let header = &*(lp as *const NMHDR);
            if matches!(header.code, LVN_BEGINLABELEDITA | LVN_BEGINLABELEDITW)
                && GetParent(header.hwndFrom) == hwnd
            {
                if !callbacks.invocation.cancelled() {
                    SetPropW(callbacks.desktop, RENAME, 1usize as _);
                }
                return 1;
            }
        }
        // UI activation must happen on the Shell STA, after the app has installed
        // its popup observer and handed foreground permission to Explorer.
        if msg == RegisterWindowMessageW(windows_sys::w!("LucidDesk.IsolatedMenu.Open.v1")) {
            if callbacks.invocation.cancelled() || callbacks.retired.get() {
                return 0;
            }
            if callbacks.busy.replace(true) {
                return 0;
            }
            #[cfg(any(debug_assertions, feature = "menu-diagnostics"))]
            let started = std::time::Instant::now();
            let result = (|| -> Result<()> {
                let context = callbacks.context.get();
                let root = GetAncestor(hwnd, GA_ROOT);
                let style = GetWindowLongPtrW(root, GWL_EXSTYLE);
                SetWindowLongPtrW(root, GWL_EXSTYLE, style & !(WS_EX_NOACTIVATE as isize));
                ShowWindow(root, SW_SHOWNOACTIVATE);
                if SetForegroundWindow(root) == 0 {
                    return Err(windows::core::Error::new(E_FAIL, "无法激活独立菜单宿主"));
                }
                callbacks.view.UIActivate(SVUIA_ACTIVATE_FOCUS.0 as u32)?;
                cursor::normal_pointer();
                {
                    if let Some(presenter) = &callbacks.presenter {
                        presenter.set_keyboard_invocation(wp != 0, hwnd as isize);
                        #[cfg(any(debug_assertions, feature = "menu-diagnostics"))]
                        for name in [windows_sys::w!("LucidDesk.Menu.PrepareCalled"), windows_sys::w!("LucidDesk.Menu.PrepareResult"), windows_sys::w!("LucidDesk.Menu.ReadyCalled"), windows_sys::w!("LucidDesk.Menu.ReadyResult"), windows_sys::w!("LucidDesk.Menu.ShowCalled")] { RemovePropW(hwnd, name); }
                    }
                    callbacks.first.set(None);
                    let menu: IContextMenu = callbacks.view.GetItemObject(SVGIO_SELECTION)?;
                    cursor::normal_pointer();
                    #[cfg(any(debug_assertions, feature = "menu-diagnostics"))]
                    SetPropW(
                        hwnd,
                        windows_sys::w!("LucidDesk.Menu.GetItemUs"),
                        started.elapsed().as_micros().saturating_add(1) as usize as _,
                    );
                    callbacks.invocation.check()?;
                    let menu = commands::wrap_cancellable(
                        menu,
                        HWND(callbacks.desktop),
                        callbacks.first.clone(),
                        callbacks.invocation.check_fn(),
                    );
                    if callbacks.presenter.is_none() {
                        return classic::show(
                            hwnd,
                            &menu,
                            context,
                            &callbacks.classic,
                            &callbacks.invocation,
                        );
                    }
                    let site: IContextMenuSite = callbacks.view.cast()?;
                    site.DoContextMenuPopup(
                        &menu,
                        CMF_ITEMMENU | CMF_CANRENAME,
                        windows::Win32::Foundation::POINT {
                            x: context.x,
                            y: context.y,
                        },
                    )
                }
            })();
            cursor::normal_pointer();
            #[cfg(any(debug_assertions, feature = "menu-diagnostics"))]
            SetPropW(
                hwnd,
                windows_sys::w!("LucidDesk.Menu.BuildUs"),
                started.elapsed().as_micros().saturating_add(1) as usize as _,
            );
            if let Err(error) = result {
                callbacks.busy.set(false);
                SetPropW(
                    callbacks.desktop,
                    super::MENU_ERROR,
                    error.code().0 as u32 as usize as _,
                );
            }
            return 0;
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

unsafe extern "system" fn host_messages(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
) -> isize {
    unsafe {
        if msg == WM_MOUSEACTIVATE {
            return MA_ACTIVATE as isize;
        }
        if msg == WM_SETCURSOR {
            SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
            return 1;
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    struct ProcessTheme {
        module: windows_sys::Win32::Foundation::HMODULE,
        set: unsafe extern "system" fn(i32) -> i32,
        previous: i32,
    }

    impl ProcessTheme {
        unsafe fn force(mode: i32) -> Self {
            use windows_sys::Win32::System::LibraryLoader::*;
            unsafe {
                let module = LoadLibraryExW(windows_sys::w!("uxtheme.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
                assert!(!module.is_null());
                let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(
                    GetProcAddress(module, 135usize as *const u8).unwrap());
                Self { module, set, previous: set(mode) }
            }
        }
    }

    impl Drop for ProcessTheme {
        fn drop(&mut self) {
            unsafe {
                (self.set)(self.previous);
                windows_sys::Win32::Foundation::FreeLibrary(self.module);
            }
        }
    }
    thread_local! {
        static POPUP_COLOR: Cell<Option<u32>> = const { Cell::new(None) };
    }

    unsafe extern "system" fn inspect_popup(hwnd: windows_sys::Win32::Foundation::HWND, _: isize) -> i32 {
        unsafe {
            if GetClassLongPtrW(hwnd, GCW_ATOM) == 32768 && IsWindowVisible(hwnd) != 0 {
                let mut rect = windows_sys::Win32::Foundation::RECT::default();
                GetClientRect(hwnd, &raw mut rect);
                let dc = windows_sys::Win32::Graphics::Gdi::GetDC(hwnd);
                if !dc.is_null() {
                    let color = windows_sys::Win32::Graphics::Gdi::GetPixel(dc, (rect.right - 20).max(1), (rect.bottom / 2).max(1));
                    windows_sys::Win32::Graphics::Gdi::ReleaseDC(hwnd, dc);
                    if color != u32::MAX { POPUP_COLOR.set(Some(color)); }
                }
            }
        }
        1
    }

    unsafe extern "system" fn inspect_and_cancel(hwnd: windows_sys::Win32::Foundation::HWND, _: u32, timer: usize, _: u32) {
        unsafe {
            KillTimer(hwnd, timer);
            EnumThreadWindows(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Some(inspect_popup), 0);
            EndMenu();
        }
    }

    #[test]
    #[ignore = "opens and automatically cancels a real fallback menu; requires an interactive desktop"]
    fn fallback_popup_matches_current_windows_theme() {
        unsafe {
            // Opt-in only: ordinal 135 has a different ABI before Windows 10 1903.
            let _theme = std::env::var("LUCIDDESK_TEST_PRIOR_THEME").ok().map(|value| {
                let mode: i32 = value.parse().expect("expected ForceDark=2 or ForceLight=3");
                assert!(matches!(mode, 2 | 3));
                ProcessTheme::force(mode)
            });
            windows::Win32::System::Ole::OleInitialize(None).unwrap();
            let owner = CreateWindowExW(0, windows_sys::w!("STATIC"), windows_sys::w!("LucidDesk fallback theme test"),
                WS_OVERLAPPEDWINDOW, 200, 200, 480, 320, null_mut(), null_mut(), null_mut(), std::ptr::null());
            assert!(!owner.is_null());
            let path = std::env::temp_dir().join(format!("luciddesk-theme-test-{}.txt", std::process::id()));
            std::fs::write(&path, b"owned theme fixture").unwrap();
            let observed = {
                let targets = ResolvedTargets::resolve(&[path.to_string_lossy().into_owned()]).unwrap();
                let context = MenuContext { owner: owner as u64, x: 250, y: 250 };
                let host = MenuHost::create_impl(owner, &targets, context, Arc::new(AtomicBool::new(false)), false).unwrap();
                let callbacks = host.callbacks.as_ref().unwrap();
                let hwnd = host.view_hwnd().unwrap() as windows_sys::Win32::Foundation::HWND;
                ShowWindow(owner, SW_SHOW);
                SetForegroundWindow(owner);
                let menu: IContextMenu = callbacks.view.GetItemObject(SVGIO_SELECTION).unwrap();
                POPUP_COLOR.set(None);
                assert_ne!(SetTimer(hwnd, 0x4c505443, 1500, Some(inspect_and_cancel)), 0);
                let result = classic::show(hwnd, &menu, context, &callbacks.classic, &callbacks.invocation);
                KillTimer(hwnd, 0x4c505443);
                result.unwrap();
                POPUP_COLOR.get()
            };
            DestroyWindow(owner);
            std::fs::remove_file(path).unwrap();
            windows::Win32::System::Ole::OleUninitialize();
            let color = observed.expect("the real popup must be visible for color inspection");
            println!("fallback_popup_background_rgb=#{:02X}{:02X}{:02X}", color & 255, (color >> 8) & 255, (color >> 16) & 255);
            if let Ok(expected) = std::env::var("LUCIDDESK_TEST_EXPECT_DARK") {
                let dark = (color & 255) + ((color >> 8) & 255) + ((color >> 16) & 255) < 384;
                assert_eq!(dark, expected == "1", "actual fallback popup color must match Windows mode");
            }
        }
    }
    #[test]
    #[ignore = "creates a native Shell view; run separately outside the restricted test sandbox"]
    fn unavailable_presenter_keeps_validated_shell_selection_and_rename_verb() {
        unsafe {
            windows::Win32::System::Ole::OleInitialize(None).unwrap();
            let owner = CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!(""),
                WS_POPUP,
                0,
                0,
                640,
                480,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            );
            assert!(!owner.is_null());
            let path = std::env::temp_dir().join(format!(
                "luciddesk-classic-capability-{}.txt",
                std::process::id()
            ));
            std::fs::write(&path, b"owned menu fixture").unwrap();
            {
                let targets =
                    ResolvedTargets::resolve(&[path.to_string_lossy().into_owned()]).unwrap();
                let mut host = MenuHost::create_impl(
                    owner,
                    &targets,
                    MenuContext {
                        owner: owner as u64,
                        x: 20,
                        y: 20,
                    },
                    Arc::new(AtomicBool::new(false)),
                    false,
                )
                .unwrap();
                assert!(
                    host.reprepare(
                        &targets,
                        MenuContext {
                            owner: owner as u64,
                            x: 30,
                            y: 30
                        },
                        Arc::new(AtomicBool::new(false)),
                    )
                    .unwrap()
                );
                // A -> B -> A must keep the same host and validate the replacement
                // selection even though all three collections have one item.
                let other_path = path.with_extension("other.txt");
                std::fs::write(&other_path, b"replacement menu fixture").unwrap();
                let original_hwnd = host.view_hwnd().unwrap();
                for target_path in [&other_path, &path] {
                    let next = ResolvedTargets::resolve(&[target_path.to_string_lossy().into_owned()]).unwrap();
                    assert!(host.reprepare(&next, MenuContext { owner: owner as u64, x: 30, y: 30 },
                        Arc::new(AtomicBool::new(false))).unwrap());
                    assert_eq!(host.view_hwnd().unwrap(), original_hwnd);
                    let folder: IFolderView2 = host.view.as_ref().unwrap().cast().unwrap();
                    let selected: IShellItemArray = folder.Items(SVGIO_SELECTION).unwrap();
                    assert_eq!(selected.GetCount().unwrap(), 1);
                    assert!(selection::contains_all(&selected, &next.items).unwrap());
                }
                std::fs::remove_file(other_path).unwrap();
                let callbacks = host.callbacks.as_ref().unwrap();
                assert!(callbacks.presenter.is_none());
                let menu: IContextMenu = callbacks.view.GetItemObject(SVGIO_SELECTION).unwrap();
                let menu = commands::wrap(menu, HWND(owner), callbacks.first.clone());
                // Exercise the real Shell menu's site contract through the
                // rename wrapper, as CDefView does before compact enumeration.
                let menu_site: IObjectWithSite = menu.cast().unwrap();
                menu_site.SetSite(&callbacks.view).unwrap();
                assert_eq!(
                    menu_site.GetSite::<IUnknown>().unwrap(),
                    callbacks.view.cast::<IUnknown>().unwrap()
                );
                let popup = windows::Win32::UI::WindowsAndMessaging::CreatePopupMenu().unwrap();
                let count = menu.QueryContextMenu(popup, 0, 1, 0x7fff, CMF_NORMAL | CMF_CANRENAME);
                count.ok().unwrap();
                assert!(
                    (0..(count.0 as usize & 0xffff)).any(|id| commands::is_rename(&menu, id)),
                    "public fallback must keep native rename without a private Presenter"
                );
                windows::Win32::UI::WindowsAndMessaging::DestroyMenu(popup).unwrap();
                menu_site.SetSite(None::<&IUnknown>).unwrap();
            }
            std::fs::remove_file(path).unwrap();
            DestroyWindow(owner);
            windows::Win32::System::Ole::OleUninitialize();
        }
    }
}
