use super::*;

fn test_state() -> (tempfile::TempDir, Rc<RefCell<PaneApp>>) {
    let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
    let directory = tempfile::tempdir().unwrap();
    state.borrow_mut().store =
        WorkspaceStore::open(&directory.path().join("workspace.db")).unwrap();
    let config = Classification {
        enabled: false,
        rules: ["pdf", "png"]
            .into_iter()
            .map(|ext| ClassificationRule {
                target_panel: 1,
                enabled: true,
                item_type: None,
                extensions: vec![ext.into()],
                name_contains: None,
                name_regex: None,
            })
            .collect(),
    };
    save(&state.borrow(), &config).unwrap();
    (directory, state)
}

#[test]
fn settings_and_tray_share_one_modeless_organizer_and_select_the_requested_tab() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    super::super::host::show_at(&state, PanelId::new(1), 16).unwrap();
    let settings: HWND = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    assert!(state.borrow().organizer.is_none());
    unsafe {
        let scale = GetDpiForWindow(settings).max(96) as f32 / 96.0;
        let mut bounds = RECT::default();
        GetClientRect(settings, &raw mut bounds);
        let width = bounds.right as f32 / scale;
        let body = scene(width, bounds.bottom as f32 / scale - TITLE_HEIGHT, 16, false,
            (PanelTheme::System, Backdrop::Mica), Default::default());
        let scene = with_titlebar(body, width, false);
        assert!(!scene.controls.iter().any(|control| matches!(control.action, Action::Classification(_))));
        let action = scene.controls.iter().find(|control| matches!(control.action, Action::OpenOrganizer)).unwrap();
        let x = ((action.bounds.left + action.bounds.right) * 0.5 * scale) as u16;
        let y = ((action.bounds.top + action.bounds.bottom) * 0.5 * scale) as u16;
        let point = (u32::from(x) | (u32::from(y) << 16)) as isize;
        SendMessageW(settings, WM_PAINT, 0, 0);
        SendMessageW(settings, WM_LBUTTONDOWN, 1, point);
        SendMessageW(settings, WM_LBUTTONUP, 0, point);
        let organizer: HWND = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
        assert_ne!(organizer, settings);
        assert_ne!(IsWindowEnabled(settings), 0);
        assert!(GetWindow(organizer, GW_OWNER).is_null());
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 0, 0), Tab::Rules as isize + 1);
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 1, 0), 0);

        show_organize_preview(&state).unwrap();
        assert_eq!(state.borrow().organizer.as_ref().unwrap().hwnd().cast::<std::ffi::c_void>(), organizer);
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 0, 0), Tab::Organize as isize + 1);
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 1, 0), 1);

        let scale = GetDpiForWindow(organizer).max(96) as f32 / 96.0;
        GetClientRect(organizer, &raw mut bounds);
        let width = bounds.right as f32 / scale;
        let body = window_scene((width, bounds.bottom as f32 / scale), Backdrop::Mica, Tab::Organize, &[], None, &Editor::default(), false);
        let body = organizer_titlebar(body, width, false);
        let rules = body.controls.iter().find(|control| matches!(control.action, Action::OrganizeTab(Tab::Rules))).unwrap();
        let x = ((rules.bounds.left + rules.bounds.right) * 0.5 * scale) as u16;
        let y = ((rules.bounds.top + rules.bounds.bottom) * 0.5 * scale) as u16;
        let point = (u32::from(x) | (u32::from(y) << 16)) as isize;
        SendMessageW(organizer, WM_LBUTTONDOWN, 1, point);
        SendMessageW(organizer, WM_LBUTTONUP, 0, point);
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 0, 0), Tab::Rules as isize + 1);

        show_rules(&state).unwrap();
        assert_eq!(SendMessageW(organizer, host::INSPECT_TAB, 0, 0), Tab::Rules as isize + 1);
        ValidateRect(settings, std::ptr::null());
        ValidateRect(organizer, std::ptr::null());
        events::preview_material(&mut state.borrow_mut(), Backdrop::Solid { color: 0x364050, opacity: 1.0 });
        assert_ne!(GetUpdateRect(settings, std::ptr::null_mut(), 0), 0);
        assert_ne!(GetUpdateRect(organizer, std::ptr::null_mut(), 0), 0);
        SendMessageW(settings, WM_CLOSE, 0, 0);
        assert!(state.borrow().settings.is_none());
        assert_ne!(IsWindow(organizer), 0);
        SendMessageW(organizer, WM_CLOSE, 0, 0);
        assert!(state.borrow().organizer.is_none());
    }
}

#[test]
fn classification_controls_persist_toggle_delete_and_enable() {
    let (_directory, state) = test_state();
    let mut editor = Editor::default();
    let hwnd = std::ptr::null_mut();
    for command in [
        Command::Toggle(0),
        Command::Enable(true),
    ] {
        editor.execute(&state, hwnd, command);
        assert!(editor.error.is_none());
    }
    let saved = load(&state.borrow()).unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.rules[0].extensions, ["pdf"]);
    assert!(!saved.rules[0].enabled);
    editor.execute(&state, hwnd, Command::Delete(0));
    assert!(editor.error.is_none());
    assert_eq!(load(&state.borrow()).unwrap().rules[0].extensions, ["png"]);
    let before = state.borrow().workspace.clone();
    assert!(hybrid::preview_organize(&state.borrow()).is_err());
    assert_eq!(state.borrow().workspace, before);
}

