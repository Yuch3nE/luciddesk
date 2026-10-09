use super::search::everything_settings;
use super::*;

#[test]
fn menu_sort_saves_once_and_preserves_other_panes() {
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    {
        let mut s = state.borrow_mut();
        for (at,item) in s.workspace.desktop_items_mut().iter_mut().enumerate() {
            item.set_display_name(["File10","File2","Other"][at]);
            item.set_placement(DesktopPlacement::Pane { pane_id: PanelId::new(if at==2 {2} else {1}), position: GridPosition::new(at as u32,0) });
        }
        let workspace = s.workspace.clone();
        s.store.save_workspace(&workspace).unwrap();
    }
    create_view(&state, PanelId::new(1)).unwrap();
    let model = state.borrow().views[0].model.clone();
    let before = state.borrow().store.change_count();
    window::dispatch_sort_menu(&model, None, false, |event| {
        assert!(model.try_borrow_mut().is_ok(), "menu must release the model before dispatch");
        handle(&state, PanelId::new(1), event).unwrap();
    });
    assert_eq!(model.borrow().items[0].label, "File2");
    let count = state.borrow().store.change_count();
    assert!(count > before);
    assert!(matches!(state.borrow().workspace.desktop_items()[1].placement(), DesktopPlacement::Pane { pane_id, position } if pane_id.get()==1 && position.column==0));
    assert!(matches!(state.borrow().workspace.desktop_items()[2].placement(), DesktopPlacement::Pane { pane_id, position } if pane_id.get()==2 && position.column==2));
    handle(&state, PanelId::new(2), Event::SortPane(PanelId::new(1), false)).unwrap();
    assert_eq!(state.borrow().store.change_count(),count);
    window::dispatch_sort_menu(&model, Some(PanelId::new(1)), true, |event| {
        assert!(model.try_borrow_mut().is_ok());
        handle(&state, PanelId::new(1), event).unwrap();
    });
    assert_eq!(model.borrow().items[0].label, "File10");
}

#[test]
fn changing_language_keeps_all_panel_windows_and_saved_panels() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    crate::i18n::with_locale(0, || {
        let state = Rc::new(RefCell::new(test_state()));
        create_view(&state, PanelId::new(1)).unwrap();
        let folder = tempfile::tempdir().unwrap();
        handle(&state, PanelId::new(0), Event::MapFolder(folder.path().into())).unwrap();
        handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
        let windows: Vec<_> = state.borrow().views.iter().map(|v| v.window.hwnd().cast()).collect();
        assert_eq!(windows.len(), 3);
        let saved_ids = || state.borrow().store.load_workspace().unwrap().panels()
            .iter().map(Panel::id).collect::<Vec<_>>();
        let original = saved_ids();
        let runtime = runtime::supervisor(&state).unwrap();
        for language in ["en-US", "zh-TW", "ja-JP", "ko-KR", "de-DE", "ru-RU", "zh-CN"] {
            state.borrow().store.save_preference("language", language).unwrap();
            unsafe { SendMessageW(runtime.hwnd().cast(), wake::READY, 0, 0); }
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(350);
            while std::time::Instant::now() < deadline {
                unsafe {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert_eq!(crate::i18n::language(), language);
            assert_eq!(state.borrow().views.len(), 3);
            assert_eq!(saved_ids(), original);
            for &hwnd in &windows {
                assert_ne!(unsafe { IsWindowVisible(hwnd) }, 0);
                assert_eq!(unsafe { SendMessageW(hwnd, WM_APP + 199, 0, 0) }, 1000);
            }
        }
    });
}

#[test]
#[ignore = "Native composition window; run alone to isolate STA graphics lifetime"]
fn language_switch_preserves_panel_items_selection_and_scroll() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    crate::i18n::with_locale(0, || {
        let state = Rc::new(RefCell::new(test_state()));
        create_view(&state, PanelId::new(1)).unwrap();
        let model = state.borrow().views[0].model.clone();
        {
            let mut m = model.borrow_mut();
            m.items = (0..100).map(|i| Item {
                identity: ShellIdentity::Namespace { parsing_name: format!("test:{i}") },
                label: format!("Item {i}"), image: None, details: Default::default(),
            }).collect();
            m.select_item(2, false, false);
            m.scroll = 40;
        }
        let runtime = runtime::supervisor(&state).unwrap();
        state.borrow().store.save_preference("language", "en-US").unwrap();
        unsafe { SendMessageW(runtime.hwnd().cast(), wake::READY, 0, 0); }
        assert_eq!(model.borrow().items.len(), 100);
        assert_eq!(model.borrow().selected, Some(2));
        assert_eq!(model.borrow().scroll, 40);
    });
}

#[test]
fn all_pane_types_fade_and_close_after_the_transition() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    state.borrow_mut().workspace.set_appearance(luciddesk_core::PanelTheme::Dark,
        luciddesk_core::Backdrop::Translucent { opacity: 1.0 });
    create_view(&state, PanelId::new(1)).unwrap();
    let folder = tempfile::tempdir().unwrap();
    handle(&state, PanelId::new(0), Event::MapFolder(folder.path().into())).unwrap();
    handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
    let windows: Vec<_> = state.borrow().views.iter().map(|v| v.window.hwnd().cast()).collect();
    assert_eq!(windows.len(), 3);
    let pump = |millis| {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(millis);
        while std::time::Instant::now() < until {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    if scrollbar::animations_enabled() {
        for &hwnd in &windows { assert_eq!(unsafe { SendMessageW(hwnd, WM_APP + 199, 0, 0) }, 0); }
    }
    pump(300);
    for hwnd in windows {
        assert_eq!(unsafe { SendMessageW(hwnd, WM_APP + 199, 0, 0) }, 1000);
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
        pump(80);
        if scrollbar::animations_enabled() {
            assert_ne!(unsafe { IsWindow(hwnd) }, 0);
            let opacity = unsafe { SendMessageW(hwnd, WM_APP + 199, 0, 0) };
            assert!((1..1000).contains(&opacity), "closing opacity: {opacity}");
            let editor = unsafe { GetPropW(hwnd, windows_sys::w!("LucidDesk.SearchInput")) };
            if !editor.is_null() {
                let mut alpha = 0;
                let mut key = 0;
                let mut flags = 0;
                assert_ne!(unsafe { GetLayeredWindowAttributes(editor, &mut key, &mut alpha, &mut flags) }, 0);
                assert!(alpha > 0 && alpha < 255);
            }
        }
        pump(300);
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }
    assert!(state.borrow().views.is_empty());
}

#[test]
fn batch_drag_preserves_order_and_moves_each_identity_once() {
    let mut s = test_state();
    transfer_many(&mut s, PanelId::new(1), &[0, 2], PanelId::new(2), 0).unwrap();
    assert_eq!(
        items_for(&s, PanelId::new(1))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["B"]
    );
    assert_eq!(
        items_for(&s, PanelId::new(2))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "C"]
    );
    transfer_many(&mut s, PanelId::new(2), &[0, 1], PanelId::new(1), 0).unwrap();
    transfer_many(&mut s, PanelId::new(1), &[0, 1], PanelId::new(1), 3).unwrap();
    assert_eq!(
        items_for(&s, PanelId::new(1))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["B", "A", "C"]
    );
    assert_eq!(s.workspace.desktop_items().len(), 3);
    assert_eq!(s.store.load_workspace().unwrap(), s.workspace);
}

