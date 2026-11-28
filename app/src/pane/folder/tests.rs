use super::*;

#[test]
fn disconnected_folder_worker_stops_loading_and_preserves_last_items() {
    let root = tempfile::tempdir().unwrap();
    let mut source = Source::start(root.path().into(), wake::Wake::default()).unwrap();
    let (sender, receiver) = mpsc::channel();
    source.updates = receiver;
    drop(sender);
    source.items = items_for(&super::super::tests::test_state(), PanelId::new(1));
    let count = source.items.len();
    assert!(source.poll_scan());
    assert!(!source.loading);
    assert!(source.status.is_some());
    assert_eq!(source.items.len(), count);
    let revision = source.content_revision;
    assert!(!source.poll_scan());
    assert_eq!(source.content_revision, revision);
}

#[test]
fn new_item_is_selected_after_scan_and_stale_navigation_is_ignored() {
    let _sta = crate::pane::test_support::apartment();
    let root = tempfile::tempdir().unwrap();
    let id = PanelId::new(1);
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    state.borrow_mut().workspace.panel_mut(id).unwrap().set_folder(Some(root.path().into()));
    ensure(&mut state.borrow_mut(), id).unwrap();
    create_view(&state, id).unwrap();
    let path = root.path().join("New folder");
    std::fs::create_dir(&path).unwrap();
    item_created(&mut state.borrow_mut(), id, path.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    let rename = loop {
        if let Some(rename) = poll(&mut state.borrow_mut()).into_iter().next() { break rename; }
        assert!(Instant::now() < deadline, "created item must arrive without a timer refresh");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(rename.0, id);
    assert_eq!(rename.1.file_system_path(), Some(path.as_path()));
    let model = state.borrow().views[0].model.clone();
    assert_eq!(model.borrow().selected_identities(), [rename.1]);
    item_created(&mut state.borrow_mut(), id, root.path().join("elsewhere").join("stale"));
    assert!(state.borrow().folders[&id].pending_rename.is_none());
    assert!(poll(&mut state.borrow_mut()).is_empty(), "rename must only be requested once");

}

#[test]
fn folder_type_follows_live_language_despite_cached_shell_name() {
    let mut details = ItemDetails { folder: true, kind: "文件夹".into(), ..Default::default() };
    for (locale, expected) in [
        (2, "Folder"), (0, "文件夹"), (1, "資料夾"), (3, "フォルダー"),
        (4, "폴더"), (5, "Ordner"), (6, "Папка"), (2, "Folder"),
    ] {
        crate::i18n::with_locale(locale, || assert_eq!(details.kind_text(), expected));
        assert_eq!(details.kind, "文件夹", "cached metadata must remain unchanged");
    }
    details.folder = false;
    details.kind = "Custom document type".into();
    crate::i18n::with_locale(2, || assert_eq!(details.kind_text(), "Custom document type"));
}

fn patch_item() -> Item {
    Item {
        identity: identity(PathBuf::from(r"C:\test\image.png")),
        label: "image.png".into(),
        image: Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![0; 4] })),
        details: ItemDetails {
            modified_time: Some(std::time::UNIX_EPOCH),
            size: Some(4),
            kind: "image".into(),
            ..Default::default()
        },
    }
}

#[test]
fn viewport_keeps_only_requested_pixels_and_reuses_them_from_the_shared_budget() {
    let root = tempfile::tempdir().unwrap();
    let mut app = super::super::tests::test_state();
    let ids = [PanelId::new(1), PanelId::new(2)];
    for id in ids {
        app.workspace.panel_mut(id).unwrap().set_folder(Some(root.path().into()));
        ensure(&mut app, id).unwrap();
    }
    assert!(Arc::ptr_eq(&app.folders[&ids[0]].cache, &app.folders[&ids[1]].cache));
    // Finish both initial scans before adding synthetic entries for this directory.
    for id in ids {
        app.folders[&id].updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    }

    let source = app.folders.get_mut(&ids[0]).unwrap();
    let mut first = patch_item();
    let mut second = patch_item();
    first.identity = identity(root.path().join("first.png"));
    second.identity = identity(root.path().join("second.png"));
    let first_pixels = Arc::downgrade(first.image.as_ref().unwrap());
    source.cache.lock().unwrap().insert(&first, 128);
    source.cache.lock().unwrap().insert(&second, 128);
    first.image = None;
    second.image = None;
    source.apply_scan(Ok(vec![first, second]));

    let view = |item: &Item| Some((128, HashSet::from([item.identity.clone()])));
    assert!(source.set_viewport(view(&source.items[0])));
    assert!(source.items[0].image.is_some() && source.items[1].image.is_none());
    assert!(source.set_viewport(view(&source.items[1])));
    assert!(source.items[0].image.is_none() && source.items[1].image.is_some());
    assert!(source.set_viewport(view(&source.items[0])));
    assert!(Arc::ptr_eq(source.items[0].image.as_ref().unwrap(), &first_pixels.upgrade().unwrap()));
    assert!(source.set_viewport(None));
    assert!(source.items.iter().all(|item| item.image.is_none()));
    *app.folder_images.lock().unwrap() = Default::default();
    assert!(first_pixels.upgrade().is_none());
}