#[test]
fn rule_drag_saves_insertion_on_release_and_cancels_other_drops() {
    let _sta = crate::pane::test_support::apartment();
    let (directory, state) = test_state();
    let mut config = load(&state.borrow()).unwrap();
    for extension in ["zip", "lnk"] {
        let mut rule = config.rules[0].clone();
        rule.extensions = vec![extension.into()];
        config.rules.push(rule);
    }
    save(&state.borrow(), &config).unwrap();
    host::show(&state, Tab::Rules).unwrap();
    let hwnd: HWND = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
    let mut bounds = RECT::default();
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    assert_ne!(unsafe { GetClientRect(hwnd, &raw mut bounds) }, 0);
    let render = || {
        let (width, height) = (bounds.right as f32 / scale, bounds.bottom as f32 / scale);
        let owner = state.borrow();
        let scene = window_scene((width, height), Backdrop::Mica, Tab::Rules, owner.workspace.panels(),
            owner.store.settings().unwrap().get(KEY), &Editor::default(), false);
        let mut scene = organizer_titlebar(scene, width, false);
        scene.scroll_to(width, height, &mut 0.0);
        scene
    };
    let order = || load(&state.borrow()).unwrap().rules.into_iter()
        .map(|rule| rule.extensions[0].clone()).collect::<Vec<_>>();
    let mouse = |message, x: f32, y: f32| {
        let x = (x * scale).round() as i16 as u16;
        let y = (y * scale).round() as i16 as u16;
        let point = (u32::from(x) | (u32::from(y) << 16)) as isize;
        unsafe { SendMessageW(hwnd, message, if message == WM_LBUTTONUP { 0 } else { 1 }, point) };
    };
    let snapshot = render();
    let row = &snapshot.rule_rows[0];
    let rule_menu = snapshot.controls[row.controls.clone()].iter().find_map(|control| {
        if let Action::Dropdown(choices) = &control.action { Some(choices) } else { None }
    }).unwrap();
    assert!(matches!(rule_menu.as_ref(), [(_, Action::Classification(Command::Delete(0)), false)]));
    let first = snapshot.rule_rows[0].grip();
    let (x, y) = ((first.left + first.right) / 2.0, (first.top + first.bottom) / 2.0);
    let third = snapshot.rule_rows[2].bounds;
    let below_third = third.bottom - 4.0;
    let initial = ["pdf", "png", "zip", "lnk"];
    mouse(WM_LBUTTONDOWN, x, y);
    mouse(WM_LBUTTONUP, x, y);
    assert_eq!(order(), initial);

    mouse(WM_LBUTTONDOWN, x, y);
    mouse(WM_MOUSEMOVE, x, below_third);
    assert_eq!(order(), initial);
    unsafe { SendMessageW(hwnd, WM_KEYDOWN, VK_ESCAPE.into(), 0); }
    mouse(WM_LBUTTONUP, x, below_third);
    assert_eq!(order(), initial);
    assert_ne!(unsafe { IsWindow(hwnd) }, 0);

    mouse(WM_LBUTTONDOWN, x, y);
    mouse(WM_MOUSEMOVE, x, below_third);
    mouse(WM_LBUTTONUP, 4.0, below_third);
    assert_eq!(order(), initial);
    unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0); }
    std::thread::sleep(SETTINGS_DURATION);

    mouse(WM_LBUTTONDOWN, x, y);
    mouse(WM_MOUSEMOVE, x, below_third);
    config.rules[3].enabled = false;
    save(&state.borrow(), &config).unwrap();
    mouse(WM_LBUTTONUP, x, below_third);
    assert_eq!(order(), initial);
    assert!(!load(&state.borrow()).unwrap().rules[3].enabled);

    mouse(WM_LBUTTONDOWN, x, y);
    mouse(WM_MOUSEMOVE, x, below_third);
    assert_eq!(order(), initial);
    mouse(WM_LBUTTONUP, x, below_third);
    assert_eq!(order(), ["png", "zip", "pdf", "lnk"]);
    unsafe {
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        assert_eq!(SendMessageW(hwnd, host::HAS_CONTROL_FOCUS, 0, 0), 0);
    }

    let persisted = WorkspaceStore::open(&directory.path().join("workspace.db")).unwrap();
    let restored = decode(persisted.settings().unwrap().get(KEY)).unwrap();
    assert_eq!(restored.rules[2].extensions, ["pdf"]);
    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0); }
}

#[test]
fn classification_shows_saved_batch_undo_and_reports_the_outcome() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    let state = state.borrow();
    let settings = state.store.settings().unwrap();
    let render = |editor: &Editor, undo_available| {
        window_scene((800.0, 560.0), Backdrop::Mica, Tab::Organize, state.workspace.panels(),
            settings.get(KEY), editor, undo_available)
    };
    let mut editor = Editor::default();
    let has_undo = |body: &Scene| body.controls.iter().any(|control| Command::UndoOrganize.is_control(&control.action));
    let assert_preview_button = |body: &Scene, label| {
        let button = body.controls.iter().find(|control| Command::Run.is_control(&control.action)).unwrap();
        assert!(button.enabled && matches!(button.kind, ControlKind::PrimaryButton));
        assert_eq!(button.label, t(label));
    };
    let initial = render(&editor, true);
    assert!(has_undo(&initial));
    assert_preview_button(&initial, "organizer-generate-preview");

    let assert_summary = |body: &Scene, title: &str, icon: &str, counts: &[(&str, &str)]| {
        let bounds = |label: &str, style: usize| body.text.iter().find_map(|(bounds, text, format)| {
            (text == label && *format == style).then_some(*bounds)
        }).unwrap();
        let title = bounds(title, Tokens::SECTION_TITLE);
        assert!(bounds(icon, 4).right < title.left);
        for &(value, label) in counts {
            let label = bounds(label, 6);
            assert!(title.bottom <= label.top);
            assert!(body.text.iter().any(|(bounds, text, style)| {
                text == value && *style == 2 && bounds.top == label.top && bounds.right == label.left
            }));
        }
    };
    use luciddesk_window::control_style::{CHECK_MARK, INFO, WARNING};
    let (sender, receiver) = std::sync::mpsc::channel();
    editor.applying = Some(PendingOrganize { command: Command::ApplyPreview, result: receiver });
    let moves = [
        (1, "Documents", "Report.pdf"),
        (2, "Images", "Photo.png"),
        (1, "Documents", "Notes.docx"),
    ].into_iter().map(|(id, group, name)| hybrid::OrganizeMove {
        identity: ShellIdentity::FileSystem { path: name.into(), volume_id: None, file_id: None },
        item_name: name.into(),
        target: PanelId::new(id),
        target_name: group.into(),
        rule: id as usize,
    }).collect();
    sender.send(Ok(hybrid::OrganizeOutcome::Applied(moves))).unwrap();
    assert!(editor.poll_organize());
    let applied = render(&editor, true);
    assert!(has_undo(&applied));
    assert_preview_button(&applied, "organizer-generate-preview");
    assert_summary(&applied, t("organizer-applied-heading"), CHECK_MARK, &[("3", t("organizer-organized"))]);
    let [documents, images] = applied.section_headers.as_slice() else { panic!("expected two destination groups") };
    assert_eq!(documents.title, "Documents");
    assert_eq!(images.title, "Images");
    for (group, count) in [(documents, "2"), (images, "1")] {
        assert_eq!(group.summary, crate::i18n::format("organizer-group-count", &[("count", count.into())]));
    }
    let row = |name| applied.text.iter().find(|(_, text, _)| text == name).unwrap().0;
    assert!(row(t("organizer-organized-icons")).bottom < documents.bounds.top);
    for name in ["Report.pdf", "Notes.docx"] {
        assert!(documents.bounds.bottom <= row(name).top && row(name).bottom < images.bounds.top);
    }
    assert!(images.bounds.bottom <= row("Photo.png").top);

    for (restored, skipped, title, icon) in [
        (2, 0, "organizer-undo-heading", CHECK_MARK),
        (2, 1, "organizer-undo-summary", WARNING),
        (0, 2, "organizer-undo-summary", WARNING),
        (0, 0, "organizer-undo-summary", INFO),
    ] {
        let (sender, receiver) = std::sync::mpsc::channel();
        editor.applying = Some(PendingOrganize { command: Command::UndoOrganize, result: receiver });
        sender.send(Ok(hybrid::OrganizeOutcome::Undone { restored, skipped })).unwrap();
        assert!(editor.poll_organize());
        let body = render(&editor, false);
        assert!(!has_undo(&body));
        assert!(body.section_headers.is_empty());
        assert_preview_button(&body, "organizer-return-to-preview");
        assert_summary(&body, t(title), icon, &[
            (&restored.to_string(), t("organizer-restored")),
            (&skipped.to_string(), t("organizer-skipped")),
        ]);
    }
}

