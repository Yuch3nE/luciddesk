use super::*;

pub(super) fn general(s: &mut Scene, width: f32, status: crate::startup::Status, busy: bool, cli_enabled: bool, skill_prompt_copied: bool) {
    let mut form = SettingsForm::new(s, width, crate::i18n::text("startup-description"));
    form.section(crate::i18n::text("startup-section"));
    form.toggle_enabled(crate::i18n::text("startup-title"), if busy { crate::i18n::text("startup-working") } else { status.message() }, status.registered(),
        status.editable() && !busy,
        Action::Startup(!status.registered()));
    form.button(crate::i18n::text("startup-manage"), crate::i18n::text("startup-manage-description"),
        crate::i18n::text("startup-open-settings"), Action::ProjectLink("ms-settings:startupapps"));
    form.section(crate::i18n::text("agent-section"));
    form.toggle(crate::i18n::text("agent-cli-title"), crate::i18n::text("agent-cli-description"),
        cli_enabled, Action::CliEnabled(!cli_enabled));
    form.button(crate::i18n::text("agent-skill-title"), crate::i18n::text("agent-skill-description"),
        crate::i18n::text(if skill_prompt_copied { "ui-copied" } else { "ui-copy" }), Action::CopySkillPrompt);
    use luciddesk_diagnostics::{Level, level};
    form.section(crate::i18n::text("diagnostics-section"));
    form.choices(crate::i18n::text("diagnostics-level"), crate::i18n::text("diagnostics-level-description"),
        [Level::Error, Level::Warn, Level::Info, Level::Debug, Level::Trace].into_iter()
            .map(|value| (value.label(), Action::LogLevel(value), value == level())).collect());
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn windows10_material_page_hides_mica_in_choices_and_preview() {
        for show_mica in [false, true] {
            for material in [Backdrop::Mica, Backdrop::MicaAlt, Backdrop::Mica.with_strength(75)] {
                let page = scene_with_mica(900.0, 700.0, 0, false,
                    (PanelTheme::Light, material), luciddesk_core::PaneOptions::default(), show_mica);
                let choices: Vec<_> = page.controls.iter().filter_map(|control| {
                    if let Action::Change(Event::Material(value)) = control.action {
                        Some((value, control.selected))
                    } else { None }
                }).collect();
                assert_eq!(choices.len(), if show_mica { 4 } else { 2 });
                if !show_mica {
                    assert!(choices.iter().all(|(value, _)| !matches!(value, Backdrop::Mica | Backdrop::MicaAlt)));
                    assert!(choices.iter().any(|(value, selected)| *value == Backdrop::Acrylic && *selected));
                    assert!(page.text.iter().all(|(_, text, _)| !text.contains("Mica")));
                    assert!(page.previews.iter().all(|(_, value)| value.base() == Backdrop::Acrylic));
                    assert_eq!(page.material.strength(), material.strength().or(Some(50)));
                }
            }
        }
    }
}

pub(super) fn folder_defaults(
    s: &mut Scene,
    width: f32,
    value: folder::Defaults,
    mode: folder::EntryMode,
) {
    let mut form = SettingsForm::new(
        s,
        width,
        crate::i18n::text("ui-set-folder-navigation-and-the-default-view-for-new-folder-panels"),
    );
    form.section(crate::i18n::text("ui-folder-navigation"));
    form.choices(
        crate::i18n::text("ui-open-subfolders-in"),
        crate::i18n::text("ui-used-on-double-click-or-enter-in-all-folder-panels"),
        [
            (crate::i18n::text("ui-this-panel"), folder::EntryMode::Inline),
            (crate::i18n::text("ui-file-explorer"), folder::EntryMode::Explorer),
        ]
        .into_iter()
        .map(|(label, choice)| (label, Action::FolderEntryMode(choice), mode == choice))
        .collect(),
    );
    form.section(crate::i18n::text("ui-new-panel-defaults"));
    form.choices(
        crate::i18n::text("ui-default-view"),
        crate::i18n::text("ui-applies-to-new-panels-existing-panels-keep-their-settings"),
        [(crate::i18n::text("ui-icons"), false), (crate::i18n::text("ui-list"), true)]
            .into_iter()
            .map(|(label, list)| {
                (
                    label,
                    Action::FolderDefaults(folder::Defaults { list, ..value }),
                    value.list == list,
                )
            })
            .collect(),
    );
    for (label, column) in [(crate::i18n::text("ui-type"), 1), (crate::i18n::text("ui-modified"), 2), (crate::i18n::text("ui-size"), 3)] {
        form.toggle(
            label,
            crate::i18n::text("ui-show-this-column-in-list-view-name-is-always-shown"),
            value.columns & (1 << column) != 0,
            Action::FolderDefaults(folder::Defaults {
                columns: value.columns ^ (1 << column),
                ..value
            }),
        );
    }
    form.button(
        crate::i18n::text("ui-reset-view-defaults"),
        crate::i18n::text("ui-reset-the-view-and-columns-for-new-panels"),
        crate::i18n::text("ui-reset"),
        Action::FolderDefaults(folder::Defaults::default()),
    );
}