#[test]
fn stationary_paints_skip_viewport_posts_and_same_count_reorders_post() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    let mut initial = super::super::tests::test_model("Folder viewport posts");
    initial.folder = Some(PathBuf::from(r"C:\test"));
    initial.list_view = true;
    let mut second = patch_item();
    second.identity = identity(PathBuf::from(r"C:\test\second.png"));
    initial.replace_items(vec![patch_item(), second]);
    let model = Rc::new(RefCell::new(initial));
    let posts = Rc::new(Cell::new(0));
    let count = Rc::clone(&posts);
    let window = window::create(RectDip::from_bounds(40.0, 40.0, 400.0, 350.0), Rc::clone(&model), move |event| {
        if matches!(event, Event::FolderViewport) { count.set(count.get() + 1); }
        false
    }).unwrap();
    let hwnd = window.hwnd().cast();
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
    let paint = || unsafe {
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        let mut msg = MSG::default();
        while PeekMessageW(&raw mut msg, hwnd, window::RUN_POSTED_ACTION, window::RUN_POSTED_ACTION, PM_REMOVE) != 0 {
            DispatchMessageW(&raw const msg);
        }
    };
    paint();
    paint();
    let initial_posts = posts.get();
    assert!(initial_posts > 0);
    model.borrow_mut().hovered_item = Some(0);
    for _ in 0..8 { paint(); }
    assert_eq!(posts.get(), initial_posts);

    let mut items = model.borrow().items.clone();
    items.reverse();
    model.borrow_mut().replace_items(items);
    paint();
    assert_eq!(posts.get(), initial_posts + 1);
    model.borrow_mut().scroll = 1;
    paint();
    assert_eq!(posts.get(), initial_posts + 2);
    model.borrow_mut().collapsed = true;
    paint();
    assert_eq!(posts.get(), initial_posts + 3);
}

#[test]
fn coalesced_viewport_post_syncs_again_when_scroll_returns_to_queued_key() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let _sta = crate::pane::test_support::apartment();
    let mut initial = super::super::tests::test_model("Coalesced folder viewport");
    initial.folder = Some(PathBuf::from(r"C:\test"));
    initial.list_view = true;
    initial.replace_items((0..20).map(|row| Item {
        identity: identity(PathBuf::from(format!(r"C:\test\{row}.png"))),
        label: format!("Row {row}"),
        image: None,
        details: ItemDetails::default(),
    }).collect());
    let model = Rc::new(RefCell::new(initial));
    let dispatches = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&dispatches);
    let live_model = Rc::clone(&model);
    let window = window::create(RectDip::from_bounds(40.0, 40.0, 400.0, 350.0), Rc::clone(&model), move |event| {
        if matches!(event, Event::FolderViewport) {
            seen.borrow_mut().push(live_model.borrow().scroll);
        }
        false
    }).unwrap();
    let hwnd = window.hwnd().cast();
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
    let dispatch = || unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&raw mut message, hwnd, window::RUN_POSTED_ACTION, window::RUN_POSTED_ACTION, PM_REMOVE) != 0 {
            DispatchMessageW(&raw const message);
        }
    };
    let paint = |scroll| {
        model.borrow_mut().scroll = scroll;
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0); }
    };
    paint(0);
    dispatch();
    paint(0);
    dispatch();
    dispatches.borrow_mut().clear();

    paint(1);
    paint(2);
    assert!(dispatches.borrow().is_empty());
    dispatch();
    assert_eq!(*dispatches.borrow(), [2], "the coalesced callback must see K2");

    paint(1);
    dispatch();
    assert_eq!(*dispatches.borrow(), [2, 1], "returning to K1 must post a new sync");
}

#[test]
fn viewport_pixel_changes_invalidate_the_render_cache_once() {
    use windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP;
    let _sta = crate::pane::test_support::apartment();
    let root = tempfile::tempdir().unwrap();
    let mut source = Source::start(root.path().into(), Default::default()).unwrap();
    source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    let first = patch_item();
    let mut second = patch_item();
    second.identity = identity(root.path().join("second.png"));
    let items = vec![first, second];
    let demand = Some((128, items.iter().map(|item| item.identity.clone()).collect()));
    source.apply_scan(Ok(items.clone()));
    source.viewport = demand.clone();

    let mut model = super::super::tests::test_model("Viewport image revision");
    model.folder = Some(root.path().into());
    model.replace_items(items.clone());
    let items_revision = model.items_revision;
    let image_revision = model.image_revision;
    let id = model.active_tab;
    let window = windows_window::Window::new("Viewport image revision")
        .size(400, 350).style(WS_POPUP).create().unwrap();
    let view = View { id, target: Rc::new(Cell::new(id)), window, model: Rc::new(RefCell::new(model)) };

    view.model.borrow_mut().merge_occluded = true;
    sync_viewport(&mut source, &view);
    {
        let model = view.model.borrow();
        assert!(model.items.iter().all(|item| item.image.is_none()));
        assert_eq!(model.items_revision, items_revision);
        assert_eq!(model.image_revision, image_revision + 1);
    }

    // The source can still have pixels to release after the view has cleared its own.
    source.items = items;
    source.viewport = demand;
    sync_viewport(&mut source, &view);
    assert!(source.items.iter().all(|item| item.image.is_none()));
    assert_eq!(view.model.borrow().image_revision, image_revision + 1);
}