#[test]
fn preview_keeps_distinct_targets_with_the_same_name() {
    let _sta = crate::pane::test_support::apartment();
    let title = "Reference documents for the new desktop organization workflow";
    let entries = [(1, "One.pdf"), (2, "Two.pdf"), (1, "Three.pdf")]
        .map(|(target, name)| hybrid::PreviewEntry::Move(hybrid::OrganizeMove {
            identity: ShellIdentity::FileSystem { path: name.into(), volume_id: None, file_id: None },
            item_name: name.into(),
            target: PanelId::new(target),
            target_name: title.into(),
            rule: target as usize,
        }));
    let editor = Editor {
        preview: Some(hybrid::OrganizePreview::fixture(entries.into(), None)),
        ..Editor::default()
    };
    let scene = window_scene((480.0, 380.0), Backdrop::Mica, Tab::Organize, &[], None, &editor, false);
    assert_eq!(scene.section_headers.len(), 2);
    assert!(scene.section_headers.iter().all(|header| header.title == title));
    let second_group = scene.section_headers[1].bounds;
    let row = |name| scene.text.iter().find(|(_, value, _)| value == name).unwrap().0;
    assert!(row("Three.pdf").bottom < second_group.top);
    assert!(row("Two.pdf").top >= second_group.bottom);
}

#[test]
fn reconfirm_keeps_rule_changes_and_expandable_skip_reasons() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    let identity = |path: &str| ShellIdentity::FileSystem {
        path: path.into(), volume_id: None, file_id: None,
    };
    let item = hybrid::OrganizeMove {
        identity: identity("Report.pdf"),
        item_name: "Report.pdf".into(),
        target: PanelId::new(1),
        target_name: "Documents".into(),
        rule: 2,
    };
    let moved = hybrid::PreviewEntry::Move(item.clone());
    let skipped = hybrid::PreviewEntry::Skip {
        identity: identity("Draft.docx"),
        item_name: "Draft.docx".into(),
        reason: hybrid::SkipReason::TemporaryFile,
    };
    let preview = hybrid::OrganizePreview::fixture(vec![moved.clone(), skipped], Some(vec![
        hybrid::PreviewChange::Changed {
            before: hybrid::PreviewEntry::Move(hybrid::OrganizeMove { rule: 1, ..item }),
            after: moved,
        },
    ]));
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut editor = Editor {
        applying: Some(PendingOrganize { command: Command::ApplyPreview, result: receiver }),
        ..Editor::default()
    };
    sender.send(Ok(hybrid::OrganizeOutcome::Reconfirm(preview))).unwrap();
    assert!(editor.poll_organize());
    let render = |editor: &Editor| {
        window_scene((480.0, 380.0), Backdrop::Mica, Tab::Organize, &[], None, editor, false)
    };
    let closed = render(&editor);
    assert!(closed.destination_changes.is_empty());
    assert!(closed.controls.iter().any(|control| {
        Command::ApplyPreview.is_control(&control.action) && control.label == t("classification-preview-confirm")
    }));
    assert!(!closed.text.iter().any(|(_, text, _)| text == t("classification-skip-temporary")));
    editor.execute(&state, std::ptr::null_mut(), Command::ToggleSkipped);
    let expanded = render(&editor);
    assert!(expanded.text.iter().any(|(_, text, _)| text == t("classification-skip-temporary")));
    assert!(expanded.controls.iter().any(|control| {
        Command::ToggleSkipped.is_control(&control.action) && control.selected
    }));
}