// Page IDs stay stable; display order is independent of routing.
pub(super) fn pages() -> [(usize, &'static str, &'static str); 10] { [
    (13, crate::i18n::text("startup-general"), "\u{e713}"),
    (0, crate::i18n::text("ui-theme-materials"), "\u{e790}"),
    (1, crate::i18n::text("ui-panel-layout"), "\u{f0e2}"),
    (11, crate::i18n::text("ui-fonts"), "\u{e8d2}"),
    (8, crate::i18n::text("ui-folder-panels"), "\u{e8b7}"),
    (4, crate::i18n::text("ui-everything-search"), "\u{e721}"),
    (3, crate::i18n::text("ui-file-preview"), "\u{e890}"),
    (6, crate::i18n::text("ui-backup-restore"), "\u{e81c}"),
    (12, crate::i18n::text("language-title"), "\u{e8d2}"),
    (5, crate::i18n::text("ui-about"), "\u{e946}"),
] }

pub(super) fn scene(
    width: f32,
    _height: f32,
    page: usize,
    search_enabled: bool,
    appearance: (PanelTheme, Backdrop),
    options: luciddesk_core::PaneOptions,
) -> Scene {
    let version = windows_version::OsVersion::current();
    scene_with_mica(width, _height, page, search_enabled, appearance, options,
        version.major > 10 || (version.major == 10 && version.build >= 22000))
}