#[test]
fn header_only_client_does_not_prefetch_first_row() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let root = tempfile::tempdir().unwrap();
    let mut source = Source::start(root.path().into(), Default::default()).unwrap();
    source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    let item = patch_item();
    source.apply_scan(Ok(vec![item.clone()]));
    let mut model = super::super::tests::test_model("Header-only viewport");
    model.folder = Some(root.path().into());
    model.list_view = true;
    model.replace_items(vec![item]);
    let id = model.active_tab;
    let window = windows_window::Window::new("Header-only viewport")
        .size(400, super::super::layout::HEADER as i32)
        .style(WS_POPUP).ex_style(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE).create().unwrap();
    unsafe { ShowWindow(window.hwnd().cast(), SW_SHOWNOACTIVATE); }
    let view = View { id, target: Rc::new(Cell::new(id)), window, model: Rc::new(RefCell::new(model)) };
    sync_viewport(&mut source, &view);
    assert!(source.viewport.is_none());
    assert!(!source.request.images_pending.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn grid_loads_file_types_only_when_requested() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("example.txt"), b"fixture").unwrap();
    let mut app = super::super::tests::test_state();
    let id = PanelId::new(1);
    let panel = app.workspace.panel_mut(id).unwrap();
    panel.set_folder(Some(root.path().into()));
    panel.set_list_view(false);
    ensure(&mut app, id).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while app.folders[&id].loading {
        poll(&mut app);
        assert!(Instant::now() < deadline, "folder snapshot did not arrive");
        std::thread::sleep(Duration::from_millis(5));
    }
    let source = &app.folders[&id];
    assert!(source.items[0].details.kind.is_empty());
    assert!(!source.types_requested.get());
    assert!(matches!(source.images.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout)));

    app.workspace.panel_mut(id).unwrap().set_list_view(true);
    let revision = app.folders[&id].content_revision;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        poll(&mut app);
        let source = &app.folders[&id];
        if !source.items[0].details.kind.is_empty() {
            assert_ne!(source.content_revision, revision);
            assert!(source.items[0].image.is_none());
            break;
        }
        assert!(Instant::now() < deadline, "list view did not request types");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reordered_types_do_not_replace_another_panes_items_or_hover() {
    let _sta = crate::pane::test_support::apartment();
    let root = tempfile::tempdir().unwrap();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    let ids = [PanelId::new(1), PanelId::new(2)];
    let mut senders = Vec::new();
    for id in ids {
        state.borrow_mut().workspace.panel_mut(id).unwrap().set_folder(Some(root.path().into()));
        create_view(&state, id).unwrap();
        let mut state = state.borrow_mut();
        let source = state.folders.get_mut(&id).unwrap();
        source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let (sender, receiver) = mpsc::channel();
        source.images = receiver;
        senders.push(sender);
    }
    let mut state = state.borrow_mut();
    let mut first = patch_item();
    first.details.kind = "A".into();
    let mut second = patch_item();
    second.identity = identity(root.path().join("second.png"));
    second.details.kind = "B".into();
    let a = state.folders.get_mut(&ids[0]).unwrap();
    a.sort = (1, false);
    a.apply_scan(Ok(vec![first.clone(), second.clone()]));
    let b = state.folders.get_mut(&ids[1]).unwrap();
    b.apply_scan(Ok(vec![second.clone()]));
    b.viewport = Some((128, HashSet::from([second.identity.clone()])));
    let generation = b.request.image_generation.load(std::sync::atomic::Ordering::Acquire);
    let revision = b.content_revision;
    for view in &state.views {
        refresh_view(&state, view, false);
        let mut model = view.model.borrow_mut();
        model.hovered_item = Some(0);
        model.select_item(0, false, false);
    }
    let model = state.views[1].model.clone();
    let items_revision = model.borrow().items_revision;

    first.details.kind = "Z".into();
    senders[0].send(Patch::Types(state.folders[&ids[0]].scan_generation, vec![first.clone()])).unwrap();
    second.details.kind = "Updated".into();
    second.image = Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![255; 4] }));
    senders[1].send(Patch::Types(state.folders[&ids[1]].scan_generation, vec![second.clone()])).unwrap();
    senders[1].send(Patch::Images(generation, vec![second])).unwrap();
    poll(&mut state);

    assert_eq!(state.views[0].model.borrow().items[1].identity, first.identity);
    assert_eq!(state.views[0].model.borrow().selected, Some(1));
    let model = model.borrow();
    assert_eq!(model.items_revision, items_revision);
    assert_eq!(model.hovered_item, Some(0));
    assert_eq!(model.selected, Some(0));
    assert_eq!(model.items[0].details.kind, "Updated");
    assert_eq!(model.items[0].image.as_ref().unwrap().data, [255; 4]);
    assert_ne!(state.folders[&ids[1]].content_revision, revision);
}

#[test]
fn offscreen_file_types_complete_type_sort_without_loading_pixels() {
    let root = tempfile::tempdir().unwrap();
    for name in ["z.txt", "b.bmp", "a.txt"] {
        std::fs::write(root.path().join(name), b"fixture").unwrap();
    }
    let mut app = super::super::tests::test_state();
    let id = PanelId::new(1);
    app.workspace.panel_mut(id).unwrap().set_folder(Some(root.path().into()));
    app.store.save_workspace(&app.workspace).unwrap();
    ensure(&mut app, id).unwrap();
    set_sort(&mut app, id, (1, false)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        poll(&mut app);
        let items = &app.folders[&id].items;
        if items.len() == 3 && items.iter().all(|item| !item.details.kind.is_empty()) {
            assert!(items.iter().all(|item| item.image.is_none()));
            assert_eq!(app.folders[&id].sort, (1, false));
            let mut expected = items.clone();
            sort_items(&mut expected, (1, false));
            assert!(items.iter().zip(expected).all(|(actual, expected)| actual.identity == expected.identity));
            break;
        }
        assert!(Instant::now() < deadline, "offscreen types did not arrive");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn image_patch_preserves_allocation_and_skips_unchanged_pixels() {
    let mut item = patch_item();
    let original = item.image.clone().unwrap();
    assert!(!replace_image(&mut item, patch_item().image.unwrap()));
    assert!(Arc::ptr_eq(&original, item.image.as_ref().unwrap()));
    let mut changed = patch_item();
    changed.image = Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![255; 4] }));
    assert!(replace_image(&mut item, changed.image.unwrap()));
    assert_eq!(item.image.as_ref().unwrap().data, vec![255; 4]);
    assert_eq!(item.details.kind, "image");
}

