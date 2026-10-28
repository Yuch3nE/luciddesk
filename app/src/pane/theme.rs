//! Neutral desktop selection; folder-view themes use a different, blue selection.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use super::assets::Pixels;

/// Local surfaces only: never fill the pane body with these colors.
pub(super) struct MaterialChrome {
    pub tab_active: windows_canvas::ColorF,
    pub tab_hover: windows_canvas::ColorF,
    pub tab_inactive: windows_canvas::ColorF,
    pub tab_incoming: windows_canvas::ColorF,
    pub border: windows_canvas::ColorF,
    pub card: windows_canvas::ColorF,
    pub card_border: windows_canvas::ColorF,
}

pub(super) fn material_chrome(backdrop: luciddesk_core::Backdrop, dark: bool) -> MaterialChrome {
    use luciddesk_core::Backdrop;
    use windows_canvas::ColorF;
    let strength = f32::from(backdrop.strength().unwrap_or(50)) / 100.0;
    // Retain a modest local surface at minimum strength for text and selection;
    // borders may fade completely. At 50 the material's reference alpha is used.
    let surface = |reference: f32| {
        if strength <= 0.5 {
            reference * (0.3 + 1.4 * strength)
        } else {
            reference + (1.0 - reference) * (strength - 0.5) * 0.5
        }
    };
    let (card, edge) = match backdrop.base() {
        Backdrop::Acrylic => (if dark { 0.22 } else { 0.44 }, if dark { 0.18 } else { 0.16 }),
        Backdrop::Mica => (if dark { 0.30 } else { 0.50 }, 0.10),
        Backdrop::MicaAlt => (if dark { 0.45 } else { 0.70 }, if dark { 0.13 } else { 0.12 }),
        _ => (if dark { 0.65 } else { 0.72 }, if dark { 0.14 } else { 0.16 }),
    };
    let channel = if dark { 58.0 / 255.0 } else { 1.0 };
    let fill = |alpha| ColorF::new(channel, channel, channel, alpha);
    let coverage = match backdrop {
        Backdrop::Solid { opacity, .. } | Backdrop::Translucent { opacity } => {
            opacity.clamp(0.0, 1.0)
        }
        _ => strength * 2.0,
    };
    let edge_channel = if dark { 0.6 } else { 0.0 };
    // Use one subtle selection baseline with small material-specific corrections.
    // Material strength changes the cue gently; transparent panels still retain it.
    let tab_dark = if let Backdrop::Solid { color, .. } = backdrop {
        luminance([
            (color >> 16 & 255) as f32 / 255.0,
            (color >> 8 & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
        ]) < 0.179
    } else { dark };
    let tab_base = if tab_dark { 0.10 } else { 0.09 };
    let material_weight = match backdrop.base() {
        Backdrop::Acrylic => 1.08,
        Backdrop::Mica => 1.0,
        Backdrop::MicaAlt => 1.04,
        Backdrop::Translucent { .. } => 1.12,
        _ => 0.96,
    };
    let strength_weight = match backdrop {
        Backdrop::Solid { opacity, .. } | Backdrop::Translucent { opacity } => {
            0.9 + opacity.clamp(0.0, 1.0) * 0.1
        }
        _ if backdrop.strength().is_some() => 0.9 + strength * 0.2,
        _ => 1.0,
    };
    let tab_alpha = tab_base * material_weight * strength_weight;
    let tab_channel = if tab_dark { 1.0 } else { 0.0 };
    let tab_fill = |alpha| ColorF::new(tab_channel, tab_channel, tab_channel, alpha);
    MaterialChrome {
        tab_active: tab_fill(tab_alpha),
        tab_hover: tab_fill(tab_alpha * 0.4),
        tab_inactive: tab_fill(tab_alpha * 0.12),
        tab_incoming: tab_fill(tab_alpha * 0.65),
        border: panel_border(dark, backdrop),
        card: fill(surface(card)),
        card_border: ColorF::new(
            edge_channel,
            edge_channel,
            edge_channel,
            edge * (0.35 + 0.65 * coverage),
        ),
    }
}

/// WinUI 3 Common_themeresources_any.xaml semantic stroke resources.
/// Self-drawn desktop panes use SurfaceStrokeColorDefault, not a second DWM frame.
fn stroke_color(argb: u32, opacity: f32) -> windows_canvas::ColorF {
    windows_canvas::ColorF::new(
        ((argb >> 16) & 255) as f32 / 255.0,
        ((argb >> 8) & 255) as f32 / 255.0,
        (argb & 255) as f32 / 255.0,
        ((argb >> 24) & 255) as f32 / 255.0 * opacity,
    )
}

fn stroke_opacity(backdrop: luciddesk_core::Backdrop) -> f32 {
    use luciddesk_core::Backdrop;
    match backdrop {
        Backdrop::Solid { opacity, .. } | Backdrop::Translucent { opacity } => {
            opacity.clamp(0.0, 1.0)
        }
        _ => (f32::from(backdrop.strength().unwrap_or(50)) / 50.0).min(1.0),
    }
}

pub fn panel_border(_dark: bool, backdrop: luciddesk_core::Backdrop) -> windows_canvas::ColorF {
    // SurfaceStrokeColorDefault is identical in Light and Default (dark).
    stroke_color(0x66757575, stroke_opacity(backdrop))
}

pub(super) fn panel_divider(
    dark: bool,
    backdrop: luciddesk_core::Backdrop,
) -> windows_canvas::ColorF {
    stroke_color(
        if dark { 0x15ffffff } else { 0x0f000000 },
        stroke_opacity(backdrop),
    )
}

/// Content-only colors. Never changes the material's theme or samples the desktop.
pub struct PanelContrast {
    pub light_text: bool,
    pub scrim: f32,
}

impl PanelContrast {
    pub fn ink(&self) -> f32 {
        if self.light_text { 1.0 } else { 0.0 }
    }
    pub fn base(&self) -> f32 {
        if self.light_text { 0.0 } else { 1.0 }
    }
}

fn luminance(rgb: [f32; 3]) -> f32 {
    rgb.into_iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(c, weight)| {
            weight
                * if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
        })
        .sum()
}