#[test]
fn classification_stages_align_and_render() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    host::show(&state, Tab::Rules).unwrap();
    let hwnd = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
    let mut draft = Editor::default();
    draft.execute(&state, hwnd, Command::AddTemplate(Template::Documents));
    assert!(draft.editing());

    let identity = |path: &str| ShellIdentity::FileSystem {
        path: path.into(), volume_id: None, file_id: None,
    };
    let move_to = |name: &str, target, target_name: &str, rule| {
        hybrid::PreviewEntry::Move(hybrid::OrganizeMove {
            identity: identity(name),
            item_name: name.into(),
            target: PanelId::new(target),
            target_name: target_name.into(),
            rule,
        })
    };
    let document = move_to("季度报告.pdf", 1, "文档", 1);
    let image = move_to("项目封面.png", 2, "项目素材", 2);
    let notes = move_to("会议纪要.docx", 1, "文档", 3);
    let skipped = hybrid::PreviewEntry::Skip {
        identity: identity("预算草稿.docx"),
        item_name: "预算草稿.docx".into(),
        reason: hybrid::SkipReason::NoMatch,
    };
    let mut before_skip = skipped.clone();
    if let hybrid::PreviewEntry::Skip { reason, .. } = &mut before_skip {
        *reason = hybrid::SkipReason::MissingTarget;
    }
    let entries = vec![document.clone(), image, notes, skipped.clone()];
    let organized = entries.iter().filter_map(|entry| match entry {
        hybrid::PreviewEntry::Move(item) => Some(item.clone()),
        hybrid::PreviewEntry::Skip { .. } => None,
    }).collect();
    let (_sender, receiver) = std::sync::mpsc::channel();
    let stages = [
        ("rules", Editor::default(), false),
        ("rule-details", Editor { expanded: Some(1), ..Editor::default() }, false),
        ("edit", draft, false),
        ("preview", Editor {
            preview: Some(hybrid::OrganizePreview::fixture(entries.clone(), None)),
            ..Editor::default()
        }, true),
        ("updated", Editor {
            preview: Some(hybrid::OrganizePreview::fixture(entries, Some(vec![
                hybrid::PreviewChange::Changed {
                    before: move_to("季度报告.pdf", 2, "项目素材", 2),
                    after: document,
                },
                hybrid::PreviewChange::Changed { before: before_skip, after: skipped },
            ]))),
            show_skipped: Some(true),
            ..Editor::default()
        }, false),
        ("empty", Editor {
            preview: Some(hybrid::OrganizePreview::fixture(vec![], None)),
            ..Editor::default()
        }, false),
        ("undoing", Editor {
            applying: Some(PendingOrganize { command: Command::UndoOrganize, result: receiver }),
            ..Editor::default()
        }, false),
        ("result", Editor {
            result: Some(OrganizeResult::Applied(organized)),
            ..Editor::default()
        }, true),
        ("undo-result", Editor {
            result: Some(OrganizeResult::Undone { restored: 2, skipped: 0 }),
            ..Editor::default()
        }, false),
        ("undo-partial", Editor {
            result: Some(OrganizeResult::Undone { restored: 2, skipped: 1 }),
            ..Editor::default()
        }, false),
    ];
    let (mut panels, mut settings) = {
        let state = state.borrow();
        (state.workspace.panels().to_vec(), state.store.settings().unwrap())
    };
    let mut rules = decode(settings.get(KEY)).unwrap();
    panels.iter_mut().find(|panel| panel.id() == PanelId::new(1)).unwrap().set_title("文档");
    panels.iter_mut().find(|panel| panel.id() == PanelId::new(2)).unwrap().set_title("项目素材");
    rules.rules[0].item_type = Some(ClassificationItemType::File);
    rules.rules[0].extensions = ["pdf", "docx", "xlsx"].map(str::to_owned).into();
    rules.rules[0].name_contains = Some("报告".into());
    rules.rules[1].target_panel = 2;
    rules.rules[1].extensions = ["png", "jpg", "webp"].map(str::to_owned).into();
    rules.rules[1].name_regex = Some("^(screenshot|capture)-[0-9]{8}-[0-9]{6}$".into());
    rules.rules.push(ClassificationRule {
        target_panel: 99,
        enabled: false,
        item_type: Some(ClassificationItemType::Shortcut),
        extensions: Vec::new(),
        name_contains: None,
        name_regex: None,
    });
    settings.insert(KEY.into(), SettingValue::Text(encode(&rules).unwrap()));
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    let export = std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some();
    for locale in 0..crate::i18n::LANGUAGES.len() - 1 {
        crate::i18n::with_locale(locale, || {
            let painter = Painter::new().unwrap();
            for (width, height, scale, dark) in [
                (480.0, 380.0, 1.0, false), (800.0, 560.0, 1.0, false),
                (800.0, 560.0, 1.0, true), (940.0, 620.0, 1.5, true),
            ] {
                for (name, editor, undo_available) in &stages {
                    let tab = if matches!(*name, "rules" | "rule-details" | "edit") { Tab::Rules } else { Tab::Organize };
                    let layout = |height| window_scene(
                        (width, height), Backdrop::Mica, tab, &panels,
                        settings.get(KEY), editor, *undo_available,
                    );
                    let fit_example = locale == 0 && width == 800.0 && *name == "updated";
                    let height = if fit_example {
                        let scene = organizer_titlebar(layout(height), width, false);
                        preferred_preview_height(&scene)
                    } else {
                        height
                    };
                    let body = layout(height);
                    let tabs: Vec<_> = body.controls.iter().filter(|control| matches!(control.kind, ControlKind::Tab(_))).collect();
                    let [rules, organize] = tabs.as_slice() else { panic!("expected two organizer tabs") };
                    assert_eq!(rules.bounds.left, SIDE_MARGIN);
                    assert_eq!(rules.bounds.right + Tokens::GAP, organize.bounds.left);
                    assert!(tabs.iter().any(|control| control.selected
                        && matches!(control.action, Action::OrganizeTab(selected) if selected == tab)));
                    for control in tabs {
                        assert_eq!(control.bounds.bottom - control.bounds.top, NavigationItem::HEIGHT);
                        assert!(painter.label_width(&control.label).unwrap() <= control.bounds.right - control.bounds.left
                            - NavigationItem::LABEL_INSET - NavigationItem::END_INSET + 0.01);
                    }
                    for card in &body.cards {
                        assert_eq!(card.left, SIDE_MARGIN);
                        assert!((card.right - (width - SIDE_MARGIN)).abs() < 0.01);
                    }
                    assert!(body.cards.windows(2).all(|cards| cards[0].bottom <= cards[1].top));
                    if let Some(last) = body.controls.iter().rev().find(|control| {
                        matches!(control.action, Action::Classification(command) if command.is_footer())
                    }) {
                        assert!((last.bounds.right - (width - SIDE_MARGIN)).abs() < 0.01);
                    }
                    let control = |command: Command| body.controls.iter().find(|control| command.is_control(&control.action));
                    if let Some(preview) = &editor.preview {
                        let refresh = control(Command::Run).unwrap();
                        assert!(refresh.enabled && matches!(refresh.kind, ControlKind::QuietButton(_)));
                        assert_eq!(refresh.label, t("organizer-refresh-preview"));
                        assert_eq!(refresh.bounds.left, SIDE_MARGIN);
                        assert!(control(Command::CancelPreview).unwrap().enabled);
                        assert_eq!(control(Command::ApplyPreview).is_some_and(|button| button.enabled && matches!(button.kind, ControlKind::PrimaryButton)),
                            preview.move_count() > 0);
                        if !preview.entries.is_empty() {
                            for key in ["organizer-to-group", "organizer-skipped"] {
                                let label = body.text.iter().find(|(_, label, _)| label == t(key)).unwrap().0;
                                assert!(body.cards[0].left < label.left && label.right < body.cards[0].right);
                                if let Some(undo) = control(Command::UndoOrganize) {
                                    assert!(label.right < undo.bounds.left || label.bottom < undo.bounds.top);
                                }
                            }
                        }
                        if preview.move_count() > 0 {
                            let row_top = |key| body.text.iter().find(|(_, text, _)| text == t(key)).unwrap().0.top;
                            let skipped = control(Command::ToggleSkipped).unwrap();
                            let ControlKind::SectionDisclosure { summary_width } = skipped.kind else {
                                panic!("expected a skipped section heading");
                            };
                            assert_eq!(skipped.label, t("classification-preview-skipped"));
                            let count_text = crate::i18n::format("organizer-group-count", &[("count", "1".into())]);
                            let skipped_count = body.text.iter().find(|(bounds, text, style)| {
                                bounds.top == skipped.bounds.top && text == &count_text && *style == Tokens::SECONDARY_VALUE
                            }).unwrap().0;
                            assert_eq!(skipped_count.right - skipped_count.left, summary_width);
                            let padding = luciddesk_window::controls::InlineControl::SECTION_DISCLOSURE_PADDING;
                            assert_eq!(skipped_count.right + padding, skipped.bounds.right);
                            assert!(row_top("classification-preview-moves") < skipped.bounds.top);
                            let expanded = preview.changes.is_some();
                            assert_eq!(skipped.selected, expanded);
                            assert_eq!(body.text.iter().any(|(bounds, text, _)| {
                                bounds.top >= skipped.bounds.bottom && text == "预算草稿.docx"
                            }), expanded);
                            let groups: Vec<_> = body.section_headers.iter().map(|header| header.title.as_str()).collect();
                            assert_eq!(groups, ["文档", "项目素材"]);
                            for (header, count) in body.section_headers.iter().zip([2, 1]) {
                                let summary = crate::i18n::format("organizer-group-count", &[("count", count.to_string())]);
                                assert_eq!(header.summary, summary);
                                assert!(summary.contains(&count.to_string()));
                                let [icon, title, count] = header.cells();
                                assert_eq!(header.bounds.left, icon.left);
                                assert!(icon.right < title.left && title.right < count.left);
                                assert_eq!(count.right, header.bounds.right);
                            }
                            let item_row = |name| body.text.iter()
                                .find(|(_, value, style)| value == name && *style != Tokens::SECTION_TITLE)
                                .unwrap().0;
                            assert!(body.section_headers[0].bounds.bottom <= item_row("季度报告.pdf").top);
                            assert!(item_row("季度报告.pdf").bottom < item_row("会议纪要.docx").top);
                            assert!(item_row("会议纪要.docx").bottom < body.section_headers[1].bounds.top);
                            assert!(body.section_headers[1].bounds.bottom <= item_row("项目封面.png").top);
                            assert!(item_row("项目封面.png").bottom < skipped.bounds.top);
                            let matched_rule = crate::i18n::format("organizer-matched-rule", &[("rule", "3".into())]);
                            assert_eq!(item_row(&matched_rule).top, item_row("会议纪要.docx").top);
                            let [_, title, count] = body.section_headers[0].cells();
                            assert_eq!(item_row("季度报告.pdf").left, title.left);
                            assert_eq!(item_row(&matched_rule).right, count.right);
                            assert_eq!(skipped_count.right, count.right);
                            if preview.changes.is_some() {
                                assert!(row_top("classification-preview-differences") < row_top("classification-preview-moves"));
                                assert_eq!(control(Command::ApplyPreview).unwrap().label, t("classification-preview-confirm"));
                                let [change] = body.destination_changes.as_slice() else { panic!("expected one destination change") };
                                let title_row = |name| body.text.iter()
                                    .find(|(_, text, style)| text == name && *style == Tokens::SECTION_TITLE)
                                    .unwrap().0;
                                let filename = title_row("季度报告.pdf");
                                let status = body.text.iter().find(|(bounds, text, style)| {
                                    *style == Tokens::SECONDARY_VALUE && bounds.top == filename.top
                                        && text == t("classification-diff-changed")
                                }).unwrap().0;
                                assert!(filename.right < status.left && filename.bottom < change.bounds.top);
                                assert_eq!(change.before.group, "项目素材");
                                assert_eq!(change.after.group, "文档");
                                assert!(change.before.rule.contains('2') && change.after.rule.contains('1'));
                                let [original, arrow, updated] = change.cells();
                                assert_eq!(original.right, arrow.left);
                                assert_eq!(arrow.right, updated.left);
                                assert!(body.cards[1].left < original.left && updated.right < body.cards[1].right);
                                assert_eq!(change.bounds.bottom - change.bounds.top, DestinationChange::HEIGHT);
                                for (bounds, destination) in change.destinations() {
                                    let [group, rule] = destination.cells(bounds);
                                    assert_eq!(group.top, rule.top);
                                    assert_eq!(group.bottom, rule.bottom);
                                    assert!(group.right < rule.left && rule.right <= bounds.right);
                                    assert!(!destination.truncated);
                                }
                                let reason_change = crate::i18n::format("classification-diff-transition", &[
                                    ("before", t("classification-skip-target").into()),
                                    ("after", t("classification-skip-no-match").into()),
                                ]);
                                let reason = item_row(&reason_change);
                                assert!(reason.top > change.bounds.bottom);
                                assert!(reason.top >= title_row("预算草稿.docx").bottom);
                            } else {
                                assert!(body.destination_changes.is_empty());
                            }
                        }
                    }
                    if editor.editing() {
                        assert!(matches!(control(Command::Save).unwrap().kind, ControlKind::PrimaryButton));
                        assert!(control(Command::Run).is_none());
                        assert_eq!(body.cards.len(), 1);
                        let regex = control(Command::DraftRegex).unwrap();
                        assert!(regex.bounds.right - regex.bounds.left >=
                            components::text_width(&regex.label, 14.0) + Tokens::CHECKBOX_INSET);
                    }
                    if editor.applying.is_some() {
                        assert!(control(Command::ApplyPreview).is_none() && control(Command::UndoOrganize).is_none());
                    }
                    if let Some(result) = &editor.result {
                        let has_details = matches!(result, OrganizeResult::Applied(moves) if !moves.is_empty());
                        assert_eq!(body.cards.len(), if has_details { 2 } else { 1 });
                        for (bounds, _, _) in &body.text {
                            assert!(body.cards.iter().any(|card| {
                                card.left < bounds.left && bounds.right <= card.right
                                    && card.top < bounds.top && bounds.bottom < card.bottom
                            }));
                            if let Some(undo) = control(Command::UndoOrganize) {
                                assert!(bounds.top >= undo.bounds.bottom || bounds.right <= undo.bounds.left || bounds.bottom <= undo.bounds.top);
                            }
                        }
                        assert!(control(Command::Run).unwrap().enabled);
                    }
                    if *undo_available {
                        assert!(control(Command::UndoOrganize).unwrap().enabled);
                    }
                    let mut scrolled = organizer_titlebar(
                        window_scene((width, height), Backdrop::Mica, tab, &panels, settings.get(KEY), editor, *undo_available),
                        width, false,
                    );
                    let mut offset = f32::MAX;
                    scrolled.scroll_to(width, height, &mut offset);
                    for (header, original) in scrolled.section_headers.iter().zip(&body.section_headers) {
                        assert_eq!(header.bounds.top, (original.bounds.top + TITLE_HEIGHT) - offset);
                    }
                    for (change, original) in scrolled.destination_changes.iter().zip(&body.destination_changes) {
                        assert_eq!(change.bounds.top, (original.bounds.top + TITLE_HEIGHT) - offset);
                    }

                    let mut body = organizer_titlebar(body, width, false);
                    assert_eq!(body.caption_icon.unwrap().left, SIDE_MARGIN);
                    body.scroll_to(width, height, &mut 0.0);
                    if fit_example {
                        assert_eq!(body.scroll_max, 0.0);
                        assert_eq!(body.viewport.unwrap().bottom - body.content_bottom(), Tokens::MARGIN);
                    }
                    for control in &body.controls {
                        assert!(control.bounds.left >= 0.0 && control.bounds.right <= width + 0.01);
                        if matches!(control.action, Action::Classification(command) if command.is_footer()) {
                            let viewport = body.viewport.unwrap();
                            assert!(control.bounds.top > viewport.bottom && control.bounds.bottom < height);
                            assert!(body.accepts_pointer(control, control.bounds.left + 2.0, control.bounds.top + 2.0));
                        }
                    }
                    let w = (width * scale) as u32;
                    let h = (height * scale) as u32;
                    let bitmap = crate::pane::canvas::Offscreen::new(&device, w, h).unwrap();
                    painter.paint(&bitmap.target, &body, width, height, scale, dark, false, None, None, &HashMap::new()).unwrap();
                    let pixels = bitmap.pixels().unwrap();
                    assert_eq!(pixels[0] < 128, dark);
                    if *name == "rules" && width == 800.0 {
                        let grip = body.rule_rows[0].grip();
                        let left = grip.left.round() as u32;
                        let right = grip.right.round() as u32;
                        let top = grip.top.round() as u32;
                        let bottom = grip.bottom.round() as u32;
                        let sample = |x, y| pixels[((y * w + x) * 4) as usize];
                        let background = sample(left, (top + bottom) / 2);
                        let points: Vec<_> = (top..bottom).flat_map(|y| (left..right).map(move |x| (x, y)))
                            .filter(|&(x, y)| sample(x, y).abs_diff(background) > 40)
                            .collect();
                        let ink_left = points.iter().map(|&(x, _)| x).min().unwrap();
                        let ink_right = points.iter().map(|&(x, _)| x).max().unwrap();
                        assert!((ink_left + ink_right + 1).abs_diff(left + right) <= 2);
                        let bands = points.windows(2).filter(|pair| pair[1].1 > pair[0].1 + 1).count() + 1;
                        assert_eq!(bands, 3);
                    }
                    if export && locale == 0 && width == 800.0 {
                        let mut bmp = vec![0u8; 54];
                        bmp[0..2].copy_from_slice(b"BM");
                        bmp[2..6].copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                        bmp[18..22].copy_from_slice(&(w as i32).to_le_bytes());
                        bmp[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes());
                        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                        bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                        bmp.extend(pixels);
                        let theme = if dark { "dark" } else { "light" };
                        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../target").join(format!("desktop-organizer-{name}-{theme}.bmp"));
                        std::fs::write(path, bmp).unwrap();
                    }
                }
            }
        });
    }
    drop(stages);
    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0); }
}