#[test]
fn image_patch_rejects_renamed_modified_replaced_and_changed_metadata() {
    for case in 0..5 {
        let mut item = patch_item();
        let original = item.image.clone().unwrap();
        let mut delayed = patch_item();
        delayed.details.kind = "stale".into();
        match case {
            0 => item.identity = identity(PathBuf::from(r"C:\test\renamed.png")),
            1 => item.details.modified_time = Some(std::time::UNIX_EPOCH + Duration::from_secs(1)),
            2 => item.details.size = Some(8),
            3 => item.details.folder = true,
            _ => delayed.details.modified_time = None,
        }
        assert!(!same_revision(&item, &delayed), "case {case}");
        assert!(Arc::ptr_eq(&original, item.image.as_ref().unwrap()));
        assert_eq!(item.details.kind, "image");
    }
}

#[test]
fn stale_thumbnail_request_cannot_replace_pixels_for_a_new_viewport() {
    use std::sync::atomic::Ordering;

    let mut app = super::super::tests::test_state();
    let id = PanelId::new(1);
    let root = tempfile::tempdir().unwrap();
    app.workspace.panel_mut(id).unwrap().set_folder(Some(root.path().into()));
    ensure(&mut app, id).unwrap();
    let source = app.folders.get_mut(&id).unwrap();
    source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    source.apply_scan(Ok(vec![patch_item()]));
    let old = source.request.image_generation.load(Ordering::Acquire);
    source.viewport = Some((128, HashSet::from([source.items[0].identity.clone()])));
    source.request.replace_images(128, Vec::new());
    let current = source.request.image_generation.load(Ordering::Acquire);

    let (sender, receiver) = mpsc::channel();
    source.images = receiver;
    let mut update = patch_item();
    update.image = Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![255; 4] }));
    sender.send(Patch::Images(old, vec![update.clone()])).unwrap();
    poll(&mut app);
    assert_eq!(app.folders[&id].items[0].image.as_ref().unwrap().data, [0; 4]);
    sender.send(Patch::Images(current, vec![update])).unwrap();
    poll(&mut app);
    assert_eq!(app.folders[&id].items[0].image.as_ref().unwrap().data, [255; 4]);
}

#[test]
fn rescan_discards_type_jobs_before_the_ui_applies_the_snapshot() {
    use std::sync::atomic::Ordering;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("example.txt"), b"fixture").unwrap();
    let mut source = Source::start(root.path().into(), Default::default()).unwrap();
    let wait = |done: &mut dyn FnMut() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "folder scan did not progress");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait(&mut || source.poll_scan());
    let previous = source.scan_generation;
    let cache = Arc::clone(&source.cache);
    let held_cache = cache.lock().unwrap();
    source.refresh();
    // Hold the worker inside the scan while we queue a job for the old snapshot.
    wait(&mut || !source.request.rescan.load(Ordering::Acquire));
    source.request_types();
    assert_eq!(source.request.type_jobs.lock().unwrap().items.len(), 1);
    drop(held_cache);

    wait(&mut || source.request.type_jobs.lock().unwrap().scan_generation != previous);
    assert!(source.request.type_jobs.lock().unwrap().items.is_empty());
    source.query_types();
    assert!(source.types_queried.get());
    assert!(source.request.type_jobs.lock().unwrap().items.is_empty());
    assert!(matches!(source.images.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout)));

    wait(&mut || source.poll_scan());
    source.request_types();
    assert!(!source.types_queried.get());
    let patch = source.images.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(patch, Patch::Types(scan, items) if scan == source.scan_generation && items.len() == 1));
}

#[test]
fn deactivation_discards_type_jobs_and_defers_new_requests() {
    let root = tempfile::tempdir().unwrap();
    let cache: images::SharedCache = Arc::default();
    let _hold_first_scan = cache.lock().unwrap();
    let mut source = Source::start_with(root.path().into(), Default::default(), Arc::clone(&cache), (0, false)).unwrap();
    source.loading = false;
    let mut item = patch_item();
    item.details.kind.clear();
    source.items.push(item);
    source.request_types();
    assert_eq!(source.request.type_jobs.lock().unwrap().items.len(), 1);

    source.set_active(false);
    assert!(source.request.type_jobs.lock().unwrap().items.is_empty());
    source.query_types();
    assert!(source.request.type_jobs.lock().unwrap().items.is_empty());
    assert!(!source.types_requested.get());
    assert!(source.types_queried.get());
}

#[test]
fn inactive_tab_waits_for_activation_before_rescanning() {
    let root = tempfile::tempdir().unwrap();
    let source = Source::start(root.path().to_path_buf(), Default::default()).unwrap();
    assert!(source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().is_empty());
    source.set_active(false);
    std::fs::write(root.path().join("new.txt"), b"new").unwrap();
    assert!(matches!(source.updates.recv_timeout(Duration::from_millis(400)), Err(mpsc::RecvTimeoutError::Timeout)));
    source.set_active(true);
    let items = source.updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "new.txt");
}

