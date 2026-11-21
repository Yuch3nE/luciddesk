//! State transitions shared by native and composited controls.
use std::time::{Duration, Instant};
use std::{collections::HashMap, hash::Hash};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
};

const DURATION: Duration = Duration::from_millis(90);
const SELECTION_DURATION: Duration = Duration::from_millis(160);
const TURN_DURATION: Duration = Duration::from_millis(850);
pub const ACTIVITY_GLYPH: &str = "\u{e72c}";

pub fn animations_enabled() -> bool {
    let mut enabled = 1i32;
    unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0) };
    enabled != 0
}

#[derive(Default)]
struct Amount {
    from: f32,
    target: f32,
    started: Option<Instant>,
}

#[derive(Default)]
pub struct PointerMotion(Amount);

impl PointerMotion {
    pub fn frame(
        &mut self,
        hovered: bool,
        pressed: bool,
        now: Instant,
        animate: bool,
    ) -> (f32, bool) {
        let target = if pressed {
            0.65
        } else {
            f32::from(u8::from(hovered))
        };
        self.0.frame(target, now, animate, DURATION)
    }
}

impl Amount {
    fn frame(
        &mut self,
        target: f32,
        now: Instant,
        animate: bool,
        duration: Duration,
    ) -> (f32, bool) {
        if !animate {
            self.started = None;
            self.target = target;
            return (target, false);
        }
        let (value, playing) = self.sample(now, duration);
        if self.target != target {
            self.from = value;
            self.target = target;
            self.started = (value != target).then_some(now);
            (value, self.started.is_some())
        } else {
            if !playing {
                self.started = None;
            }
            (value, playing)
        }
    }

    fn sample(&self, now: Instant, duration: Duration) -> (f32, bool) {
        let Some(started) = self.started else {
            return (self.target, false);
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= duration {
            return (self.target, false);
        }
        let remaining = 1.0 - elapsed.as_secs_f32() / duration.as_secs_f32();
        (
            self.target + (self.from - self.target) * remaining.powi(2),
            true,
        )
    }
}

#[derive(Default)]
pub struct SelectionMotion(Amount);

impl SelectionMotion {
    pub fn settled(selected: bool) -> Self {
        Self(Amount {
            target: f32::from(u8::from(selected)),
            ..Amount::default()
        })
    }

    pub fn frame(&mut self, selected: bool, now: Instant, animate: bool) -> (f32, bool) {
        self.0.frame(
            f32::from(u8::from(selected)),
            now,
            animate,
            SELECTION_DURATION,
        )
    }
}

/// Retains feedback for stable control identities while they are visible.
pub struct Highlights<K> {
    controls: HashMap<K, HighlightTrack>,
    initialized: bool,
}

#[derive(Default)]
struct HighlightTrack {
    hover: PointerMotion,
    selection: SelectionMotion,
    visited: bool,
}

#[derive(Default)]
pub struct Highlight {
    pub hover: f32,
    pub selected: f32,
    pub playing: bool,
}

impl<K> Default for Highlights<K> {
    fn default() -> Self {
        Self {
            controls: HashMap::new(),
            initialized: false,
        }
    }
}

impl<K: Eq + Hash + Clone> Highlights<K> {
    pub fn sample(
        &mut self,
        key: &K,
        hovered: bool,
        pressed: bool,
        selected: bool,
        now: Instant,
        animate: bool,
    ) -> Highlight {
        let track = if let Some(track) = self.controls.get_mut(key) {
            track
        } else if hovered || pressed || selected {
            self.controls
                .entry(key.clone())
                .or_insert_with(|| HighlightTrack {
                    selection: SelectionMotion::settled(selected && !self.initialized),
                    ..HighlightTrack::default()
                })
        } else {
            return Highlight::default();
        };
        let (hover, pointer_playing) = track.hover.frame(hovered, pressed, now, animate);
        let (selected, selection_playing) = track.selection.frame(selected, now, animate);
        let playing = pointer_playing || selection_playing;
        track.visited = playing || hover > 0.0 || selected > 0.0;
        Highlight {
            hover,
            selected,
            playing,
        }
    }

