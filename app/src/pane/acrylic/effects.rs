//! Shared GPU luminosity/tint graph following the public WinUI 2.8 material recipes.
//! https://github.com/microsoft/microsoft-ui-xaml/blob/v2.8.0/dev/Materials/Backdrop/SystemBackdropBrushFactory.cpp
//! https://github.com/microsoft/microsoft-ui-xaml/blob/v2.8.0/dev/Materials/Acrylic/AcrylicBrush.cpp
use std::cell::RefCell;
use windows::{
    Foundation::{IPropertyValue, PropertyValue},
    Graphics::Effects::{
        IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
    },
    UI::{
        Color,
        Composition::{
            CompositionBrush, CompositionEffectFactory, CompositionEffectSourceParameter,
            Compositor,
        },
    },
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL},
        Graphics::Direct2D::{
            CLSID_D2D1Blend,
            Common::{D2D1_BLEND_MODE_COLOR, D2D1_BLEND_MODE_LUMINOSITY},
        },
        System::WinRT::Graphics::Direct2D::{
            GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop,
            IGraphicsEffectD2D1Interop_Impl,
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR, Result, implement},
};

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Blend {
    name: RefCell<HSTRING>,
    mode: u32,
    sources: [IGraphicsEffectSource; 2],
}
impl IGraphicsEffectSource_Impl for Blend_Impl {}
impl IGraphicsEffect_Impl for Blend_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }
    fn SetName(&self, name: &HSTRING) -> Result<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}
impl IGraphicsEffectD2D1Interop_Impl for Blend_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1Blend)
    }
    fn GetNamedPropertyMapping(
        &self,
        _: &PCWSTR,
        _: *mut u32,
        _: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(1)
    }
    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        if index != 0 {
            return Err(E_INVALIDARG.into());
        }
        PropertyValue::CreateUInt32(self.mode)?.cast()
    }
    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        self.sources
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_INVALIDARG.into())
    }
    fn GetSourceCount(&self) -> Result<u32> {
        Ok(2)
    }
}
fn blend(
    mode: u32,
    background: IGraphicsEffectSource,
    foreground: IGraphicsEffectSource,
) -> IGraphicsEffect {
    Blend {
        name: RefCell::default(),
        mode,
        sources: [background, foreground],
    }
    .into()
}
pub(super) fn factory(compositor: &Compositor) -> Result<CompositionEffectFactory> {
    let source = |name: &str| -> Result<IGraphicsEffectSource> {
        CompositionEffectSourceParameter::Create(&HSTRING::from(name))?.cast()
    };
    // Preserve the native modes used by WinUI: its implementation explicitly
    // notes that the Color/Luminosity mode names are swapped in this pipeline.
    let luminosity = blend(
        D2D1_BLEND_MODE_COLOR.0 as u32,
        source("Backdrop")?,
        source("Luminosity")?,
    );
    let tint = blend(
        D2D1_BLEND_MODE_LUMINOSITY.0 as u32,
        luminosity.cast()?,
        source("Tint")?,
    );
    compositor.CreateEffectFactory(&tint)
}
pub(super) fn mica_palette(dark: bool, alt: bool) -> (Color, Color) {
    // Verified against MicaController Base/BaseAlt on Windows App Runtime
    // 1.6.618 (CBS 6000.900.156.100); see docs/development/mica-materials.md.
    // Tint colors are not fallback colors. Dark BaseAlt deliberately has no tint.
    let channel = match (dark, alt) {
        (true, false) => 32,
        (false, false) => 243,
        (true, true) => 10,
        (false, true) => 218,
    };
    let opacity: f32 = match (dark, alt) {
        (true, false) => 0.8,
        (false, false) => 0.5,
        (true, true) => 0.0,
        (false, true) => 0.5,
    };
    let luminosity = Color {
        A: 255,
        R: channel,
        G: channel,
        B: channel,
    };
    (
        luminosity,
        Color {
            A: (opacity * 255.0).round() as u8,
            ..luminosity
        },
    )
}
// Neutral theme colors keep the public API limited to material strength.
// AcrylicBrush.cpp: GetTintOpacityModifier / GetLuminosityColor, specialized
// to neutral colors (HSV saturation = 0). This is the window-acrylic path:
// HostBackdrop already supplies blur, so this graph adds no Gaussian blur.
pub(super) fn acrylic_palette(dark: bool) -> (Color, Color) {
    let channel: u8 = if dark { 32 } else { 243 };
    let strength: f32 = 120.0 / 255.0; // Preserve the previous default strength.
    let value = f32::from(channel) / 255.0;
    let modifier = if value > 0.5 {
        0.9 - (0.9 - 0.45) * ((value - 0.5) / 0.5)
    } else {
        0.9 - (0.9 - 0.85) * ((0.5 - value) / 0.5)
    };
    let luminosity_channel = (value.clamp(0.125, 0.965) * 255.0).round() as u8;
    let luminosity = Color {
        A: ((strength * 0.88 + 0.15).min(1.0) * 255.0).round() as u8,
        R: luminosity_channel,
        G: luminosity_channel,
        B: luminosity_channel,
    };
    let tint = Color {
        A: (strength * modifier * 255.0).round() as u8,
        R: channel,
        G: channel,
        B: channel,
    };
    (luminosity, tint)
}