fn scene_with_mica(
    width: f32,
    _height: f32,
    page: usize,
    search_enabled: bool,
    appearance: (PanelTheme, Backdrop),
    options: luciddesk_core::PaneOptions,
    show_mica: bool,
) -> Scene {
    // A portable configuration may have been saved on Windows 11. Reflect the
    // downlevel acrylic fallback without rewriting the persisted preference.
    let material_strength = appearance.1.strength();
    let appearance = if !show_mica && matches!(appearance.1.base(), Backdrop::Mica | Backdrop::MicaAlt) {
        (appearance.0, Backdrop::Acrylic.with_strength(appearance.1.strength().unwrap_or(50)))
    } else { appearance };
    let mut s = Scene {
        material: appearance.1,
        viewport: None,
        scroll_max: 0.0,
        scroll_offset: 0.0,
        text: vec![],
        cards: vec![],
        separators: vec![],
        controls: vec![],
        previews: vec![],
        app_icon: None,
    };
    s.text(Rect::from_xywh(28.0, 26.0, 172.0, 32.0), "LucidDesk", 2);
    for (position, &(id, name, icon)) in pages().iter().enumerate() {
        // Keep spacing tied to semantic groups, not insertion positions.
        let gap = match id {
            4 | 3 => 10.0,
            6 | 12 | 5 => 20.0,
            _ => 0.0,
        };
        let stride = ((_height - 136.0) / (pages().len() - 1) as f32).clamp(34.0, 42.0);
        let y = 78.0 + position as f32 * stride + gap;
        s.control(
            ControlKind::Navigation,
            Rect::from_xywh(12.0, y, Tokens::content_x() - 48.0, 38.0),
            name,
            Action::Page(id),
            page == id || (page == 7 && id == 0) || (matches!(page, 9 | 10) && id == 6),
        );
        s.text(Rect::from_xywh(30.0, y, 22.0, 38.0), icon, 5);
    }
    let x = Tokens::content_x();
    let w = (width - x - Tokens::MARGIN).min(Tokens::MAX_WIDTH);
    s.text(
        Rect::from_xywh(x, 28.0, w, 44.0),
        pages().iter().find(|(id, _, _)| *id == page).map_or(
            match page {
                9 => crate::i18n::text("ui-manage-backups"),
                10 => crate::i18n::text("ui-advanced-options"),
                _ => crate::i18n::text("ui-colors"),
            },
            |(_, name, _)| *name,
        ),
        3,
    );
    if page == 7 {
        let Backdrop::Solid { color, opacity } = appearance.1 else {
            return s;
        };
        let mut form =
            SettingsForm::new(&mut s, width, crate::i18n::text("ui-choose-a-preset-or-adjust-rgb-changes-apply-immediately"));
        form.back(crate::i18n::text("ui-back"), Action::Page(0));
        form.preview(
            &crate::i18n::format("ui-panel-preview", &[("arg0", format!("{}", (opacity * 100.0).round() as u8))]),
            color,
            opacity,
        );
        form.colors(color);
        form.section(crate::i18n::text("ui-custom-color"));
        for (channel, name) in [crate::i18n::text("ui-red-r"), crate::i18n::text("ui-green-g"), crate::i18n::text("ui-blue-b")].into_iter().enumerate() {
            let value = ((color >> ((2 - channel) * 8)) & 255) as u8;
            form.slider(
                name,
                "",
                Slider {
                    value: f32::from(value),
                    max: 255.0,
                    centered: false,
                    channel: Some(channel as u8),
                },
                &value.to_string(),
                Action::Channel(channel as u8, value),
            );
        }
        form.button(
            crate::i18n::text("ui-hex-color"),
            crate::i18n::text("ui-enter-a-six-digit-hex-color-and-press-enter"),
            &format!("#{color:06X}"),
            Action::StyleInput(false),
        );
        form.button(
            crate::i18n::text("ui-reset-solid-color"),
            crate::i18n::text("ui-restore-the-default-color-and-opacity"),
            crate::i18n::text("ui-reset"),
            Action::SolidReset,
        );
        return s;
    }
    if page == 0 {
        let mut form = SettingsForm::new(
            &mut s,
            width,
            crate::i18n::text("ui-adjust-the-theme-material-and-transparency-changes-apply-immediately"),
        );
        form.section(crate::i18n::text("ui-appearance"));
        form.choices(
            crate::i18n::text("ui-app-theme"),
            crate::i18n::text("ui-choose-light-dark-or-system-default"),
            [
                (crate::i18n::text("ui-system-default"), PanelTheme::System),
                (crate::i18n::text("ui-light"), PanelTheme::Light),
                (crate::i18n::text("ui-dark"), PanelTheme::Dark),
            ]
            .into_iter()
            .map(|(name, value)| {
                (
                    name,
                    Action::Change(Event::Theme(value)),
                    appearance.0 == value,
                )
            })
            .collect(),
        );
        let solid = if matches!(appearance.1, Backdrop::Solid { .. }) {
            appearance.1
        } else {
            Backdrop::solid_default(theme::is_dark(appearance.0))
        };
        form.choices(
            crate::i18n::text("ui-window-material"),
            crate::i18n::text("ui-choose-the-panel-background-material"),
            [
                (crate::i18n::text("ui-acrylic"), Backdrop::Acrylic),
                ("Mica", Backdrop::Mica),
                ("Mica Alt", Backdrop::MicaAlt),
                (crate::i18n::text("ui-solid-color"), solid),
            ]
            .into_iter()
            .filter(|(_, material)| show_mica || !matches!(material, Backdrop::Mica | Backdrop::MicaAlt))
            .map(|(name, value)| {
                (
                    name,
                    Action::Change(Event::Material(value)),
                    appearance.1.kind() == value.kind(),
                )
            })
            .collect(),
        );
        form.section(crate::i18n::text("ui-effects-preview"));
        let name = match appearance.1.base() {
            Backdrop::Mica => crate::i18n::text("ui-mica-soft-tones"),
            Backdrop::MicaAlt => crate::i18n::text("ui-mica-alt-vivid-depth"),
            Backdrop::Solid { .. } => crate::i18n::text("ui-solid-color-panel"),
            _ => crate::i18n::text("ui-acrylic-frosted-glass"),
        };
        form.material_preview(name, appearance.1);
        if let Backdrop::Solid { color, opacity } = appearance.1 {
            form.button(
                crate::i18n::text("ui-background-color"),
                &crate::i18n::format("ui-current-color", &[("color", format!("{:06X}", color))]),
                crate::i18n::text("ui-edit-colors"),
                Action::SolidColor,
            );
            let value = (opacity * 100.0).round() as u8;
            form.slider(
                crate::i18n::text("ui-panel-opacity"),
                crate::i18n::text("ui-lower-values-make-the-background-more-transparent"),
                Slider::linear(f32::from(value), 100.0),
                &format!("{value}%"),
                Action::Opacity(value),
            );
            form.button(
                crate::i18n::text("ui-reset-solid-color"),
                crate::i18n::text("ui-restore-the-default-color-and-opacity"),
                crate::i18n::text("ui-reset"),
                Action::SolidReset,
            );
        }
        if let Some(value) = material_strength {
            form.slider(
                crate::i18n::text("ui-effect-strength"),
                crate::i18n::text("ui-from-transparent-to-dense-the-center-is-the-default"),
                Slider::centered(f32::from(value), 100.0),
                &if value == 50 {
                    crate::i18n::text("ui-default").into()
                } else {
                    format!("{:+}", i16::from(value) - 50)
                },
                Action::Strength(value),
            );
            form.button(
                crate::i18n::text("ui-reset-material"),
                crate::i18n::text("ui-reset-effect-strength-to-its-default"),
                crate::i18n::text("ui-reset"),
                Action::StrengthReset,
            );
        }
    } else if page == 1 {
        let mut form = SettingsForm::new(
            &mut s,
            width,
            crate::i18n::text("ui-adjust-panel-shape-text-and-icon-layout-changes-apply-immediately"),
        );
        form.section(crate::i18n::text("ui-panel-shape"));
        form.slider(
            crate::i18n::text("ui-corner-radius"),
            crate::i18n::text("ui-adjust-corner-rounding-for-panels-and-tabs"),
            Slider::linear(
                options.corner_radius,
                luciddesk_core::PaneOptions::MAX_CORNER_RADIUS,
            ),
            &format!("{:.1}", options.corner_radius),
            Action::Radius(options.corner_radius),
        );
        for (title, description, value, event) in [
            (
                crate::i18n::text("ui-show-border"),
                crate::i18n::text("ui-separate-panels-from-the-desktop-with-a-thin-border"),
                options.border,
                Event::ToggleBorder,
            ),
            (
                crate::i18n::text("ui-edge-snapping"),
                crate::i18n::text("ui-snap-to-nearby-panels-and-screen-edges-when-moving"),
                options.snap,
                Event::ToggleSnap,
            ),
            (
                crate::i18n::text("ui-header-divider"),
                crate::i18n::text("ui-show-a-thin-line-between-the-header-and-content"),
                header_divider::enabled(),
                Event::ToggleHeaderDivider,
            ),
        ] {
            form.toggle(title, description, value, Action::Change(event));
        }
        if show_mica {
            form.toggle(crate::i18n::text("compact-menu-title"),
                crate::i18n::text("compact-menu-description"), compact_menu::enabled(),
                Action::Change(Event::ToggleCompactMenu));
        }
        form.section(crate::i18n::text("ui-text-icons"));
        form.toggle(
            crate::i18n::text("title-emoji-rendering"),
            crate::i18n::text("title-emoji-rendering-description"),
            title_emoji::color(),
            Action::Change(Event::SetTitleEmojiColor(!title_emoji::color())),
        );
        form.choices(
            crate::i18n::text("ui-panel-text"),
            crate::i18n::text("ui-automatic-mode-selects-text-brightness-for-the-background"),
            [
                (crate::i18n::text("ui-automatic"), luciddesk_core::PanelText::Auto),
                (crate::i18n::text("ui-light-text"), luciddesk_core::PanelText::Light),
                (crate::i18n::text("ui-dark-text"), luciddesk_core::PanelText::Dark),
            ]
            .into_iter()
            .map(|(label, value)| {
                (
                    label,
                    Action::Change(Event::SetPanelText(value)),
                    options.text == value,
                )
            })
            .collect(),
        );
        form.toggle(
            crate::i18n::text("ui-text-contrast-backing"),
            crate::i18n::text("ui-improve-text-readability-disable-to-preserve-full-transparency"),
            options.text_protection,
            Action::Change(Event::ToggleTextProtection),
        );
        form.slider(
            crate::i18n::text("ui-icon-grid-scale"),
            crate::i18n::text("ui-adjust-icon-size-and-spacing-together"),
            Slider::centered(grid_slider_position(options.grid_scale), 1.0),
            &format!("{:.0}%", options.grid_scale),
            Action::GridSize(options.grid_scale),
        );
        form.button(
            crate::i18n::text("ui-reset-panel-layout"),
            crate::i18n::text("ui-reset-this-page-to-its-defaults"),
            crate::i18n::text("ui-reset"),
            Action::Change(Event::ResetPaneOptions),
        );
    } else if page == 3 {
        let value = peek::settings();
        let resolved = peek::resolved(&value);
        let mut form =
            SettingsForm::new(&mut s, width, crate::i18n::text("ui-connect-a-preview-app-to-preview-files-with-a-shortcut"));
        form.section(crate::i18n::text("ui-preview-service"));
        form.toggle_enabled(
            crate::i18n::text("ui-enable-file-preview"),
            crate::i18n::text("ui-available-after-a-compatible-app-is-found"),
            value.enabled && resolved.is_some(),
            resolved.is_some(),
            Action::PeekEnable,
        );
        form.choices(
            crate::i18n::text("ui-preview-app"),
            crate::i18n::text("ui-choose-the-app-used-for-file-previews"),
            [peek::Provider::Peek, peek::Provider::QuickLook]
                .into_iter()
                .map(|provider| {
                    (
                        provider.name(),
                        Action::PreviewProvider(provider),
                        value.provider == provider,
                    )
                })
                .collect(),
        );
        let path = resolved
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| crate::i18n::format("ui-not-found-select-the-app", &[("arg0", format!("{}", value.provider.name()))]));
        form.path(
            if value.active_path().is_empty() {
                crate::i18n::text("ui-app-path-automatic")
            } else {
                crate::i18n::text("ui-app-path")
            },
            &path,
            vec![
                (crate::i18n::text("ui-browse"), Action::PeekBrowse),
                (crate::i18n::text("ui-detect-automatically"), Action::PeekDetect),
            ],
        );
        form.section(crate::i18n::text("ui-keyboard-shortcut"));
        form.shortcut(
            crate::i18n::text("ui-preview-shortcut"),
            crate::i18n::text("ui-works-only-within-panels"),
            &peek::shortcut_label(&value),
            Action::PeekShortcut,
            Action::PeekReset,
        );
    } else if page == 4 {
        let value = everything_settings::settings();
        let resolved = everything_settings::resolved(&value);
        let mut form = SettingsForm::new(&mut s, width, crate::i18n::text("ui-connect-everything-to-quickly-find-local-files"));
        form.section(crate::i18n::text("ui-search-service"));
        form.toggle_enabled(
            crate::i18n::text("ui-enable-search-panel"),
            crate::i18n::text("ui-available-after-everything-is-found"),
            search_enabled && resolved.is_some(),
            resolved.is_some(),
            Action::Change(Event::ToggleSearch),
        );
        let path = resolved
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| crate::i18n::text("ui-everything-not-found-select-the-app").into());
        form.path(
            if value.path.is_empty() {
                crate::i18n::text("ui-app-path-automatic")
            } else {
                crate::i18n::text("ui-app-path")
            },
            &path,
            vec![
                (crate::i18n::text("ui-browse"), Action::EverythingBrowse),
                (crate::i18n::text("ui-detect-automatically"), Action::EverythingDetect),
                (crate::i18n::text("ui-start"), Action::EverythingLaunch),
            ],
        );
        form.section(crate::i18n::text("ui-keyboard-shortcut"));
        let status = search_hotkey::status();
        form.shortcut(
            crate::i18n::text("ui-global-shortcut"),
            if status.is_empty() {
                crate::i18n::text("ui-focus-the-desktop-search-panel-from-any-app")
            } else {
                &status
            },
            &search_hotkey::label(search_hotkey::settings()),
            Action::SearchShortcut,
            Action::SearchReset,
        );
    }

    s
}

