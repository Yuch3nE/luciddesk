//! Deterministic wallpaper study; illustrative, not a capture of the DWM backdrop.
use super::*;

fn mix(a: [f32; 3], b: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * amount)
}
fn rgb(value: u32) -> [f32; 3] {
    [
        ((value >> 16) & 255) as f32,
        ((value >> 8) & 255) as f32,
        (value & 255) as f32,
    ]
}
// Non-separable luminosity/color blend, with gamut clipping preserving luminance.
fn luminance(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}
fn set_luminance(c: [f32; 3], value: f32) -> [f32; 3] {
    let delta = value - luminance(c);
    let mut result = c.map(|v| v + delta);
    let min = result.into_iter().fold(f32::INFINITY, f32::min);
    if min < 0.0 {
        result = result.map(|v| value + (v - value) * value / (value - min));
    }
    let max = result.into_iter().fold(f32::NEG_INFINITY, f32::max);
    if max > 255.0 {
        result = result.map(|v| value + (v - value) * (255.0 - value) / (max - value));
    }
    result
}
pub(super) fn pixels(material: Backdrop, dark: bool) -> Vec<u8> {
    let (luminosity, tint) = super::super::acrylic::material_colors(material, dark);
    let lum = [
        luminosity.R as f32,
        luminosity.G as f32,
        luminosity.B as f32,
    ];
    let neutral = [tint.R as f32, tint.G as f32, tint.B as f32];
    let mut pixels = Vec::with_capacity(400 * 240 * 4);
    for y in 0..240 {
        for x in 0..400 {
            let u = x as f32 / 399.0;
            let v = y as f32 / 239.0;
            let wallpaper = mix(
                mix(rgb(0x538ac4), rgb(0xb991b4), u),
                rgb(0x63b4ae),
                (v * 0.65 + u * v * 0.25).clamp(0.0, 1.0),
            );
            let wallpaper = mix(
                wallpaper,
                if dark { [0.0; 3] } else { [255.0; 3] },
                if dark { 0.68 } else { 0.72 },
            );
            let inside = (20..380).contains(&x) && (20..220).contains(&y);
            let color = if !inside {
                wallpaper
            } else if let Backdrop::Solid { color, opacity } = material {
                mix(wallpaper, rgb(color), opacity)
            } else {
                let mut source = wallpaper;
                if material.base() == Backdrop::Acrylic {
                    // A blurred background window distinguishes glass from wallpaper-only Mica.
                    let window = ((u - 0.22) * 8.0).clamp(0.0, 1.0)
                        * ((0.8 - u) * 8.0).clamp(0.0, 1.0)
                        * ((v - 0.28) * 8.0).clamp(0.0, 1.0);
                    source = mix(
                        source,
                        if dark { [55.0; 3] } else { [250.0; 3] },
                        window * 0.6,
                    );
                }
                // D2D Color/Luminosity names are swapped in the native Mica graph.
                // Wallpaper capture and blur remain illustrative; blend recipes are shared.
                let toned = set_luminance(source, luminance(lum));
                let result = mix(source, toned, luminosity.A as f32 / 255.0);
                let result = mix(
                    result,
                    set_luminance(neutral, luminance(result)),
                    tint.A as f32 / 255.0,
                );
                result
            };
            // Match the mini-pane's rounded outline instead of leaving square
            // wallpaper/material corners under a rounded vector stroke.
            let dx = (x as f32 + 0.5 - 200.0).abs() - 172.0;
            let dy = (y as f32 + 0.5 - 120.0).abs() - 92.0;
            let distance = dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - 8.0;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            let color = mix(wallpaper, color, coverage);
            pixels.extend([
                color[2].round() as u8,
                color[1].round() as u8,
                color[0].round() as u8,
                255,
            ]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn material_previews_distinguish_modes_and_follow_strength_and_theme() {
        for dark in [false, true] {
            let mica = pixels(Backdrop::Mica, dark);
            let alt = pixels(Backdrop::MicaAlt, dark);
            assert_ne!(mica, alt);
            let center = (120 * 400 + 200) * 4;
            let brightness = |p: &[u8]| p[..3].iter().map(|v| u32::from(*v)).sum::<u32>();
            assert!(brightness(&alt[center..]) < brightness(&mica[center..]));
            assert_ne!(mica, pixels(Backdrop::Acrylic, dark));
            assert_ne!(mica, pixels(Backdrop::Mica.with_strength(0), dark));
            assert_ne!(mica, pixels(Backdrop::Mica.with_strength(100), dark));
            assert!(mica.chunks_exact(4).all(|p| p[3] == 255));
            let solid = pixels(
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 1.0,
                },
                dark,
            );
            let center = (120 * 400 + 200) * 4;
            assert_eq!(&solid[center..center + 4], &[0x56, 0x34, 0x12, 255]);
        }
        assert_ne!(pixels(Backdrop::Mica, true), pixels(Backdrop::Mica, false));
    }
}