#[test]
fn classification_templates_prefill_editable_rules_and_require_a_target() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    for id in [PanelId::new(1), PanelId::new(2)] {
        state.borrow_mut().workspace.panel_mut(id).unwrap().set_locked(true);
    }
    host::show(&state, Tab::Rules).unwrap();
    let hwnd: HWND = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
    let mut editor = Editor::default();

    {
        let state = state.borrow();
        let settings = state.store.settings().unwrap();
        let no_targets = window_scene((480.0, 380.0), Backdrop::Mica, Tab::Rules, &[], settings.get(KEY), &editor, false);
        for label in ["classification-add", "organizer-rule-templates"] {
            assert!(no_targets.controls.iter().any(|control| control.label == t(label) && !control.enabled));
        }
        assert!(no_targets.text.iter().any(|(_, text, _)| text == t("classification-no-target")));
    }

    for (template, example) in [
        (Template::Documents, "pdf"),
        (Template::Images, "png"),
        (Template::Archives, "zip"),
        (Template::Shortcuts, "lnk"),
    ] {
        editor.execute(&state, hwnd, Command::AddTemplate(template));
        let draft = editor.draft.as_ref().unwrap();
        let rule = draft.edited_rule();
        assert!(rule.extensions.iter().any(|ext| ext == example));
        assert_eq!(rule.item_type, (template == Template::Documents).then_some(ClassificationItemType::File));
        assert_eq!(rule.target_panel, 0, "two groups are available");
        editor.execute(&state, hwnd, Command::Save);
        assert!(editor.editing() && editor.error.is_some());
        assert_eq!(load(&state.borrow()).unwrap().rules.len(), 2);
        editor.execute(&state, hwnd, Command::Cancel);
    }

    editor.execute(&state, hwnd, Command::AddTemplate(Template::Documents));
    let body = {
        let state = state.borrow();
        let settings = state.store.settings().unwrap();
        window_scene((800.0, 560.0), Backdrop::Mica, Tab::Rules, state.workspace.panels(), settings.get(KEY), &editor, false)
    };
    let shown_types = body.controls.iter().find_map(|control| match &control.action {
        Action::Dropdown(options) if options.iter().any(|(_, action, _)| {
            matches!(action, Action::Classification(Command::DraftItemType(_)))
        }) => Some(options),
        _ => None,
    }).unwrap();
    assert_eq!(shown_types.len(), 4);
    assert!(shown_types.iter().any(|(_, action, selected)| {
        *selected && Command::DraftItemType(Some(ClassificationItemType::File)).is_control(action)
    }));
    let extensions = unsafe { GetDlgItem(hwnd, Field::Extensions.id()) };
    unsafe {
        SetWindowTextW(extensions, windows_sys::w!("pdf, docx"));
    }
    editor.execute(&state, hwnd, Command::Target(PanelId::new(2)));
    editor.execute(&state, hwnd, Command::Save);
    assert!(editor.error.is_none());
    assert!(!editor.editing());
    let saved = load(&state.borrow()).unwrap();
    assert!(!saved.enabled);
    assert_eq!(saved.rules.len(), 3);
    assert_eq!(saved.rules[2].target_panel, 2);
    assert_eq!(saved.rules[2].extensions, ["pdf", "docx"]);
    assert_eq!(saved.rules[2].item_type, Some(ClassificationItemType::File));
    assert!(saved.rules[..2].iter().all(|rule| rule.item_type.is_none()));

    editor.execute(&state, hwnd, Command::Add);
    editor.execute(&state, hwnd, Command::DraftItemType(Some(ClassificationItemType::Folder)));
    editor.execute(&state, hwnd, Command::Save);
    assert!(editor.error.is_none());
    let saved = load(&state.borrow()).unwrap();
    assert_eq!(saved.rules[3].item_type, Some(ClassificationItemType::Folder));
    assert!(saved.rules[3].extensions.is_empty());

    editor.execute(&state, hwnd, Command::Edit(2));
    editor.execute(&state, hwnd, Command::DraftItemType(None));
    editor.execute(&state, hwnd, Command::Save);
    assert!(editor.error.is_none());
    assert_eq!(load(&state.borrow()).unwrap().rules[2].item_type, None);
    unsafe {
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

#[test]
fn classification_details_show_one_rule_without_writing_settings() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    let mut config = load(&state.borrow()).unwrap();
    config.rules[0].item_type = Some(ClassificationItemType::File);
    config.rules[0].extensions.extend(["presentation", "spreadsheet", "technical-drawing", "document-template"].map(str::to_owned));
    config.rules[0].name_contains = Some("报告".into());
    config.rules[1].name_regex = Some("^Screenshot-.*$".into());
    config.rules[1].target_panel = 99;
    config.rules[1].enabled = false;
    save(&state.borrow(), &config).unwrap();
    let revision = state.borrow().store.change_count();
    let mut editor = Editor::default();
    let render = |editor: &Editor| {
        let mut body = Scene::empty(Backdrop::Mica, Tokens::MARGIN, TITLE_HEIGHT);
        table::draw(
            &mut body,
            Rect::from_xywh(Tokens::MARGIN, 0.0, 480.0 - 2.0 * Tokens::MARGIN, 0.0),
            state.borrow().workspace.panels(),
            &config.rules,
            editor,
        );
        body
    };
    let contains = |body: &Scene, value: &str, format: usize| body.text.iter().any(|(_, text, style)| text == value && *style == format);
    let hwnd = std::ptr::null_mut();
    let collapsed = render(&editor);
    let extensions = config.rules[0].extensions.join(", ");
    let condition_values = [extensions.as_str(), "报告", "png", "^Screenshot-.*$"];
    assert!(!collapsed.text.iter().any(|(_, value, _)| {
        condition_values.contains(&value.as_str())
    }));
    assert_eq!(
        collapsed.rule_rows[0].bounds.bottom - collapsed.rule_rows[0].bounds.top,
        collapsed.rule_rows[1].bounds.bottom - collapsed.rule_rows[1].bounds.top,
    );
    let unavailable = collapsed.controls.iter().find(|control| Command::ToggleDetails(1).is_control(&control.action)).unwrap();
    assert_eq!(unavailable.label, t("classification-missing"));
    assert!(matches!(unavailable.kind, ControlKind::RuleDisclosure { missing_target: true }));
    assert!(collapsed.controls.iter().any(|control| Command::Toggle(1).is_control(&control.action) && !control.selected));
    for (bounds, _, _) in &collapsed.text {
        assert!(bounds.left >= Tokens::MARGIN && bounds.right <= 480.0 - Tokens::MARGIN);
    }

    editor.execute(&state, hwnd, Command::ToggleDetails(0));
    let first = render(&editor);
    assert!(contains(&first, &extensions, 1));
    assert!(contains(&first, "报告", 1));
    assert!(!first.text.iter().any(|(_, value, _)| value == "png"));
    assert!(first.controls.iter().any(|control| {
        Command::ToggleDetails(0).is_control(&control.action) && control.selected
    }));

    editor.detail_motions.insert(0, Motion::settled(0.5, Instant::now()));
    let expanding = render(&editor);
    let clip = &expanding.disclosure_clips[0].bounds;
    let (extension_index, _) = expanding.text.iter().enumerate()
        .find(|(_, (_, value, _))| value == &extensions).unwrap();
    assert!(expanding.disclosure_text_clip(extension_index).is_some());
    assert!(collapsed.rule_rows[1].bounds.top < expanding.rule_rows[1].bounds.top);
    assert!(expanding.rule_rows[1].bounds.top < first.rule_rows[1].bounds.top);
    assert!(clip.bottom <= expanding.rule_rows[1].bounds.top);
    editor.finish_disclosure_animations();

    editor.execute(&state, hwnd, Command::ToggleDetails(1));
    let second = render(&editor);
    assert!(!contains(&second, &extensions, 1));
    assert!(contains(&second, "png", 1));
    assert!(contains(&second, "^Screenshot-.*$", 1));
    editor.execute(&state, hwnd, Command::ToggleDetails(1));
    assert!(!contains(&render(&editor), "png", 1));
    assert_eq!(state.borrow().store.change_count(), revision);
}

