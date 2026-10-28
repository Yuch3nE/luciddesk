//! Process-wide display preferences, persisted before their cached value changes.
use luciddesk_storage::WorkspaceStore;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) static TITLE_EMOJI: Flag = Flag::new("pane_title_emoji_color", true);
pub(super) static COMPACT_MENU: Flag = Flag::new("pane_compact_menu", true);
// Defaults for newly created panels; rendering reads each panel's IconDisplay.
pub(super) static ICON_ONLY: Flag = Flag::new("pane_icon_only", false);
pub(super) static TAB_LEFT_ALIGNED: Flag = Flag::new("pane_tab_left_aligned", false);
pub(super) static HEADER_DIVIDER: Flag = Flag::new("pane_header_divider", true);

pub(super) static PANEL_SHADOW: Flag = Flag::new("pane_shadow", false);
pub(super) use luciddesk_core::IconSize;
static ICON_SIZE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
pub(super) fn icon_size() -> IconSize {
    match ICON_SIZE.load(Ordering::Relaxed) { 0 => IconSize::Small, 1 => IconSize::Medium, _ => IconSize::Large }
}
pub(super) fn read_icon_size(store: &WorkspaceStore) -> Result<IconSize, String> {
    Ok(store.preference("pane_icon_size").map_err(|e| e.to_string())?
        .as_deref().and_then(IconSize::parse).unwrap_or(IconSize::Large))
}
pub(super) fn load_icon_size(store: &WorkspaceStore) -> Result<(), String> {
    ICON_SIZE.store(read_icon_size(store)? as u8, Ordering::Relaxed);
    Ok(())
}
pub(super) fn save_icon_size(store: &WorkspaceStore, value: IconSize) -> Result<(), String> {
    store.save_preference("pane_icon_size", value.name()).map_err(|e| e.to_string())?;
    ICON_SIZE.store(value as u8, Ordering::Relaxed);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum SystemCorners { Square, Round, Small }
impl SystemCorners {
    pub fn name(self) -> &'static str {
        match self { Self::Square => "square", Self::Round => "round", Self::Small => "small" }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value { "square" => Some(Self::Square), "round" => Some(Self::Round), "small" => Some(Self::Small), _ => None }
    }
}
static SYSTEM_CORNERS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);
pub(super) fn system_corners() -> SystemCorners {
    match SYSTEM_CORNERS.load(Ordering::Relaxed) { 0 => SystemCorners::Square, 2 => SystemCorners::Small, _ => SystemCorners::Round }
}
pub(super) fn read_system_corners(store: &WorkspaceStore) -> Result<SystemCorners, String> {
    if let Some(value) = store.preference("pane_system_corners").map_err(|e| e.to_string())? {
        return Ok(SystemCorners::parse(&value).unwrap_or(SystemCorners::Round));
    }
    // Preserve the previous two-choice preference in existing development profiles.
    Ok(if store.preference("pane_small_corners").map_err(|e| e.to_string())?.as_deref() == Some("true") {
        SystemCorners::Small
    } else { SystemCorners::Round })
}
pub(super) fn load_system_corners(store: &WorkspaceStore) -> Result<(), String> {
    SYSTEM_CORNERS.store(read_system_corners(store)? as u8, Ordering::Relaxed);
    Ok(())
}
pub(super) fn save_system_corners(store: &WorkspaceStore, value: SystemCorners) -> Result<(), String> {
    store.save_preference("pane_system_corners", value.name()).map_err(|e| e.to_string())?;
    SYSTEM_CORNERS.store(value as u8, Ordering::Relaxed);
    Ok(())
}

