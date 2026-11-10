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
fn classification_controls_persist_order_toggle_delete_and_enable() {
    let (_directory, state) = test_state();
    let mut editor = Editor::default();
    let hwnd = std::ptr::null_mut();
    for command in [
        Command::Move(1, -1),
        Command::Toggle(0),
        Command::Enable(true),
    ] {
        editor.execute(&state, hwnd, command);
        assert!(editor.error.is_none());
    }
    let saved = load(&state.borrow()).unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.rules[0].extensions, ["png"]);
    assert!(!saved.rules[0].enabled);
    editor.execute(&state, hwnd, Command::Delete(0));
    assert!(editor.error.is_none());
    assert_eq!(load(&state.borrow()).unwrap().rules[0].extensions, ["pdf"]);
    let before = state.borrow().workspace.clone();
    assert!(hybrid::preview_organize(&state.borrow()).is_err());
    assert_eq!(state.borrow().workspace, before);
}

#[test]
fn classification_templates_prefill_editable_rules_and_require_a_target() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    for id in [PanelId::new(1), PanelId::new(2)] {
        state.borrow_mut().workspace.panel_mut(id).unwrap().set_locked(true);
    }
    host::show_at(&state, PanelId::new(1), 16).unwrap();
    let hwnd: HWND = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    let mut editor = Editor::default();

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
        assert_eq!(rule.target_panel, 0, "two groups are available");
        editor.execute(&state, hwnd, Command::Save);
        assert!(editor.editing() && editor.error.is_some());
        assert_eq!(load(&state.borrow()).unwrap().rules.len(), 2);
        editor.execute(&state, hwnd, Command::Cancel);
    }

    editor.execute(&state, hwnd, Command::AddTemplate(Template::Documents));
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
    unsafe {
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}

#[test]
fn classification_details_show_one_rule_without_writing_settings() {
    let (_directory, state) = test_state();
    let mut config = load(&state.borrow()).unwrap();
    config.rules[0].name_contains = Some("报告".into());
    config.rules[1].name_regex = Some("^Screenshot-.*$".into());
    save(&state.borrow(), &config).unwrap();
    let revision = state.borrow().store.change_count();
    let mut editor = Editor::default();
    let render = |editor: &Editor| {
        let mut body = scene(
            800.0,
            560.0,
            16,
            false,
            (PanelTheme::Light, Backdrop::Mica),
            Default::default(),
        );
        table::draw(
            &mut body,
            Rect::from_xywh(220.0, 0.0, 540.0, 0.0),
            state.borrow().workspace.panels(),
            &config.rules,
            editor,
        );
        body
    };
    let contains = |body: &Scene, value: &str| body.text.iter().any(|(_, text, _)| text == value);
    let hwnd = std::ptr::null_mut();
    let collapsed = render(&editor);
    assert!(!contains(&collapsed, "pdf"));
    assert!(!contains(&collapsed, "png"));

    editor.execute(&state, hwnd, Command::ToggleDetails(0));
    let first = render(&editor);
    assert!(contains(&first, "pdf"));
    assert!(contains(&first, "报告"));
    assert!(!contains(&first, "png"));
    assert!(first.controls.iter().any(|control| {
        Command::ToggleDetails(0).is_control(&control.action) && control.selected
    }));

    editor.execute(&state, hwnd, Command::ToggleDetails(1));
    let second = render(&editor);
    assert!(!contains(&second, "pdf"));
    assert!(contains(&second, "png"));
    assert!(contains(&second, "^Screenshot-.*$"));
    editor.execute(&state, hwnd, Command::ToggleDetails(1));
    assert!(!contains(&render(&editor), "png"));
    assert_eq!(state.borrow().store.change_count(), revision);
}