#[test]
fn rule_animations_reverse_and_clear_after_the_transition() {
    let _sta = crate::pane::test_support::apartment();
    let start = Instant::now();
    let mut editor = Editor { expanded: Some(0), ..Editor::default() };
    editor.animate_details(None, start);
    assert_eq!(editor.detail_fraction(0, start), 0.0);
    let halfway = start + SETTINGS_DURATION / 2;
    let fraction = editor.detail_fraction(0, halfway);
    assert!(fraction > 0.0 && fraction < 1.0);

    editor.expanded = None;
    editor.animate_details(Some(0), halfway);
    assert_eq!(editor.detail_fraction(0, halfway), fraction);
    assert!(editor.tick_disclosure_animations(halfway));
    let finished = halfway + SETTINGS_DURATION;
    assert!(!editor.tick_disclosure_animations(finished));
    assert_eq!(editor.detail_fraction(0, finished), 0.0);
}

#[test]
fn skipped_preview_animates_the_visible_rows_and_reverses_without_jumping() {
    let _sta = crate::pane::test_support::apartment();
    let preview = hybrid::OrganizePreview::fixture((1..=8).map(|index| {
        let item_name = format!("Draft {index}.txt");
        hybrid::PreviewEntry::Skip {
            identity: ShellIdentity::FileSystem {
                path: item_name.clone().into(), volume_id: None, file_id: None,
            },
            item_name,
            reason: hybrid::SkipReason::NoMatch,
        }
    }).collect(), None);
    let render = |expanded, fraction| {
        let mut scene = Scene::empty(Backdrop::Mica, SIDE_MARGIN, TITLE_HEIGHT);
        table::draw_preview(&mut scene, Rect::from_xywh(SIDE_MARGIN, TITLE_HEIGHT, 768.0, 0.0), &preview, (expanded, fraction));
        scene
    };
    let start = Instant::now();
    let mut editor = Editor { show_skipped: Some(true), ..Editor::default() };
    assert!(editor.animate_skipped(false, start, true));
    assert_eq!(editor.skipped_fraction(true, start), 0.0);
    let halfway = start + SETTINGS_DURATION / 2;
    let fraction = editor.skipped_fraction(true, halfway);
    assert!(fraction > 0.0 && fraction < 1.0);

    let closed = render(false, 0.0);
    let expanding = render(true, fraction);
    let open = render(true, 1.0);
    let clip = &expanding.disclosure_clips[0];
    assert!(!clip.text.is_empty() && !clip.separators.is_empty());
    assert!(expanding.disclosure_text_clip(clip.text.start).is_some());
    assert!(expanding.disclosure_separator_clip(clip.separators.end - 1).is_some());
    assert!(closed.content_bottom() < expanding.content_bottom());
    assert!(expanding.content_bottom() < open.content_bottom());

    editor.show_skipped = Some(false);
    assert!(editor.animate_skipped(true, halfway, true));
    assert!((editor.skipped_fraction(false, halfway) - fraction).abs() < 0.001);
    assert!(!editor.tick_disclosure_animations(halfway + SETTINGS_DURATION));
    assert_eq!(editor.skipped_fraction(false, halfway + SETTINGS_DURATION), 0.0);
    editor.show_skipped = Some(true);
    assert!(!editor.animate_skipped(false, halfway + SETTINGS_DURATION, false));
    assert_eq!(editor.skipped_fraction(true, halfway + SETTINGS_DURATION), 1.0);
}