fn reconcile(workspace: &mut Workspace, inventory: Vec<DesktopItem>) {
    workspace.reconcile_desktop_items(inventory);
    let valid: Vec<_> = workspace.panels().iter().map(Panel::id).collect();
    let mut next = workspace
        .desktop_items()
        .iter()
        .filter_map(|item| match item.placement() {
            DesktopPlacement::Pane { pane_id, position } if *pane_id == PanelId::new(1) => {
                Some(position.column)
            }
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for item in workspace.desktop_items_mut() {
        if !matches!(item.placement(),DesktopPlacement::Pane{pane_id,..} if valid.contains(pane_id))
        {
            item.set_placement(DesktopPlacement::Pane {
                pane_id: PanelId::new(1),
                position: GridPosition::new(next, 0),
            });
            next = next.saturating_add(1);
        }
    }
}

pub(super) fn test_model(title: &str) -> GroupModel {
    GroupModel {
        merge_preview: Vec::new(),
        merge_occluded: false,
        tabs: Vec::new(),
        active_tab: luciddesk_core::PanelId::new(0),
        folder_sort: (0, false),
        folder_columns: None,
        folder_visible_columns: 15,
        folder_navigation: [false; 2],
        list_view: false,
        fixed_grid: false,
            free_layout: false,
        minimum_icon_width: 0.0,
        folder: None,
        folder_status: None,
        options: luciddesk_core::PaneOptions::default(),
        theme: luciddesk_core::PanelTheme::Dark,
        dark: true,
        hovered_item: None,
        scrollbar: Default::default(),
        hovered_tab: None,
        hovered_button: None,
        pressed_button: None,
        focused: false,
        auto_hide: false,
        locked: false,
        reveal: 1.0,
        backdrop: luciddesk_core::Backdrop::Mica,
        native_material: false,
        title: title.into(),
        items: vec![],
        icon_size: 48.0,

        selected: None,
        selection: Default::default(),
        selection_anchor: None,
        renaming: None,
        scroll: 0,
        collapsed: false,
        loading: false,
    }
}

pub(super) fn test_state() -> PaneApp {
    // Match main's DPI context before any snapshot is captured. Other UI tests
    // may initialize process DPI awareness concurrently when creating windows.
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let mut workspace = Workspace::new();
    for id in [1, 2] {
        workspace
            .add_panel(Panel::new(
                PanelId::new(id),
                format!("Group {id}"),
                RectDip::default(),
            ))
            .unwrap();
    }
    let inventory = ["A", "B", "C"]
        .into_iter()
        .map(|name| {
            DesktopItem::new(
                ShellIdentity::Namespace {
                    parsing_name: format!("test:{name}"),
                },
                name,
            )
        })
        .collect();
    reconcile(&mut workspace, inventory);
    let (_, receiver) = mpsc::channel();
    PaneApp {
        wake: Default::default(),
        folders: HashMap::new(),
        tab_models: HashMap::new(),
        settings: None,
        session: None,
        drops: Vec::new(),
        runtime: None,
        workspace,
        store: WorkspaceStore::open_in_memory().unwrap(),
        views: Vec::new(),
        images: HashMap::new(),
        receiver,
    }
}

#[test]
fn mapped_folder_never_takes_desktop_membership() {
    let mut state = test_state();
    normalize_pane_orders(&mut state);
    let panel = PanelId::new(2);
    state
        .workspace
        .panel_mut(panel)
        .unwrap()
        .set_folder(Some(std::path::PathBuf::from(r"C:\Mapped")));
    let desktop_items = state.workspace.desktop_items().to_vec();
    let desktop_view = items_for(&state, PanelId::new(1));
    set_order(&mut state.workspace, panel, &desktop_view);
    normalize_pane_orders(&mut state);
    assert_eq!(state.workspace.desktop_items(), desktop_items);
    assert!(items_for(&state, panel).is_empty());
    remove_panel(&mut state.workspace, panel);
    assert_eq!(state.workspace.desktop_items(), desktop_items);
}

#[test]
fn empty_first_desktop_pane_is_ready_without_icon_results() {
    let _apartment = crate::pane::test_support::apartment();
    let mut app = test_state();
    app.workspace.reconcile_desktop_items([]);
    let state = Rc::new(RefCell::new(app));
    create_view(&state, PanelId::new(1)).unwrap();
    let app = state.borrow();
    let model = app.views[0].model.borrow();
    assert!(model.items.is_empty());
    assert!(!model.loading, "an empty pane has no icon work to wait for");
}

#[test]
fn desktop_list_toggle_preserves_membership_and_restores_view() {
    let _apartment = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    let id = PanelId::new(1);
    create_view(&state, id).unwrap();
    let before = state.borrow().workspace.desktop_items().to_vec();
    assert!(!state.borrow().views[0].model.borrow().is_list());
    handle(&state, id, Event::ToggleListView).unwrap();
    {
        let app = state.borrow();
        assert_eq!(app.workspace.desktop_items(), before);
        let model = app.views[0].model.borrow();
        assert!(model.is_list());
        let grid = model.grid(440.0, 300.0);
        assert_eq!(grid.columns, 1);
        assert_eq!(grid.content_top, layout::HEADER + layout::PADDING);
        assert!(app.store.load_workspace().unwrap().panel(id).unwrap().list_view());
    }
    handle(&state, id, Event::ToggleListView).unwrap();
    assert!(!state.borrow().views[0].model.borrow().is_list());
    assert!(!state.borrow().store.load_workspace().unwrap().panel(id).unwrap().list_view());
}

#[test]
fn custom_icon_grid_previews_reflows_and_keeps_list_geometry() {
    let _apartment = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    let id = PanelId::new(1);
    create_view(&state, id).unwrap();
    let original = state.borrow().workspace.pane_options();
    let members = state.borrow().workspace.desktop_items().to_vec();
    events::preview_grid(&mut state.borrow_mut(), 150.0);
    assert_eq!(state.borrow().store.load_workspace().unwrap().pane_options(), original);
    {
        let app = state.borrow();
        let model = app.views[0].model.borrow();
        let grid = model.grid(440.0, 300.0);
        assert_eq!((grid.cell_width, grid.cell_height), (132.0, 144.0));
        assert_eq!(grid.columns, 3);
        let (x, y) = model.cell(grid, 1);
        assert_eq!(model.hit(grid, x + 20.0, y + 5.0, 1.0), Some(1));
    }
    events::commit_grid(&mut state.borrow_mut(), original.grid_scale).unwrap();
    assert_eq!(state.borrow().store.load_workspace().unwrap().pane_options().grid_scale, 150.0);
    handle(&state, id, Event::ToggleListView).unwrap();
    let before = state.borrow().views[0].model.borrow().grid(440.0, 300.0);
    handle(&state, id, Event::SetIconGrid(180.0)).unwrap();
    let after = state.borrow().views[0].model.borrow().grid(440.0, 300.0);
    assert_eq!((before.cell_width, before.cell_height), (after.cell_width, after.cell_height));
    assert_eq!(state.borrow().workspace.desktop_items(), members);
    handle(&state, id, Event::ResetPaneOptions).unwrap();
    assert_eq!(state.borrow().workspace.pane_options(), original);
}

#[test]
fn search_pane_creation_and_close_preserve_desktop_membership() {
    let _apartment = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    state.borrow_mut().workspace.set_appearance(
        luciddesk_core::PanelTheme::Dark,
        luciddesk_core::Backdrop::Translucent { opacity: 1.0 },
    );
    let original = state.borrow().workspace.desktop_items().to_vec();
    handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
    handle(&state, PanelId::new(0), Event::EnableSearch).unwrap();
    assert_eq!(state.borrow().views.len(), 1);
    assert_eq!(
        state
            .borrow()
            .workspace
            .panels()
            .iter()
            .filter(|p| p.is_search())
            .count(),
        1
    );
    let id = state.borrow().views[0].id;
    let hwnd = state.borrow().views[0].window.hwnd().cast();
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetPropW, GetWindowLongW, SetWindowTextW, WS_CAPTION,
    };
    let mut bounds = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &raw mut bounds);
    }
    let compact_height = bounds.bottom - bounds.top;
    assert!(compact_height < (100.0 * unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0) as i32);
    assert_eq!(
        unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_CAPTION,
        0
    );
    let edit = unsafe { GetPropW(hwnd, windows_sys::w!("LucidDesk.SearchInput")) };
    assert!(!edit.is_null());
    unsafe {
        SetWindowTextW(edit, windows_sys::w!("luciddesk-query-test"));
    }
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, search::INPUT, 0, 0);
    }
    unsafe {
        GetWindowRect(hwnd, &raw mut bounds);
    }
    assert!(bounds.bottom - bounds.top > compact_height);
    unsafe {
        SetWindowTextW(edit, windows_sys::w!(""));
    }
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, search::INPUT, 0, 0);
    }
    unsafe {
        GetWindowRect(hwnd, &raw mut bounds);
    }
    assert_eq!(bounds.bottom - bounds.top, compact_height);
    assert!(state.borrow().workspace.panel(id).unwrap().is_search());
    assert!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .is_search()
    );
    assert!(items_for(&state.borrow(), id).is_empty());
    let source = items_for(&state.borrow(), PanelId::new(1));
    set_order(&mut state.borrow_mut().workspace, id, &source);
    assert_eq!(state.borrow().workspace.desktop_items(), original);
    handle(
        &state,
        id,
        Event::Geometry(RectDip::new(420.0, 320.0, 520.0, 200.0)),
    )
    .unwrap();
    handle(&state, id, Event::ToggleTopmost).unwrap();
    let saved_panel = state.borrow().workspace.panel(id).unwrap().clone();
    handle(&state, id, Event::ToggleSearch).unwrap();
    assert!(state.borrow().views.is_empty());
    assert!(!everything_settings::enabled(&state.borrow().store).unwrap());
    assert_eq!(
        state.borrow().store.load_workspace().unwrap().panel(id),
        Some(&saved_panel)
    );
    assert_eq!(state.borrow().workspace.desktop_items(), original);
    handle(&state, PanelId::new(0), Event::ToggleSearch).unwrap();
    assert_eq!(state.borrow().views.len(), 1);
    assert_eq!(state.borrow().views[0].id, id);
    assert!(everything_settings::enabled(&state.borrow().store).unwrap());
    assert_eq!(state.borrow().workspace.panel(id), Some(&saved_panel));
    handle(&state, id, Event::ClosePane).unwrap();
    assert!(!everything_settings::enabled(&state.borrow().store).unwrap());
    assert_eq!(state.borrow().workspace.panel(id), Some(&saved_panel));
}

