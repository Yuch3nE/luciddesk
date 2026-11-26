//! Connect scene controls to their animation state.
use super::*;
use crate::pane::animation::{HOVER_DURATION, Motion, SETTINGS_DURATION};
use luciddesk_window::controls::motion::{
    ActivityMotion, PointerMotion, SelectionMotion, SliderMotion, SliderThumb,
};
use std::time::{Duration, Instant};

const PRESS_DURATION: Duration = Duration::from_millis(75);
const SLIDE_DURATION: Duration = SETTINGS_DURATION;
const PAGE_ENTER_DURATION: Duration = Duration::from_millis(135);

#[derive(Clone, Copy, Default)]
pub(super) struct ControlMotion {
    pub hover: Option<f32>,
    pub press: f32,
    pub disclosure: Option<f32>,
    pub check: Option<f32>,
    pub selection: Option<f32>,
    pub slider: Option<SliderThumb>,
}

#[derive(Default)]
pub(super) struct MotionFrame {
    pub controls: HashMap<usize, ControlMotion>,
    pub nav_indicator: Option<Rect>,
    pub segment_pills: HashMap<usize, Rect>,
    pub page_enter: Option<(f32, f32)>,
    pub grip: Option<(usize, f32)>,
    pub scrollbars: [f32; 2],
    pub activity: Option<f32>,
}

pub(super) struct MotionInput {
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub scrollbar: Option<usize>,
    pub now: Instant,
    pub enabled: bool,
}

#[derive(Default)]
struct SlideTrack {
    from: (f32, f32),
    target: (f32, f32),
    started: Option<Instant>,
    initialized: bool,
}