/// All cached display preferences read by settings pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DisplayPreferences {
    title_emoji: bool,
    compact_menu: bool,
    header_divider: bool,
    tab_left_aligned: bool,
    panel_shadow: bool,
    icon_only: bool,
    icon_size: IconSize,
    system_corners: SystemCorners,
}
pub(super) fn display_preferences() -> DisplayPreferences {
    DisplayPreferences {
        title_emoji: TITLE_EMOJI.get(),
        compact_menu: COMPACT_MENU.get(),
        header_divider: HEADER_DIVIDER.get(),
        tab_left_aligned: TAB_LEFT_ALIGNED.get(),
        panel_shadow: PANEL_SHADOW.get(),
        icon_only: ICON_ONLY.get(),
        icon_size: icon_size(),
        system_corners: system_corners(),
    }
}

// In shadow mode DWM clips the whole HWND; fill rectangular content underneath
// so a saved custom radius cannot leave holes inside the system's rounded edge.
pub(super) fn pane_corner_radius(custom: f32) -> f32 {
    if PANEL_SHADOW.get() { 0.0 } else { custom }
}

pub(super) struct Flag { key: &'static str, default: bool, value: AtomicBool }
impl Flag {
    const fn new(key: &'static str, default: bool) -> Self { Self { key, default, value: AtomicBool::new(default) } }
    pub fn get(&self) -> bool { self.value.load(Ordering::Relaxed) }
    pub fn load(&self, store: &WorkspaceStore) -> Result<(), String> {
        let value = store.preference(self.key).map_err(|e| e.to_string())?;
        self.value.store(value.as_deref().map_or(self.default, |value| value != "false"), Ordering::Relaxed);
        Ok(())
    }
    pub fn save(&self, store: &WorkspaceStore, value: bool) -> Result<(), String> {
        store.save_preference(self.key, if value { "true" } else { "false" }).map_err(|e| e.to_string())?;
        self.value.store(value, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::pane::{Event, PanelId, events, tests::test_state};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn settings_events_persist_flags_and_reset_the_same_cached_values() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore([bool; 4], SystemCorners);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (flag, value) in [&TITLE_EMOJI, &COMPACT_MENU, &HEADER_DIVIDER, &PANEL_SHADOW].into_iter().zip(self.0) {
                    flag.value.store(value, Ordering::Relaxed);
                }
                SYSTEM_CORNERS.store(self.1 as u8, Ordering::Relaxed);
            }
        }
        let _restore = Restore([TITLE_EMOJI.get(), COMPACT_MENU.get(), HEADER_DIVIDER.get(), PANEL_SHADOW.get()], system_corners());
        let state = Rc::new(RefCell::new(test_state()));
        let id = PanelId::new(1);
        load_system_corners(&state.borrow().store).unwrap();
        assert_eq!(system_corners(), SystemCorners::Round);
        for corners in [SystemCorners::Square, SystemCorners::Small, SystemCorners::Round] {
            events::handle(&state, id, Event::SetSystemCorners(corners)).unwrap();
            SYSTEM_CORNERS.store(99, Ordering::Relaxed);
            load_system_corners(&state.borrow().store).unwrap();
            assert_eq!(system_corners(), corners);
        }
        events::handle(&state, id, Event::SetSystemCorners(SystemCorners::Small)).unwrap();

        PANEL_SHADOW.load(&state.borrow().store).unwrap();
        assert!(!PANEL_SHADOW.get());
        for enabled in [true, false, true] {
            assert!(!events::handle(&state, id, Event::SetSystemRendering(enabled)).unwrap());
            assert_eq!(PANEL_SHADOW.get(), enabled);
            PANEL_SHADOW.value.store(!enabled, Ordering::Relaxed);
            PANEL_SHADOW.load(&state.borrow().store).unwrap();
            assert_eq!(PANEL_SHADOW.get(), enabled);
        }

        crate::pane::snap::save_gap(&state.borrow().store, 32).unwrap();
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
        assert_eq!(crate::pane::snap::gap(), crate::pane::snap::DEFAULT_GAP);
        for (event, flag) in [(Event::SetTitleEmojiColor(false), &TITLE_EMOJI),
            (Event::ToggleCompactMenu, &COMPACT_MENU), (Event::ToggleHeaderDivider, &HEADER_DIVIDER)] {
            assert!(!events::handle(&state, id, event).unwrap());
            assert!(!flag.get());
            assert_eq!(state.borrow().store.preference(flag.key).unwrap().as_deref(), Some("false"));
        }
        crate::pane::snap::save_gap(&state.borrow().store, 32).unwrap();
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
        assert_eq!(crate::pane::snap::gap(), crate::pane::snap::DEFAULT_GAP);
        assert!(PANEL_SHADOW.get());
        assert_eq!(system_corners(), SystemCorners::Round);
        assert_eq!(state.borrow().store.preference("pane_shadow").unwrap().as_deref(), Some("true"));
        for flag in [&TITLE_EMOJI, &COMPACT_MENU, &HEADER_DIVIDER] {
            assert!(flag.get());
            assert_eq!(state.borrow().store.preference(flag.key).unwrap().as_deref(), Some("true"));
        }
    }
}

#[cfg(test)]
mod title_emoji_tests {
    use super::*;
    use super::TITLE_EMOJI as FLAG;

