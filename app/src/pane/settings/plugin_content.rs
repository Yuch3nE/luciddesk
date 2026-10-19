//! Generic plugin settings pages, rendered and interacted with inside the main settings window.
use super::*;
use luciddesk_plugin_api::{Item, Page, SETTINGS_PAGE_CAPABILITY};
use serde_json::{Value, json};

pub(super) fn pages() -> Vec<luciddesk_plugin_api::Manifest> {
    crate::plugins::snapshots()
        .into_iter()
        .filter(|p| {
            p.state == luciddesk_plugin_host::PluginState::Ready
                && p.manifest.supports(SETTINGS_PAGE_CAPABILITY)
        })
        .map(|p| p.manifest)
        .collect()
}

pub(super) fn load(id: &str) -> Result<Value, String> {
    if !crate::plugins::supports(id, SETTINGS_PAGE_CAPABILITY) {
        return Err(format!("Plugin {id} settings are unavailable"));
    }
    crate::plugins::request(id, json!({"op":"settings"}))
}

pub(super) fn draw(scene: &mut Scene, width: f32, id: &str, value: Result<Value, String>) {
    let page =
        value.and_then(|value| serde_json::from_value::<Page>(value).map_err(|e| e.to_string()));
    let page = match page {
        Ok(page) => page,
        Err(error) => {
            SettingsForm::new(scene, width, "").section(&error);
            return;
        }
    };
    let action = |value| Action::PluginAction(id.to_owned(), value);
    let mut form = SettingsForm::new(scene, width, &page.description);
    for item in page.items {
        match item {
            Item::Section(title) => form.section(&title),
            Item::Toggle {
                title,
                description,
                selected,
                action: value,
            } => form.toggle(&title, &description, selected, action(value)),
            Item::Toggles { title, values } => form.toggle_group(
                &title,
                values
                    .iter()
                    .map(|(s, a, b)| (s.as_str(), action(a.clone()), *b))
                    .collect(),
            ),
            Item::Choices { title, values } => form.choices(
                &title,
                "",
                values
                    .iter()
                    .map(|(s, a, b)| (s.as_str(), action(a.clone()), *b))
                    .collect(),
            ),
            Item::Dropdown { title, values } => form.dropdown(
                &title,
                "",
                values
                    .iter()
                    .map(|(s, a, b)| (s.as_str(), action(a.clone()), *b))
                    .collect(),
            ),
            Item::Reorder { title, values } => form.reorder(
                &title,
                values
                    .into_iter()
                    .map(|(s, up, down)| (s, up.map(&action), down.map(&action)))
                    .collect(),
            ),
            Item::Button {
                title,
                label,
                action: value,
            } => form.button(&title, "", &label, action(value)),
        }
    }
}

