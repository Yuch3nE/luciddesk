use super::*;

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
fn image_patch_preserves_allocation_and_skips_unchanged_pixels() {
    let mut item = patch_item();
    let original = item.image.clone().unwrap();
    assert!(!apply_image_patch(&mut item, patch_item()));
    assert!(Arc::ptr_eq(&original, item.image.as_ref().unwrap()));
    let mut changed = patch_item();
    changed.image = Some(Arc::new(assets::Pixels { width: 1, height: 1, data: vec![255; 4] }));
    assert!(apply_image_patch(&mut item, changed));
    assert_eq!(item.image.as_ref().unwrap().data, vec![255; 4]);
    let mut kind = item.clone();
    kind.details.kind = "new type".into();
    assert!(apply_image_patch(&mut item, kind));
    assert_eq!(item.details.kind, "new type");
}

#[test]
fn image_patch_rejects_renamed_modified_replaced_and_unknown_items() {
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
            _ => { item.details.modified_time = None; delayed.details.modified_time = None; }
        }
        assert!(!apply_image_patch(&mut item, delayed), "case {case}");
        assert!(Arc::ptr_eq(&original, item.image.as_ref().unwrap()));
        assert_eq!(item.details.kind, "image");
    }
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
    let source = Source::start_with(root.clone(), Default::default(), Arc::clone(&cache), (2, true)).unwrap();
    let first = source.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    eprintln!("directory membership: {} entries in {:?}", first.len(), started.elapsed());
    let target = first.len().min(32);
    let mut loaded = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(45);
    while loaded.len() < target {
        let batch = source.images.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("thumbnail batch");
        for item in batch {
            if let Some(image) = item.image { loaded.insert(item.identity.persistent_key(), image); }
        }
    }
    eprintln!("first {} real images: {:?}", loaded.len(), started.elapsed());
    let logical_bytes: usize = loaded.values().map(|image| image.data.len()).sum();
    let unique: HashMap<_, _> = loaded.values().map(|image| (Arc::as_ptr(image), image.data.len())).collect();
    eprintln!("real image pixel storage: {logical_bytes} unshared bytes -> {} shared bytes ({} unique images)",
        unique.values().sum::<usize>(), unique.len());
    let child = first.iter().find(|item| item.details.folder).and_then(|item| item.identity.file_system_path()).map(Path::to_path_buf);
    drop(source);
    if let Some(child) = child {
        let child = Source::start_with(child, Default::default(), Arc::clone(&cache), (2, true)).unwrap();
        child.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
        drop(child);
    }
    let started = Instant::now();
    let returned = Source::start_with(root, Default::default(), cache, (2, true)).unwrap();
    let first = returned.updates.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    let reused = first.iter().filter(|item| {
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
            // The UI consumes both bounded channels. Leaving thumbnails
            // unread eventually blocks the worker before its final snapshot.
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
        result.as_ref().is_ok_and(|items| {
            items.len() == 2 && items.iter().all(|item| item.image.is_some())
        })
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
    assert!(first.image.is_some());
    assert!(!first.details.kind.is_empty());
    assert_ne!(first.details.kind, "—");
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
    super::super::handle(&state, id, super::super::Event::SortFolderMenu(0)).unwrap();
    assert_eq!(state.borrow().folders[&id].sort, (0, true));
    {
        let mut s = state.borrow_mut();
        s.store.save_preference("panel_folder_sort:2", "2:desc").unwrap();
        apply_saved_preferences(&mut s, id).unwrap();
    }
    super::super::handle(&state, id, super::super::Event::SortFolderMenu(2)).unwrap();
    assert_eq!(state.borrow().folders[&id].sort, (2, false));
}