    #[test]
    fn title_emoji_mode_persists_and_changes_rendered_pixels() {
        use crate::pane::{canvas, native_graphics::canvas_result};
        use windows_canvas::{ColorF, GpuDevice, ParagraphAlignment, TextFormat, TextLayout};
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                FLAG.value.store(self.0, Ordering::Relaxed);
            }
        }
        let _restore = Restore(FLAG.get());
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        FLAG.load(&store).unwrap();
        assert!(FLAG.get());
        let device = GpuDevice::new_warp().unwrap();
        let format = TextFormat::new("Segoe UI", 16.0)
            .unwrap()
            .with_paragraph_alignment(ParagraphAlignment::Center);
        canvas::apply_fallback(&format).unwrap();
        let layout = TextLayout::new("📁 AI", &format, 140.0, 40.0).unwrap();
        let surface = canvas::Offscreen::new(&device, 140, 40).unwrap();
        for enabled in [true, false] {
            FLAG.save(&store, enabled).unwrap();
            FLAG.value.store(!enabled, Ordering::Relaxed);
            FLAG.load(&store).unwrap();
            assert_eq!(FLAG.get(), enabled);
            canvas::draw(&surface.target, 1.0, |frame| {
                frame.clear(ColorF::default());
                let brush = canvas_result(frame.create_solid_brush(ColorF::WHITE))?;
                frame.clipped_color_layout("📁 AI", &layout, 0.0, 0.0, &brush)?;
                frame.finish()
            })
            .unwrap();
            let pixels = surface.pixels().unwrap();
            assert!(pixels.chunks_exact(4).any(|p| p[3] > 100));
            assert_eq!(
                pixels.chunks_exact(4).any(|p| p[0] != p[1] || p[1] != p[2]),
                enabled
            );
        }
        assert_eq!(
            store
                .backup_snapshot()
                .unwrap()
                .preference(FLAG.key)
                .unwrap()
                .as_deref(),
            Some("false")
        );
    }
}

