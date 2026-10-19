//! Reusable single-selection flyout. Business actions stay with the caller;
//! the shared menu host owns window lifetime, composition and dismissal.
use super::menu::Entry;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, RECT};

mod painter;
pub(super) const ROW_HEIGHT: f32 = super::control_style::HEIGHT;
pub(super) const PADDING: f32 = 4.0;
const MAX_ROWS: usize = 8;

#[derive(Clone, Copy)]
pub(super) struct Options {
    pub anchor: RECT,
    pub current: Option<usize>,
}

pub(super) fn show(
    owner: HWND,
    anchor: RECT,
    appearance: (luciddesk_core::PanelTheme, luciddesk_core::Backdrop),
    mut rows: Vec<Entry>,
    current: Option<usize>,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    // Return a position regardless of the caller's command IDs.
    for (index, row) in rows.iter_mut().enumerate() {
        row.id = i32::try_from(index + 1).ok()?;
        row.children.clear();
    }
    let command = super::menu::show_picker(owner, appearance, rows, Options { anchor, current });
    usize::try_from(command.checked_sub(1)?).ok()
}

pub(super) fn placement(
    anchor: RECT,
    work: RECT,
    scale: f32,
    desired_width: f32,
    count: usize,
) -> (RECT, usize) {
    let inset = (8.0 * scale).round() as i32;
    let gap = (4.0 * scale).round() as i32;
    let left_edge = work.left + inset.min((work.right - work.left).max(0) / 4);
    let right_edge = work.right - inset.min((work.right - work.left).max(0) / 4);
    let top_edge = work.top + inset.min((work.bottom - work.top).max(0) / 4);
    let bottom_edge = work.bottom - inset.min((work.bottom - work.top).max(0) / 4);
    let width = ((desired_width * scale).ceil() as i32)
        .max(anchor.right - anchor.left)
        .min((right_edge - left_edge).max(1));
    let desired_height =
        ((count.min(MAX_ROWS) as f32 * ROW_HEIGHT + 2.0 * PADDING) * scale).round() as i32;
    let below = (bottom_edge - anchor.bottom - gap).max(0);
    let above = (anchor.top - gap - top_edge).max(0);
    let upwards = below < desired_height && above > below;
    let space = if upwards { above } else { below };
    let available = space
        .max(((ROW_HEIGHT + 2.0 * PADDING) * scale) as i32)
        .min((bottom_edge - top_edge).max(1));
    let visible = (((available as f32 / scale - 2.0 * PADDING) / ROW_HEIGHT).floor() as usize)
        .max(1)
        .min(count.max(1))
        .min(MAX_ROWS);
    let height =
        (((visible as f32 * ROW_HEIGHT + 2.0 * PADDING) * scale).round() as i32).min(available);
    let left = anchor
        .left
        .clamp(left_edge, (right_edge - width).max(left_edge));
    let top = (if upwards {
        anchor.top - gap - height
    } else {
        anchor.bottom + gap
    })
    .clamp(top_edge, (bottom_edge - height).max(top_edge));
    (
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
        visible,
    )
}