// Shared by the live page and raster tests so status and actions use the same layout.
#[cfg(test)]
pub(super) fn about_status(s: &mut Scene, width: f32, status: &str, copied: bool) {
    about_updates(s, width, status, copied, &crate::updates::Controller::default());
}

pub(super) fn about_updates(s: &mut Scene, width: f32, status: &str, copied: bool, updates: &crate::updates::Controller) {
    let mut form = SettingsForm::new(s, width, crate::i18n::text("ui-turn-your-windows-desktop-into-a-workspace-that-works-for-you"));
    form.brand();
    form.actions(
        crate::i18n::text("update-title"),
        &updates.status(),
        vec![
            (crate::i18n::text("update-check"), Action::Update),
            (crate::i18n::text("update-open-page"), Action::ProjectLink(crate::updates::PAGE)),
        ],
    );
    form.button(
        crate::i18n::text("ui-developer-license"),
        concat!(env!("CARGO_PKG_AUTHORS"), " · ", env!("CARGO_PKG_LICENSE")),
        crate::i18n::text("ui-project-website"),
        Action::ProjectLink("https://github.com/Yuch3nE/luciddesk"),
    );
    form.actions(
        crate::i18n::text("ui-project-resources"),
        "GitHub · Yuch3nE/luciddesk",
        vec![
            (crate::i18n::text("ui-download-releases"), Action::ProjectLink("https://github.com/Yuch3nE/luciddesk/releases")),
            (crate::i18n::text("ui-release-history"), Action::ProjectLink(crate::i18n::text("ui-changelog-url"))),
            (crate::i18n::text("ui-report-an-issue"), Action::ProjectLink("https://github.com/Yuch3nE/luciddesk/issues")),
        ],
    );
    form.section(crate::i18n::text("ui-status"));
    form.info(crate::i18n::text("ui-operating-system"), &format!("{} · {}", crate::system_info::system().summary(), std::env::consts::ARCH));
    form.actions(
        crate::i18n::text("ui-desktop-connection"),
        status,
        vec![
            (crate::i18n::text("ui-reconnect"), Action::Change(Event::RetryDesktop)),
            (
                if copied { crate::i18n::text("ui-copied") } else { crate::i18n::text("ui-copy-diagnostics") },
                Action::CopyDiagnostics,
            ),
        ],
    );
    for control in &mut s.controls {
        if matches!(control.action, Action::Update) { control.enabled = !updates.busy(); }
    }
}