#[cfg(test)]
mod compact_menu_tests {
    use super::*;
    use super::COMPACT_MENU as FLAG;
    #[test]
    fn defaults_on_and_persists_disabled() {
        let original = FLAG.get();
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        FLAG.load(&store).unwrap();
        assert!(FLAG.get());
        FLAG.save(&store, false).unwrap();
        FLAG.load(&store).unwrap();
        assert!(!FLAG.get());
        assert_eq!(store.preference(FLAG.key).unwrap().as_deref(), Some("false"));
        FLAG.save(&store, true).unwrap();
        assert!(FLAG.get());
        FLAG.value.store(original, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod header_divider_tests {
    use super::*;
    use super::HEADER_DIVIDER as FLAG;
    #[test]
    fn divider_setting_round_trips_and_changes_rendering_without_layout_changes() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool);
        impl Drop for Restore { fn drop(&mut self) { FLAG.value.store(self.0, Ordering::Relaxed); } }
        let _restore = Restore(FLAG.get());
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        FLAG.load(&store).unwrap(); assert!(FLAG.get());
        let mut state = super::super::tests::test_state();
        state.workspace.set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let model = super::super::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap();
        let mut renderer = super::super::render::Renderer::new().unwrap();
        let shown = renderer.pixels(420, 300, 1.0, &model).unwrap();
        FLAG.save(&store, false).unwrap();
        assert_ne!(shown, renderer.pixels(420, 300, 1.0, &model).unwrap());
        FLAG.value.store(true, Ordering::Relaxed);
        FLAG.load(&store).unwrap(); assert!(!FLAG.get());
        assert_eq!(store.backup_snapshot().unwrap().preference(FLAG.key).unwrap().as_deref(), Some("false"));
    }
}

#[cfg(test)]
mod icon_only_tests {
    use super::*;
    use crate::pane::{create_model, events, render::Renderer, tests::test_state, Event};
    use luciddesk_core::PanelId;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn icon_only_persists_hides_labels_preserves_list_and_resets() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) { ICON_ONLY.value.store(self.0, Ordering::Relaxed); }
        }
        let _restore = Restore(ICON_ONLY.get());
        let state = Rc::new(RefCell::new(test_state()));
        {
            let mut s = state.borrow_mut();
            let workspace = s.workspace.clone();
            s.store.save_workspace(&workspace).unwrap();
        }
        let id = PanelId::new(1);
        ICON_ONLY.load(&state.borrow().store).unwrap();
        assert!(!ICON_ONLY.get());
        let mut model = create_model(&state.borrow(), id).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let grid = model.grid(420.0, 300.0);
        let cell = model.cell(grid, 0);
        let shown = renderer.pixels(420, 300, 1.0, &model).unwrap();
        let display = luciddesk_core::IconDisplay { icon_only: true, size: IconSize::Large };
        events::handle(&state, id, Event::SetPanelIconDisplay(id, display)).unwrap();
        assert_eq!(state.borrow().store.load_workspace().unwrap().panel(id).unwrap().icon_display(), display);
        model.icon_display = display;
        assert_eq!(cell, model.cell(model.grid(420.0, 300.0), 0));
        let grid = model.grid(420.0, 300.0);
        assert_eq!(grid.cell_width, grid.cell_height);
        assert_eq!(grid.cell_width, 88.0);
        assert_eq!(model.selection_bounds(grid, 0, 1.0).height, 84.0);
        assert_eq!(model.hit(grid, cell.0 + grid.cell_width / 2.0, cell.1 + 10.0, 1.0), Some(0));
        let hidden = renderer.pixels(420, 300, 1.0, &model).unwrap();
        assert_ne!(shown, hidden);
        let image = crate::pane::assets::Pixels { width: 1, height: 1, data: vec![20, 40, 80, 255] };
        for scale in [1.0, 1.25, 2.0] {
            let preview = crate::pane::drag_drop::image::item_pixels(&image, "filename", grid, scale).unwrap();
            let renamed = crate::pane::drag_drop::image::item_pixels(&image, "other name", grid, scale).unwrap();
            assert_eq!(preview.data, renamed.data);
            assert_eq!(preview.data.len(), (preview.width * preview.height * 4) as usize);
            assert_eq!(preview.height, (grid.cell_height * scale).ceil() as u32);
        }
        model.items[0].label = "A completely different filename.txt".into();
        assert_eq!(hidden, renderer.pixels(420, 300, 1.0, &model).unwrap());
        // Square cells preserve spacing while giving icon-only artwork more room.
        for percent in [50.0, 100.0, 200.0] {
            let mut many = model.clone();
            many.fixed_grid = false;
            many.free_layout = false;
            many.options.grid_scale = percent;
            many.items = vec![many.items[0].clone(); 80];
            many.icon_display.icon_only = false;
            let ordinary = many.grid(420.0, 300.0);
            many.icon_display.icon_only = true;
            let compact = many.grid(420.0, 300.0);
            assert_eq!(compact.cell_width, compact.cell_height);
            assert_eq!(compact.cell_width, 88.0);
            assert_eq!(compact.icon_size, 64.0);
            assert_eq!(compact.columns, 4);
            let index = compact.columns + 1;
            let (x, y) = many.cell(compact, index);
            assert_eq!(many.hit(compact, x + compact.cell_width / 2.0,
                y + compact.cell_height / 2.0, 1.0), Some(index));
            assert_eq!(crate::pane::layout::icon_row_height(compact,
                ["A very long name that must not affect the row height"].into_iter()), compact.cell_height);
            many.icon_display.icon_only = false;
            let restored = many.grid(420.0, 300.0);
            assert_eq!((restored.columns, restored.cell_width, restored.cell_height),
                (ordinary.columns, ordinary.cell_width, ordinary.cell_height));
        }
        // Warm the free-layout cache in both modes; stored coordinates remain untouched.
        let mut free = model.clone();
        free.fixed_grid = true;
        free.free_layout = true;
        free.items.truncate(1);
        let position = luciddesk_core::PointDip::new(500.0, 600.0);
        free.items[0].details.free_position = Some(position);
        free.icon_display.icon_only = false;
        let ordinary = free.grid(420.0, 300.0);
        let original_cell = free.cell(ordinary, 0);
        free.icon_display.icon_only = true;
        let compact = free.grid(420.0, 300.0);
        assert_eq!(free.items[0].details.free_position, Some(position));
        assert_eq!(free.cell(compact, 0), original_cell);
        assert_eq!(compact.horizontal_limit, ordinary.horizontal_limit);
        assert_ne!(compact.scroll_limit, ordinary.scroll_limit);
        free.icon_display.icon_only = false;
        let restored = free.grid(420.0, 300.0);
        assert_eq!(restored.horizontal_limit, ordinary.horizontal_limit);
        assert_eq!(restored.scroll_limit, ordinary.scroll_limit);
        model.list_view = true;
        let list = renderer.pixels(420, 300, 1.0, &model).unwrap();
        model.icon_display.icon_only = false;
        assert_eq!(list, renderer.pixels(420, 300, 1.0, &model).unwrap());
        model.icon_display.icon_only = true;
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
        assert!(!ICON_ONLY.get());
        ICON_ONLY.load(&state.borrow().store).unwrap();
        assert!(!ICON_ONLY.get());
        assert_eq!(state.borrow().workspace.panel(id).unwrap().icon_display(), display);
    }
}

