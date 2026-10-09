use super::*;
use luciddesk_core::Backdrop;

#[test]
fn diagnostic_bypass_never_enables_host_or_wallpaper_backdrops() {
    let _sta = crate::pane::test_support::apartment();
    let window = windows_window::Window::new("Backdrop isolation")
        .size(96, 64)
        .style(windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP)
        .ex_style(windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP)
        .create().unwrap();
    let device = super::super::native_graphics::gpu_device().unwrap();
    let swap = device.create_swap_chain(96, 64).unwrap();
    let native_swap = super::super::native_graphics::native_interface(swap.raw_swap_chain()).unwrap();
    let mut acrylic = Acrylic::new_with_content(HWND(window.hwnd().cast()), 1.0, &native_swap).unwrap();
    for disabled_by_policy in [false, true] {
        acrylic.disable_backdrop = !disabled_by_policy;
        acrylic.effects_override.set(Some(!disabled_by_policy));
        for dark in [true, false] {
            for material in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::MicaAlt] {
                for strength in [0, 50, 100] {
                    acrylic.material(material.with_strength(strength), dark).unwrap();
                    acrylic.assert_solid_color(if dark { 0x202020 } else { 0xf3f3f3 }, 1.0);
                    assert!(acrylic.host_backdrop.borrow().is_none());
                    assert!(acrylic.material_brush.borrow().is_none());
                    acrylic.assert_content_visible();
                }
            }
        }
    }
    acrylic.disable_backdrop = false;
    acrylic.effects_override.set(Some(true));
    acrylic.material(Backdrop::Acrylic, false).unwrap();
    acrylic.assert_material_effect();
    acrylic.assert_content_visible();
}

#[test]
fn missing_wallpaper_uses_acrylic_then_opaque_color_without_hiding_content() {
    let _sta = crate::pane::test_support::apartment();
    let window = windows_window::Window::new("Material fallback regression")
        .size(96, 64)
        .style(windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP)
        .ex_style(windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP)
        .create()
        .unwrap();
    let device = super::super::native_graphics::gpu_device().unwrap();
    let swap = device.create_swap_chain(96, 64).unwrap();
    let native_swap =
        super::super::native_graphics::native_interface(swap.raw_swap_chain()).unwrap();
    let acrylic =
        Acrylic::new_with_content(HWND(window.hwnd().cast()), 1.0, &native_swap).unwrap();
    acrylic.unavailable_backdrops.set((true, false));
    for dark in [false, true] {
        for requested in [Backdrop::Mica, Backdrop::MicaAlt] {
            for strength in [0, 25, 50, 75, 100] {
                let requested = requested.with_strength(strength);
                acrylic.material(requested, dark).unwrap();
                assert!(!acrylic.material_brush.borrow().as_ref().unwrap().0);
                acrylic.assert_material_effect();
                acrylic.assert_material_colors(
                    Backdrop::Acrylic.with_strength(requested.strength().unwrap_or(50)),
                    dark,
                );
                acrylic.assert_content_visible();
            }
        }
        // Fault injection must also bypass a previously cached acrylic brush.
        acrylic.unavailable_backdrops.set((true, true));
        acrylic.material(Backdrop::Mica, dark).unwrap();
        acrylic.assert_solid_color(if dark { 0x0020_2020 } else { 0x00f3_f3f3 }, 1.0);
        assert!(acrylic.material_brush.borrow().is_none());
        assert!(acrylic.host_backdrop.borrow().is_none());
        acrylic.assert_content_visible();
        acrylic.unavailable_backdrops.set((true, false));
    }
    acrylic.material(Backdrop::Mica, true).unwrap();
    let cached = acrylic.material_brush.borrow().as_ref().unwrap().1.clone();
    acrylic.visible(false).unwrap();
    assert!(acrylic.host_backdrop.borrow().is_none());
    acrylic.material(Backdrop::Mica, true).unwrap();
    assert_eq!(acrylic.material_brush.borrow().as_ref().unwrap().1, cached);
    assert!(acrylic.host_backdrop.borrow().is_some());
    acrylic
        .material(
            Backdrop::Solid {
                color: 0x0012_3456,
                opacity: 0.5,
            },
            true,
        )
        .unwrap();
    assert!(acrylic.host_backdrop.borrow().is_none());
    acrylic.assert_solid_color(0x0012_3456, 0.5);
    acrylic.unavailable_backdrops.set((false, false));
    acrylic.material(Backdrop::Acrylic, true).unwrap();
    acrylic.assert_material_effect();
    acrylic.assert_material_colors(Backdrop::Acrylic, true);
    acrylic.assert_content_visible();
}