#[test]
fn classification_editing_survives_tab_switch_and_only_saves_valid_input() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    host::show(&state, Tab::Rules).unwrap();
    let hwnd: HWND = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
    unsafe {
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let w = rect.right as f32 / scale;
        let owner = state.borrow();
        let appearance = owner
            .workspace
            .appearance()
            .unwrap_or((PanelTheme::System, Backdrop::Mica));
        let body = window_scene((w, rect.bottom as f32 / scale), appearance.1, Tab::Rules, owner.workspace.panels(),
            owner.store.settings().unwrap().get(KEY), &Editor::default(), false);
        drop(owner);
        let body = organizer_titlebar(body, w, false);
        assert!(
            body.text
                .iter()
                .any(|(_, text, _)| text == t("classification-enabled"))
        );
        assert!(!body.text.iter().any(|(_, text, _)| matches!(text.as_str(), "1" | "2")));
        for row in &body.rule_rows {
            let grip = row.grip();
            assert!(body.controls.iter().any(|control| {
                control.label == "Group 1"
                    && control.bounds.top == grip.top
                    && control.bounds.left > grip.right
                    && matches!(control.kind, ControlKind::RuleDisclosure { missing_target: false })
            }));
        }
        assert!(body.controls.iter().any(|control| {
            Command::Add.is_control(&control.action)
                && matches!(control.kind, ControlKind::PrimaryButton)
        }));
        assert!(!body.controls.iter().any(|control| Command::Run.is_control(&control.action)));
        let templates = body
            .controls
            .iter()
            .find(|control| control.label == t("organizer-rule-templates"))
            .unwrap();
        assert!(matches!(templates.kind, ControlKind::Combo));
        let Action::Dropdown(options) = &templates.action else {
            panic!("Rule templates must open the template menu");
        };
        assert_eq!(options.len(), Template::ALL.len());
        for (option, template) in options.iter().zip(Template::ALL) {
            assert_eq!(option.0, template.label());
            assert!(
                matches!(option.1, Action::Classification(Command::AddTemplate(value)) if value == template)
            );
        }
        assert!(body.controls.iter().any(|control| {
            matches!(control.action, Action::Classification(Command::Toggle(0)))
                && control.is_toggle()
        }));
        let click = |command: Command| {
            let bounds = body
                .controls
                .iter()
                .find(|c| command.is_control(&c.action))
                .unwrap()
                .bounds;
            let x = ((bounds.left + bounds.right) * 0.5 * scale) as u16;
            let y = ((bounds.top + bounds.bottom) * 0.5 * scale) as u16;
            let point = (u32::from(x) | (u32::from(y) << 16)) as isize;
            SendMessageW(hwnd, WM_LBUTTONDOWN, 1, point);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, point);
            SendMessageW(hwnd, WM_PAINT, 0, 0);
        };
        let press = |input, key| {
            SendMessageW(input, WM_KEYDOWN, usize::from(key), 0);
            let mut message = MSG::default();
            assert_ne!(
                PeekMessageW(
                    &raw mut message,
                    hwnd,
                    CLASSIFICATION_KEY,
                    CLASSIFICATION_KEY,
                    PM_REMOVE
                ),
                0
            );
            DispatchMessageW(&raw const message);
        };
        click(Command::Add);
        let new_rule = GetDlgItem(hwnd, Field::Extensions.id());
        assert!(!new_rule.is_null());
        press(new_rule, VK_ESCAPE);
        assert!(GetDlgItem(hwnd, Field::Extensions.id()).is_null());
        click(Command::Toggle(0));
        assert!(!load(&state.borrow()).unwrap().rules[0].enabled);
        click(Command::Edit(0));
        let extensions = GetDlgItem(hwnd, Field::Extensions.id());
        let keyword = GetDlgItem(hwnd, Field::Keyword.id());
        assert!(!extensions.is_null());
        assert!(!keyword.is_null());
        assert_eq!(GetParent(extensions), hwnd);
        assert_eq!(
            GetWindowLongW(extensions, GWL_STYLE) as u32 & (WS_CHILD | WS_POPUP),
            WS_CHILD
        );
        assert_ne!(GetWindowLongW(extensions, GWL_STYLE) as u32 & WS_VISIBLE, 0);
        assert_ne!(IsWindowEnabled(hwnd), 0);

        SetWindowTextW(extensions, windows_sys::w!("draft"));
        SendMessageW(hwnd, host::SELECT_TAB, Tab::Organize as usize, 0);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        assert_ne!(IsWindow(extensions), 0, "page changes preserve the draft");
        assert_eq!(IsWindowVisible(extensions), 0, "inputs stay hidden on other pages");
        SendMessageW(hwnd, host::SELECT_TAB, Tab::Rules as usize, 0);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        assert_eq!(GetDlgItem(hwnd, Field::Extensions.id()), extensions);
        let mut retained = [0u16; 32];
        let length = GetWindowTextW(extensions, retained.as_mut_ptr(), retained.len() as i32);
        assert_eq!(String::from_utf16_lossy(&retained[..length as usize]), "draft");
        SetWindowTextW(extensions, windows_sys::w!(""));
        press(extensions, VK_RETURN);
        assert_ne!(
            IsWindow(extensions),
            0,
            "invalid input keeps the editor open"
        );
        assert_eq!(load(&state.borrow()).unwrap().rules[0].extensions, ["pdf"]);

        SetWindowTextW(extensions, windows_sys::w!("docx，txt"));
        SetWindowTextW(keyword, windows_sys::w!("报告"));
        press(keyword, VK_RETURN);
        assert_eq!(IsWindow(extensions), 0);
        let saved = load(&state.borrow()).unwrap();
        assert_eq!(saved.rules[0].extensions, ["docx", "txt"]);
        assert_eq!(saved.rules[0].name_contains.as_deref(), Some("报告"));
        assert!(!saved.rules[0].enabled);

        // Reopen through the same list entry, then cancel without writing.
        SendMessageW(hwnd, host::SELECT_TAB, Tab::Rules as usize, 0);
        click(Command::Edit(0));
        let extensions = GetDlgItem(hwnd, Field::Extensions.id());
        assert!(!extensions.is_null());
        SetWindowTextW(extensions, windows_sys::w!("zip"));
        let mut changed = load(&state.borrow()).unwrap();
        changed.enabled = true;
        save(&state.borrow(), &changed).unwrap();
        press(extensions, VK_RETURN);
        assert_ne!(
            IsWindow(extensions),
            0,
            "a conflicting change keeps the draft open"
        );
        press(extensions, VK_ESCAPE);
        assert_eq!(IsWindow(extensions), 0);
        assert_eq!(
            load(&state.borrow()).unwrap().rules[0].extensions,
            ["docx", "txt"]
        );
        assert!(load(&state.borrow()).unwrap().enabled);

        SendMessageW(hwnd, host::SELECT_TAB, Tab::Rules as usize, 0);
        click(Command::Edit(1));
        let extensions = GetDlgItem(hwnd, Field::Extensions.id());
        let keyword = GetDlgItem(hwnd, Field::Keyword.id());
        assert!(!extensions.is_null());
        assert!(!keyword.is_null());
        assert_eq!(load(&state.borrow()).unwrap().rules.len(), 2);
        SetWindowTextW(extensions, windows_sys::w!(""));
        press(keyword, VK_TAB);
        SendMessageW(hwnd, WM_KEYDOWN, VK_SPACE as usize, 0);
        SetWindowTextW(keyword, windows_sys::w!("["));
        press(keyword, VK_RETURN);
        assert_ne!(
            IsWindow(keyword),
            0,
            "invalid regular expressions keep the draft open"
        );
        assert_eq!(load(&state.borrow()).unwrap().rules.len(), 2);
        SetWindowTextW(keyword, windows_sys::w!("^(报告|合同)[0-9]+$"));
        press(keyword, VK_RETURN);
        let saved = load(&state.borrow()).unwrap();
        assert_eq!(saved.rules.len(), 2);
        assert_eq!(
            saved.rules[1].name_regex.as_deref(),
            Some("^(报告|合同)[0-9]+$")
        );
        assert!(saved.rules[1].name_contains.is_none());
        assert_eq!(IsWindow(extensions), 0);
        SendMessageW(hwnd, host::SELECT_TAB, Tab::Rules as usize, 0);
        SendMessageW(hwnd, host::SELECT_TAB, Tab::Organize as usize, 0);
        assert_eq!(SendMessageW(hwnd, host::INSPECT_TAB, 1, 0), 1);
        assert_ne!(
            IsWindowEnabled(hwnd),
            0,
            "disconnected status stays in the organize tab"
        );
        assert_eq!(load(&state.borrow()).unwrap().rules.len(), 2);
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