#[cfg(test)]
mod icon_size_tests {
    use super::*;
    use crate::pane::{create_model, events, layout, render::Renderer, tests::test_state, Event};
    use luciddesk_core::PanelId;
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn icon_sizes_persist_scale_and_preserve_cells_and_normal_mode() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool, IconSize);
        impl Drop for Restore {
            fn drop(&mut self) {
                ICON_ONLY.value.store(self.0, Ordering::Relaxed);
                ICON_SIZE.store(self.1 as u8, Ordering::Relaxed);
            }
        }
        let _restore = Restore(ICON_ONLY.get(), icon_size());
        let state = Rc::new(RefCell::new(test_state()));
        {
            let mut s = state.borrow_mut();
            let workspace = s.workspace.clone();
            s.store.save_workspace(&workspace).unwrap();
        }
        let id = PanelId::new(1);
        load_icon_size(&state.borrow().store).unwrap();
        assert_eq!(icon_size(), IconSize::Large);
        ICON_ONLY.save(&state.borrow().store, true).unwrap();
        let mut model = create_model(&state.borrow(), id).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let mut previous = None;
        for (size, expected) in [(IconSize::Small, 48.0), (IconSize::Medium, 56.0), (IconSize::Large, 64.0)] {
            let display = luciddesk_core::IconDisplay { icon_only: true, size };
            events::handle(&state, id, Event::SetPanelIconDisplay(id, display)).unwrap();
            model.icon_display = display;
            assert_eq!(state.borrow().store.load_workspace().unwrap().panel(id).unwrap().icon_display(), display);
            for percent in [50.0, 100.0, 200.0] {
                let grid = layout::desktop_grid(420.0, 300.0, 48.0, percent, display);
                assert_eq!((grid.cell_width, grid.cell_height), (88.0, 88.0));
                assert_eq!(grid.icon_size, expected);
                assert_eq!(grid.icon_top(), (grid.cell_height - grid.icon_size) / 2.0);
            }
            let pixels = renderer.pixels(420, 300, 1.0, &model).unwrap();
            if let Some(previous) = previous { assert_ne!(pixels, previous); }
            previous = Some(pixels);
            ICON_ONLY.save(&state.borrow().store, false).unwrap();
            let ordinary = layout::desktop_grid(420.0, 300.0, 48.0, 100.0, luciddesk_core::IconDisplay::DEFAULT);
            assert_eq!((ordinary.icon_size, ordinary.cell_width, ordinary.cell_height), (48.0, 88.0, 96.0));
            ICON_ONLY.save(&state.borrow().store, true).unwrap();
        }
        events::handle(&state, id, Event::SetIconSize(IconSize::Small)).unwrap();
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
        assert_eq!(icon_size(), IconSize::Large);
        assert!(!ICON_ONLY.get());
        assert!(IconSize::parse("invalid").is_none());
        state.borrow().store.save_preference("pane_icon_size", "invalid").unwrap();
        load_icon_size(&state.borrow().store).unwrap();
        assert_eq!(icon_size(), IconSize::Large);
    }
}