#[test]
fn folder_pane_creation_switch_and_close_preserve_real_files() {
    let _apartment = crate::pane::test_support::apartment();
    let root = std::env::temp_dir().join(format!(
        "luciddesk-folder-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("child")).unwrap();
    std::fs::write(root.join("keep.txt"), b"keep").unwrap();
    let mut initial = test_state();
    initial
        .workspace
        .set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
    let state = Rc::new(RefCell::new(initial));
    handle(&state, PanelId::new(0), Event::MapFolder(root.clone())).unwrap();
    let id = PanelId::new(3);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        folder::poll(&mut state.borrow_mut());
        if !state.borrow().folders[&id].loading {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(state.borrow().views[0].model.borrow().items.len(), 2);
    assert!(state.borrow().views[0].model.borrow().is_list());
    for column in 1..=3 {
        handle(&state, id, Event::ToggleFolderColumn(column)).unwrap();
    }
    handle(&state, id, Event::ToggleFolderColumn(0)).unwrap();
    assert_eq!(state.borrow().views[0].model.borrow().folder_visible_columns, 1);
    assert_eq!(folder::visible_columns(&state.borrow().store, id).unwrap(), 1);
    assert_eq!(folder::visible_columns(&state.borrow().store, PanelId::new(1)).unwrap(), 15);
    state.borrow().views[0]
        .model
        .borrow_mut()
        .select_item(0, false, false);
    handle(&state, id, Event::ToggleListView).unwrap();
    assert!(!state.borrow().views[0].model.borrow().is_list());
    assert!(
        !state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .list_view()
    );
    assert_eq!(state.borrow().views[0].model.borrow().selected, Some(0));
    handle(&state, id, Event::ToggleListView).unwrap();
    assert!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .list_view()
    );
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .folder(),
        Some(root.as_path())
    );
    handle(&state, id, Event::SetFolder(root.join("child"))).unwrap();
    assert_eq!(state.borrow().views[0].model.borrow().folder_visible_columns, 1);
    assert_eq!(
        state.borrow().views[0].model.borrow().folder.as_deref(),
        Some(root.join("child").as_path())
    );
    assert!(state.borrow().views[0].model.borrow().items.is_empty());
    handle(&state, id, Event::ClosePane).unwrap();
    assert!(state.borrow().folders.is_empty());
    assert!(state.borrow().views.is_empty());
    assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"keep");
    assert!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .is_none()
    );
    drop(state);
    std::fs::remove_file(root.join("keep.txt")).unwrap();
    std::fs::remove_dir(root.join("child")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn activation_releases_state_and_model_before_shell_reentry() {
    // Pumping real windows leaves native rendering state in this UI thread.
    // Isolate this message-loop regression from the other GPU/window fixtures.
    const CHILD: &str = "LUCIDDESK_ACTIVATION_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pane::tests::activation_releases_state_and_model_before_shell_reentry",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Activation regression timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let _apartment = crate::pane::test_support::apartment();
    let state = Rc::new(RefCell::new(test_state()));
    // Geometry updates persist existing panel rows, as in a running app.
    {
        let mut owner = state.borrow_mut();
        let workspace = owner.workspace.clone();
        owner.store.save_workspace(&workspace).unwrap();
    }
    create_view(&state, PanelId::new(1)).unwrap();
    create_view(&state, PanelId::new(2)).unwrap();
    let model = Rc::clone(&state.borrow().views[0].model);
    let expected = model.borrow().items[0].identity.clone();
    let called = Rc::new(std::cell::Cell::new(false));
    let observed = Rc::clone(&called);
    let reentrant = Rc::clone(&state);
    events::activate_item_with(&state, PanelId::new(1), 0, move |_, identity| {
        assert_eq!(identity, &expected);
        assert!(reentrant.try_borrow_mut().is_ok());
        assert!(model.try_borrow_mut().is_ok());
        // Simulate Shell pumping an event for a different pane.
        handle(
            &reentrant,
            PanelId::new(2),
            Event::Geometry(RectDip::default()),
        )
        .unwrap();
        observed.set(true);
        Ok(())
    })
    .unwrap();
    assert!(!called.get(), "Opening must wait until the caller returns");
    // The action is already queued; no sleeping or WM_TIMER dispatch is needed.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut message = MSG::default();
        let hwnd = state.borrow().views[0].window.hwnd().cast();
        assert_ne!(
            PeekMessageW(
                &mut message,
                hwnd,
                window::RUN_POSTED_ACTION,
                window::RUN_POSTED_ACTION,
                PM_REMOVE
            ),
            0
        );
        DispatchMessageW(&message);
    }
    assert!(called.get());
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(root.path().join("file.txt"), b"keep").unwrap();
    handle(&state, PanelId::new(0), Event::MapFolder(root.path().to_path_buf())).unwrap();
    let id = PanelId::new(3);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while state.borrow().folders[&id].loading {
        assert!(std::time::Instant::now() < deadline);
        folder::poll(&mut state.borrow_mut());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let model = Rc::clone(&state.borrow().views.iter().find(|v| v.id == id).unwrap().model);
    let index = model.borrow().items.iter().position(|item| item.identity.file_system_path() == Some(child.as_path())).unwrap();
    let file = model.borrow().items.iter().position(|item| item.label == "file.txt").unwrap();
    assert_eq!(folder::entry_mode::navigation_target(&state.borrow(), id, file).unwrap(), None);
    assert_eq!(folder::entry_mode::navigation_target(&state.borrow(), id, index).unwrap(), Some(child.clone()));
    folder::EntryMode::Explorer.save(&state.borrow().store).unwrap();
    let opened = Rc::new(std::cell::Cell::new(false));
    let observed = Rc::clone(&opened);
    let reentrant = Rc::clone(&state);
    let expected = child.clone();
    events::activate_item_with(&state, id, index, move |_, identity| {
        assert_eq!(identity.file_system_path(), Some(expected.as_path()));
        assert!(reentrant.try_borrow_mut().is_ok());
        assert!(model.try_borrow_mut().is_ok());
        observed.set(true);
        Ok(())
    }).unwrap();
    assert!(!opened.get());
    assert_eq!(state.borrow().folders[&id].path, root.path());
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut message = MSG::default();
        let hwnd = state.borrow().views.iter().find(|v| v.id == id).unwrap().window.hwnd().cast();
        assert_ne!(PeekMessageW(&mut message, hwnd, window::RUN_POSTED_ACTION, window::RUN_POSTED_ACTION, PM_REMOVE), 0);
        DispatchMessageW(&message);
    }
    assert!(opened.get());
    folder::EntryMode::Inline.save(&state.borrow().store).unwrap();
    events::activate_item_with(&state, id, index, |_, _| panic!("Inline folders must stay in the pane")).unwrap();
    assert_eq!(state.borrow().folders[&id].path, child);
    assert_eq!(state.borrow().workspace.panel(id).unwrap().folder(), Some(root.path()));
    handle(&state, id, Event::FolderBack).unwrap();
    assert_eq!(state.borrow().folders[&id].path, root.path());
    let views = std::mem::take(&mut state.borrow_mut().views);
    for view in &views {
        window::prepare_close(view.window.hwnd().cast());
    }
    drop(views);
}

#[test]
fn snapped_content_bottom_and_scrollbar_use_the_same_row_metrics() {
    let mut model = test_model("Sizing test");
    for label in ["Short", "Warhammer 40,000 ????"] {
        model.items = (0..10)
            .map(|i| Item {
                details: Default::default(),
                identity: ShellIdentity::Namespace {
                    parsing_name: format!("test-{i}"),
                },
                label: if i == 9 { label.into() } else { "Icon".into() },
                image: None,
            })
            .collect();
        let grid = model.grid(376.0, 500.0);
        let rows = model.row_contents(grid);
        let height = layout::pane_content_height(3, grid.cell_height, &rows);
        assert_eq!(
            height - (layout::HEADER + layout::PADDING + 2.0 * grid.cell_height + rows[2]),
            layout::PADDING
        );
        for reduction in [0.0, 5.0, 10.0] {
            let grid = model.grid(376.0, height - reduction);
            assert_eq!(grid.max_scroll(model.items.len()), 0);
            assert_eq!(grid.visible_rows, 3);
        }
        assert_eq!(
            model
                .grid(376.0, height - 14.0)
                .max_scroll(model.items.len()),
            1
        );
    }
}

