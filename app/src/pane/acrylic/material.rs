//! Material selection, brush reuse and ordered fallback.
use super::super::native_graphics::{DWMWA_USE_HOSTBACKDROPBRUSH, set_attribute};
use super::{Acrylic, effects, host, material_colors};
use luciddesk_core::Backdrop;
use windows::{
    UI::{
        Color,
        Composition::{CompositionBrush, CompositionColorBrush, SpriteVisual},
    },
    core::{Interface, Result},
};

const TRANSPARENT: Color = Color {
    A: 0,
    R: 0,
    G: 0,
    B: 0,
};

impl Acrylic {
    pub fn material(&self, material: Backdrop, dark: bool) -> Result<()> {
        #[cfg(test)]
        if self.fail_material_once.replace(false) {
            return Err(windows::Win32::Foundation::E_FAIL.into());
        }
        if let Backdrop::Solid { color, opacity } = material {
            return self.solid(color, opacity);
        }
        let brush = self.material_or_fallback(material, dark)?;
        self.replace_background(&brush)?;
        let clear_tint: CompositionColorBrush = self.tint.Brush()?.cast()?;
        clear_tint.SetColor(TRANSPARENT)?;
        self.visible(true)
    }

    fn solid(&self, color: u32, opacity: f32) -> Result<()> {
        self.clear_effects();
        let brush = self.runtime.compositor.CreateColorBrushWithColor(Color {
            A: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            R: (color >> 16) as u8,
            G: (color >> 8) as u8,
            B: color as u8,
        })?;
        self.replace_background(&brush.cast()?)?;
        self.set_color(&self.tint, TRANSPARENT)?;
        self.visible(true)
    }

    fn set_color(&self, visual: &SpriteVisual, color: Color) -> Result<()> {
        if let Ok(brush) = visual.Brush()?.cast::<CompositionColorBrush>() {
            brush.SetColor(color)
        } else {
            visual.SetBrush(&self.runtime.compositor.CreateColorBrushWithColor(color)?)
        }
    }

    fn clear_effects(&self) {
        self.material_brush.borrow_mut().take();
        self.host_backdrop.borrow_mut().take();
    }

    fn material_or_fallback(&self, material: Backdrop, dark: bool) -> Result<CompositionBrush> {
        let wallpaper = matches!(material.base(), Backdrop::Mica | Backdrop::MicaAlt);
        let requested = self.effect_brush(material, dark, wallpaper);
        let result = match requested {
            Err(_) if wallpaper => {
                // Keep the requested strength without changing the persisted preference.
                self.effect_brush(
                    Backdrop::Acrylic.with_strength(material.strength().unwrap_or(50)),
                    dark,
                    false,
                )
            }
            result => result,
        };
        match result {
            Ok(brush) => Ok(brush),
            Err(error) => {
                crate::pane::render_debug::render_trace(format_args!(
                    "hwnd={:?} opaque fallback: {error}",
                    self.hwnd
                ));
                // An unavailable backdrop/effect must remain opaque and legible.
                self.clear_effects();
                self.runtime
                    .compositor
                    .CreateColorBrushWithColor(effects::fallback_color(material, dark))?
                    .cast()
            }
        }
    }

    fn effect_brush(
        &self,
        material: Backdrop,
        dark: bool,
        wallpaper: bool,
    ) -> Result<CompositionBrush> {
        if self.disable_backdrop || !self.effects_enabled() {
            return Err(windows::Win32::Foundation::E_NOTIMPL.into());
        }
        #[cfg(test)]
        if if wallpaper {
            self.unavailable_backdrops.get().0
        } else {
            self.unavailable_backdrops.get().1
        } {
            return Err(windows::Win32::Foundation::E_NOTIMPL.into());
        }
        let compositor = &self.runtime.compositor;
        let (luminosity, tint) = material_colors(material, dark);
        if !wallpaper && self.host_backdrop.borrow().is_none() {
            *self.host_backdrop.borrow_mut() = Some(host::HostBackdrop::enable(self.hwnd)?);
        }
        let mut cached = self.material_brush.borrow_mut();
        if let Some((old_wallpaper, brush)) = cached.as_ref()
            && *old_wallpaper == wallpaper
        {
            effects::update_colors(brush, luminosity, tint, self.animate_material())?;
            return Ok(brush.clone());
        }
        let mut factory = self.runtime.material_factory.borrow_mut();
        if factory.is_none() {
            *factory = Some(effects::factory(compositor)?);
        }
        let backdrop: CompositionBrush = if wallpaper {
            // Downlevel systems reject this attribute; wallpaper availability is
            // decided by the brush API, independently of composition setup.
            let _ = unsafe { set_attribute(self.hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &1i32) };
            compositor
                .TryCreateBlurredWallpaperBackdropBrush()?
                .cast()?
        } else {
            compositor.CreateHostBackdropBrush()?.cast()?
        };
        let brush = effects::brush(
            compositor,
            factory.as_ref().unwrap(),
            &backdrop,
            luminosity,
            tint,
        )?;
        *cached = Some((wallpaper, brush.clone()));
        Ok(brush)
    }
}