#[test]
fn delete_undo_restores_order_and_rejects_concurrent_rule_changes() {
    let (_directory, state) = test_state();
    let mut editor = Editor::default();
    let hwnd = std::ptr::null_mut();
    let original = encode(&load(&state.borrow()).unwrap()).unwrap();
    editor.execute(&state, hwnd, Command::Delete(0));
    assert!(editor.error.is_none(), "{:?}", editor.error);
    assert_eq!(load(&state.borrow()).unwrap().rules[0].extensions, ["png"]);
    editor.execute(&state, hwnd, Command::UndoDelete);
    assert!(editor.error.is_none());
    assert_eq!(encode(&load(&state.borrow()).unwrap()).unwrap(), original);
    assert!(editor.deleted.is_none());
    editor.execute(&state, hwnd, Command::Delete(1));
    let mut changed = load(&state.borrow()).unwrap();
    changed.rules[0].extensions = vec!["docx".into()];
    save(&state.borrow(), &changed).unwrap();
    editor.execute(&state, hwnd, Command::UndoDelete);
    assert!(editor.error.is_some());
    assert_eq!(encode(&load(&state.borrow()).unwrap()).unwrap(), encode(&changed).unwrap());
}

#[test]
fn partially_visible_native_input_uses_a_region_and_restores_full_text_area() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    host::show(&state, Tab::Rules).unwrap();
    let hwnd: HWND = state.borrow().organizer.as_ref().unwrap().hwnd().cast();
    let rule = load(&state.borrow()).unwrap().rules[0].clone();
    let mut inputs = Inputs::new(hwnd, &rule).unwrap();
    let mut body = scene(800.0, 560.0, 16, false, (PanelTheme::Light, Backdrop::Mica), Default::default());
    body.controls.clear();
    body.viewport = Some(Rect::from_xywh(200.0, 50.0, 400.0, 80.0));
    body.button(Rect::from_xywh(220.0, 25.0, 200.0, 40.0), "", Command::Input(Field::Extensions).action(), false);
    inputs.sync(&body, 1.0, false, false);
    unsafe {
        let input = GetDlgItem(hwnd, Field::Extensions.id());
        assert_ne!(IsWindowVisible(input), 0);
        let region = CreateRectRgn(0, 0, 0, 0);
        assert_ne!(GetWindowRgn(input, region), 0);
        let mut bounds = RECT::default();
        GetRgnBox(region, &raw mut bounds);
        assert!(bounds.top > 0 && bounds.bottom > bounds.top);
        body.controls[0].bounds = Rect::from_xywh(220.0, 60.0, 200.0, 40.0);
        inputs.sync(&body, 1.0, false, false);
        GetWindowRgn(input, region);
        GetRgnBox(region, &raw mut bounds);
        assert_eq!(bounds.top, 0);
        DeleteObject(region);
        drop(inputs);
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}