pub(super) struct Model {
    pub current: Option<usize>,
    pub focus: Option<usize>,
    pub first: usize,
    pub visible: usize,
    enabled: Vec<usize>,
    labels: Vec<String>,
    search: String,
    typed_at: Option<Instant>,
    wheel_remainder: i32,
}
impl Model {
    pub fn new(rows: &[Entry], current: Option<usize>, visible: usize) -> Self {
        let enabled: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| row.enabled.then_some(i))
            .collect();
        let current = current.filter(|i| enabled.contains(i));
        let mut value = Self {
            current,
            focus: current.or_else(|| enabled.first().copied()),
            first: current.unwrap_or(0).saturating_sub(visible / 2),
            visible: visible.max(1),
            enabled,
            labels: rows
                .iter()
                .map(|row| row.label.trim().to_lowercase())
                .collect(),
            search: String::new(),
            typed_at: None,
            wheel_remainder: 0,
        };
        value.reveal();
        value
    }
    pub fn range(&self) -> std::ops::Range<usize> {
        self.first..(self.first + self.visible).min(self.labels.len())
    }
    fn reveal(&mut self) {
        if let Some(index) = self.focus {
            if index < self.first {
                self.first = index;
            } else if index >= self.first + self.visible {
                self.first = index + 1 - self.visible;
            }
        }
        self.first = self
            .first
            .min(self.labels.len().saturating_sub(self.visible));
    }
    pub fn step(&mut self, delta: i32) {
        if self.enabled.is_empty() {
            return;
        }
        let index = self
            .focus
            .and_then(|i| self.enabled.iter().position(|j| *j == i));
        let next = index.map_or_else(
            || {
                if delta < 0 {
                    self.enabled.len() - 1
                } else {
                    self.enabled
                        .iter()
                        .position(|i| *i >= self.first)
                        .unwrap_or(0)
                }
            },
            |i| {
                i.saturating_add_signed(delta as isize)
                    .min(self.enabled.len() - 1)
            },
        );
        self.focus = Some(self.enabled[next]);
        self.reveal();
    }
    pub fn edge(&mut self, end: bool) {
        self.focus = if end {
            self.enabled.last()
        } else {
            self.enabled.first()
        }
        .copied();
        self.reveal();
    }
    pub fn wheel(&mut self, delta: i32) -> bool {
        self.wheel_remainder += delta;
        let rows = self.wheel_remainder / 120 * 3;
        self.wheel_remainder %= 120;
        let old = self.first;
        self.first = self
            .first
            .saturating_add_signed(-(rows as isize))
            .min(self.labels.len().saturating_sub(self.visible));
        old != self.first
    }
    pub fn scrollbar(&self, height: f32) -> Option<(f32, f32)> {
        if self.labels.len() <= self.visible {
            return None;
        }
        let track = (height - 16.0).max(1.0);
        let thumb = (track * self.visible as f32 / self.labels.len() as f32)
            .max(18.0)
            .min(track);
        let top = 8.0
            + (track - thumb) * self.first as f32
                / self.labels.len().saturating_sub(self.visible).max(1) as f32;
        Some((top, thumb))
    }
    pub fn drag_scrollbar(&mut self, y: f32, grab: f32, height: f32) {
        if let Some((_, thumb)) = self.scrollbar(height) {
            let fraction = ((y - 8.0 - grab) / (height - 16.0 - thumb).max(1.0)).clamp(0.0, 1.0);
            self.first =
                (fraction * self.labels.len().saturating_sub(self.visible) as f32).round() as usize;
        }
    }
    pub fn type_char(&mut self, ch: char, now: Instant) {
        if ch.is_control() || self.labels.is_empty() {
            return;
        }
        if self
            .typed_at
            .is_none_or(|at| now.saturating_duration_since(at) > Duration::from_millis(900))
        {
            self.search.clear();
        }
        self.typed_at = Some(now);
        let text = ch.to_lowercase().collect::<String>();
        let cycling = self.search == text;
        if !cycling && self.search.len() < 128 {
            self.search.push_str(&text);
        }
        let start = self.focus.unwrap_or(0);
        let find = |prefix: &str, skip_current: bool| {
            (0..self.labels.len())
                .map(|step| (start + step + usize::from(skip_current)) % self.labels.len())
                .find(|i| {
                    self.enabled.binary_search(i).is_ok() && self.labels[*i].starts_with(prefix)
                })
        };
        let matched = find(&self.search, cycling).or_else(|| find(&text, true));
        if let Some(index) = matched {
            if !self.labels[index].starts_with(&self.search) {
                self.search = text;
            }
            self.focus = Some(index);
            self.reveal();
        }
    }
    pub fn hit(&self, x: f32, y: f32, width: f32) -> Option<usize> {
        let trailing = if self.labels.len() > self.visible {
            10.0
        } else {
            PADDING
        };
        if x < PADDING
            || x >= width - trailing
            || y < PADDING
            || y >= PADDING + self.visible as f32 * ROW_HEIGHT
        {
            return None;
        }
        let index = self.first + ((y - PADDING) / ROW_HEIGHT) as usize;
        self.enabled.binary_search(&index).is_ok().then_some(index)
    }
}

pub(super) struct Popup {
    pub bounds: RECT,
    pub model: Model,
    painter: painter::Painter,
    tooltip: Option<super::tooltip::Tooltip>,
    tooltip_row: Option<usize>,
}
impl Popup {
    pub fn new(
        rows: &[Entry],
        options: Options,
        work: RECT,
        scale: f32,
    ) -> windows::core::Result<Self> {
        let mut painter = painter::Painter::new(rows)?;
        let (bounds, visible) = placement(options.anchor, work, scale, painter.width, rows.len());
        painter.fit((bounds.right - bounds.left) as f32 / scale)?;
        Ok(Self {
            bounds,
            model: Model::new(rows, options.current, visible),
            painter,
            tooltip: None,
            tooltip_row: None,
        })
    }
    pub fn hide_tip(&mut self) {
        if let Some(tip) = &mut self.tooltip {
            tip.hide();
        }
        self.tooltip_row = None;
    }
    pub fn hover_tip(&mut self, hwnd: HWND, rows: &[Entry], index: Option<usize>, scale: f32) {
        let index = index.filter(|i| self.painter.truncated(*i));
        if index == self.tooltip_row {
            return;
        }
        self.hide_tip();
        if let Some(index) = index {
            if self.tooltip.is_none() {
                self.tooltip = super::tooltip::Tooltip::new(hwnd);
            }
            if let Some(tip) = &mut self.tooltip {
                let top = PADDING + (index - self.model.first) as f32 * ROW_HEIGHT;
                tip.show(
                    &rows[index].label,
                    RECT {
                        left: (PADDING * scale) as i32,
                        top: (top * scale) as i32,
                        right: self.bounds.right - self.bounds.left - (PADDING * scale) as i32,
                        bottom: ((top + ROW_HEIGHT) * scale) as i32,
                    },
                );
            }
            self.tooltip_row = Some(index);
        }
    }
    pub fn paint(
        &mut self,
        target: &windows_canvas::ID2D1DeviceContext,
        scale: f32,
        highlights: &[f32],
        native: bool,
        dark: bool,
    ) -> windows::core::Result<()> {
        self.painter.paint(
            target,
            self.bounds,
            scale,
            &self.model,
            highlights,
            (native, dark),
        )
    }
}

#[cfg(test)]
mod tests;