#[test]
fn refresh_event_reads_an_unchanged_folder_again() {
    let root = std::env::temp_dir().join(format!("luciddesk-manual-refresh-{}-{}",
        std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let state = Rc::new(RefCell::new(super::super::tests::test_state()));
    let id = PanelId::new(1);
    state.borrow_mut().folders.insert(id, Source::start(root.clone(), Default::default()).unwrap());
    assert!(state.borrow().folders[&id].updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().is_empty());
    for _ in 0..2 {
        // No file writes: only the explicit Refresh event can request a new snapshot.
        assert!(matches!(state.borrow().folders[&id].updates.recv_timeout(Duration::from_millis(100)), Err(mpsc::RecvTimeoutError::Timeout)));
        super::super::events::handle(&state, id, Event::Refresh).unwrap();
        assert!(state.borrow().folders[&id].updates.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().is_empty());
    }
    drop(state);
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn sizes_format_and_sort_numerically_with_empty_values() {
    assert_eq!(size_text(Some(0), false), "0 B");
    assert_eq!(size_text(Some(1023), false), "1023 B");
    assert_eq!(size_text(Some(1536), false), "1.5 KB");
    assert_eq!(size_text(Some(1024 * 1024), false), "1.0 MB");
    assert_eq!(size_text(None, false), "—");
    assert_eq!(size_text(None, true), "");
    let make = |name: &str, size, folder| Item {
        identity: ShellIdentity::Namespace { parsing_name: name.into() },
        label: name.into(), image: None,
        details: ItemDetails { size, folder, ..Default::default() },
    };
    let mut items = vec![make("large", Some(1024 * 1024), false), make("unknown", None, false), make("small", Some(9), false), make("folder", None, true)];
    sort_items(&mut items, (3, false));
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["folder", "small", "large", "unknown"]);
    sort_items(&mut items, (3, true));
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["large", "small", "unknown", "folder"]);
}

#[test]
fn name_type_and_size_match_explorer_view_order() {
    // Captured from IFolderView2 with folder_sort_probe on this Windows
    // installation. In particular, descending type and size do not reverse ties.
    let make = |name: &str, kind: &str, size| Item {
        identity: ShellIdentity::Namespace { parsing_name: name.into() },
        label: name.into(), image: None,
        details: ItemDetails { kind: kind.into(), size, folder: size.is_none(), ..Default::default() },
    };
    let original = vec![
        make("dir10", "文件夹", None), make("dir2", "文件夹", None),
        make("file10.txt", "文本文档", Some(3)), make("file2.txt", "文本文档", Some(3)),
        make("a.zip", "ZIP 压缩文件", Some(3)), make("b.txt", "文本文档", Some(3)),
        make("c.txt", "文本文档", Some(3)), make("file1.bin", "BIN 文件", Some(3)),
        make("large.txt", "文本文档", Some(1024)),
    ];
    for (sort, expected) in [
        ((0, false), ["dir2", "dir10", "a.zip", "b.txt", "c.txt", "file1.bin", "file2.txt", "file10.txt", "large.txt"]),
        ((0, true), ["large.txt", "file10.txt", "file2.txt", "file1.bin", "c.txt", "b.txt", "a.zip", "dir10", "dir2"]),
        ((1, false), ["dir2", "dir10", "file1.bin", "a.zip", "b.txt", "c.txt", "file2.txt", "file10.txt", "large.txt"]),
        ((1, true), ["dir2", "dir10", "b.txt", "c.txt", "file2.txt", "file10.txt", "large.txt", "a.zip", "file1.bin"]),
        ((3, false), ["dir2", "dir10", "a.zip", "b.txt", "c.txt", "file1.bin", "file2.txt", "file10.txt", "large.txt"]),
        ((3, true), ["large.txt", "a.zip", "b.txt", "c.txt", "file1.bin", "file2.txt", "file10.txt", "dir2", "dir10"]),
    ] {
        let mut items = original.clone();
        sort_items(&mut items, sort);
        assert_eq!(items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), expected, "sort={sort:?}");
    }
}

#[test]
#[ignore = "Read-only thumbnail diagnostic; set LUCIDDESK_TEST_FOLDER"]
fn real_folder_images_survive_parent_child_navigation() {
    let root = PathBuf::from(std::env::var_os("LUCIDDESK_TEST_FOLDER").expect("test folder"));
    let cache: images::SharedCache = Arc::default();
    let started = Instant::now();
    let mut source = Source::start_with(root.clone(), Default::default(), Arc::clone(&cache), (2, true)).unwrap();
    let first = source.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    eprintln!("directory membership: {} entries in {:?}", first.len(), started.elapsed());
    let target = first.len().min(32);
    let viewport = (128, first.iter().take(target).map(|item| item.identity.clone()).collect());
    let child = first.iter().find(|item| item.details.folder).and_then(|item| item.identity.file_system_path()).map(Path::to_path_buf);
    source.apply_scan(Ok(first));
    source.set_viewport(Some(viewport.clone()));
    let mut loaded = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(45);
    while loaded.len() < target {
        let Patch::Images(_, batch) = source.images.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("thumbnail batch") else { continue; };
        for item in batch {
            if let Some(image) = item.image { loaded.insert(item.identity.persistent_key(), image); }
        }
    }
    eprintln!("first {} real images: {:?}", loaded.len(), started.elapsed());
    let logical_bytes: usize = loaded.values().map(|image| image.data.len()).sum();
    let unique: HashMap<_, _> = loaded.values().map(|image| (Arc::as_ptr(image), image.data.len())).collect();
    eprintln!("real image pixel storage: {logical_bytes} unshared bytes -> {} shared bytes ({} unique images)",
        unique.values().sum::<usize>(), unique.len());
    drop(source);
    if let Some(child) = child {
        let child = Source::start_with(child, Default::default(), Arc::clone(&cache), (2, true)).unwrap();
        child.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
        drop(child);
    }
    let started = Instant::now();
    let mut returned = Source::start_with(root, Default::default(), cache, (2, true)).unwrap();
    let first = returned.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    returned.apply_scan(Ok(first));
    returned.set_viewport(Some(viewport));
    let reused = returned.items.iter().filter(|item| {
        item.image.as_ref().zip(loaded.get(&item.identity.persistent_key())).is_some_and(|(image, old)| Arc::ptr_eq(image, old))
    }).count();
    eprintln!("back navigation: {reused}/{} prior image allocations reused in first snapshot ({:?})", loaded.len(), started.elapsed());
    assert_eq!(reused, loaded.len());
}