pub(super) fn backup_page(
    s: &mut Scene,
    width: f32,
    view: &recovery::View,
    policy: recovery::Policy,
    advanced: bool,
) {
    let first_control = s.controls.len();
    let mut form = SettingsForm::new(s, width, crate::i18n::text("ui-backups-contain-settings-and-layouts-not-your-actual-files"));
    if advanced {
        form.back(crate::i18n::text("ui-back-to-backup-restore"), Action::Page(6));
        form.section(crate::i18n::text("ui-configuration-maintenance"));
        form.button(
            crate::i18n::text("ui-configuration-folder"),
            crate::i18n::text("ui-open-the-folder-containing-your-configuration"),
            crate::i18n::text("ui-open-folder"),
            Action::Change(Event::OpenConfigDirectory),
        );
        form.button(
            crate::i18n::text("ui-reload-configuration"),
            crate::i18n::text("ui-read-configuration-from-disk-and-apply-it-to-this-workspace"),
            crate::i18n::text("ui-reload"),
            Action::Change(Event::ReloadConfig),
        );
        form.button(
            crate::i18n::text("ui-export-configuration"),
            crate::i18n::text("ui-choose-where-to-save-a-configuration-copy"),
            crate::i18n::text("ui-export"),
            Action::Change(Event::ExportBackup),
        );
    } else {
        let status = if view.status.is_empty() {
            crate::i18n::text("ui-no-backups-yet")
        } else {
            &view.status
        };
        if view.status.contains(crate::i18n::text("ui-failed")) {
            form.button(crate::i18n::text("ui-backup-status"), status, crate::i18n::text("ui-view-details"), Action::BackupStatus);
        } else {
            form.info(crate::i18n::text("ui-backup-status"), status);
        }
        form.section(crate::i18n::text("ui-backup-restore"));
        form.actions(
            crate::i18n::text("ui-manual-actions"),
            crate::i18n::text("ui-create-a-backup-now-or-restore-an-existing-one"),
            vec![
                (crate::i18n::text("ui-back-up-now"), Action::Change(Event::CreateBackup)),
                (crate::i18n::text("ui-restore-from-file"), Action::Change(Event::RestoreBackup)),
            ],
        );
        form.link(
            crate::i18n::text("ui-manage-backups"),
            crate::i18n::text("ui-view-restore-export-or-delete-existing-backups"),
            Action::Page(9),
        );
        form.section(crate::i18n::text("ui-automatic-backups"));
        form.toggle(
            crate::i18n::text("ui-automatic-backups"),
            crate::i18n::text("ui-save-settings-and-layouts-at-the-chosen-interval"),
            policy.enabled,
            Action::BackupPolicy(0),
        );
        form.combo(
            crate::i18n::text("ui-backup-interval"),
            crate::i18n::text("ui-how-often-automatic-backups-run"),
            &crate::i18n::format("ui-minutes", &[("arg0", format!("{}", policy.minutes))]),
            Action::BackupPolicy(1),
        );
        form.combo(
            crate::i18n::text("ui-keep-automatic-backups"),
            crate::i18n::text("ui-older-automatic-backups-beyond-this-limit-are-removed"),
            &crate::i18n::format("ui-last-backups", &[("arg0", format!("{}", policy.keep))]),
            Action::BackupPolicy(2),
        );
        if let Some(path) = &view.undo {
            form.button(
                crate::i18n::text("ui-undo-this-restore"),
                crate::i18n::text("ui-return-to-the-configuration-before-this-restore"),
                crate::i18n::text("ui-undo-restore"),
                Action::Change(Event::RestoreBackupPath(path.clone())),
            );
        }
        form.link(
            crate::i18n::text("ui-advanced-options"),
            crate::i18n::text("ui-manage-the-configuration-folder-reload-or-export-settings"),
            Action::BackupAdvanced,
        );
    }
    if view.busy {
        for control in &mut s.controls[first_control..] {
            if !matches!(
                control.action,
                Action::BackupPolicy(_)
                    | Action::Page(_)
                    | Action::BackupAdvanced
                    | Action::BackupStatus
            ) {
                control.enabled = false;
            }
        }
    }
}
pub(super) fn backup_history(s: &mut Scene, width: f32, view: &recovery::View, offset: usize) {
    let mut form = SettingsForm::new(s, width, crate::i18n::text("ui-newest-first-manual-backups-are-not-automatically-removed"));
    form.back(crate::i18n::text("ui-back-to-backup-restore"), Action::Page(6));
    form.button(
        crate::i18n::text("ui-backup-folder"),
        crate::i18n::text("ui-view-local-backups-in-file-explorer"),
        crate::i18n::text("ui-open-folder"),
        Action::Change(Event::OpenBackups),
    );
    form.section(crate::i18n::text("ui-backup-history"));
    for record in view.records.iter().skip(offset).take(6) {
        form.button(
            &record.date,
            &format!(
                "{} · {}",
                record.kind,
                folder::size_text(Some(record.bytes), false)
            ),
            crate::i18n::text("ui-manage"),
            Action::BackupRecord(record.path.clone()),
        );
    }
    if view.records.is_empty() {
        form.info(crate::i18n::text("ui-no-backups"), crate::i18n::text("ui-your-backups-will-appear-here-after-the-first-one-is-created"));
    }
    if view.records.len() > 6 {
    form.pager(
        &crate::i18n::format("ui-total", &[("arg0", format!("{}", offset / 6 + 1)), ("arg1", format!("{}", view.records.len().div_ceil(6).max(1))), ("arg2", format!("{}", view.records.len()))]),
        (Action::BackupPage(-1), offset > 0),
        (Action::BackupPage(1), offset + 6 < view.records.len()),
    );
    }
    if view.busy {
        for c in &mut s.controls {
            if matches!(c.action, Action::BackupRecord(_)) {
                c.enabled = false;
            }
        }
    }
}

