//! Connect scene controls to their animation state.
use super::*;
use crate::pane::animation::{HOVER_DURATION, Motion, SETTINGS_DURATION};
use luciddesk_window::controls::motion::{
    ActivityMotion, PointerMotion, SelectionMotion, SliderMotion, SliderThumb,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Default)]
pub(super) struct ControlMotion {
    pub hover: Option<f32>,
    pub disclosure: Option<f32>,
    pub check: Option<f32>,
    pub selection: Option<f32>,
    pub slider: Option<SliderThumb>,
}

#[derive(Default)]
pub(super) struct MotionFrame {
    pub controls: HashMap<usize, ControlMotion>,
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
pub(super) struct InterfaceMotion {
    activity: ActivityMotion,
    controls: HashMap<ControlSlot, ControlTrack>,
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
                        | ControlKind::IconButton(_)
                        | ControlKind::QuietButton(_)
                        | ControlKind::Combo
                        | ControlKind::Checkbox
                        | ControlKind::SectionDisclosure { .. }
                        | ControlKind::Tab(_)
                        | ControlKind::Navigation(_)
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
            let selectable = navigation || row;
            let hoverable = hoverable
                || (control.enabled
                    && matches!(
                        control.kind,
                        ControlKind::Slider(_)
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
}
