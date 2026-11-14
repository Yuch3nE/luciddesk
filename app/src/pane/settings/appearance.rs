//! Appearance value conversion, validation and saved style lookup.
use super::*;

pub(super) fn grid_range() -> (f32, f32) {
    luciddesk_core::PaneOptions::GRID_SCALE_RANGE
}

pub(super) fn grid_slider_position(value: f32) -> f32 {
    let (min, max) = grid_range();
    let value = value.clamp(min, max);
    if value <= 100.0 {
        0.5 * (value - min) / (100.0 - min)
    } else {
        0.5 + 0.5 * (value - 100.0) / (max - 100.0)
    }
}

pub(super) fn grid_slider_value(position: f32) -> f32 {
    let (min, max) = grid_range();
    let position = position.clamp(0.0, 1.0);
    if position <= 0.5 {
        (min + position * 2.0 * (100.0 - min)).round()
    } else {
        (100.0 + (position - 0.5) * 2.0 * (max - 100.0)).round()
    }
}

pub(super) fn radius_from_pointer(bounds: Rect, x: f32) -> f32 {
    let progress = controls::slider_fraction(bounds, x);
    progress * luciddesk_core::PaneOptions::MAX_CORNER_RADIUS
}

pub(super) fn solid_style(store: &luciddesk_storage::WorkspaceStore, dark: bool) -> Backdrop {
    if let Ok(Some(value)) = store.preference("solid_style") {
        if let Some((color, opacity)) = value.split_once('|') {
            if let (Ok(color), Ok(opacity)) = (color.parse::<u32>(), opacity.parse::<f32>()) {
                if color <= 0xffffff && opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
                    return Backdrop::Solid { color, opacity };
                }
            }
        }
    }
    Backdrop::solid_default(dark)
}

pub(super) fn edited_solid(backdrop: Backdrop, percentage: bool, text: &str) -> Option<Backdrop> {
    let Backdrop::Solid {
        mut color,
        mut opacity,
    } = backdrop
    else {
        return None;
    };
    if percentage {
        let value = text.trim().trim_end_matches('%').parse::<u8>().ok()?;
        if value > 100 {
            return None;
        }
        opacity = f32::from(value) / 100.0;
    } else {
        let text = text.trim().trim_prefix('#');
        if text.len() != 6 || !text.is_ascii() {
            return None;
        }
        color = u32::from_str_radix(text, 16).ok()?;
    }
    Some(Backdrop::Solid { color, opacity })
}

pub(super) fn settings_backdrop(backdrop: Backdrop, dark: bool) -> Backdrop {
    match backdrop {
        Backdrop::Solid { .. } => Backdrop::Solid {
            color: if dark { 0x202020 } else { 0xf3f3f3 },
            opacity: 1.0,
        },
        other => other.base(),
    }
}

pub(super) fn material_style(
    store: &luciddesk_storage::WorkspaceStore,
    backdrop: Backdrop,
) -> Backdrop {
    backdrop
        .strength_key()
        .and_then(|key| store.preference(key).ok().flatten())
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .map_or(backdrop, |value| backdrop.with_strength(value))
}

pub(super) fn color_channel(color: u32, channel: u8, value: u8) -> u32 {
    let shift = (2 - u32::from(channel.min(2))) * 8;
    (color & !(255 << shift)) | (u32::from(value) << shift)
}