#[cfg(test)]
mod display_cache_tests {
    use super::*;

    #[test]
    fn settings_snapshot_tracks_each_display_preference() {
        let baseline = display_preferences();
        for flag in [&TITLE_EMOJI, &COMPACT_MENU, &HEADER_DIVIDER, &PANEL_SHADOW, &ICON_ONLY, &TAB_LEFT_ALIGNED] {
            let original = flag.get();
            flag.value.store(!original, Ordering::Relaxed);
            let changed = display_preferences();
            flag.value.store(original, Ordering::Relaxed);
            assert_ne!(changed, baseline);
            assert_eq!(display_preferences(), baseline);
        }
        let original = icon_size();
        let alternate = if original == IconSize::Small { IconSize::Large } else { IconSize::Small };
        ICON_SIZE.store(alternate as u8, Ordering::Relaxed);
        let changed = display_preferences();
        ICON_SIZE.store(original as u8, Ordering::Relaxed);
        assert_ne!(changed, baseline);
        let original = system_corners();
        let alternate = if original == SystemCorners::Square { SystemCorners::Round } else { SystemCorners::Square };
        SYSTEM_CORNERS.store(alternate as u8, Ordering::Relaxed);
        let changed = display_preferences();
        SYSTEM_CORNERS.store(original as u8, Ordering::Relaxed);
        assert_ne!(changed, baseline);
        assert_eq!(display_preferences(), baseline);
    }
}