#[test]
fn unrelated_keys_do_not_select_first_icon_or_emit_pane_focus() {
    // Real focus/default-key dispatch interacts with process-wide windowing
    // state left by other live UI fixtures. Exercise it in a fresh process,
    // while still requiring all of the native message assertions to pass.
    const ISOLATED: &str = "LUCIDDESK_KEYBOARD_TEST_CHILD";
    if std::env::var_os(ISOLATED).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pane::tests::unrelated_keys_do_not_select_first_icon_or_emit_pane_focus",
                "--test-threads=1",
            ])
            .env(ISOLATED, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let timed_out = loop {
            if child.try_wait().unwrap().is_some() {
                break false;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                break true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let output = child.wait_with_output().unwrap();
        assert!(
            !timed_out && output.status.success(),
            "keyboard child: status={}, timed_out={timed_out}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    // Only this disposable test process: native failures must produce a
    // failing exit status instead of leaving a modal crash dialog behind.
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            GetErrorMode, SEM_NOGPFAULTERRORBOX, SetErrorMode,
        };
        SetErrorMode(GetErrorMode() | SEM_NOGPFAULTERRORBOX);
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageW, WM_KEYDOWN, WM_KILLFOCUS, WM_SETFOCUS,
    };
    let _apartment = crate::pane::test_support::apartment();
    let model = Rc::new(RefCell::new(test_model("Keyboard regression")));
    model.borrow_mut().items = (0..6)
        .map(|index| Item {
            details: Default::default(),
            identity: ShellIdentity::Namespace {
                parsing_name: format!("test:{index}"),
            },
            label: format!("Item {index}"),
            image: None,
        })
        .collect();
    let focus_events = Rc::new(std::cell::Cell::new(0));
    let observed = Rc::clone(&focus_events);
    let keyboard_events = Rc::new(RefCell::new(Vec::new()));
    let observed_keyboard = Rc::clone(&keyboard_events);
    let pane = window::create(
        RectDip::new(40.0, 40.0, 200.0, 160.0),
        Rc::clone(&model),
        move |event| {
            if matches!(
                event,
                Event::Activate(_)
                    | Event::ActivateSelection
                    | Event::RenameItem(_)
                    | Event::Refresh
                    | Event::FileCommand(_)
                    | Event::SetFolderColumns(_)
                    | Event::SortFolder(_)
            ) {
                observed_keyboard.borrow_mut().push(event.clone());
            }
            if matches!(event, Event::PaneItemFocus) {
                observed.set(observed.get() + 1);
            }
            false
        },
    )
    .unwrap();
    let hwnd = pane.hwnd().cast();
    if scrollbar::animations_enabled() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WM_APP, WM_TIMER};
        // Delayed startup must not consume the fade before the UI loop runs.
        let opacity = || unsafe { SendMessageW(hwnd, WM_APP + 199, 0, 0) };
        assert_eq!(opacity(), 0);
        std::thread::sleep(std::time::Duration::from_millis(200));
        unsafe { SendMessageW(hwnd, WM_TIMER, 0x4c5056, 0); }
        assert!(opacity() < 100);
        std::thread::sleep(std::time::Duration::from_millis(70));
        unsafe { SendMessageW(hwnd, WM_TIMER, 0x4c5056, 0); }
        assert!((100..1000).contains(&opacity()));
        std::thread::sleep(std::time::Duration::from_millis(200));
        unsafe { SendMessageW(hwnd, WM_TIMER, 0x4c5056, 0); }
        assert_eq!(opacity(), 1000);
    }
    for selected in [None, Some(3)] {
        model.borrow_mut().selected = selected;
        model.borrow_mut().selection = selected.into_iter().collect();
        model.borrow_mut().scroll = 1;
        unsafe {
            SendMessageW(hwnd, WM_KILLFOCUS, 0, 0);
            SendMessageW(hwnd, WM_SETFOCUS, 0, 0);
        }
        let before = focus_events.get();
        // Letters, digits, modifiers, space, Tab, Backspace and unhandled function keys.
        for key in [0x41, 0x5a, 0x30, 0x10, 0x11, 0x12, 0x20, 0x09, 0x08, 0x70] {
            unsafe {
                SendMessageW(hwnd, WM_KEYDOWN, key, 0);
            }
            assert_eq!(model.borrow().selected, selected, "key={key:x}");
            assert_eq!(model.borrow().scroll, 1, "key={key:x}");
            assert_eq!(focus_events.get(), before, "key={key:x}");
        }
    }
    model.borrow_mut().select_item(0, false, false);
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x27, 0);
    } // Right still navigates.
    assert_eq!(model.borrow().selected, Some(1));
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x23, 0); // End
    }
    assert_eq!(model.borrow().selected, Some(5));
    assert!(model.borrow().scroll > 0);
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x24, 0); // Home
    }
    assert_eq!(model.borrow().selected, Some(0));
    assert_eq!(model.borrow().scroll, 0);
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x0d, 0);
        SendMessageW(hwnd, WM_KEYDOWN, 0x0d, 1 << 30); // held Enter is ignored
        SendMessageW(hwnd, WM_KEYDOWN, 0x71, 0);
        SendMessageW(hwnd, WM_KEYDOWN, 0x74, 0);
        SendMessageW(hwnd, WM_KEYDOWN, 0x2e, 0);
        SendMessageW(hwnd, WM_KEYDOWN, 0x2e, 1 << 30); // held Delete is ignored
    }
    assert!(matches!(
        keyboard_events.borrow().as_slice(),
        [
            Event::ActivateSelection,
            Event::RenameItem(_),
            Event::Refresh,
            Event::FileCommand(luciddesk_shell::FileCommand::Delete)
        ]
    ));
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x1b, 0);
    } // Escape still clears.
    assert_eq!(model.borrow().selected, None);
    unsafe {
        SendMessageW(hwnd, WM_KEYDOWN, 0x41, 0);
    }
    assert_eq!(model.borrow().selected, None);
    // Set modifiers only in this disposable UI test thread and restore them
    // before returning. No synthetic global keyboard input is sent.
    let chord = |key: usize, ctrl: bool, shift: bool| unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, SetKeyboardState};
        let mut saved = [0u8; 256];
        assert_ne!(GetKeyboardState(saved.as_mut_ptr()), 0);
        let mut pressed = saved;
        for key in [0x10, 0x11, 0xa0, 0xa1, 0xa2, 0xa3] {
            pressed[key] = 0;
        }
        pressed[0x11] = if ctrl { 0x80 } else { 0 };
        pressed[0x10] = if shift { 0x80 } else { 0 };
        assert_ne!(SetKeyboardState(pressed.as_ptr()), 0);
        SendMessageW(hwnd, WM_KEYDOWN, key, 0);
        assert_ne!(SetKeyboardState(saved.as_ptr()), 0);
    };
    chord(0x41, true, false);
    assert_eq!(
        model.borrow().selection.iter().copied().collect::<Vec<_>>(),
        (0..6).collect::<Vec<_>>()
    );
    chord(0x24, false, false); // Home resets selection.
    chord(0x27, false, true); // Shift+Right extends range.
    assert_eq!(
        model.borrow().selection.iter().copied().collect::<Vec<_>>(),
        vec![0, 1]
    );
    chord(0x27, true, false); // Ctrl+Right moves focus, not selection.
    assert_eq!(model.borrow().selected, Some(2));
    assert_eq!(
        model.borrow().selection.iter().copied().collect::<Vec<_>>(),
        vec![0, 1]
    );
    chord(0x20, true, false); // Ctrl+Space toggles focused item.
    assert_eq!(
        model.borrow().selection.iter().copied().collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    chord(0x23, true, true); // Ctrl+Shift+End adds the range.
    assert_eq!(model.borrow().selection.len(), 6);
    chord(0x1b, false, false);
    assert!(model.borrow().selection.is_empty());
    // Client hover must appear immediately, then clear on either a
    // non-client border move or a leave notification without re-arming.
    unsafe {
        use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClientRect, WM_MOUSEMOVE, WM_NCMOUSEMOVE,
        };
        let mut rect = windows_sys::Win32::Foundation::RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let x = ((layout::header_button_x(rect.right as f32 / dpi, 1) + 14.0) * dpi) as i32;
        let y = (19.0 * dpi) as i32;
        let position = ((y as isize) << 16) | x as isize;
        for leave in [WM_NCMOUSEMOVE, WM_MOUSELEAVE] {
            SendMessageW(hwnd, WM_MOUSEMOVE, 0, position);
            assert_eq!(model.borrow().hovered_button, Some(1));
            SendMessageW(hwnd, leave, 0, 0);
            assert_eq!(model.borrow().hovered_button, None);
            assert_eq!(model.borrow().hovered_item, None);
        }
    }
    // Empty panes keep the proposed size even inside the grid magnet.
    model.borrow_mut().items.clear();
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            for edge in [
                WMSZ_LEFT,
                WMSZ_RIGHT,
                WMSZ_TOP,
                WMSZ_BOTTOM,
                WMSZ_TOPLEFT,
                WMSZ_TOPRIGHT,
                WMSZ_BOTTOMLEFT,
                WMSZ_BOTTOMRIGHT,
            ] {
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: (293.0 * dpi) as i32,
                    bottom: (259.0 * dpi) as i32,
                };
                let before = rect;
                SendMessageW(hwnd, WM_SIZING, edge as usize, (&raw mut rect) as isize);
                assert_eq!(
                    (rect.left, rect.top, rect.right, rect.bottom),
                    (before.left, before.top, before.right, before.bottom)
                );
            }
        }
    }
    // Simulate a leave notification consumed by the nested menu loop.
    // A hidden pane cannot be under the pointer: resync must clear both
    // stale header and item highlights without another mouse movement.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow, WM_APP};
        ShowWindow(hwnd, SW_HIDE);
        model.borrow_mut().hovered_button = Some(1);
        model.borrow_mut().hovered_item = Some(0);
        SendMessageW(hwnd, WM_APP + 11, 0, 0);
    }
    assert_eq!(model.borrow().hovered_button, None);
    assert_eq!(model.borrow().hovered_item, None);
    // Exercise the real mouse-message path: resizing must not sort, must save
    // once on release, and must restore the last saved widths on cancellation.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        model.borrow_mut().folder = Some(std::path::PathBuf::from("C:\\"));
        model.borrow_mut().list_view = true;
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, (640.0 * dpi) as i32,
            (300.0 * dpi) as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let width = model.borrow().grid(rect.right as f32 / dpi, rect.bottom as f32 / dpi).cell_width;
        let position = |x: f32| (((52.0 * dpi).round() as isize) << 16)
            | (((x + layout::PADDING) * dpi).round() as isize);
        keyboard_events.borrow_mut().clear();
        let before = model.borrow().list_columns(width);
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(before[1]));
        SendMessageW(hwnd, WM_MOUSEMOVE, 1, position(before[1] + 30.0));
        assert!(model.borrow().list_columns(width)[1] > before[1] + 28.0);
        assert!(keyboard_events.borrow().is_empty());
        SendMessageW(hwnd, WM_LBUTTONUP, 0, position(before[1] + 30.0));
        assert!(matches!(keyboard_events.borrow().as_slice(), [Event::SetFolderColumns(_)]));
        let saved = model.borrow().folder_columns;
        let state = test_state();
        folder::save_columns(&state, PanelId::new(1), saved.unwrap()).unwrap();
        let restored = folder::saved_columns(&state.store, PanelId::new(1)).unwrap().unwrap();
        assert!(restored.iter().zip(saved.unwrap()).all(|(a, b)| (a - b).abs() < 0.000001));
        assert_eq!(folder::saved_columns(&state.store, PanelId::new(2)).unwrap(), None);
        keyboard_events.borrow_mut().clear();
        for (message, key) in [(WM_KEYDOWN, 0x1b), (WM_CAPTURECHANGED, 0)] {
            let boundary = model.borrow().list_columns(width)[2];
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(boundary));
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, position(boundary - 20.0));
            assert_ne!(model.borrow().folder_columns, saved);
            SendMessageW(hwnd, message, key, 0);
            assert_eq!(model.borrow().folder_columns, saved);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, position(boundary - 20.0));
        }
        assert!(keyboard_events.borrow().is_empty());
        let bounds = model.borrow().list_columns(width);
        for column in 0..4 {
            let point = position((bounds[column] + bounds[column + 1]) / 2.0);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, point);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, point);
        }
        assert!(matches!(keyboard_events.borrow().as_slice(),
            [Event::SortFolder(0), Event::SortFolder(1), Event::SortFolder(2), Event::SortFolder(3)]));
        keyboard_events.borrow_mut().clear();
        model.borrow_mut().folder_visible_columns = 9; // Name and size only.
        let before = model.borrow().list_columns(width);
        let weights = model.borrow().folder_columns.unwrap();
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(before[3]));
        SendMessageW(hwnd, WM_MOUSEMOVE, 1, position(before[3] + 15.0));
        SendMessageW(hwnd, WM_LBUTTONUP, 0, position(before[3] + 15.0));
        assert!((model.borrow().list_columns(width)[3] - before[3] - 15.0).abs() < 1.0);
        let resized = model.borrow().folder_columns.unwrap();
        assert!((resized[1] - weights[1]).abs() < 0.0001);
        assert!((resized[2] - weights[2]).abs() < 0.0001);
        let after = model.borrow().list_columns(width);
        SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position((after[3] + after[4]) / 2.0));
        SendMessageW(hwnd, WM_LBUTTONUP, 0, position((after[3] + after[4]) / 2.0));
        assert!(matches!(keyboard_events.borrow().as_slice(), [Event::SetFolderColumns(_), Event::SortFolder(3)]));
    }
    // Real messages exercise capture, paging, hover, and frame hit testing for
    // mapped lists, desktop lists, and icon grids without sending global input.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let width = rect.right as f32 / dpi;
        let height = rect.bottom as f32 / dpi;
        let position = |x: f32, y: f32| (((y * dpi).round() as i32 as u16 as isize) << 16)
            | ((x * dpi).round() as i32 as u16 as isize);
        for (list, folder) in [(true, true), (true, false), (false, false)] {
            {
                let mut m = model.borrow_mut();
                m.list_view = list;
                m.folder = folder.then(|| std::path::PathBuf::from("C:\\"));
                m.scroll = 0;
                m.items = (0..100).map(|i| Item {
                    identity: ShellIdentity::Namespace { parsing_name: format!("scroll:{i}") },
                    label: format!("Item {i}"), image: None, details: Default::default(),
                }).collect();
                m.select_item(0, false, false);
            }
            keyboard_events.borrow_mut().clear();
            let bar = super::scrollbar::Bar::for_model(&model.borrow(), width, height).unwrap();
            let x = bar.left + super::scrollbar::Bar::WIDTH / 2.0;
            let y = bar.thumb_top + 5.0;
            let mut point = windows_sys::Win32::Foundation::POINT {
                x: (x * dpi).round() as i32, y: (y * dpi).round() as i32,
            };
            windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &raw mut point);
            let screen = ((point.y as u16 as isize) << 16) | point.x as u16 as isize;
            assert_eq!(SendMessageW(hwnd, WM_NCHITTEST, 0, screen), HTCLIENT as isize);
            point.x += ((width - 1.0 - x) * dpi).round() as i32;
            let screen = ((point.y as u16 as isize) << 16) | point.x as u16 as isize;
            assert_eq!(SendMessageW(hwnd, WM_NCHITTEST, 0, screen), HTRIGHT as isize);
            SendMessageW(hwnd, WM_MOUSEMOVE, 0, position(x, y));
            assert!(model.borrow().scrollbar.hovered);
            assert_eq!(model.borrow().hovered_item, None);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(x, y));
            assert!(model.borrow().scrollbar.dragging);
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, position(x, bar.top + bar.height + 100.0));
            assert_eq!(model.borrow().scroll, bar.max);
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, position(x, bar.top - 50.0));
            assert_eq!(model.borrow().scroll, 0);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, position(x, y));
            assert!(!model.borrow().scrollbar.dragging);
            assert_ne!(windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture(), hwnd);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(x, bar.top + bar.height - 1.0));
            SendMessageW(hwnd, WM_LBUTTONUP, 0, position(x, bar.top + bar.height - 1.0));
            assert_eq!(model.borrow().scroll, bar.page.min(bar.max));
            assert_eq!(model.borrow().selected, Some(0));
            assert!(keyboard_events.borrow().is_empty());
            model.borrow_mut().scroll = 0;
            for (message, key) in [(WM_KEYDOWN, 0x1b), (WM_CAPTURECHANGED, 0)] {
                SendMessageW(hwnd, WM_LBUTTONDOWN, 0, position(x, y));
                assert!(model.borrow().scrollbar.dragging);
                SendMessageW(hwnd, message, key, 0);
                assert!(!model.borrow().scrollbar.dragging);
                SendMessageW(hwnd, WM_LBUTTONUP, 0, position(x, y));
            }
            SendMessageW(hwnd, windows_sys::Win32::UI::Controls::WM_MOUSELEAVE, 0, 0);
            assert!(!model.borrow().scrollbar.hovered);
            model.borrow_mut().collapsed = true;
            assert!(super::scrollbar::Bar::for_model(&model.borrow(), width, height).is_none());
            model.borrow_mut().collapsed = false;
            model.borrow_mut().items.clear();
            assert!(super::scrollbar::Bar::for_model(&model.borrow(), width, height).is_none());
        }
    }
    window::prepare_close(hwnd);
    drop(pane);
}