impl SlideTrack {
    fn sample(&self, now: Instant) -> ((f32, f32), f32, bool) {
        let Some(started) = self.started else {
            return (self.target, 1.0, false);
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed >= SLIDE_DURATION {
            return (self.target, 1.0, false);
        }
        let t = (elapsed.as_secs_f32() / SLIDE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let inv = 1.0 - t;
        let eased = 1.0 - inv * inv * inv;
        let value = (
            self.from.0 + (self.target.0 - self.from.0) * eased,
            self.from.1 + (self.target.1 - self.from.1) * eased,
        );
        (value, eased, true)
    }

    fn frame(&mut self, target: (f32, f32), now: Instant, animate: bool) -> ((f32, f32), f32, bool) {
        if !self.initialized || !animate {
            self.initialized = true;
            self.from = target;
            self.target = target;
            self.started = None;
            return (target, 1.0, false);
        }
        let (current, progress, playing) = self.sample(now);
        if (self.target.0 - target.0).abs() > 0.25 || (self.target.1 - target.1).abs() > 0.25 {
            self.from = current;
            self.target = target;
            self.started = Some(now);
            (current, 0.0, true)
        } else {
            if !playing {
                self.started = None;
            }
            (current, progress, playing)
        }
    }
}

#[derive(Default)]
pub(super) struct InterfaceMotion {
    activity: ActivityMotion,
    controls: HashMap<ControlSlot, ControlTrack>,
    nav_indicator: SlideTrack,
    nav_last_scroll: f32,
    segment_tracks: HashMap<usize, (usize, SlideTrack)>,
    page_signature: Option<(Option<usize>, String)>,
    page_enter: Option<Instant>,
    grip: Option<(usize, PointerMotion)>,
    scrollbars: [PointerMotion; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ControlSlot {
    Navigation(usize),
    Content(usize),
}

struct ControlTrack {
    kind: std::mem::Discriminant<ControlKind>,
    action: std::mem::Discriminant<Action>,
    label: String,
    hover: PointerMotion,
    press: Option<Motion>,
    disclosure: Option<Motion>,
    check: Option<Motion>,
    selection: Option<SelectionMotion>,
    slider: SliderMotion,
}

impl ControlTrack {
    fn label(control: &Control) -> &str {
        if let (ControlKind::Combo, Action::Dropdown(options)) = (control.kind, &control.action)
            && let Some((label, _, _)) = options.first()
        {
            return label;
        }
        &control.label
    }

    fn new(control: &Control) -> Self {
        Self {
            kind: std::mem::discriminant(&control.kind),
            action: std::mem::discriminant(&control.action),
            label: Self::label(control).into(),
            hover: PointerMotion::default(),
            press: None,
            disclosure: None,
            check: None,
            selection: None,
            slider: SliderMotion::default(),
        }
    }

    fn matches(&self, control: &Control) -> bool {
        self.kind == std::mem::discriminant(&control.kind)
            && self.action == std::mem::discriminant(&control.action)
            && self.label == Self::label(control)
    }
}

fn track(
    motion: &mut Option<Motion>,
    to: f32,
    initial: f32,
    duration: Duration,
    input: &MotionInput,
    playing: &mut bool,
) -> Option<f32> {
    let value = motion
        .get_or_insert_with(|| Motion::settled(initial, input.now))
        .retarget_with_duration(to, input.now, input.enabled, duration);
    if (value - to).abs() > 0.0001 {
        *playing = true;
        Some(value)
    } else {
        None
    }
}

impl InterfaceMotion {
    pub fn frame(&mut self, scene: &Scene, input: MotionInput) -> (MotionFrame, bool) {
        let mut playing = false;
        let mut frame = MotionFrame::default();
        let mut nav_index = 0;
        for (index, control) in scene.controls.iter().enumerate() {
            let navigation = matches!(
                control.kind,
                ControlKind::Tab(_) | ControlKind::Navigation(_)
            );
            // Content can change length without changing the tabs or sidebar items.
            let slot = if navigation {
                let slot = ControlSlot::Navigation(nav_index);
                nav_index += 1;
                slot
            } else {
                ControlSlot::Content(index)
            };
            let hoverable = control.enabled
                && matches!(
                    control.kind,
                    ControlKind::Button
                        | ControlKind::PrimaryButton
                        | ControlKind::Segment
                        | ControlKind::IconButton(_)
                        | ControlKind::QuietButton(_)
                        | ControlKind::Combo
                        | ControlKind::Checkbox
                        | ControlKind::SectionDisclosure { .. }
                        | ControlKind::Tab(_)
                        | ControlKind::Navigation(_)
                        | ControlKind::Caption
                );
            let disclosure = match control.kind {
                ControlKind::Combo | ControlKind::IconButton(_)
                    if matches!(control.action, Action::Dropdown(_)) =>
                {
                    Some(controls::dropdown_is_open(control.bounds))
                }
                ControlKind::SectionDisclosure { .. } | ControlKind::RuleDisclosure { .. } => {
                    Some(control.selected)
                }
                _ => None,
            };
            let check = matches!(control.kind, ControlKind::Checkbox);
            let slider = matches!(control.kind, ControlKind::Slider(_));
            let row = matches!(control.kind, ControlKind::Row | ControlKind::ForwardRow);
            let selectable = navigation || row || matches!(control.kind, ControlKind::Segment);
            let hoverable = hoverable
                || (control.enabled
                    && matches!(
                        control.kind,
                        ControlKind::Toggle
                            | ControlKind::Slider(_)
                            | ControlKind::Row
                            | ControlKind::ForwardRow
                            | ControlKind::BackButton
                            | ControlKind::FooterLink
                    ));
            if !hoverable && disclosure.is_none() && !check && !selectable {
                self.controls.remove(&slot);
                continue;
            }
            let current = self
                .controls
                .entry(slot)
                .or_insert_with(|| ControlTrack::new(control));
            if !current.matches(control) {
                *current = ControlTrack::new(control);
            }
            let mut style = ControlMotion::default();
            let (hover, transitioning) = current.hover.frame(
                hoverable && input.hover == Some(index),
                hoverable && input.pressed == Some(index),
                input.now,
                input.enabled && hoverable,
            );
            playing |= transitioning;
            style.hover = (transitioning || hover > 0.0).then_some(hover);
            let is_pressed = hoverable && input.pressed == Some(index);
            let press_target = f32::from(u8::from(is_pressed));
            if is_pressed || current.press.is_some() {
                let press_value = current
                    .press
                    .get_or_insert_with(|| Motion::settled(0.0, input.now))
                    .retarget_with_duration(press_target, input.now, input.enabled, PRESS_DURATION);
                let press_moving = (press_value - press_target).abs() > 0.0001;
                playing |= press_moving;
                style.press = press_value;
                if !press_moving && press_target == 0.0 {
                    current.press = None;
                }
            }
            if let Some(open) = disclosure {
                let target = u8::from(open) as f32;
                style.disclosure = track(
                    &mut current.disclosure,
                    target,
                    target,
                    SETTINGS_DURATION,
                    &input,
                    &mut playing,
                );
            }
            if check {
                let target = u8::from(control.selected) as f32;
                style.check = track(
                    &mut current.check,
                    target,
                    target,
                    HOVER_DURATION,
                    &input,
                    &mut playing,
                );
            }
            if selectable {
                let selected = control.selected && control.enabled;
                let (fraction, transitioning) = current
                    .selection
                    .get_or_insert_with(|| SelectionMotion::settled(selected))
                    .frame(selected, input.now, input.enabled);
                playing |= transitioning;
                style.selection = transitioning.then_some(fraction);
            }
            if slider && control.enabled {
                let (thumb, transitioning) = current.slider.frame(
                    input.hover == Some(index),
                    input.pressed == Some(index),
                    input.now,
                    input.enabled,
                );
                playing |= transitioning;
                style.slider = Some(thumb);
            }
            if style.hover.is_some()
                || style.press > 0.0
                || style.disclosure.is_some()
                || style.check.is_some()
                || style.selection.is_some()
                || style.slider.is_some()
            {
                frame.controls.insert(index, style);
            }
        }
        self.controls.retain(|slot, state| match *slot {
            ControlSlot::Navigation(index) => index < nav_index,
            ControlSlot::Content(index) => scene
                .controls
                .get(index)
                .is_some_and(|control| state.matches(control)),
        });

        // Sliding vertical indicator for the sidebar navigation rail.
        if let Some((selected_nav_index, selected_nav)) = scene
            .controls
            .iter()
            .enumerate()
            .find(|(_, control)| matches!(control.kind, ControlKind::Navigation(_)) && control.selected && control.enabled)
        {
            let scroll_delta = scene.navigation_offset - self.nav_last_scroll;
            self.nav_last_scroll = scene.navigation_offset;
            if scroll_delta.abs() > 0.01 && self.nav_indicator.initialized {
                self.nav_indicator.from.0 -= scroll_delta;
                self.nav_indicator.target.0 -= scroll_delta;
            }
            let center_y = (selected_nav.bounds.top + selected_nav.bounds.bottom) * 0.5;
            let press = frame.controls.get(&selected_nav_index).map_or(0.0, |m| m.press);
            let ((y, _), progress, moving) = self.nav_indicator.frame((center_y, 0.0), input.now, input.enabled);
            playing |= moving;
            let travel_stretch = if moving {
                let arc = (progress * std::f32::consts::PI).sin();
                (self.nav_indicator.target.0 - self.nav_indicator.from.0).abs().min(40.0) * 0.18 * arc
            } else {
                0.0
            };
            let height = (16.0 + travel_stretch - 2.0 * press).max(12.0);
            frame.nav_indicator = Some(Rect::from_xywh(
                selected_nav.bounds.left + 3.0,
                y - height * 0.5,
                3.0,
                height,
            ));
        } else {
            self.nav_indicator = SlideTrack::default();
        }

        // Subtle upward entrance when switching between settings pages.
        if scene.navigation_viewport.is_some() {
            let active_nav = scene
                .controls
                .iter()
                .position(|c| matches!(c.kind, ControlKind::Navigation(_)) && c.selected);
            let title = scene
                .text
                .iter()
                .find(|(_, _, style)| *style == 3)
                .map(|(_, text, _)| text.clone())
                .unwrap_or_default();
            let signature = (active_nav, title);
            if let Some(previous) = &self.page_signature {
                if *previous != signature {
                    self.page_signature = Some(signature);
                    self.segment_tracks.clear();
                    self.page_enter = input.enabled.then_some(input.now);
                }
            } else {
                self.page_signature = Some(signature);
            }
            if let Some(started) = self.page_enter {
                if !input.enabled {
                    self.page_enter = None;
                } else {
                    let elapsed = input.now.saturating_duration_since(started);
                    if elapsed >= PAGE_ENTER_DURATION {
                        self.page_enter = None;
                    } else {
                        let t = (elapsed.as_secs_f32() / PAGE_ENTER_DURATION.as_secs_f32()).clamp(0.0, 1.0);
                        let inv = 1.0 - t;
                        let eased = 1.0 - inv * inv * inv;
                        frame.page_enter = Some((8.0 * (1.0 - eased), 0.55 + 0.45 * eased));
                        playing = true;
                    }
                }
            }
        }

        // Sliding horizontal pill for each segmented control track.
        let mut active_tracks = Vec::new();
        for (track_index, track) in scene.segment_tracks.iter().enumerate() {
            let mut count = 0usize;
            let mut selected_rect = None;
            for control in &scene.controls {
                if matches!(control.kind, ControlKind::Segment)
                    && control.bounds.left >= track.left
                    && control.bounds.right <= track.right
                    && control.bounds.top >= track.top
                    && control.bounds.bottom <= track.bottom
                {
                    count += 1;
                    if control.selected {
                        selected_rect = Some(control.bounds);
                    }
                }
            }
            if let Some(bounds) = selected_rect {
                let target = (bounds.left - track.left, bounds.right - bounds.left);
                let entry = self
                    .segment_tracks
                    .entry(track_index)
                    .or_insert_with(|| (count, SlideTrack::default()));
                if entry.0 != count {
                    *entry = (count, SlideTrack::default());
                }
                let ((offset_x, width), _, moving) = entry.1.frame(target, input.now, input.enabled);
                playing |= moving;
                frame.segment_pills.insert(
                    track_index,
                    Rect::from_xywh(
                        track.left + offset_x,
                        bounds.top,
                        width,
                        bounds.bottom - bounds.top,
                    ),
                );
                active_tracks.push(track_index);
            }
        }
        self.segment_tracks.retain(|index, _| active_tracks.contains(index));
        if let Some(index) = scene.rule_grip_hover
            && self.grip.as_ref().is_none_or(|(old, _)| *old != index)
        {
            self.grip = Some((index, PointerMotion::default()));
        }
        if let Some((index, motion)) = &mut self.grip {
            let (hover, transitioning) = motion.frame(
                scene.rule_grip_hover == Some(*index),
                false,
                input.now,
                input.enabled,
            );
            playing |= transitioning;
            frame.grip = Some((*index, hover));
        }
        for (index, slot) in self.scrollbars.iter_mut().enumerate() {
            let (hover, transitioning) = slot.frame(
                input.scrollbar == Some(index),
                false,
                input.now,
                input.enabled,
            );
            frame.scrollbars[index] = hover;
            playing |= transitioning;
        }
        frame.activity = self
            .activity
            .frame(scene.activity.is_some(), input.now, input.enabled);
        playing |= frame.activity.is_some() && input.enabled;
        (frame, playing)
    }
}

pub(super) fn scrollbar_at(scene: &Scene, x: f32, y: f32) -> Option<usize> {
    [
        (scene.scrollbar(), scene.viewport),
        (scene.navigation_scrollbar(), scene.navigation_viewport),
    ]
    .into_iter()
    .position(|(bar, viewport)| {
        bar.is_some()
            && viewport.is_some_and(|viewport| {
                contains(&viewport, x, y)
                    && x >= viewport.right - luciddesk_window::controls::Scrollbar::GUTTER
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_transition_then_settle_without_system_animation() {
        let _sta = crate::pane::test_support::apartment();
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        let target = canvas::Offscreen::new(&device, 640, 400).unwrap();
        let painter = Painter::new().unwrap();
        let mut scene = Scene::empty(Backdrop::Mica, 16.0, 48.0);
        for (kind, top, label) in [
            (
                ControlKind::SectionDisclosure { summary_width: 0.0 },
                64.0,
                "Skipped icons",
            ),
            (ControlKind::Checkbox, 112.0, "Regular expression"),
            (ControlKind::Slider(Slider::linear(50.0, 100.0)), 160.0, ""),
        ] {
            scene.control(
                kind,
                Rect::from_xywh(32.0, top, 240.0, 32.0),
                label,
                Action::Page(0),
                false,
            );
        }
        let start = Instant::now();
        let entered = start + SETTINGS_DURATION;
        let input = |now, enabled| MotionInput {
            hover: Some(0),
            pressed: (now >= entered).then_some(2),
            scrollbar: Some(0),
            now,
            enabled,
        };
        let mut motion = InterfaceMotion::default();
        motion.frame(&scene, input(start, true));
        scene.controls[0].selected = true;
        scene.controls[1].selected = true;
        scene.activity = Some(Rect::from_xywh(280.0, 64.0, 32.0, 32.0));
        motion.frame(&scene, input(entered, true));

        let midway = entered + HOVER_DURATION / 2;
        let (frame, playing) = motion.frame(&scene, input(midway, true));
        assert!(playing);
        assert!(
            frame.controls[&0]
                .disclosure
                .is_some_and(|value| value > 0.0 && value < 1.0)
        );
        assert!(
            frame.controls[&1]
                .check
                .is_some_and(|value| value > 0.0 && value < 1.0)
        );
        assert!(frame.activity.is_some_and(|phase| phase > 0.0));
        assert!(frame.controls[&2].slider.is_some_and(
            |thumb| (8.0..9.5).contains(&thumb.outer) && (4.0..5.0).contains(&thumb.inner)
        ));
        for dark in [false, true] {
            painter
                .paint_animated(
                    &target.target,
                    &scene,
                    640.0,
                    400.0,
                    1.0,
                    dark,
                    false,
                    Some(0),
                    None,
                    &HashMap::new(),
                    &frame,
                )
                .unwrap();
            assert!(!target.pixels().unwrap().is_empty());
        }

        let (settled, playing) = motion.frame(&scene, input(midway, false));
        assert!(!playing);
        assert_eq!(settled.controls[&0].disclosure, None);
        assert_eq!(settled.scrollbars, [1.0, 0.0]);
        assert_eq!(settled.activity, Some(0.0));
    }

    #[test]
    fn replacing_content_does_not_animate_it_or_reset_navigation_feedback() {
        let _sta = crate::pane::test_support::apartment();
        let mut scene = Scene::empty(Backdrop::Mica, 16.0, 48.0);
        let bounds = Rect::from_xywh(32.0, 64.0, 200.0, 36.0);
        scene.control(
            ControlKind::Navigation(""),
            bounds,
            "General",
            Action::Page(1),
            true,
        );
        scene.control(
            ControlKind::Navigation(""),
            bounds,
            "Appearance",
            Action::Page(2),
            false,
        );
        let mut motion = InterfaceMotion::default();
        let start = Instant::now();
        let input = |now| MotionInput {
            hover: None,
            pressed: None,
            scrollbar: None,
            now,
            enabled: true,
        };
        assert!(!motion.frame(&scene, input(start)).1);

        scene.control(
            ControlKind::Button,
            bounds,
            "New result",
            Action::Update,
            false,
        );
        scene.controls.rotate_right(1);
        assert!(!motion.frame(&scene, input(start)).1);

        scene.controls[1].selected = false;
        scene.controls[2].selected = true;
        assert!(motion.frame(&scene, input(start)).1);
        let (frame, playing) = motion.frame(&scene, input(start + SETTINGS_DURATION / 2));
        assert!(playing && !frame.controls.contains_key(&0));
        for index in [1, 2] {
            assert!(
                frame.controls[&index]
                    .selection
                    .is_some_and(|progress| progress > 0.0 && progress < 1.0)
            );
        }
        assert!(!motion.frame(&scene, input(start + SETTINGS_DURATION)).1);
    }

    #[test]
    fn toggle_press_nav_indicator_segment_pill_and_page_enter_animate_and_settle() {
        let mut scene = Scene::empty(Backdrop::Mica, 220.0, 40.0);
        scene.navigation_viewport = Some(Rect::from_xywh(0.0, 40.0, 200.0, 500.0));
        scene.text(Rect::from_xywh(220.0, 60.0, 200.0, 32.0), "Theme", 3);
        scene.control(
            ControlKind::Navigation(""),
            Rect::from_xywh(8.0, 48.0, 180.0, 36.0),
            "Theme",
            Action::Page(0),
            true,
        );
        scene.control(
            ControlKind::Navigation(""),
            Rect::from_xywh(8.0, 86.0, 180.0, 36.0),
            "General",
            Action::Page(1),
            false,
        );
        let track = Rect::from_xywh(220.0, 120.0, 206.0, 32.0);
        scene.segment_tracks.push(track);
        scene.control(
            ControlKind::Segment,
            Rect::from_xywh(223.0, 123.0, 100.0, 26.0),
            "Auto",
            Action::Page(0),
            true,
        );
        scene.control(
            ControlKind::Segment,
            Rect::from_xywh(323.0, 123.0, 100.0, 26.0),
            "Dark",
            Action::Page(0),
            false,
        );
        scene.control(
            ControlKind::Toggle,
            Rect::from_xywh(220.0, 170.0, 40.0, 20.0),
            "",
            Action::Page(0),
            false,
        );

        let mut motion = InterfaceMotion::default();
        let start = Instant::now();
        let input = |now, hover, pressed, enabled| MotionInput {
            hover,
            pressed,
            scrollbar: None,
            now,
            enabled,
        };
        let (initial, playing) = motion.frame(&scene, input(start, None, None, true));
        assert!(!playing);
        assert!(initial.nav_indicator.is_some());
        assert_eq!(initial.segment_pills[&0].left, 223.0);
        assert!(initial.page_enter.is_none());

        // Press toggle & switch active segment.
        scene.controls[2].selected = false;
        scene.controls[3].selected = true;
        let _ = motion.frame(&scene, input(start, Some(4), Some(4), true));
        let mid = start + PRESS_DURATION / 2;
        let (frame, playing) = motion.frame(&scene, input(mid, Some(4), Some(4), true));
        assert!(playing);
        assert!(frame.controls[&4].press > 0.0 && frame.controls[&4].press < 1.0);
        assert!(frame.segment_pills[&0].left > 223.0 && frame.segment_pills[&0].left < 323.0);
        let (settled, _) = motion.frame(&scene, input(start + SLIDE_DURATION, None, None, true));
        assert!((settled.segment_pills[&0].left - 323.0).abs() < 0.01);

        // Switch page: nav indicator glides with mid-flight stretch & page enters smoothly.
        let t_switch = start + Duration::from_secs(1);
        scene.controls[0].selected = false;
        scene.controls[1].selected = true;
        scene.text[0].1 = "General".into();
        let (at_switch, playing) = motion.frame(&scene, input(t_switch, None, None, true));
        assert!(playing);
        assert!(at_switch.page_enter.is_some_and(|(dy, op)| dy > 7.5 && op < 0.6));
        let (mid_switch, playing) =
            motion.frame(&scene, input(t_switch + SLIDE_DURATION / 2, None, None, true));
        assert!(playing);
        let pill = mid_switch.nav_indicator.unwrap();
        assert!(pill.bottom - pill.top > 16.0);
        assert!(mid_switch.page_enter.is_some_and(|(dy, op)| dy > 0.0 && dy < 8.0 && op > 0.55 && op < 1.0));
        let (done, playing) = motion.frame(&scene, input(t_switch + SLIDE_DURATION, None, None, true));
        assert!(!playing);
        assert!(done.page_enter.is_none());
        assert!(((done.nav_indicator.unwrap().bottom - done.nav_indicator.unwrap().top) - 16.0).abs() < 0.01);

        // Reduced motion disables entrance and settles immediately.
        let t_reduced = t_switch + Duration::from_secs(1);
        scene.controls[0].selected = true;
        scene.controls[1].selected = false;
        scene.text[0].1 = "Theme".into();
        let (instant, playing) = motion.frame(&scene, input(t_reduced, None, None, false));
        assert!(!playing);
        assert!(instant.page_enter.is_none());
    }
}