#[cfg(test)]
mod panel_icon_defaults_tests {
    use super::*;
    use crate::pane::{create_model, events, tests::test_state, Event};
    use luciddesk_core::{IconDisplay, Panel, PanelId, RectDip};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn panel_icons_are_independent_from_defaults_reset_and_other_tabs() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool, IconSize);
        impl Drop for Restore {
            fn drop(&mut self) {
                ICON_ONLY.value.store(self.0, Ordering::Relaxed);
                ICON_SIZE.store(self.1 as u8, Ordering::Relaxed);
            }
        }
        let _restore = Restore(ICON_ONLY.get(), icon_size());
        let state = Rc::new(RefCell::new(test_state()));
        {
            let mut s = state.borrow_mut();
            let workspace = s.workspace.clone();
            s.store.save_workspace(&workspace).unwrap();
        }
        let first = PanelId::new(1);
        let second = PanelId::new(101);
        state.borrow_mut().workspace.add_panel(Panel::new(second, "Second", RectDip::default())).unwrap();
        let original = state.borrow().workspace.panel(first).unwrap().icon_display();
        let custom = IconDisplay { icon_only: true, size: IconSize::Small };
        let before = state.borrow().store.change_count();
        events::handle(&state, first, Event::SetPanelIconDisplay(first, custom)).unwrap();
        assert_eq!(state.borrow().store.change_count(), before + 1);
        assert_eq!(state.borrow().workspace.panel(second).unwrap().icon_display(), original);
        let other = create_model(&state.borrow(), second).unwrap();
        let mut model = create_model(&state.borrow(), first).unwrap();
        assert!(model.grid(420.0, 300.0).icon_only);
        assert!(!other.grid(420.0, 300.0).icon_only);
        crate::pane::tabs::decorate(&state.borrow().workspace, second, &mut model);
        assert_eq!(model.icon_display, original);
        crate::pane::tabs::decorate(&state.borrow().workspace, first, &mut model);
        assert_eq!(model.icon_display, custom);
        ICON_ONLY.save(&state.borrow().store, false).unwrap();
        events::handle(&state, first, Event::ToggleIconOnly).unwrap();
        events::handle(&state, first, Event::SetIconSize(IconSize::Medium)).unwrap();
        let third = PanelId::new(102);
        state.borrow_mut().workspace.add_panel(Panel::new(third, "Third", RectDip::default())).unwrap();
        assert_eq!(state.borrow().workspace.panel(third).unwrap().icon_display(), IconDisplay { icon_only: true, size: IconSize::Medium });
        events::handle(&state, first, Event::ResetPaneOptions).unwrap();
        assert_eq!(state.borrow().workspace.panel(first).unwrap().icon_display(), custom);
        assert_eq!(state.borrow().workspace.panel(second).unwrap().icon_display(), original);
        assert_eq!(state.borrow().workspace.panel(third).unwrap().icon_display(), IconDisplay { icon_only: true, size: IconSize::Medium });
    }
}