#[test]
#[ignore = "Opens a real popup; run alone in an interactive desktop session"]
fn fold_finishes_while_a_context_menu_is_open() {
    let _apartment = crate::pane::test_support::apartment();
    let model = Rc::new(RefCell::new(test_model("Fold menu test")));
    let pane = window::create(RectDip::new(40.0, 40.0, 200.0, 160.0), model.clone(), |_| false).unwrap();
    let hwnd = pane.hwnd().cast();
    // Keep a real popup open past the fold duration and inspect the pane
    // before dismissing it. The old in-callback modal loop loses its ticks.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        const RESULT: windows_sys::core::PCWSTR = windows_sys::w!("LucidDesk.FoldMenuTest");
        unsafe extern "system" fn check_fold(
            hwnd: windows_sys::Win32::Foundation::HWND,
            _: u32,
            id: usize,
            _: u32,
        ) {
            unsafe {
                KillTimer(hwnd, id);
                let mut r = RECT::default();
                GetClientRect(hwnd, &raw mut r);
                let dpi =
                    windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
                let mut popup = std::ptr::null_mut();
                loop {
                    popup = FindWindowExW(
                        std::ptr::null_mut(),
                        popup,
                        std::ptr::null(),
                        windows_sys::w!("\u{5206}\u{7ec4}\u{83dc}\u{5355}"),
                    );
                    if popup.is_null() || GetWindow(popup, GW_OWNER) == hwnd {
                        break;
                    }
                }
                let passed =
                    GetWindow(popup, GW_OWNER) == hwnd && r.bottom == (300.0 * dpi).round() as i32;
                SetPropW(hwnd, RESULT, (if passed { 1usize } else { 2usize }) as _);
                if GetWindow(popup, GW_OWNER) == hwnd {
                    PostMessageW(popup, WM_CLOSE, 0, 0);
                }
            }
        }
        model.borrow_mut().collapsed = false;
        SendMessageW(hwnd, window::ANIMATE_FOLD, 0, 300);
        SetTimer(hwnd, 98, 500, Some(check_fold));
        SendMessageW(hwnd, WM_CONTEXTMENU, 0, -1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while GetPropW(hwnd, RESULT).is_null() && std::time::Instant::now() < deadline {
            let mut msg = MSG::default();
            while PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            RemovePropW(hwnd, RESULT) as usize,
            1,
            "fold must finish while the popup is still open"
        );
        assert_eq!(model.borrow().reveal, 1.0);
        KillTimer(hwnd, 98);
    }
    window::prepare_close(hwnd);
    drop(pane);
}