    pub fn finish_frame(&mut self) {
        self.controls
            .retain(|_, control| std::mem::take(&mut control.visited));
        self.initialized = true;
    }
}

#[derive(Default)]
pub struct SliderMotion {
    hover: PointerMotion,
    press: Amount,
}

#[derive(Clone, Copy)]
pub struct SliderThumb {
    pub outer: f32,
    pub inner: f32,
}

impl Default for SliderThumb {
    fn default() -> Self {
        Self {
            outer: 8.0,
            inner: 5.0,
        }
    }
}

impl SliderMotion {
    pub fn frame(
        &mut self,
        hovered: bool,
        pressed: bool,
        now: Instant,
        animate: bool,
    ) -> (SliderThumb, bool) {
        let (hover, hover_playing) = self.hover.frame(hovered || pressed, false, now, animate);
        let (press, press_playing) =
            self.press
                .frame(f32::from(u8::from(pressed)), now, animate, DURATION);
        let mut thumb = SliderThumb::default();
        thumb.outer += 1.5 * hover;
        thumb.inner -= press;
        (thumb, hover_playing || press_playing)
    }
}

#[derive(Default)]
pub struct ActivityMotion(Option<Instant>);

impl ActivityMotion {
    pub fn frame(&mut self, active: bool, now: Instant, animate: bool) -> Option<f32> {
        if !active || !animate {
            self.0 = None;
            return active.then_some(0.0);
        }
        let start = *self.0.get_or_insert(now);
        let phase =
            now.saturating_duration_since(start).as_secs_f32() % TURN_DURATION.as_secs_f32();
        Some(phase / TURN_DURATION.as_secs_f32() * std::f32::consts::TAU)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_feedback_reverses_at_the_current_amount_and_honors_reduced_motion() {
        let start = Instant::now();
        let mut motion = PointerMotion::default();
        assert_eq!(motion.frame(true, false, start, true), (0.0, true));
        let midway = start + DURATION / 2;
        let (amount, playing) = motion.frame(true, false, midway, true);
        assert!(playing && amount > 0.0 && amount < 1.0);
        assert_eq!(motion.frame(false, false, midway, true), (amount, true));
        assert_eq!(
            motion.frame(false, false, midway + DURATION, true),
            (0.0, false)
        );
        assert_eq!(motion.frame(true, false, midway, false), (1.0, false));
        assert_eq!(motion.frame(true, true, midway, false), (0.65, false));
    }

    #[test]
    fn selection_and_hover_follow_the_control_when_rows_move() {
        let start = Instant::now();
        let mut controls = Highlights::default();
        assert_eq!(
            controls
                .sample(&"first", false, false, true, start, true)
                .selected,
            1.0
        );
        controls.finish_frame();

        let leaving = controls.sample(&"first", false, false, false, start, true);
        let entering = controls.sample(&"second", true, false, true, start, true);
        assert!(leaving.playing && entering.playing);
        assert_eq!(
            (leaving.selected, entering.selected, entering.hover),
            (1.0, 0.0, 0.0)
        );
        controls.finish_frame();

        let now = start + DURATION / 2;
        let second = controls.sample(&"second", true, false, true, now, true);
        assert!(second.playing && second.selected > 0.0 && second.selected < 1.0);
        assert!(second.hover > 0.0 && second.hover < 1.0);
        let replacement = controls.sample(&"replacement", false, false, false, now, true);
        assert_eq!(
            (replacement.hover, replacement.selected, replacement.playing),
            (0.0, 0.0, false)
        );
        controls.finish_frame();
        assert!(!controls.controls.contains_key("first"));
        assert_eq!(
            controls
                .sample(&"second", true, false, true, now, false)
                .selected,
            1.0
        );
    }

    #[test]
    fn slider_feedback_releases_without_moving_the_thumb_value() {
        let start = Instant::now();
        let mut motion = SliderMotion::default();
        let (initial, playing) = motion.frame(false, true, start, true);
        assert!(playing);
        assert_eq!((initial.outer, initial.inner), (8.0, 5.0));
        let (pressed, playing) = motion.frame(false, true, start + DURATION, true);
        assert!(!playing);
        assert_eq!((pressed.outer, pressed.inner), (9.5, 4.0));
        let (releasing, playing) = motion.frame(false, false, start + DURATION, true);
        assert!(playing);
        assert_eq!(
            (releasing.outer, releasing.inner),
            (pressed.outer, pressed.inner)
        );
        let (restored, playing) = motion.frame(false, false, start + DURATION, false);
        assert!(!playing);
        assert_eq!((restored.outer, restored.inner), (8.0, 5.0));
    }

    #[test]
    fn activity_starts_and_stops_with_busy_state_and_system_motion() {
        let start = Instant::now();
        let mut activity = ActivityMotion::default();
        assert_eq!(activity.frame(true, start, true), Some(0.0));
        assert_eq!(
            activity.frame(true, start + TURN_DURATION / 2, true),
            Some(std::f32::consts::PI)
        );
        assert_eq!(
            activity.frame(true, start + TURN_DURATION / 2, false),
            Some(0.0)
        );
        assert_eq!(activity.frame(false, start + TURN_DURATION, true), None);
        assert_eq!(activity.frame(true, start + TURN_DURATION, true), Some(0.0));
    }
}