#[cfg(test)]
pub(super) fn fonts(s: &mut Scene, width: f32, choices: &[String], _offset: usize) {
    fonts_status(s, width, choices, None);
}

pub(super) fn fonts_status(s: &mut Scene, width: f32, choices: &[String], status: Option<&'static str>) {
    let selected = super::super::fonts::family();
    let mut form = SettingsForm::new(s, width, crate::i18n::text("font-description"));
    form.button(
        crate::i18n::text("ui-current-font"),
        &format!("{}\n{}: {}", selected, crate::i18n::text("font-fallback"), crate::i18n::default_font()),
        crate::i18n::text("ui-reset"),
        Action::Font(crate::i18n::default_font().into()),
    );
    form.option(crate::i18n::text("font-search"), Action::FontSearch, false);
    form.section(&format!("{} · {}", crate::i18n::text("ui-available-fonts"), choices.len()));
    for name in choices {
        form.font_option(name, name == &selected);
    }
    if let Some(status) = status {
        form.info(crate::i18n::text(status), "");
    } else if choices.is_empty() {
        form.info(crate::i18n::text("ui-no-fonts-available"), crate::i18n::text("font-search-empty"));
    }


}


pub(super) fn language(s: &mut Scene, width: f32, selected: &str) {
    let mut form = SettingsForm::new(s, width, crate::i18n::text("language-description"));
    for (code, name) in crate::i18n::LANGUAGES {
        let name = if code == "system" { crate::i18n::text("language-system") } else { name };
        form.option(name, Action::Language(code), selected == code);
    }
}