pub fn panel_contrast(
    backdrop: luciddesk_core::Backdrop,
    dark: bool,
    mode: luciddesk_core::PanelText,
    native: bool,
) -> PanelContrast {
    use luciddesk_core::{Backdrop, PanelText};
    let forced = match mode {
        PanelText::Auto => None,
        PanelText::Light => Some(true),
        PanelText::Dark => Some(false),
    };
    // Standard material recipes already provide a stable theme-colored base.
    // Add protection for custom transparency/strength and manual overrides.
    if !native
        || (!matches!(
            backdrop,
            Backdrop::Solid { .. } | Backdrop::Translucent { .. }
        ) && backdrop.strength().unwrap_or(50) >= 50
            && forced.is_none_or(|light| light == dark))
    {
        return PanelContrast {
            light_text: forced.unwrap_or(dark),
            scrim: 0.0,
        };
    }
    let (low, high, nominal) = if let Backdrop::Solid { color, opacity } = backdrop {
        let rgb = [
            (color >> 16 & 255) as f32 / 255.0,
            (color >> 8 & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
        ];
        let alpha = opacity.clamp(0.0, 1.0);
        (
            rgb.map(|c| c * alpha),
            rgb.map(|c| c * alpha + 1.0 - alpha),
            rgb,
        )
    } else {
        // Estimate how far a weakened native recipe can depart from its theme
        // base. This changes continuously with strength, rather than jumping
        // from a full scrim to none at the default value. Solid colors above
        // use exact compositing bounds; native effects remain an estimate.
        let base = if dark { 32.0 / 255.0 } else { 243.0 / 255.0 };
        let coverage = (f32::from(backdrop.strength().unwrap_or(0)) / 50.0).min(1.0);
        (
            [base * coverage; 3],
            [base * coverage + 1.0 - coverage; 3],
            [base; 3],
        )
    };
    let light = forced.unwrap_or_else(|| luminance(nominal) < 0.179);
    let worst = if light { high } else { low };
    let contrast = |alpha: f32| {
        let background =
            luminance(worst.map(|c| c * (1.0 - alpha) + if light { 0.0 } else { alpha }));
        if light {
            1.05 / (background + 0.05)
        } else {
            (background + 0.05) / 0.05
        }
    };
    // Leave contrast headroom for secondary text and selection fills, including
    // black/white content behind a transparent solid material.
    let mut lower = 0.0;
    let mut upper = 1.0;
    if contrast(0.0) >= 7.0 {
        upper = 0.0;
    } else {
        for _ in 0..16 {
            let mid = (lower + upper) * 0.5;
            if contrast(mid) >= 7.0 {
                upper = mid;
            } else {
                lower = mid;
            }
        }
    }
    PanelContrast {
        light_text: light,
        scrim: upper,
    }
}

pub fn is_dark(theme: luciddesk_core::PanelTheme) -> bool {
    match theme {
        luciddesk_core::PanelTheme::Dark => true,
        luciddesk_core::PanelTheme::Light => false,
        luciddesk_core::PanelTheme::System => {
            use windows::UI::ViewManagement::{UIColorType, UISettings};
            UISettings::new()
                .and_then(|s| s.GetColorValue(UIColorType::Foreground))
                .map_or(true, |c| {
                    u32::from(c.R) + u32::from(c.G) + u32::from(c.B) > 384
                })
        }
    }
}

/// Fits the highlight to icon and measured label, excluding the inter-row gap.
pub fn selection_height(icon_size: f32, text_height: f32, cell_height: f32) -> f32 {
    (icon_size + super::layout::LABEL_OFFSET + 1.0 + text_height).min(cell_height - 2.0)
}

pub fn selection(width: u32, height: u32, dpi: u32, state: i32) -> Option<Pixels> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return None;
    }
    let opacity = match state {
        2 => 0.10,
        5 => 0.17,
        6 => 0.24,
        _ => 0.21,
    };
    let radius = (dpi as f32 / 96.0).max(1.0);
    let mut data = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let dx = (radius - (x as f32 + 0.5).min(width as f32 - x as f32 - 0.5)).max(0.0);
            let dy = (radius - (y as f32 + 0.5).min(height as f32 - y as f32 - 0.5)).max(0.0);
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            let alpha = (255.0 * opacity * coverage).round() as u8;
            let at = ((y * width + x) * 4) as usize;
            data[at..at + 4].fill(alpha);
        }
    }
    Some(Pixels {
        width,
        height,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_tab_fill_follows_color_instead_of_application_theme() {
        use luciddesk_core::Backdrop;
        for (color, light_fill) in [(0x000000, true), (0x202020, true),
            (0x24364b, true), (0x0000ff, true), (0xffffff, false),
            (0xe9edf2, false), (0xffff00, false), (0x808080, false)] {
            for opacity in [0.0, 0.5, 0.85, 1.0] {
                let material = Backdrop::Solid { color, opacity };
                let light = material_chrome(material, false);
                let dark = material_chrome(material, true);
                for (a, b) in [(light.tab_active, dark.tab_active), (light.tab_hover, dark.tab_hover),
                    (light.tab_incoming, dark.tab_incoming)] {
                    assert_eq!([a.r, a.g, a.b, a.a], [b.r, b.g, b.b, b.a]);
                    assert_eq!(a.r, if light_fill { 1.0 } else { 0.0 });
                }
                assert!(light.tab_active.a > light.tab_hover.a);
            }
        }
    }

    #[test]
    fn tab_material_changes_keep_selection_weight_consistent() {
        use luciddesk_core::Backdrop;
        for dark in [false, true] {
            for strength in [0, 50, 100] {
                let alphas: Vec<_> = [Backdrop::Acrylic, Backdrop::Mica,
                    Backdrop::MicaAlt, Backdrop::solid_default(dark)]
                    .into_iter().map(|material| material_chrome(material.with_strength(strength), dark).tab_active.a)
                    .collect();
                let low = alphas.iter().copied().fold(f32::INFINITY, f32::min);
                let high = alphas.iter().copied().fold(0.0_f32, f32::max);
                assert!(high / low < 1.3, "switching materials should not change selection weight abruptly");
            }
            for opacity in [0.0, 0.25, 0.5, 0.85, 1.0] {
                let selected = material_chrome(Backdrop::Solid {
                    color: if dark { 0x202020 } else { 0xe9edf2 }, opacity,
                }, dark).tab_active;
                assert!(selected.a >= if dark { 0.085 } else { 0.077 });
                assert!(selected.a <= if dark { 0.10 } else { 0.09 });
            }
        }
    }

    #[test]
    fn tab_selection_contrasts_with_light_and_dark_materials() {
        use luciddesk_core::Backdrop;
        for dark in [false, true] {
            for material in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::MicaAlt,
                Backdrop::solid_default(dark), Backdrop::Translucent { opacity: 0.86 }] {
                for strength in [0, 50, 100] {
                    let chrome = material_chrome(material.with_strength(strength), dark);
                    let background = if dark { 0.14 } else { 0.94 };
                    let composite = |color: windows_canvas::ColorF| color.r * color.a + background * (1.0 - color.a);
                    let selected = composite(chrome.tab_active);
                    let hovered = composite(chrome.tab_hover);
                    assert!((selected - background).abs() > 0.05);
                    assert!((selected - hovered).abs() > 0.03);
                    assert_eq!(selected > background, dark);
                }
            }
        }
    }

    #[test]
    fn chrome_keeps_selection_visible_and_adapts_to_material_strength() {
        use luciddesk_core::Backdrop;
        for dark in [false, true] {
            let mica = material_chrome(Backdrop::Mica, dark);
            let alt = material_chrome(Backdrop::MicaAlt, dark);
            assert!(alt.tab_active.a > mica.tab_active.a);
            assert!(alt.card.a > mica.card.a);
            for material in [Backdrop::Acrylic, Backdrop::Mica] {
                let mut previous = None;
                for strength in 0..=100 {
                    let chrome = material_chrome(material.with_strength(strength), dark);
                    assert!(chrome.tab_active.a > chrome.tab_incoming.a);
                    assert!(chrome.tab_incoming.a > chrome.tab_inactive.a);
                    assert!(chrome.tab_active.a > chrome.tab_hover.a);
                    assert!(chrome.tab_hover.a > chrome.tab_inactive.a);
                    assert!(chrome.card_border.a > 0.0);
                    let current = [chrome.tab_active.a, chrome.card.a, chrome.border.a];
                    assert!(current.iter().all(|a| (0.0..=1.0).contains(a)));
                    if let Some(previous) = previous {
                        for (now, before) in current.into_iter().zip(previous) {
                            assert!(now >= before);
                        }
                    }
                    previous = Some(current);
                }
            }
        }
    }

    #[test]
    fn border_follows_background_opacity_and_material_strength() {
        use luciddesk_core::Backdrop;
        for dark in [false, true] {
            let full = panel_border(
                dark,
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 1.0,
                },
            );
            for opacity in [0.0, 0.25, 0.5, 1.0] {
                let border = panel_border(
                    dark,
                    Backdrop::Solid {
                        color: 0x123456,
                        opacity,
                    },
                );
                assert!((border.a - full.a * opacity).abs() < 0.00001);
                assert_eq!((border.r, border.g, border.b), (full.r, full.g, full.b));
            }
            for material in [Backdrop::Acrylic, Backdrop::Mica] {
                let alphas: Vec<_> = [0, 25, 50, 75, 100]
                    .into_iter()
                    .map(|strength| panel_border(dark, material.with_strength(strength)).a)
                    .collect();
                assert_eq!(alphas[0], 0.0);
                assert!(alphas.windows(2).all(|pair| pair[0] <= pair[1]));
                assert_eq!(alphas[2], panel_border(dark, material).a);
            }
        }
    }

    #[test]
    fn solid_text_remains_readable_over_extreme_desktop_colors() {
        use luciddesk_core::{Backdrop, PanelText};
        for color in [0, 0xffffff, 0x808080, 0xff0000, 0x00ff00, 0x0000ff] {
            for opacity in [0.0, 0.1, 0.5, 0.9, 1.0] {
                for mode in [PanelText::Auto, PanelText::Light, PanelText::Dark] {
                    let style =
                        panel_contrast(Backdrop::Solid { color, opacity }, true, mode, true);
                    for behind in [0.0, 1.0] {
                        let rgb = [16, 8, 0].map(|shift| {
                            let c = ((color >> shift) & 255) as f32 / 255.0;
                            (c * opacity + behind * (1.0 - opacity)) * (1.0 - style.scrim)
                                + style.base() * style.scrim
                        });
                        let l = luminance(rgb);
                        let ratio = if style.light_text {
                            1.05 / (l + 0.05)
                        } else {
                            (l + 0.05) / 0.05
                        };
                        assert!(ratio >= 4.499, "{color:x} {opacity} {mode:?}: {ratio}");
                    }
                }
            }
        }
        for (color, light) in [(0xffffff, false), (0, true)] {
            let style = panel_contrast(
                Backdrop::Solid {
                    color,
                    opacity: 1.0,
                },
                !light,
                PanelText::Auto,
                true,
            );
            assert_eq!(style.light_text, light);
            assert_eq!(
                style.scrim, 0.0,
                "Opaque readable colors need no protection"
            );
        }
    }

    #[test]
    fn transparent_materials_and_manual_overrides_are_protected() {
        use luciddesk_core::{Backdrop, PanelText};
        for dark in [false, true] {
            let standard = panel_contrast(Backdrop::Acrylic, dark, PanelText::Auto, true);
            assert_eq!(standard.light_text, dark);
            assert_eq!(standard.scrim, 0.0);
            let clear = panel_contrast(
                Backdrop::Acrylic.with_strength(0),
                dark,
                PanelText::Auto,
                true,
            );
            assert!(clear.scrim > 0.0);
            let forced = panel_contrast(
                Backdrop::Mica,
                dark,
                if dark {
                    PanelText::Dark
                } else {
                    PanelText::Light
                },
                true,
            );
            assert_eq!(forced.light_text, !dark);
            assert!(forced.scrim > 0.0);
        }
    }

    #[test]
    fn desktop_selection_is_neutral_and_fits_text_instead_of_grid_spacing() {
        assert!((selection_height(48.0, 16.0, 98.0) - 71.0).abs() < f32::EPSILON);
        assert!((selection_height(48.0, 32.0, 98.0) - 87.0).abs() < f32::EPSILON);
        assert!(selection_height(48.0, 64.0, 98.0) < 98.0);
        let mut alphas = Vec::new();
        for state in [2, 5, 3, 6] {
            let pixels = selection(112, 103, 144, state).unwrap();
            assert!(
                pixels
                    .data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[0] == p[1] && p[1] == p[2] && p[2] == p[3])
            );
            alphas.push(pixels.data[4 * (50 * 112 + 50) + 3]);
        }
        assert!(alphas.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}

pub(super) fn mica_fallback(
    backdrop: luciddesk_core::Backdrop,
    dark: bool,
) -> Option<windows_canvas::ColorF> {
    use luciddesk_core::Backdrop;
    if !matches!(backdrop.base(), Backdrop::Mica | Backdrop::MicaAlt) {
        return None;
    }
    // Runtime controller fallback values differ from the XAML BaseAlt resource.
    let channel = if dark {
        32
    } else if backdrop.base() == Backdrop::MicaAlt {
        232
    } else {
        243
    };
    let c = channel as f32 / 255.0;
    Some(windows_canvas::ColorF::new(c, c, c, 1.0))
}
