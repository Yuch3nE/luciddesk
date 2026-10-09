//! Viewport-local rubber-band selection, preserving identities across refreshes.
use super::{GroupModel, layout::Grid};
use luciddesk_core::{RectDip, ShellIdentity};
use std::collections::{BTreeSet, HashSet};
use windows_sys::Win32::Foundation::POINT;

pub(super) struct Marquee {
    pub start: POINT,
    pub rect: Option<RectDip>,
    original: HashSet<ShellIdentity>,
    focus: Option<ShellIdentity>,
    anchor: Option<ShellIdentity>,
    ctrl: bool,
    additive: bool,
}

impl Marquee {
    pub fn new(start: POINT, model: &GroupModel, ctrl: bool, shift: bool) -> Self {
        Self {
            start,
            rect: None,
            original: model.selected_identities().into_iter().collect(),
            focus: model
                .selected
                .and_then(|i| model.items.get(i))
                .map(|item| item.identity.clone()),
            anchor: model
                .selection_anchor
                .and_then(|i| model.items.get(i))
                .map(|item| item.identity.clone()),
            ctrl,
            additive: ctrl || shift,
        }
    }

    pub fn update(
        &mut self,
        model: &mut GroupModel,
        grid: Grid,
        scale: f32,
        point: POINT,
        viewport: RectDip,
    ) {
        let clamp = |point: POINT| {
            (
                (point.x as f32 / scale).clamp(viewport.x, viewport.x + viewport.width),
                (point.y as f32 / scale).clamp(viewport.y, viewport.y + viewport.height),
            )
        };
        let (sx, sy) = clamp(self.start);
        let (x, y) = clamp(point);
        let rect = RectDip {
            x: sx.min(x),
            y: sy.min(y),
            width: (sx - x).abs(),
            height: (sy - y).abs(),
        };
        self.rect = Some(rect);
        let hits: BTreeSet<_> = model
            .visible_indices(grid, viewport.y, viewport.y + viewport.height)
            .filter(|&index| overlaps(rect, model.selection_bounds(grid, index, scale)))
            .collect();
        let original: BTreeSet<_> = model
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| self.original.contains(&item.identity).then_some(i))
            .collect();
        model.selection = if self.ctrl {
            original.symmetric_difference(&hits).copied().collect()
        } else if self.additive {
            original.union(&hits).copied().collect()
        } else {
            hits
        };
        model.selected = model.selection.first().copied();
        model.selection_anchor = model.selected;
    }

    pub fn restore(&self, model: &mut GroupModel) {
        model.selection = model
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| self.original.contains(&item.identity).then_some(i))
            .collect();
        model.selected = self
            .focus
            .as_ref()
            .and_then(|id| model.items.iter().position(|item| &item.identity == id));
        model.selection_anchor = self
            .anchor
            .as_ref()
            .and_then(|id| model.items.iter().position(|item| &item.identity == id));
    }
}