#[test]
fn pane_layer_switch_and_wallpaper_material_initialize() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongW, WS_EX_TOPMOST};
    let _apartment = crate::pane::test_support::apartment();
    let model = Rc::new(RefCell::new(test_model("Layer test")));
    let pane = window::create(
        RectDip::new(40.0, 40.0, 200.0, 160.0),
        Rc::clone(&model),
        |_| false,
    )
    .unwrap();
    assert!(
        model.borrow().native_material,
        "System wallpaper brush was unavailable"
    );
    let hwnd = pane.hwnd().cast();
    let mut frame_enabled = 1i32;
    unsafe {
        windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            windows::Win32::Foundation::HWND(hwnd),
            windows::Win32::Graphics::Dwm::DWMWA_NCRENDERING_ENABLED,
            (&raw mut frame_enabled).cast(),
            4,
        )
        .unwrap();
    }
    assert_eq!(
        frame_enabled, 0,
        "Pane must not use the DWM activation frame"
    );
    for material in [
        luciddesk_core::Backdrop::Mica,
        luciddesk_core::Backdrop::MicaAlt,
        luciddesk_core::Backdrop::Acrylic,
    ] {
        for dark in [false, true] {
            {
                let mut m = model.borrow_mut();
                m.backdrop = material;
                m.dark = dark;
            }
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, 0x0008, 0, 0); // WM_KILLFOCUS
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, 0x000f, 0, 0); // WM_PAINT
            }
            assert!(
                model.borrow().native_material,
                "Composition material failed after focus loss"
            );
        }
    }
    window::set_layer(hwnd, false);
    window::set_layer(hwnd, true);
    assert_ne!(
        unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST,
        0
    );
    window::set_layer(hwnd, false);
    assert_eq!(
        unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST,
        0
    );
    window::prepare_close(hwnd);
    drop(pane);
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsWindow, MSG, PM_REMOVE, PeekMessageW, WM_QUIT,
        };
        assert_eq!(IsWindow(hwnd), 0);
        let mut message = MSG::default();
        assert_eq!(
            PeekMessageW(
                &raw mut message,
                std::ptr::null_mut(),
                WM_QUIT,
                WM_QUIT,
                PM_REMOVE
            ),
            0,
            "closing one pane must not quit the app"
        );
    }
}

#[test]
fn desktop_sort_preserves_pane_order_after_a_gap_and_legacy_grid() {
    let mut state = test_state();
    let id = PanelId::new(1);
    // Two rows from the former native-pane layout share a column.
    for (item, position) in state.workspace.desktop_items_mut().iter_mut().zip([
        GridPosition::new(0, 0),
        GridPosition::new(1, 0),
        GridPosition::new(0, 1),
    ]) {
        item.set_placement(DesktopPlacement::Pane {
            pane_id: id,
            position,
        });
    }
    let names = |s: &PaneApp| {
        items_for(s, id)
            .into_iter()
            .map(|i| i.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&state), ["A", "B", "C"]);
    normalize_pane_orders(&mut state);
    let mut inventory = state.workspace.desktop_items().to_vec();
    inventory.reverse();
    state.workspace.reconcile_desktop_items(inventory);
    assert_eq!(names(&state), ["A", "B", "C"]);
    let a = items_for(&state, id)[0].identity.clone();
    state
        .workspace
        .desktop_item_mut(&a)
        .unwrap()
        .set_placement(DesktopPlacement::default());
    normalize_pane_orders(&mut state);
    let at = items_for(&state, id).len();
    state
        .workspace
        .desktop_item_mut(&a)
        .unwrap()
        .set_placement(DesktopPlacement::Pane {
            pane_id: id,
            position: GridPosition::new(at as u32, 0),
        });
    let mut inventory = state.workspace.desktop_items().to_vec();
    inventory.reverse();
    state.workspace.reconcile_desktop_items(inventory);
    assert_eq!(names(&state), ["B", "C", "A"]);
    state.store.save_workspace(&state.workspace).unwrap();
    state.workspace = state.store.load_workspace().unwrap();
    assert_eq!(names(&state), ["B", "C", "A"]);
}

#[test]
fn moving_between_groups_keeps_identity_unique_and_persists_order() {
    let mut state = test_state();
    let before: Vec<_> = state
        .workspace
        .desktop_items()
        .iter()
        .map(|i| i.identity().clone())
        .collect();
    transfer(&mut state, PanelId::new(1), 1, PanelId::new(2), 0).unwrap();
    assert_eq!(
        items_for(&state, PanelId::new(1))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "C"]
    );
    assert_eq!(items_for(&state, PanelId::new(2))[0].label, "B");
    assert_eq!(
        state
            .workspace
            .desktop_items()
            .iter()
            .map(|i| i.identity().clone())
            .collect::<Vec<_>>(),
        before
    );
    state.workspace = state.store.load_workspace().unwrap();
    assert_eq!(items_for(&state, PanelId::new(2))[0].label, "B");
    transfer(&mut state, PanelId::new(2), 0, PanelId::new(1), 1).unwrap();
    assert_eq!(
        items_for(&state, PanelId::new(1))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert!(items_for(&state, PanelId::new(2)).is_empty());
}

#[test]
fn closing_groups_releases_items_and_persists_an_empty_workspace() {
    let mut state = test_state();
    transfer(&mut state, PanelId::new(1), 0, PanelId::new(2), 0).unwrap();
    let identities: Vec<_> = state
        .workspace
        .desktop_items()
        .iter()
        .map(|item| item.identity().clone())
        .collect();
    let state = Rc::new(RefCell::new(state));
    handle(&state, PanelId::new(1), Event::ClosePane).unwrap();
    {
        let s = state.borrow();
        assert!(s.workspace.panel(PanelId::new(1)).is_none());
        assert_eq!(items_for(&s, PanelId::new(2))[0].label, "A");
        assert_eq!(
            s.workspace
                .desktop_items()
                .iter()
                .filter(|item| matches!(item.placement(), DesktopPlacement::FreeDesktop { .. }))
                .count(),
            2
        );
    }
    handle(&state, PanelId::new(2), Event::ClosePane).unwrap();
    let s = state.borrow();
    let loaded = s.store.load_workspace().unwrap();
    assert!(loaded.panels().is_empty());
    assert_eq!(
        loaded
            .desktop_items()
            .iter()
            .map(|item| item.identity().clone())
            .collect::<Vec<_>>(),
        identities
    );
    assert!(
        loaded
            .desktop_items()
            .iter()
            .all(|item| matches!(item.placement(), DesktopPlacement::FreeDesktop { .. }))
    );
}

#[test]
fn pane_options_apply_globally_and_can_disable_snapping() {
    let state = Rc::new(RefCell::new(test_state()));
    let id = PanelId::new(1);
    for event in [
        Event::SetCornerRadius(0.0),
        Event::ToggleBorder,
        Event::ToggleSnap,
    ] {
        handle(&state, id, event).unwrap();
    }
    let options = luciddesk_core::PaneOptions {
        corner_radius: 0.0,
        border: false,
        snap: false,
        text: luciddesk_core::PanelText::Auto,
        text_protection: false,
        ..luciddesk_core::PaneOptions::DEFAULT
    };
    assert_eq!(state.borrow().workspace.pane_options(), options);
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .pane_options(),
        options
    );
    let mut rect = RECT {
        left: 7,
        top: 11,
        right: 307,
        bottom: 211,
    };
    handle(&state, id, Event::Moving(&raw mut rect)).unwrap();
    assert_eq!(
        (rect.left, rect.top, rect.right, rect.bottom),
        (7, 11, 307, 211)
    );
    handle(&state, PanelId::new(2), Event::ToggleSnap).unwrap();
    assert!(state.borrow().workspace.pane_options().snap);
    handle(&state, id, Event::SetCornerRadius(255.0)).unwrap();
    assert_eq!(state.borrow().workspace.pane_options().corner_radius, 24.0);
    handle(&state, id, Event::SetCornerRadius(11.0)).unwrap();
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .pane_options()
            .corner_radius,
        11.0
    );
    handle(
        &state,
        id,
        Event::SetPanelText(luciddesk_core::PanelText::Light),
    )
    .unwrap();
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .pane_options()
            .text,
        luciddesk_core::PanelText::Light
    );
    handle(&state, id, Event::ToggleTextProtection).unwrap();
    assert!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .pane_options()
            .text_protection
    );
    handle(&state, id, Event::ResetPaneOptions).unwrap();
    assert_eq!(
        state.borrow().workspace.pane_options(),
        luciddesk_core::PaneOptions::default()
    );
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .pane_options(),
        luciddesk_core::PaneOptions::default()
    );
}