// 50 preserves the recommended recipe. Below it, reveal more backdrop;
// above it, converge smoothly on the opaque theme color.
pub(super) fn adjust_strength(luminosity: Color, tint: Color, strength: u8) -> (Color, Color) {
    let value = f32::from(strength.min(100)) / 100.0;
    let alpha = |a: u8| -> u8 {
        let a = f32::from(a);
        (if value <= 0.5 {
            a * value * 2.0
        } else {
            a + (255.0 - a) * (value - 0.5) * 2.0
        })
        .round() as u8
    };
    (
        Color {
            A: alpha(luminosity.A),
            ..luminosity
        },
        Color {
            A: alpha(tint.A),
            ..tint
        },
    )
}

pub(super) fn update_colors(
    brush: &CompositionBrush,
    luminosity: Color,
    tint: Color,
) -> Result<()> {
    let effect: windows::UI::Composition::CompositionEffectBrush = brush.cast()?;
    for (name, color) in [("Luminosity", luminosity), ("Tint", tint)] {
        let source: windows::UI::Composition::CompositionColorBrush =
            effect.GetSourceParameter(&HSTRING::from(name))?.cast()?;
        source.SetColor(color)?;
    }
    Ok(())
}

pub(super) fn brush(
    compositor: &Compositor,
    factory: &CompositionEffectFactory,
    backdrop: &CompositionBrush,
    luminosity: Color,
    tint: Color,
) -> Result<CompositionBrush> {
    let effect = factory.CreateBrush()?;
    effect.SetSourceParameter(&HSTRING::from("Backdrop"), backdrop)?;
    effect.SetSourceParameter(
        &HSTRING::from("Luminosity"),
        &compositor.CreateColorBrushWithColor(luminosity)?,
    )?;
    effect.SetSourceParameter(
        &HSTRING::from("Tint"),
        &compositor.CreateColorBrushWithColor(tint)?,
    )?;
    effect.cast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_palettes_and_blend_contract() {
        // windows-rs caches agile WinRT factories for the process lifetime.
        // libtest tears down a fresh STA after each test; keep COM alive while
        // those caches remain reachable, just as the real app's UI loop does.
        static COM_RUNTIME: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        COM_RUNTIME.get_or_init(|| unsafe {
            windows::Win32::System::Com::CoIncrementMTAUsage()
                .unwrap()
                .0 as usize
        });
        let _sta = crate::pane::test_support::apartment();
        for dark in [false, true] {
            let (base, tint) = mica_palette(dark, false);
            let (alt_base, alt) = mica_palette(dark, true);
            assert!(alt_base.R < base.R);
            assert_eq!(base.A, 255);
            assert_eq!(base.R, if dark { 32 } else { 243 });
            assert_eq!((base.R, base.G), (base.G, base.B));
            assert_eq!(tint.A, if dark { 204 } else { 128 });
            assert_eq!(alt.A, if dark { 0 } else { 128 });
            assert_eq!(alt_base.R, if dark { 10 } else { 218 });
        }
        let (dark_luminosity, dark_tint) = acrylic_palette(true);
        let (light_luminosity, light_tint) = acrylic_palette(false);
        assert_eq!(dark_luminosity.A, 144);
        assert_eq!(light_luminosity.A, 144);
        assert_eq!(dark_tint.A, 104);
        assert_eq!(light_tint.A, 59);
        assert_eq!(dark_tint.R, 32);
        assert_eq!(light_tint.R, 243);
        assert!(dark_luminosity.A < mica_palette(true, false).0.A);
        for (luminosity, tint) in [
            mica_palette(true, false),
            mica_palette(false, true),
            acrylic_palette(true),
            acrylic_palette(false),
        ] {
            assert_eq!(adjust_strength(luminosity, tint, 50), (luminosity, tint));
            let low = adjust_strength(luminosity, tint, 0);
            let high = adjust_strength(luminosity, tint, 100);
            assert_eq!((low.0.A, low.1.A), (0, 0));
            assert_eq!((high.0.A, high.1.A), (255, 255));
            assert_eq!((low.0.R, high.0.R), (luminosity.R, luminosity.R));
            for value in 1..=100 {
                let before = adjust_strength(luminosity, tint, value - 1);
                let after = adjust_strength(luminosity, tint, value);
                assert!(before.0.A <= after.0.A && before.1.A <= after.1.A);
            }
        }
        let background: IGraphicsEffectSource =
            CompositionEffectSourceParameter::Create(&HSTRING::from("Background"))
                .unwrap()
                .cast()
                .unwrap();
        let foreground: IGraphicsEffectSource =
            CompositionEffectSourceParameter::Create(&HSTRING::from("Foreground"))
                .unwrap()
                .cast()
                .unwrap();
        let graph = blend(
            D2D1_BLEND_MODE_COLOR.0 as u32,
            background.clone(),
            foreground.clone(),
        );
        let interop: IGraphicsEffectD2D1Interop = graph.cast().unwrap();
        unsafe {
            assert_eq!(interop.GetEffectId().unwrap(), CLSID_D2D1Blend);
            assert_eq!(interop.GetPropertyCount().unwrap(), 1);
            assert_eq!(interop.GetProperty(0).unwrap().GetUInt32().unwrap(), 22);
            assert!(interop.GetProperty(1).is_err());
            assert_eq!(interop.GetSourceCount().unwrap(), 2);
            assert_eq!(interop.GetSource(0).unwrap(), background);
            assert_eq!(interop.GetSource(1).unwrap(), foreground);
            assert!(interop.GetSource(2).is_err());
        }
    }
}