#[test]
fn classification_editing_stays_in_settings_and_only_saves_valid_input() {
    let _sta = crate::pane::test_support::apartment();
    let (_directory, state) = test_state();
    host::show_at(&state, PanelId::new(1), 16).unwrap();
    let hwnd: HWND = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    unsafe {
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let w = rect.right as f32 / scale;
        let h = rect.bottom as f32 / scale;
        let owner = state.borrow();
        let appearance = owner
            .workspace
            .appearance()
            .unwrap_or((PanelTheme::System, Backdrop::Mica));
        let mut body = scene(
            w,
            h - TITLE_HEIGHT,
            16,
            false,
            appearance,
            owner.workspace.pane_options(),
        );
        draw(
            &mut body,
            w,
            owner.workspace.panels(),
            owner.store.settings().unwrap().get(KEY),
            &Editor::default(),
        );
        drop(owner);
        let body = with_titlebar(body, w, false);
        assert!(
            body.text
                .iter()
                .any(|(_, text, _)| text == t("classification-enabled"))
        );
        for order in ["1", "2"] {
            let (order_bounds, _, _) = body.text.iter().find(|(_, text, _)| text == order).unwrap();
            assert!(body.controls.iter().any(|control| {
                control.label == "Group 1"
                    && control.bounds.top == order_bounds.top
                    && control.bounds.left > order_bounds.right
                    && matches!(control.kind, ControlKind::Disclosure)
            }));
        }
        for command in [Command::Add, Command::Run] {
            assert!(body.controls.iter().any(|control| {
                command.is_control(&control.action)
                    && matches!(control.kind, ControlKind::Button)
                    && !control.label.is_empty()
            }));
        }
        let add = body
            .controls
            .iter()
            .find(|control| control.label == t("classification-add"))
            .unwrap();
        let Action::Dropdown(options) = &add.action else {
            panic!("Add rule must open the template menu");
        };
        assert_eq!(options.len(), 1 + Template::ALL.len());
        assert!(matches!(options[0].1, Action::Classification(Command::Add)));
        for (option, template) in options[1..].iter().zip(Template::ALL) {
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
        SendMessageW(hwnd, SELECT_TEST_PAGE, 0, 0);
        SendMessageW(hwnd, WM_PAINT, 0, 0);
        assert_ne!(IsWindow(extensions), 0, "page changes preserve the draft");
        assert_eq!(IsWindowVisible(extensions), 0, "inputs stay hidden on other pages");
        SendMessageW(hwnd, SELECT_TEST_PAGE, 16, 0);
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
        SendMessageW(hwnd, SELECT_TEST_PAGE, 16, 0);
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

        SendMessageW(hwnd, SELECT_TEST_PAGE, 16, 0);
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
        SendMessageW(hwnd, SELECT_TEST_PAGE, 16, 0);
        click(Command::Run);
        assert_ne!(
            IsWindowEnabled(hwnd),
            0,
            "disconnected status stays in settings"
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
    host::show_at(&state, PanelId::new(1), 16).unwrap();
    let hwnd: HWND = state.borrow().settings.as_ref().unwrap().hwnd().cast();
    let rule = load(&state.borrow()).unwrap().rules[0].clone();
    let mut inputs = Inputs::new(hwnd, &rule).unwrap();
    let mut body = scene(800.0, 560.0, 16, false, (PanelTheme::Light, Backdrop::Mica), Default::default());
    body.controls.clear();
    body.viewport = Some(Rect::from_xywh(200.0, 50.0, 400.0, 80.0));
    body.button(Rect::from_xywh(220.0, 25.0, 200.0, 40.0), "", Command::Input(Field::Extensions).action(), false);
    inputs.sync(&body, 1.0, false);
    unsafe {
        let input = GetDlgItem(hwnd, Field::Extensions.id());
        assert_ne!(IsWindowVisible(input), 0);
        let region = CreateRectRgn(0, 0, 0, 0);
        assert_ne!(GetWindowRgn(input, region), 0);
        let mut bounds = RECT::default();
        GetRgnBox(region, &raw mut bounds);
        assert!(bounds.top > 0 && bounds.bottom > bounds.top);
        body.controls[0].bounds = Rect::from_xywh(220.0, 60.0, 200.0, 40.0);
        inputs.sync(&body, 1.0, false);
        GetWindowRgn(input, region);
        GetRgnBox(region, &raw mut bounds);
        assert_eq!(bounds.top, 0);
        DeleteObject(region);
        drop(inputs);
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}