pub(super) fn desktop_defaults(s: &mut Scene, width: f32, mode: layout_defaults::Mode) {
    let bottom = s.cards.iter().map(|r| r.bottom)
        .chain(s.controls.iter().filter(|c| c.bounds.left >= Tokens::content_x()).map(|c| c.bounds.bottom))
        .fold(120.0_f32, f32::max);
    let mut form = SettingsForm::continuation(s, width, bottom + 24.0);
    form.section(crate::i18n::text("ui-icon-arrangement"));
    form.toggle(crate::i18n::text("ui-align-icons-to-grid"),
        crate::i18n::text("desktop-default-align-description"), mode != layout_defaults::Mode::Free,
        Action::LayoutDefaults(mode.toggle_align()));
    form.toggle(crate::i18n::text("ui-auto-arrange-icons"),
        crate::i18n::text("desktop-default-arrange-description"), mode == layout_defaults::Mode::Compact,
        Action::LayoutDefaults(mode.toggle_auto_arrange()));
}

pub(super) fn show_panels_shortcut(s: &mut Scene, width: f32, enabled: bool, shortcut: search_hotkey::Shortcut) {
    // Append below the existing panel-layout controls, retaining normal page scrolling.
    let bottom = s.cards.iter().map(|r| r.bottom)
        .chain(s.controls.iter().filter(|c| c.bounds.left >= Tokens::content_x()).map(|c| c.bounds.bottom))
        .fold(120.0_f32, f32::max);
    let mut form = SettingsForm::continuation(s, width, bottom + 24.0);
    form.section(crate::i18n::text("show-panels-hotkey"));
    form.toggle(crate::i18n::text("show-panels-enable"), crate::i18n::text("show-panels-description"),
        enabled, Action::ShowPanelsEnable);
    let status = show_hotkey::status();
    form.shortcut(crate::i18n::text("ui-global-shortcut"),
        if status.is_empty() { crate::i18n::text("show-panels-disabled") } else { &status },
        &search_hotkey::label(shortcut), Action::ShowPanelsShortcut, Action::ShowPanelsReset);
}