#[test]
fn locked_panel_rejects_title_changes_until_unlocked() {
    let state = Rc::new(RefCell::new(test_state()));
    let id = PanelId::new(1);
    let original = state
        .borrow()
        .workspace
        .panel(id)
        .unwrap()
        .title()
        .to_string();
    handle(&state, id, Event::ToggleLocked).unwrap();
    handle(&state, id, Event::RenameTitle).unwrap();
    handle(&state, id, Event::SetTitle("Blocked".into())).unwrap();
    assert_eq!(
        state.borrow().workspace.panel(id).unwrap().title(),
        original
    );
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .title(),
        original
    );
    handle(&state, id, Event::ToggleLocked).unwrap();
    handle(&state, id, Event::SetTitle("Renamed".into())).unwrap();
    assert_eq!(
        state
            .borrow()
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .title(),
        "Renamed"
    );
}

#[test]
fn appearance_is_global_while_behavior_remains_per_group() {
    let state = Rc::new(RefCell::new(test_state()));
    let id = PanelId::new(1);
    let other = state
        .borrow()
        .workspace
        .panel(PanelId::new(2))
        .unwrap()
        .clone();
    let before = state.borrow().workspace.panel(id).unwrap().clone();
    for event in [
        Event::Theme(luciddesk_core::PanelTheme::Dark),
        Event::Material(luciddesk_core::Backdrop::Acrylic),
        Event::ToggleAutoHide,
        Event::ToggleTopmost,
        Event::ToggleLocked,
    ] {
        handle(&state, id, event).unwrap();
    }
    let s = state.borrow();
    let stored = s.store.load_workspace().unwrap();
    let panel = stored.panel(id).unwrap();
    assert_eq!(panel.theme(), luciddesk_core::PanelTheme::Dark);
    assert_eq!(panel.backdrop(), luciddesk_core::Backdrop::Acrylic);
    assert_eq!(panel.auto_hide(), !before.auto_hide());
    assert_eq!(panel.always_on_top(), !before.always_on_top());
    assert_eq!(panel.locked(), !before.locked());
    let other_stored = stored.panel(PanelId::new(2)).unwrap();
    assert_eq!(other_stored.theme(), luciddesk_core::PanelTheme::Dark);
    assert_eq!(other_stored.backdrop(), luciddesk_core::Backdrop::Acrylic);
    assert_eq!(other_stored.auto_hide(), other.auto_hide());
    assert_eq!(other_stored.always_on_top(), other.always_on_top());
    assert_eq!(other_stored.locked(), other.locked());
    assert_eq!(
        stored.appearance(),
        Some((
            luciddesk_core::PanelTheme::Dark,
            luciddesk_core::Backdrop::Acrylic
        ))
    );
}

#[test]
fn settings_window_applies_clicks_and_closes_without_exiting() {
    // Composition owns native dispatch state beyond Rust test-thread lifetimes.
    // Run the complete window scenario in one fresh process/STA, like the app.
    const CHILD: &str = "LUCIDDESK_SETTINGS_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pane::tests::settings_window_applies_clicks_and_closes_without_exiting",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "native settings test failed: {}\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }

    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _apartment = crate::pane::test_support::apartment();
    composition::animation_tests::settings_content_survives_material_changes_and_resize();
    let state = Rc::new(RefCell::new(test_state()));
    settings::show(&state, PanelId::new(1)).unwrap();
    let hwnd = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    // WS_VISIBLE is set during precomposition; the window is usable only after
    // DWM cloaking is removed. Pump dispatch rather than blocking the compositor.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let mut cloaked = 0u32;
        unsafe {
            windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                windows::Win32::Foundation::HWND(hwnd),
                windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                4,
            )
            .unwrap();
        }
        if unsafe { IsWindowVisible(hwnd) } != 0 && cloaked == 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "settings remained cloaked"
        );
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    unsafe {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        GetWindowRect(hwnd, &raw mut outer);
        GetClientRect(hwnd, &raw mut client);
        let dpi = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut monitor = windows_sys::Win32::Graphics::Gdi::MONITORINFO {
            cbSize: size_of::<windows_sys::Win32::Graphics::Gdi::MONITORINFO>() as u32,
            ..Default::default()
        };
        windows_sys::Win32::Graphics::Gdi::GetMonitorInfoW(
            windows_sys::Win32::Graphics::Gdi::MonitorFromWindow(
                hwnd,
                windows_sys::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
            ),
            &raw mut monitor,
        );
        assert_eq!(
            client.right,
            ((900.0 * dpi).round() as i32).min(monitor.rcWork.right - monitor.rcWork.left),
            "initial width must already use DPI before resizing"
        );
        assert_eq!(
            client.bottom,
            ((settings::DEFAULT_HEIGHT as f32 * dpi).round() as i32)
                .min(monitor.rcWork.bottom - monitor.rcWork.top),
            "initial height must fit the monitor"
        );
        assert_eq!(
            outer.right - outer.left,
            client.right,
            "no system side frame"
        );
        assert_eq!(
            outer.bottom - outer.top,
            client.bottom,
            "no system caption band"
        );
        SendMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0);
        assert_ne!(IsZoomed(hwnd), 0);
        SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE as usize, 0);
        assert_eq!(IsZoomed(hwnd), 0);
        {
            // Explorer/Hook can synchronously send these messages while a
            // desktop update holds the mutable workspace borrow.
            let _updating = state.borrow_mut();
            SendMessageW(hwnd, WM_NCHITTEST, 0, 0);
            SendMessageW(hwnd, WM_ACTIVATE, WA_INACTIVE as usize, 0);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
        }
        settings::tests::preference_pages_survive_reentry(&state, hwnd);
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &raw mut bounds);
        let x = bounds.right - (80.0 * scale) as i32;
        let y = (228.0 * scale) as i32;
        let point = ((y as isize) << 16) | (x as isize & 0xffff);
        SendMessageW(hwnd, WM_LBUTTONDOWN, 1, point);
        SendMessageW(hwnd, WM_LBUTTONUP, 0, point);
        assert_eq!(
            state
                .borrow()
                .store
                .load_workspace()
                .unwrap()
                .panel(PanelId::new(1))
                .unwrap()
                .theme(),
            luciddesk_core::PanelTheme::Dark
        );
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
        assert_eq!(IsWindow(hwnd), 0);
        assert!(state.borrow().settings.is_none());
        for _ in 0..3 {
            settings::show(&state, PanelId::new(2)).unwrap();
            let reopened = state.borrow().settings.as_ref().unwrap().hwnd().cast();
            assert_ne!(IsWindowVisible(reopened), 0);
            SendMessageW(reopened, WM_KEYDOWN, 0x1b, 0);
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, reopened, WM_CLOSE, WM_CLOSE, PM_REMOVE) != 0 {
                DispatchMessageW(&raw const message);
            }
            assert_eq!(IsWindow(reopened), 0);
            assert!(state.borrow().settings.is_none());
            assert_eq!(
                PeekMessageW(
                    &raw mut message,
                    std::ptr::null_mut(),
                    WM_QUIT,
                    WM_QUIT,
                    PM_REMOVE
                ),
                0,
                "settings destruction must not quit the app"
            );
        }
    }
    // Keep composition-window scenarios on the same STA and dispatcher lifetime.
    settings::tests::solid_settings_edit_preview_save_and_remember_style();
}

#[test]
fn reconciliation_preserves_groups_and_appends_new_items_after_existing_order() {
    let mut state = test_state();
    transfer(&mut state, PanelId::new(1), 0, PanelId::new(2), 0).unwrap();
    let mut fresh = state.workspace.desktop_items().to_vec();
    fresh.push(DesktopItem::new(
        ShellIdentity::Namespace {
            parsing_name: "test:D".into(),
        },
        "D",
    ));
    reconcile(&mut state.workspace, fresh);
    assert_eq!(items_for(&state, PanelId::new(2))[0].label, "A");
    assert_eq!(
        items_for(&state, PanelId::new(1))
            .iter()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>(),
        ["B", "C", "D"]
    );
    let before = state.workspace.clone();
    assert!(transfer(&mut state, PanelId::new(1), 0, PanelId::new(999), 0).is_err());
    assert_eq!(state.workspace, before);
}