pub(super) fn execute(
    state: &Rc<RefCell<PaneApp>>,
    selected: PanelId,
    id: &str,
    action: Value,
) -> Result<(), String> {
    if !crate::plugins::supports(id, SETTINGS_PAGE_CAPABILITY) {
        return Err(format!("Plugin {id} settings are unavailable"));
    }
    crate::plugins::request(id, json!({"op":"settings_action", "action":action}))?;
    if id == monitor::PLUGIN_ID {
        if state.borrow().monitor_child {
            handle(
                state,
                selected,
                Event::MonitorOptions(monitor::Preferences::load()?),
            )?;
        } else {
            monitor::process::configure(state)?;
        }
    }
    state.borrow().wake.notify();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_settings_pages_render_inside_the_group_at_multiple_scales() {
        let _sta = crate::pane::test_support::apartment();
        let pages = pages();
        if pages.is_empty() {
            return;
        }
        let painter = Painter::new().unwrap();
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        for plugin in pages {
            for width in [800.0, 940.0] {
                for scale in [1.0, 1.5, 2.0] {
                    for dark in [false, true] {
                        for bottom in [false, true] {
                            let height = 600.0;
                            let mut body = layout::plugin_scene(
                                width,
                                height - TITLE_HEIGHT,
                                &plugin.id,
                                (
                                    if dark {
                                        PanelTheme::Dark
                                    } else {
                                        PanelTheme::Light
                                    },
                                    Backdrop::Acrylic,
                                ),
                                Default::default(),
                            );
                            draw(&mut body, width, &plugin.id, load(&plugin.id));
                            let mut scene = with_titlebar(body, width, false);
                            scene.scroll_to(
                                width,
                                height,
                                &mut if bottom { f32::MAX } else { 0.0 },
                            );
                            let mut navigation = 0.0;
                            scene.reveal_navigation(&mut navigation);
                            scene.scroll_navigation(&mut navigation);
                            for c in scene
                                .controls
                                .iter()
                                .filter(|c| c.bounds.left >= Tokens::content_x())
                            {
                                assert!(c.bounds.right <= width);
                                assert!(c.bounds.top >= -scene.scroll_max);
                                assert!(c.bounds.bottom <= height + scene.scroll_max);
                            }
                            let bitmap = canvas::Offscreen::new(
                                &device,
                                (width * scale) as u32,
                                (height * scale) as u32,
                            )
                            .unwrap();
                            painter
                                .paint(
                                    &bitmap.target,
                                    &scene,
                                    width,
                                    height,
                                    scale,
                                    dark,
                                    false,
                                    None,
                                    None,
                                    &Default::default(),
                                )
                                .unwrap();
                            if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some()
                                && width == 940.0
                                && scale == 1.0
                                && !dark
                            {
                                let pixels = bitmap.pixels().unwrap();
                                let mut bmp = vec![0_u8; 54];
                                bmp[0..2].copy_from_slice(b"BM");
                                bmp[2..6]
                                    .copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                                bmp[10..14].copy_from_slice(&54_u32.to_le_bytes());
                                bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
                                bmp[18..22].copy_from_slice(&(width as i32).to_le_bytes());
                                bmp[22..26].copy_from_slice(&(-(height as i32)).to_le_bytes());
                                bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
                                bmp[28..30].copy_from_slice(&32_u16.to_le_bytes());
                                bmp.extend(pixels);
                                std::fs::write(
                                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                        .join("../target")
                                        .join(format!(
                                            "settings-plugin-{}{}.bmp",
                                            plugin.id,
                                            if bottom { "-bottom" } else { "" }
                                        )),
                                    bmp,
                                )
                                .unwrap();
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn overflowing_navigation_scrolls_without_moving_the_brand_or_accepting_clipped_hits() {
        let mut scene = with_titlebar(
            layout::scene(
                900.0,
                260.0,
                15,
                false,
                (PanelTheme::Light, Backdrop::Acrylic),
                Default::default(),
            ),
            900.0,
            false,
        );
        let brand = scene.text[0].0;
        let mut offset = f32::MAX;
        scene.scroll_navigation(&mut offset);
        assert!(scene.navigation_max > 0.0);
        assert!(scene.navigation_thumb().is_some());
        assert_eq!(scene.text[0].0.top, brand.top);
        assert_eq!(scene.text[0].0.bottom, brand.bottom);
        let viewport = scene.navigation_viewport.unwrap();
        let last = scene
            .controls
            .iter()
            .filter(|c| matches!(c.kind, ControlKind::Navigation))
            .last()
            .unwrap();
        assert!(last.bounds.bottom <= viewport.bottom);
        let first = scene
            .controls
            .iter()
            .find(|c| matches!(c.kind, ControlKind::Navigation))
            .unwrap();
        assert!(!scene.accepts_pointer(
            first,
            (first.bounds.left + first.bounds.right) * 0.5,
            (first.bounds.top + first.bounds.bottom) * 0.5
        ));
    }

    fn pump() {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                assert_ne!(message.message, WM_QUIT);
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    #[test]
    fn installed_pages_are_contiguous_under_the_plugin_group_and_hidden_when_disabled() {
        let entries = layout::navigation();
        if crate::plugins::installed().is_empty() {
            assert!(
                !entries
                    .iter()
                    .any(|e| matches!(e.action, Action::PluginPage(_) | Action::Page(15)))
            );
            return;
        }
        let manager = entries
            .iter()
            .position(|e| matches!(e.action, Action::Page(15)))
            .unwrap();
        assert!(
            entries[..manager]
                .iter()
                .all(|e| matches!(e.action, Action::Page(_)))
        );
        assert!(
            entries[manager + 1..]
                .iter()
                .all(|e| matches!(e.action, Action::PluginPage(_)))
        );
        if std::env::var_os("LUCIDDESK_TEST_SETTINGS_PLUGIN").is_some() {
            assert!(
                entries.iter().any(
                    |e| matches!(&e.action, Action::PluginPage(id) if id == "example.settings")
                )
            );
            crate::plugins::set_enabled("example.settings", false).unwrap();
            assert!(!pages().iter().any(|p| p.id == "example.settings"));
            assert!(load("example.settings").is_err());
            crate::plugins::set_enabled("example.settings", true).unwrap();
        }
    }

    #[test]
    fn native_plugin_pages_use_the_main_settings_window_and_route_generic_actions() {
        if std::env::var_os("LUCIDDESK_TEST_SETTINGS_PLUGIN").is_none() {
            return;
        }
        let _sta = crate::pane::test_support::apartment();
        let directory = tempfile::tempdir().unwrap();
        let state = Rc::new(RefCell::new(super::super::super::tests::test_state()));
        state.borrow_mut().store =
            WorkspaceStore::open(&directory.path().join("workspace.db")).unwrap();
        crate::plugins::initialize(&state.borrow().store).unwrap();
        let global_before = std::fs::read(directory.path().join("config.toml")).ok();
        host::show_plugin(&state, "example.settings").unwrap();
        pump();
        let hwnd = state.borrow().settings.as_ref().unwrap().hwnd().cast();
        let click_toggle = |id: &str| {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &raw mut rect);
            }
            let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
            let width = rect.right as f32 / scale;
            let mut scene = layout::plugin_scene(
                width,
                rect.bottom as f32 / scale - TITLE_HEIGHT,
                id,
                (PanelTheme::System, Backdrop::Mica),
                Default::default(),
            );
            draw(&mut scene, width, id, load(id));
            let mut scene = with_titlebar(scene, width, false);
            scene.scroll_to(width, rect.bottom as f32 / scale, &mut 0.0);
            let toggle = scene.controls.iter().find(|c| c.is_toggle()).unwrap();
            let x = ((toggle.bounds.left + toggle.bounds.right) * 0.5 * scale) as isize;
            let y = ((toggle.bounds.top + toggle.bounds.bottom) * 0.5 * scale) as isize;
            let point = (y << 16) | (x & 0xffff);
            unsafe {
                SendMessageW(hwnd, WM_LBUTTONDOWN, 1, point);
                SendMessageW(hwnd, WM_LBUTTONUP, 0, point);
                SendMessageW(hwnd, WM_PAINT, 0, 0);
            }
        };
        assert_eq!(
            crate::plugins::request("example.settings", json!({"op":"get"})).unwrap(),
            json!(false)
        );
        click_toggle("example.settings");
        assert_eq!(
            crate::plugins::request("example.settings", json!({"op":"get"})).unwrap(),
            json!(true)
        );
        if monitor::available() {
            host::show_plugin(&state, monitor::PLUGIN_ID).unwrap();
            pump();
            assert_eq!(state.borrow().settings.as_ref().unwrap().hwnd(), hwnd);
            // The monitor's first control enables its feature; keep this test process-local.
            let mut options = monitor::Preferences::load().unwrap();
            options.compact = true;
            execute(
                &state,
                PanelId::new(0),
                monitor::PLUGIN_ID,
                json!({"op":"save", "options":options}),
            )
            .unwrap();
            assert!(monitor::Preferences::load().unwrap().compact);
            plugin_page::set_enabled(&state, monitor::PLUGIN_ID, false).unwrap();
            unsafe {
                SendMessageW(hwnd, WM_PAINT, 0, 0);
            }
            assert!(!pages().iter().any(|p| p.id == monitor::PLUGIN_ID));
            plugin_page::set_enabled(&state, monitor::PLUGIN_ID, true).unwrap();
        }
        assert_eq!(
            std::fs::read(directory.path().join("config.toml")).ok(),
            global_before
        );
        assert_ne!(unsafe { IsWindow(hwnd) }, 0);
        unsafe {
            SendMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        assert!(state.borrow().settings.is_none());
        crate::plugins::shutdown();
    }
}