fn overlaps(a: RectDip, b: RectDip) -> bool {
    a.width > 0.0
        && a.height > 0.0
        && a.x < b.x + b.width
        && a.x + a.width > b.x
        && a.y < b.y + b.height
        && a.y + a.height > b.y
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> GroupModel {
        let mut model = super::super::tests::test_model("Marquee test");
        model.items = (0..9)
            .map(|i| super::super::Item {
                identity: ShellIdentity::Namespace {
                    parsing_name: format!("test:{i}"),
                },
                label: format!("Item {i}"),
                image: None,
                details: Default::default(),
            })
            .collect();
        model
    }

    #[test]
    fn marquee_selects_intersecting_icons_in_both_directions_at_each_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            let mut model = model();
            let grid = model.grid(300.0, 300.0);
            let first = model.selection_bounds(grid, 0, scale);
            let second = model.selection_bounds(grid, 1, scale);
            let points = [
                POINT {
                    x: (first.x * scale) as i32,
                    y: (first.y * scale) as i32,
                },
                POINT {
                    x: ((second.x + second.width) * scale) as i32,
                    y: ((second.y + second.height) * scale) as i32,
                },
            ];
            for reverse in [false, true] {
                let (start, end) = if reverse {
                    (points[1], points[0])
                } else {
                    (points[0], points[1])
                };
                let mut marquee = Marquee::new(start, &model, false, false);
                marquee.update(
                    &mut model,
                    grid,
                    scale,
                    end,
                    RectDip {
                        x: 0.0,
                        y: 40.0,
                        width: 300.0,
                        height: 260.0,
                    },
                );
                assert_eq!(model.selection, BTreeSet::from([0, 1]));
                marquee.update(
                    &mut model,
                    grid,
                    scale,
                    start,
                    RectDip {
                        x: 0.0,
                        y: 40.0,
                        width: 300.0,
                        height: 260.0,
                    },
                );
                assert!(model.selection.is_empty());
            }
        }
    }

    #[test]
    fn marquee_modifiers_and_cancel_preserve_identity_after_refresh() {
        for (ctrl, shift, expected) in [(true, false, vec![1]), (false, true, vec![0, 1])] {
            let mut model = model();
            model.select_item(0, false, false);
            let grid = model.grid(300.0, 300.0);
            let first = model.selection_bounds(grid, 0, 1.0);
            let second = model.selection_bounds(grid, 1, 1.0);
            let mut marquee = Marquee::new(
                POINT {
                    x: first.x as i32,
                    y: first.y as i32,
                },
                &model,
                ctrl,
                shift,
            );
            marquee.update(
                &mut model,
                grid,
                1.0,
                POINT {
                    x: (second.x + second.width) as i32,
                    y: (second.y + second.height) as i32,
                },
                RectDip {
                    x: 0.0,
                    y: 40.0,
                    width: 300.0,
                    height: 260.0,
                },
            );
            assert_eq!(
                model.selection.iter().copied().collect::<Vec<_>>(),
                expected
            );
            let mut items = model.items.clone();
            items.reverse();
            model.replace_items(items);
            marquee.restore(&mut model);
            assert_eq!(model.selection, BTreeSet::from([8]));
            assert_eq!(model.selected, Some(8));
            assert_eq!(model.selection_anchor, Some(8));
        }
    }

    #[test]
    fn marquee_window_messages_commit_cancel_and_ignore_mapped_folders() {
        use std::{cell::RefCell, rc::Rc};
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _apartment = crate::pane::test_support::apartment();
        let model = Rc::new(RefCell::new(model()));
        let pane = super::super::window::create(
            RectDip {
                x: 80.0,
                y: 80.0,
                width: 300.0,
                height: 300.0,
            },
            model.clone(),
            |_| false,
        )
        .unwrap();
        let hwnd = pane.hwnd().cast();
        unsafe {
            let scale = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let mut client = windows_sys::Win32::Foundation::RECT::default();
            GetClientRect(hwnd, &raw mut client);
            let grid = model
                .borrow()
                .grid(client.right as f32 / scale, client.bottom as f32 / scale);
            let second = model.borrow().selection_bounds(grid, 1, scale);
            let position = |x: f32, y: f32| {
                (((y * scale).round() as i32 as u16 as isize) << 16)
                    | (x * scale).round() as i32 as u16 as isize
            };
            let start = position(2.0, model.borrow().content_header() + 1.0);
            let end = position(second.x + second.width, second.y + second.height);
            model.borrow_mut().select_item(8, false, false);
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, start);
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, end);
            assert_eq!(model.borrow().selection, BTreeSet::from([0, 1]));
            SendMessageW(hwnd, WM_LBUTTONUP, 0, end);
            assert_eq!(model.borrow().selection, BTreeSet::from([0, 1]));
            assert_ne!(
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture(),
                hwnd
            );

            for (message, key) in [
                (WM_KEYDOWN, 0x1b),
                (WM_CANCELMODE, 0),
                (WM_CAPTURECHANGED, 0),
            ] {
                model.borrow_mut().select_item(8, false, false);
                SendMessageW(hwnd, WM_LBUTTONDOWN, 0, start);
                SendMessageW(hwnd, WM_MOUSEMOVE, 1, end);
                assert_eq!(model.borrow().selection, BTreeSet::from([0, 1]));
                SendMessageW(hwnd, message, key, 0);
                assert_eq!(model.borrow().selection, BTreeSet::from([8]));
                SendMessageW(hwnd, WM_LBUTTONUP, 0, end);
            }
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, start);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, start);
            assert!(model.borrow().selection.is_empty());

            model.borrow_mut().folder = Some(std::path::PathBuf::from("C:\\"));
            SendMessageW(hwnd, WM_LBUTTONDOWN, 0, start);
            SendMessageW(hwnd, WM_MOUSEMOVE, 1, end);
            SendMessageW(hwnd, WM_LBUTTONUP, 0, end);
            assert!(model.borrow().selection.is_empty());
        }
        super::super::window::prepare_close(hwnd);
        drop(pane);
    }
}