#[test]
fn multiselection_preserves_anchor_toggle_and_file_identity_on_refresh() {
    let mut model = test_model("Sizing test");
    model.items = (0..8)
        .map(|i| Item {
            details: Default::default(),
            identity: ShellIdentity::Namespace {
                parsing_name: format!("selection:{i}"),
            },
            label: i.to_string(),
            image: None,
        })
        .collect();
    model.select_item(2, false, false);
    model.select_item(5, false, true);
    assert_eq!(
        model.selection.iter().copied().collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    model.select_item(3, false, true);
    assert_eq!(
        model.selection.iter().copied().collect::<Vec<_>>(),
        vec![2, 3]
    );
    model.select_item(7, true, false);
    model.select_item(2, true, false);
    assert_eq!(
        model.selection.iter().copied().collect::<Vec<_>>(),
        vec![3, 7]
    );
    let identities = model.selected_identities();
    let mut items = model.items.clone();
    items.reverse();
    items.retain(|item| item.label != "2");
    model.replace_items(items);
    assert_eq!(
        model
            .selected_identities()
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        identities
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    );
    assert_eq!(model.selected, None);
    assert_eq!(model.selection_anchor, None);
    model.clear_selection();
    assert!(model.selected_identities().is_empty());
    model.replace_items(vec![]);
    model.select_all();
    assert!(model.selection.is_empty());

    // Large multi-selections must survive reordered snapshots and removed files.
    model.items = (0..10_000).map(|i| Item {
        details: Default::default(), identity: ShellIdentity::Namespace { parsing_name: format!("large:{i}") },
        label: i.to_string(), image: None,
    }).collect();
    model.select_all();
    model.selected = Some(9_999);
    model.selection_anchor = Some(5_000);
    let mut replacement = model.items.clone();
    replacement.reverse();
    replacement.retain(|item| item.label != "5000");
    let started = std::time::Instant::now();
    model.replace_items(replacement);
    eprintln!("10,000 selected items remapped in {:?}", started.elapsed());
    assert_eq!(model.selection.len(), 9_999);
    assert_eq!(model.selected, Some(0));
    assert_eq!(model.selection_anchor, None);
    assert_eq!(model.items.last().unwrap().label, "0");
}

#[test]
fn auto_hide_is_transient_across_saves_tabs_and_manual_fold() {
    let _sta = crate::pane::test_support::apartment();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.db");
    let mut initial = test_state();
    initial.workspace.set_tab_groups(vec![luciddesk_core::PaneTabs {
        members: vec![PanelId::new(1), PanelId::new(2)], active: PanelId::new(1),
    }]).unwrap();
    initial.store = WorkspaceStore::open(&path).unwrap();
    initial.store.save_workspace(&initial.workspace).unwrap();
    initial.workspace = initial.store.load_workspace().unwrap();
    let state = Rc::new(RefCell::new(initial));
    let first = PanelId::new(1);
    let second = PanelId::new(2);
    create_view(&state, first).unwrap();
    handle(&state, first, Event::ToggleAutoHide).unwrap();
    let revision = state.borrow().store.change_count();
    let bytes = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    for _ in 0..10 {
        for collapsed in [true, false] {
            handle(&state, first, Event::AutoHideCollapsed(collapsed)).unwrap();
            assert_eq!(state.borrow().views[0].model.borrow().collapsed, collapsed);
            assert!(!state.borrow().workspace.panel(first).unwrap().collapsed());
        }
    }
    handle(&state, first, Event::AutoHideCollapsed(true)).unwrap();
    save(&mut state.borrow_mut()).unwrap();
    assert_eq!(state.borrow().store.change_count(), revision);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);

    // The same native window retains its hover state while its active tab changes.
    tabs::select(&state, first, second).unwrap();
    assert!(state.borrow().views[0].model.borrow().collapsed);
    assert!(!state.borrow().store.load_workspace().unwrap().panel(second).unwrap().collapsed());
    let height = state.borrow().workspace.panel(second).unwrap().rect().height;
    handle(&state, second, Event::Geometry(RectDip {
        x: 300.0, y: 320.0, width: 480.0, height: layout::HEADER,
    })).unwrap();
    assert_eq!(state.borrow().store.load_workspace().unwrap().panel(second).unwrap().rect().height, height);

    // Disabling hover restores the saved manual state, not the last mouse position.
    handle(&state, second, Event::ToggleAutoHide).unwrap();
    assert!(!state.borrow().views[0].model.borrow().collapsed);
    handle(&state, second, Event::AutoHideCollapsed(true)).unwrap();
    assert!(!state.borrow().views[0].model.borrow().collapsed);
    handle(&state, second, Event::Collapse).unwrap();
    assert!(state.borrow().store.load_workspace().unwrap().panel(second).unwrap().collapsed());
    handle(&state, second, Event::ToggleAutoHide).unwrap();
    let revision = state.borrow().store.change_count();
    handle(&state, second, Event::AutoHideCollapsed(false)).unwrap();
    assert!(!state.borrow().views[0].model.borrow().collapsed);
    assert_eq!(state.borrow().store.change_count(), revision);
    assert!(WorkspaceStore::open(&path).unwrap().load_workspace().unwrap().panel(second).unwrap().collapsed());
    handle(&state, second, Event::ToggleAutoHide).unwrap();
    assert!(state.borrow().views[0].model.borrow().collapsed);
}


#[test]
#[ignore = "35-second isolated UI/log I/O measurement; run alone"]
fn normal_interactions_do_not_write_diagnostic_log() {
    use std::time::{Duration,Instant};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workspace.db");
    crate::init_logging(&path);
    luciddesk_diagnostics::set_level(luciddesk_diagnostics::Level::Error);
    let mut app = test_state();
    app.store = WorkspaceStore::open(&path).unwrap();
    app.store.save_workspace(&app.workspace).unwrap();
    let state = Rc::new(RefCell::new(app));
    let id = PanelId::new(1);
    create_view(&state,id).unwrap();
    handle(&state,id,Event::ToggleAutoHide).unwrap();
    settings::show(&state,id).unwrap();
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error,"measurement","baseline marker before measurement");
    let log = dir.path().join("logs/diagnostic.log");
    let bytes = std::fs::read(&log).unwrap();
    let modified = std::fs::metadata(&log).unwrap().modified().unwrap();
    let start = Instant::now();
    let mut samples = 0;
    let mut transitions = 0;
    let mut next_transition = Instant::now();
    while start.elapsed() < Duration::from_secs(35) {
        if Instant::now() >= next_transition {
            transitions += 1;
            handle(&state,id,Event::AutoHideCollapsed(transitions % 2 == 1)).unwrap();
            let hwnd = state.borrow().views[0].window.hwnd();
            unsafe {
                SendMessageW(hwnd.cast(),WM_MOUSEMOVE,0,100 | (100 << 16));
                SendMessageW(hwnd.cast(),WM_MOUSEWHEEL,(120u32 << 16) as usize,0);
            }
            next_transition = Instant::now()+Duration::from_millis(500);
        }
        unsafe {
            let mut msg = std::mem::zeroed();
            while PeekMessageW(&mut msg,std::ptr::null_mut(),0,0,PM_REMOVE) != 0 {
                TranslateMessage(&msg); DispatchMessageW(&msg);
            }
        }
        assert_eq!(std::fs::read(&log).unwrap(),bytes,"unexpected log write during normal interaction");
        assert_eq!(std::fs::metadata(&log).unwrap().modified().unwrap(),modified);
        samples += 1;
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("measurement: duration_ms={} samples={} auto_hide_transitions={} log_bytes_before={} log_bytes_after={} modification_time_unchanged=true",start.elapsed().as_millis(),samples,transitions,bytes.len(),std::fs::metadata(&log).unwrap().len());
}

#[test]
fn desktop_refresh_skips_unchanged_items_but_detects_label_position_and_image() {
    let _sta = crate::pane::test_support::apartment();
    let mut state = test_state();
    let id = PanelId::new(1);
    let current = items_for(&state, id);
    assert!(!current.is_empty());
    assert!(desktop_items_for(&state, id, Some(&current)).is_none());
    state.workspace.desktop_item_mut(&current[0].identity).unwrap().set_display_name("Changed");
    let renamed = desktop_items_for(&state, id, Some(&current)).unwrap();
    assert!(renamed.iter().any(|item| item.label == "Changed"));
    assert!(desktop_items_for(&state, id, Some(&renamed)).is_none());
    state.workspace.panel_mut(id).unwrap().set_fixed_grid(true);
    state.workspace.panel_mut(id).unwrap().set_free_layout(true);
    state.workspace.desktop_item_mut(&current[0].identity).unwrap()
        .set_pane_position(Some(luciddesk_core::PointDip::new(22.5, 37.25)));
    let positioned = desktop_items_for(&state, id, Some(&renamed)).unwrap();
    assert!(desktop_items_for(&state, id, Some(&positioned)).is_none());
    state.images.insert(current[0].identity.persistent_key(), Arc::new(assets::Pixels { width: 1, height: 1, data: vec![0; 4] }));
    assert!(desktop_items_for(&state, id, Some(&positioned)).is_some());
}
