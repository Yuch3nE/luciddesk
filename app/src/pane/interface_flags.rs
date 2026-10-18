//! Three process-wide display preferences, persisted before their cached value changes.
use luciddesk_storage::WorkspaceStore;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) static TITLE_EMOJI: Flag = Flag::new("pane_title_emoji_color");
pub(super) static COMPACT_MENU: Flag = Flag::new("pane_compact_menu");
pub(super) static HEADER_DIVIDER: Flag = Flag::new("pane_header_divider");

pub(super) struct Flag { key: &'static str, value: AtomicBool }
impl Flag {
    const fn new(key: &'static str) -> Self { Self { key, value: AtomicBool::new(true) } }
    pub fn get(&self) -> bool { self.value.load(Ordering::Relaxed) }
    pub fn load(&self, store: &WorkspaceStore) -> Result<(), String> {
        let value = store.preference(self.key).map_err(|e| e.to_string())?;
        self.value.store(value.as_deref() != Some("false"), Ordering::Relaxed);
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
        struct Restore([bool; 3]);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (flag, value) in [&TITLE_EMOJI, &COMPACT_MENU, &HEADER_DIVIDER].into_iter().zip(self.0) {
                    flag.value.store(value, Ordering::Relaxed);
                }
            }
        }
        let _restore = Restore([TITLE_EMOJI.get(), COMPACT_MENU.get(), HEADER_DIVIDER.get()]);
        let state = Rc::new(RefCell::new(test_state()));
        let id = PanelId::new(1);
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
        for (event, flag) in [(Event::SetTitleEmojiColor(false), &TITLE_EMOJI),
            (Event::ToggleCompactMenu, &COMPACT_MENU), (Event::ToggleHeaderDivider, &HEADER_DIVIDER)] {
            assert!(!events::handle(&state, id, event).unwrap());
            assert!(!flag.get());
            assert_eq!(state.borrow().store.preference(flag.key).unwrap().as_deref(), Some("false"));
        }
        events::handle(&state, id, Event::ResetPaneOptions).unwrap();
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