#[cfg(test)]
mod tab_alignment_tests {
    use super::*;
    use crate::pane::{Event, PanelId, create_model, events, tabs, tests::test_state};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn tab_alignment_persists_renders_and_resets_without_changing_selection() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) { TAB_LEFT_ALIGNED.value.store(self.0, Ordering::Relaxed); }
        }
        let _restore = Restore(TAB_LEFT_ALIGNED.get());
        let state = Rc::new(RefCell::new(test_state()));
        TAB_LEFT_ALIGNED.load(&state.borrow().store).unwrap();
        assert!(!TAB_LEFT_ALIGNED.get());
        let mut model = create_model(&state.borrow(), PanelId::new(1)).unwrap();
        model.tabs = vec![(PanelId::new(1), "工作".into()), (PanelId::new(2), "Notes".into())];
        model.active_tab = PanelId::new(1);
        model.select_item(0, false, false);
        let selection = model.selection.clone();
        let mut renderer = crate::pane::render::Renderer::new().unwrap();
        let filled = renderer.pixels(600, 240, 1.0, &model).unwrap();
        let filled_geometry = tabs::strip(&model, 600.0);
        for button in 0..2 {
            let x = model.header_button_x(600.0, button) + 14.0;
            assert!(!model.header_button_enabled(button));
            assert_eq!(model.header_button(600.0, x, 15.0), None);
        }
        events::handle(&state, PanelId::new(1), Event::SetTabLeftAligned(true)).unwrap();
        assert!(TAB_LEFT_ALIGNED.get());
        assert_eq!(state.borrow().store.preference("pane_tab_left_aligned").unwrap().as_deref(), Some("true"));
        TAB_LEFT_ALIGNED.value.store(false, Ordering::Relaxed);
        TAB_LEFT_ALIGNED.load(&state.borrow().store).unwrap();
        assert!(TAB_LEFT_ALIGNED.get());
        let left = renderer.pixels(600, 240, 1.0, &model).unwrap();
        assert_ne!(filled, left);
        for collapsed in [false, true] {
            model.collapsed = collapsed;
            for width in [90.0, 260.0, 600.0] {
                for button in 0..2 {
                    let x = model.header_button_x(width, button) + 14.0;
                    assert!(model.header_button_enabled(button));
                    assert_eq!(model.header_button(width, x, 15.0), Some(button));
                    assert_eq!(tabs::hit(&model, width, x, 15.0), None);
                }
                for (_, bounds) in tabs::strip(&model, width) {
                    assert!(bounds.x + bounds.width <= model.header_button_x(width, 0));
                }
            }
        }
        model.collapsed = false;
        let geometry = tabs::strip(&model, 600.0);
        let last = geometry.last().unwrap().1;
        assert!(last.x + last.width < 400.0);
        assert_eq!(tabs::hit(&model, 600.0, 500.0, 15.0), None);
        for (id, bounds) in geometry {
            assert_eq!(tabs::hit(&model, 600.0, bounds.x + bounds.width / 2.0, 15.0), Some(id));
        }
        assert_eq!(model.selection, selection);
        assert_eq!(model.active_tab, PanelId::new(1));
        model.tabs.extend([(PanelId::new(3), "网络与远程".into()), (PanelId::new(4), "其他工具".into())]);
        for width in [260.0, 340.0, 600.0] {
            let tabs = tabs::strip(&model, width);
            assert_eq!(tabs.len(), 4);
            for (id, bounds) in tabs {
                let x = bounds.x + bounds.width / 2.0;
                assert_eq!(tabs::hit(&model, width, x, 15.0), Some(id));
                assert_eq!(model.header_button(width, x, 15.0), None);
                assert!(bounds.x + bounds.width <= model.header_button_x(width, 0));
            }
        }
        let four_tabs = renderer.pixels(510, 360, 1.5, &model).unwrap();
        model.hovered_tab = Some(PanelId::new(3));
        let hovered = renderer.pixels(510, 360, 1.5, &model).unwrap();
        assert_ne!(four_tabs, hovered);
        model.dark = false;
        let light = renderer.pixels(510, 360, 1.5, &model).unwrap();
        model.tabs.truncate(2);
        if let Some(directory) = std::env::var_os("LUCIDDESK_TAB_SNAPSHOT_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("tabs-fill.bgra"), filled).unwrap();
            std::fs::write(directory.join("tabs-left.bgra"), left).unwrap();
            std::fs::write(directory.join("tabs-four.bgra"), four_tabs).unwrap();
            std::fs::write(directory.join("tabs-hover.bgra"), hovered).unwrap();
            std::fs::write(directory.join("tabs-light.bgra"), light).unwrap();
            model.tabs.extend([(PanelId::new(3), "网络与远程".into()), (PanelId::new(4), "其他工具".into())]);
            for dark in [false, true] {
                model.dark = dark;
                for (name, material) in [
                    ("acrylic", luciddesk_core::Backdrop::Acrylic),
                    ("mica", luciddesk_core::Backdrop::Mica),
                    ("mica-alt", luciddesk_core::Backdrop::MicaAlt),
                    ("solid", luciddesk_core::Backdrop::solid_default(dark)),
                    ("translucent", luciddesk_core::Backdrop::Translucent { opacity: 0.86 }),
                ] {
                    model.backdrop = material;
                    let pixels = renderer.pixels(510, 360, 1.5, &model).unwrap();
                    std::fs::write(directory.join(format!("{name}-{dark}.bgra")), pixels).unwrap();
                }
            }
            model.tabs.truncate(2);
        }
        events::handle(&state, PanelId::new(1), Event::ResetPaneOptions).unwrap();
        assert!(!TAB_LEFT_ALIGNED.get());
        assert_eq!(tabs::strip(&model, 600.0), filled_geometry);
    }
}