impl Acrylic {
    #[cfg(test)]
    pub fn assert_solid_color(&self, color: u32, opacity: f32) {
        let brush: windows::UI::Composition::CompositionColorBrush =
            self.backdrop.Brush().unwrap().cast().unwrap();
        let actual = brush.Color().unwrap();
        assert_eq!(
            (actual.R, actual.G, actual.B),
            ((color >> 16) as u8, (color >> 8) as u8, color as u8)
        );
        assert_eq!(actual.A, (opacity * 255.0).round() as u8);
        let tint: windows::UI::Composition::CompositionColorBrush =
            self.tint.Brush().unwrap().cast().unwrap();
        assert_eq!(tint.Color().unwrap().A, 0);
        assert_eq!(self.content.as_ref().unwrap().Opacity().unwrap(), 1.0);
    }

    #[cfg(test)]
    pub fn assert_material_colors(&self, material: luciddesk_core::Backdrop, dark: bool) {
        let (luminosity, tint) = if material.base() == luciddesk_core::Backdrop::Acrylic {
            effects::acrylic_palette(dark)
        } else {
            effects::mica_palette(dark, material.base() == luciddesk_core::Backdrop::MicaAlt)
        };
        let (luminosity, tint) =
            effects::adjust_strength(luminosity, tint, material.strength().unwrap_or(50));
        let effect: windows::UI::Composition::CompositionEffectBrush =
            self.backdrop.Brush().unwrap().cast().unwrap();
        for (name, expected) in [("Luminosity", luminosity), ("Tint", tint)] {
            let brush: windows::UI::Composition::CompositionColorBrush = effect
                .GetSourceParameter(&windows::core::HSTRING::from(name))
                .unwrap()
                .cast()
                .unwrap();
            assert_eq!(brush.Color().unwrap(), expected);
        }
    }

    #[cfg(test)]
    pub fn assert_material_effect(&self) {
        let effect: windows::UI::Composition::CompositionEffectBrush = self
            .backdrop
            .Brush()
            .unwrap()
            .cast()
            .expect("Material must use the GPU blend graph, not fallback");
        for name in ["Backdrop", "Luminosity", "Tint"] {
            effect
                .GetSourceParameter(&windows::core::HSTRING::from(name))
                .unwrap();
        }
        let tint: windows::UI::Composition::CompositionColorBrush =
            self.tint.Brush().unwrap().cast().unwrap();
        assert_eq!(
            tint.Color().unwrap().A,
            0,
            "Do not overlay the old tint twice"
        );
    }

    #[cfg(test)]
    pub fn assert_content_visible(&self) {
        assert!(self._target.IsTopmost().unwrap());
        assert!(self.root.IsVisible().unwrap());
        assert!(self.content.as_ref().unwrap().IsVisible().unwrap());
    }

    #[cfg(test)]
    pub fn opacity_value(&self) -> Result<f32> {
        self.root.Opacity()
    }

    #[cfg(test)]
    pub fn assert_rounded_clip(&self, width: u32, height: u32, radius: f32) {
        let (geometry, bounds) = self.rounded_clip.as_ref().unwrap();
        assert_eq!(*bounds, (width, height, radius));
        assert!(self.root.Clip().is_ok());
        assert_eq!(geometry.Size().unwrap(), Vector2 { X: width as f32, Y: height as f32 });
        assert_eq!(geometry.CornerRadius().unwrap(), Vector2 { X: radius, Y: radius });
    }
}