#[test]
#[ignore = "Read-only diagnostic; set LUCIDDESK_TEST_FOLDER to an existing directory"]
fn real_folder_snapshot_matches_directory_and_sorts_newest_first() {
    let root = PathBuf::from(std::env::var_os("LUCIDDESK_TEST_FOLDER").expect("test folder"));
    let mut expected: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    expected.sort();
    let started = Instant::now();
    let source = Source::start(root, Default::default()).unwrap();
    let mut items = source
        .updates
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    let mut actual: Vec<_> = items
        .iter()
        .map(|item| item.identity.file_system_path().unwrap().to_path_buf())
        .collect();
    actual.sort();
    assert_eq!(actual, expected);
    sort_items(&mut items, (2, true));
    assert!(
        items
            .windows(2)
            .all(|pair| pair[0].details.modified_time >= pair[1].details.modified_time)
    );
    eprintln!(
        "{} entries, first sorted snapshot in {:?}",
        items.len(),
        started.elapsed()
    );
    for item in items.iter().take(10) {
        eprintln!("{}  {}", item.details.modified, item.label);
    }
}

#[test]
fn first_snapshot_includes_hidden_children_before_loading_images() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW,
    };
    let root = std::env::temp_dir().join(format!(
        "luciddesk-complete-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    for index in 0..256 {
        std::fs::write(root.join(format!("file-{index}.txt")), b"").unwrap();
    }
    let hidden = root.join("file-0.txt");
    let wide: Vec<_> = hidden.as_os_str().encode_wide().chain(Some(0)).collect();
    assert_ne!(
        unsafe {
            SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
        },
        0
    );
    std::fs::create_dir(root.join("child")).unwrap();
    std::fs::write(root.join("child/nested.txt"), b"").unwrap();
    let source = Source::start(root.clone(), Default::default()).unwrap();
    let request = Arc::downgrade(&source.request);
    let started = Instant::now();
    let items = source
        .updates
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    eprintln!(
        "first folder snapshot: {} entries in {:?}",
        items.len(),
        started.elapsed()
    );
    assert_eq!(items.len(), 257);
    assert!(items.iter().all(|item| item.details.size == if item.details.folder { None } else { Some(0) }));
    assert!(
        items
            .iter()
            .any(|item| item.identity.file_system_path() == Some(hidden.as_path()))
    );
    assert!(items.iter().all(|item| item.image.is_none()));
    assert!(
        items
            .iter()
            .all(|item| item.identity.file_system_path().unwrap().parent()
                == Some(root.as_path()))
    );
    drop(source);
    let deadline = Instant::now() + Duration::from_secs(10);
    while request.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(request.upgrade().is_none());
    // Only the uniquely created test directory is removed.
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dropping_an_idle_source_wakes_worker_and_releases_handles() {
    let source = Source::start(
        std::env::temp_dir().join(format!(
            "luciddesk-missing-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
        Default::default(),
    )
    .unwrap();
    let request = Arc::downgrade(&source.request);
    assert!(
        source
            .updates
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .is_err()
    );
    drop(source);
    let deadline = Instant::now() + Duration::from_secs(1);
    while request.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        request.upgrade().is_none(),
        "shutdown must interrupt the directory retry wait"
    );
}

#[test]
fn sorting_uses_snapshot_metadata_without_accessing_paths() {
    let make = |label: &str, folder, seconds: Option<u64>| Item {
        identity: ShellIdentity::Namespace {
            parsing_name: format!("test:{label}"),
        },
        label: label.into(),
        image: None,
        details: ItemDetails {
            folder,
            modified_time: seconds.map(|s| std::time::UNIX_EPOCH + Duration::from_secs(s)),
            ..Default::default()
        },
    };
    let mut items = vec![
        make("z-folder", true, None),
        make("a-new", false, Some(20)),
        make("z-old", false, Some(10)),
    ];
    sort_items(&mut items, (2, false));
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["z-old", "a-new", "z-folder"]
    );
    sort_items(&mut items, (2, true));
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["a-new", "z-old", "z-folder"]
    );
    sort_items(&mut items, (0, false));
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["z-folder", "a-new", "z-old"]
    );
}

#[test]
fn metadata_sort_keeps_unknowns_last_and_ties_natural() {
    let make = |name: &str, kind: &str, seconds: Option<u64>| Item {
        identity: identity(PathBuf::from(name)), label: name.into(), image: None,
        details: ItemDetails { kind: kind.into(), modified_time: seconds.map(|s| std::time::UNIX_EPOCH + Duration::from_secs(s)), ..Default::default() },
    };
    let original = vec![make("file10", "Text", Some(10)), make("file2", "Text", Some(10)),
        make("unknown", "—", None), make("pending", "", None)];
    for column in [1, 2] {
        for descending in [false, true] {
            let mut items = original.clone();
            sort_items(&mut items, (column, descending));
            assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["file2", "file10", "pending", "unknown"]);
        }
    }
}

#[test]
fn reselecting_mapped_root_leaves_child_and_persists_title() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    let mut state = super::super::tests::test_state();
    let id = PanelId::new(2);
    state.workspace.panel_mut(id).unwrap().set_folder(Some(root.path().to_path_buf()));
    ensure(&mut state, id).unwrap();
    navigate(&mut state, id, Some(child)).unwrap();
    let state = Rc::new(RefCell::new(state));
    super::super::events::handle(&state, id, Event::SetFolder(root.path().to_path_buf())).unwrap();
    let state = state.borrow();
    assert_eq!(state.folders[&id].path, root.path());
    assert_eq!(state.folders[&id].navigation(), [false, false]);
    let saved = state.store.load_workspace().unwrap();
    assert_eq!(saved.panel(id).unwrap().title(), root.path().file_name().unwrap().to_string_lossy());
}

#[test]
fn navigation_and_sort_keep_the_mapping_and_back_history() {
    let root = std::env::temp_dir().join(format!(
        "luciddesk-navigation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("child")).unwrap();
    std::fs::write(root.join("z.txt"), b"z").unwrap();
    std::fs::write(root.join("a.txt"), b"aaa").unwrap();
    let mut state = super::super::tests::test_state();
    let id = PanelId::new(2);
    state
        .workspace
        .panel_mut(id)
        .unwrap()
        .set_folder(Some(root.clone()));
    state.store.save_workspace(&state.workspace).unwrap();
    ensure(&mut state, id).unwrap();
    let items = state.folders[&id]
        .updates
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    state.folders.get_mut(&id).unwrap().items = items;
    sort(&mut state, id, 2).unwrap();
    assert_eq!(state.folders[&id].sort, (2, true));
    sort(&mut state, id, 2).unwrap();
    assert_eq!(state.folders[&id].sort, (2, false));
    sort(&mut state, id, 0).unwrap();
    sort(&mut state, id, 0).unwrap();
    assert_eq!(
        state.folders[&id]
            .items
            .iter()
            .map(|i| i
                .identity
                .file_system_path()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>(),
        ["z.txt", "a.txt", "child"]
    );
    sort(&mut state, id, 0).unwrap();
    assert_eq!(
        state.folders[&id]
            .items
            .iter()
            .map(|i| i
                .identity
                .file_system_path()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned())
            .collect::<Vec<_>>(),
        ["child", "a.txt", "z.txt"]
    );
    for expected in [["child", "z.txt", "a.txt"], ["a.txt", "z.txt", "child"]] {
        sort(&mut state, id, 3).unwrap();
        assert_eq!(state.folders[&id].items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>(), expected);
    }
    navigate(&mut state, id, Some(root.join("child"))).unwrap();
    ensure(&mut state, id).unwrap();
    assert_eq!(state.folders[&id].path, root.join("child"));
    assert_eq!(state.folders[&id].navigation(), [true, true]);
    assert_eq!(
        state
            .store
            .load_workspace()
            .unwrap()
            .panel(id)
            .unwrap()
            .folder(),
        Some(root.as_path())
    );
    navigate(&mut state, id, None).unwrap();
    assert_eq!(state.folders[&id].path, root);
    navigate(&mut state, id, Some(root.join("child"))).unwrap();
    home(&mut state, id).unwrap();
    assert_eq!(state.folders[&id].path, root);
    assert!(state.folders[&id].history.is_empty());
    assert_eq!(state.folders[&id].navigation(), [false, false]);
    navigate(&mut state, id, None).unwrap();
    assert_eq!(state.folders[&id].path, root);
    state.folders.remove(&id);
    ensure(&mut state, id).unwrap();
    assert_eq!(state.folders[&id].sort, (3, true));
    state.folders.clear();
    std::fs::remove_file(root.join("a.txt")).unwrap();
    std::fs::remove_file(root.join("z.txt")).unwrap();
    std::fs::remove_dir(root.join("child")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn folder_watch_tracks_children_and_recovers_after_missing_directory() {
    let root = std::env::temp_dir().join(format!(
        "luciddesk-folder-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut source = Source::start(root.clone(), Default::default()).unwrap();
    let wait = |source: &mut Source, predicate: &dyn Fn(&Result<Vec<Item>, String>) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // A folder may publish offscreen type names between watch events.
            while source.images.try_recv().is_ok() {}
            assert!(Instant::now() < deadline, "folder update timed out");
            let result = match source.updates.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => result,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(error) => panic!("folder worker disconnected: {error}"),
            };
            if predicate(&result) {
                return result;
            }
        }
    };
    assert!(
        wait(&mut source, &|result| result
            .as_ref()
            .is_ok_and(Vec::is_empty))
        .is_ok()
    );
    std::fs::write(root.join("first.txt"), b"original").unwrap();
    std::fs::create_dir(root.join("child")).unwrap();
    std::fs::write(root.join("child/nested.txt"), b"nested").unwrap();
    let items = wait(&mut source, &|result| {
        result.as_ref().is_ok_and(|items| items.len() == 2)
    })
    .unwrap();
    assert!(
        items
            .iter()
            .all(|item| item.identity.file_system_path().unwrap().parent()
                == Some(root.as_path()))
    );
    let first = items
        .iter()
        .find(|item| item.identity.file_system_path() == Some(root.join("first.txt").as_path()))
        .unwrap();
    assert!(first.image.is_none(), "watch snapshots carry metadata even without a viewport");
    assert_eq!(first.details.modified.len(), 16);
    std::fs::rename(root.join("first.txt"), root.join("renamed.txt")).unwrap();
    wait(&mut source, &|result| {
        result.as_ref().is_ok_and(|items| {
            items.iter().any(|item| {
                item.identity.file_system_path() == Some(root.join("renamed.txt").as_path())
            })
        })
    })
    .unwrap();
    std::fs::remove_file(root.join("renamed.txt")).unwrap();
    wait(&mut source, &|result| {
        result.as_ref().is_ok_and(|items| items.len() == 1)
    })
    .unwrap();
    std::fs::remove_file(root.join("child/nested.txt")).unwrap();
    std::fs::remove_dir(root.join("child")).unwrap();
    std::fs::remove_dir(&root).unwrap();
    source.refresh();
    assert!(wait(&mut source, &Result::is_err).is_err());
    std::fs::create_dir(&root).unwrap();
    wait(&mut source, &|result| {
        result.as_ref().is_ok_and(Vec::is_empty)
    })
    .unwrap();
    drop(source);
    std::fs::remove_dir(&root).unwrap();
}

#[test]
fn folder_copy_rejects_self_recursive_and_virtual_sources() {
    let destination = Path::new(r"C:\Data\Folder");
    assert!(accepts_copy(
        &[identity(PathBuf::from(r"C:\Other\file.txt"))],
        destination
    ));
    for path in [r"C:\Data", r"C:\Data\Folder", r"C:\Data\Folder\file.txt"] {
        assert!(!accepts_copy(&[identity(PathBuf::from(path))], destination));
    }
    assert!(!accepts_copy(
        &[ShellIdentity::Namespace {
            parsing_name: "recycle".into()
        }],
        destination
    ));
}

#[test]
fn menu_sort_uses_loaded_and_externally_updated_folder_rule() {
    let _sta = crate::pane::test_support::apartment();
    let root = tempfile::tempdir().unwrap();
    let mut app = super::super::tests::test_state();
    let id = PanelId::new(2);
    app.workspace.panel_mut(id).unwrap().set_folder(Some(root.path().to_path_buf()));
    app.store.save_workspace(&app.workspace).unwrap();
    app.store.save_preference("panel_folder_sort:2", "0:asc").unwrap();
    ensure(&mut app, id).unwrap();
    let state = Rc::new(RefCell::new(app));
    let model = RefCell::new(super::super::create_model(&state.borrow(), id).unwrap());
    super::super::window::dispatch_sort_menu(&model, None, 0, |event| {
        assert!(model.try_borrow_mut().is_ok());
        super::super::handle(&state, id, event).unwrap();
    });
    assert_eq!(state.borrow().folders[&id].sort, (0, true));
    {
        let mut s = state.borrow_mut();
        s.store.save_preference("panel_folder_sort:2", "2:desc").unwrap();
        apply_saved_preferences(&mut s, id).unwrap();
    }
    super::super::window::dispatch_sort_menu(&model, None, 2, |event| {
        super::super::handle(&state, id, event).unwrap();
    });
    assert_eq!(state.borrow().folders[&id].sort, (2, false));
}

#[test]
fn transient_scan_error_keeps_last_snapshot_until_success() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("kept.txt"), b"data").unwrap();
    let mut source = Source::start(root.path().to_path_buf(), Default::default()).unwrap();
    let first = source.updates.recv_timeout(Duration::from_secs(5)).unwrap();
    source.apply_scan(first);
    assert_eq!(source.items.len(), 1);
    source.apply_scan(Err("temporary network error".into()));
    assert_eq!(source.items[0].label, "kept.txt");
    assert!(source.status.is_some());
    source.apply_scan(Ok(Vec::new()));
    assert!(source.items.is_empty());
    assert!(source.status.is_none());
}

#[test]
fn enumeration_preserves_native_type_hidden_and_size_attributes() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dir")).unwrap();
    let file = root.path().join("hidden.txt");
    std::fs::write(&file, b"data").unwrap();
    let wide: Vec<_> = file.as_os_str().encode_wide().chain(Some(0)).collect();
    assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) }, 0);
    let entries = luciddesk_shell::enumerate_folder(root.path()).unwrap();
    let hidden = entries.iter().find(|e| e.display_name == "hidden.txt").unwrap();
    assert!(hidden.attributes.hidden);
    assert!(!hidden.attributes.folder);
    assert_eq!(hidden.attributes.filesystem_directory, Some(false));
    assert_eq!(hidden.size, Some(4));
    assert!(hidden.modified.is_some());
    let directory = entries.iter().find(|e| e.display_name == "dir").unwrap();
    assert!(directory.attributes.folder);
    assert_eq!(directory.attributes.filesystem_directory, Some(true));
    assert_eq!(entries.len(), 2);
    assert!(luciddesk_shell::enumerate_folder(&root.path().join("missing")).is_err());
}

#[test]
fn content_revision_tracks_same_count_scans_and_source_recreation() {
    let root = tempfile::tempdir().unwrap();
    let mut source = Source::start(root.path().into(), Default::default()).unwrap();
    let initial = source.content_revision;
    source.apply_scan(Ok(vec![patch_item()]));
    let first = source.content_revision;
    assert_ne!(initial, first);
    let mut renamed = patch_item();
    renamed.label = "renamed".into();
    source.apply_scan(Ok(vec![renamed]));
    assert_eq!(source.items.len(), 1);
    assert_ne!(source.content_revision, first);
    let revision = source.content_revision;
    drop(source);
    let replacement = Source::start(root.path().into(), Default::default()).unwrap();
    assert_ne!(replacement.content_revision, revision);
}
