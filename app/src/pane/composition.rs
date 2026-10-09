//! Transparent content surface over the system-owned backdrop; no whole-window alpha.
#![allow(clippy::wildcard_imports)]
use super::native_graphics::*;
use luciddesk_core::Backdrop;
use windows::Win32::Foundation::HWND;

use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{Error, Interface, Result};
use windows_canvas::{GpuDevice, ID2D1DeviceContext, SwapChain};

mod recovery;
pub(super) use recovery::PaintRecovery;

pub struct Surface {
    retry_at: std::cell::Cell<Option<std::time::Instant>>,
    hwnd: HWND,
    present: IDXGISwapChain1,
    rounded_backdrop: Option<HWND>,
    pub pane_corner_radius: f32,
    dark: bool,
    opacity: std::cell::Cell<f32>,
    acrylic: Option<super::acrylic::Acrylic>,
    _device: GpuDevice,
    #[cfg(test)]
    context: ID3D11DeviceContext,
    drawing: ID2D1DeviceContext,
    layer: Option<luciddesk_graphics::Layer>,
    swap: SwapChain,
    material: Option<Backdrop>,
    effects_enabled: Option<bool>,
    pub native: bool,
}

impl Surface {
    #[cfg(test)]
    pub(super) fn defer_frame_for_test(&self, duration: std::time::Duration) {
        self.retry_at.set(Some(std::time::Instant::now() + duration));
    }

    #[cfg(test)]
    pub fn current_opacity(&self) -> f32 {
        self.opacity.get()
    }
    /// Suppress the DWM non-client frame; forced system rounding can still cast a shadow.
    /// Keep this separate from shared surface initialization so flyout shadows remain.
    pub fn disable_window_shadow(hwnd: HWND) -> Result<()> {
        let policy = DWMNCRP_DISABLED;
        unsafe { set_attribute(hwnd, DWMWA_NCRENDERING_POLICY, &policy) }
    }

    pub fn opacity(&self, opacity: f32) -> Result<()> {
        if self.opacity.get() == opacity {
            return Ok(());
        }
        crate::pane::render_debug::render_trace(format_args!("hwnd={:?} opacity {} -> {opacity}", self.hwnd, self.opacity.get()));
        if let Some(layer) = &self.layer {
            canvas_result(layer.opacity(opacity))?;
        }
        if let Some(acrylic) = &self.acrylic {
            acrylic.opacity(opacity)?;
        }
        if let Some(layer) = &self.layer {
            canvas_result(layer.commit())?;
        }
        self.opacity.set(opacity);
        Ok(())
    }
    pub fn new(hwnd: HWND) -> Result<Self> {
        Self::new_with_opacity(hwnd, 1.0)
    }

    pub fn commit_ready(&self) -> Result<Box<dyn Fn() -> bool>> {
        if let Some(acrylic) = &self.acrylic {
            acrylic.commit_ready()
        } else {
            Ok(Box::new(|| true))
        }
    }

    pub fn new_settings(hwnd: HWND) -> Result<Self> {
        Self::create(hwnd, 1.0, gpu_device()?, true)
    }

    pub fn new_flyout(hwnd: HWND, initial_opacity: f32) -> Result<Self> {
        let mut surface = Self::create(hwnd, initial_opacity, gpu_device()?, true)?;
        surface.rounded_backdrop = Some(hwnd);
        surface.pane_corner_radius = super::menu::CORNER_RADIUS;
        Ok(surface)
    }

    pub fn fade_in(&self) -> Result<()> {
        // Only a shared tree can animate content and material atomically.
        if self.layer.is_none()
            && let Some(acrylic) = &self.acrylic
        {
            acrylic.fade_in()?;
        }
        Ok(())
    }

    pub fn new_pane(hwnd: HWND) -> Result<Self> {
        let mut surface = if crate::pane::render_debug::shared_pane_tree() {
            Self::create(hwnd, 1.0, gpu_device()?, true)?
        } else {
            Self::new(hwnd)?
        };
        Self::disable_window_shadow(hwnd)?;
        // On Windows 11, forced DWM rounding casts an activation shadow even with
        // non-client rendering disabled. Round our backdrop instead of the HWND.
        // Windows 10 has no DWM corner preference (E_INVALIDARG). It does not
        // need this Windows 11 workaround; keep our own backdrop clipping active.
        if let Err(error) = unsafe {
            set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_DONOTROUND)
        } {
            if error.code() != windows::Win32::Foundation::E_INVALIDARG {
                return Err(error);
            }
        }
        surface.rounded_backdrop = Some(hwnd);
        Ok(surface)
    }

    pub fn new_with_opacity(hwnd: HWND, initial_opacity: f32) -> Result<Self> {
        Self::new_with_device(hwnd, initial_opacity, gpu_device()?)
    }

    fn new_with_device(hwnd: HWND, initial_opacity: f32, device: GpuDevice) -> Result<Self> {
        Self::create(hwnd, initial_opacity, device, false)
    }

    fn create(
        hwnd: HWND,
        initial_opacity: f32,
        device: GpuDevice,
        shared_tree: bool,
    ) -> Result<Self> {
        unsafe {
            let d3d: ID3D11Device = native_interface(device.d3d_device())?;
            #[cfg(test)]
            let context = d3d.GetImmediateContext()?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let mut bounds = windows_sys::Win32::Foundation::RECT::default();
            windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd.0, &raw mut bounds);
            let mut swap = canvas_result(
                device.create_swap_chain(bounds.right.max(1) as u32, bounds.bottom.max(1) as u32),
            )?;
            // Obtain Canvas's persistent context once. Subsequent frames keep the
            // renderer's explicit BeginDraw/EndDraw so all drawing errors propagate.
            let drawing: ID2D1DeviceContext = {
                let session = canvas_result(swap.begin_draw())?;
                session.raw().clone()
            };
            let native_swap: IDXGISwapChain1 = native_interface(swap.raw_swap_chain())?;
            let acrylic = if shared_tree {
                match super::acrylic::Acrylic::new_with_content(hwnd, initial_opacity, &native_swap)
                {
                    Ok(material) => Some(material),
                    Err(error) => {
                        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Warn, "pane.composition", "Shared composition unavailable: {error}");
                        crate::pane::render_debug::render_trace(format_args!("hwnd={hwnd:?} shared composition failed: {error}"));
                        None
                    }
                }
            } else {
                None
            };
            let layer = if acrylic.is_some() {
                None
            } else {
                Some(create_layer(hwnd, &dxgi, &native_swap, initial_opacity)?)
            };
            crate::pane::render_debug::render_trace(format_args!("hwnd={hwnd:?} surface created shared={} opacity={initial_opacity}", acrylic.is_some()));
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            extend_frame(hwnd, &margins)?;
            let dark = 1i32;
            // The content renderer owns the single border. Suppress the second DWM outline.
            let border = 0xffff_fffeu32;
            let _ = set_attribute(hwnd, DWMWA_BORDER_COLOR, &border);
            let _ = set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark);
            let corner = DWMWCP_ROUND;
            let _ = set_attribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner);
            Ok(Self {
                retry_at: std::cell::Cell::new(None),
                hwnd,
                present: native_swap,
                rounded_backdrop: None,
                pane_corner_radius: luciddesk_core::PaneOptions::DEFAULT.corner_radius,
                dark: true,
                opacity: std::cell::Cell::new(initial_opacity),
                effects_enabled: None,
                acrylic,
                _device: device,
                #[cfg(test)]
                context,
                drawing,
                layer,
                swap,
                material: None,
                native: false,
            })
        }
    }

    pub fn theme(&mut self, hwnd: HWND, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.material = None;
            let value = i32::from(dark);
            unsafe {
                let _ = set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &value);
            }
        }
    }

    pub fn material(&mut self, hwnd: HWND, material: Backdrop) {
        let effects_enabled = self.acrylic.as_ref().is_none_or(|acrylic| acrylic.effects_enabled());
        if self.material == Some(material) && self.effects_enabled == Some(effects_enabled) {
            return;
        }
        self.effects_enabled = Some(effects_enabled);
        let kind = DWMSBT_NONE;
        self.native = unsafe { set_attribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &kind).is_ok() }
            && kind != DWMSBT_NONE;
        if matches!(
            material.base(),
            Backdrop::Acrylic | Backdrop::Mica | Backdrop::MicaAlt | Backdrop::Solid { .. }
        ) {
            if self.acrylic.is_none() {
                self.acrylic =
                    super::acrylic::Acrylic::new_with_opacity(hwnd, self.opacity.get()).ok();
            }
            self.native = self
                .acrylic
                .as_ref()
                .is_some_and(|acrylic| acrylic.material(material, self.dark).is_ok());
            if !self.native
                && let Some(acrylic) = &self.acrylic
            {
                let _ = acrylic.visible(false);
            }
        } else if let Some(acrylic) = &self.acrylic {
            let _ = acrylic.visible(false);
        }
        self.material = Some(material);
        crate::pane::render_debug::render_trace(format_args!("hwnd={hwnd:?} material={material:?} dark={} composition_material={}", self.dark, self.native));
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        if (self.swap.width(), self.swap.height()) != (width, height) {
            canvas_result(self.swap.resize(width, height))?;
        }
        if let (Some(hwnd), Some(acrylic)) = (self.rounded_backdrop, &mut self.acrylic) {
            let scale =
                unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd.0) } as f32 / 96.0;
            // Include the outline's half-DIP outset when clipping the backdrop.
            acrylic.round_corners(
                width,
                height,
                if self.pane_corner_radius == 0.0 {
                    0.0
                } else {
                    (self.pane_corner_radius + 0.5) * scale
                },
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn present(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|count| count.checked_mul(4));
        if expected != Some(pixels.len()) {
            return Err(Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        self.resize(width, height)?;
        unsafe {
            let swap: IDXGISwapChain1 = native_interface(self.swap.raw_swap_chain())?;
            let buffer: ID3D11Texture2D = swap.GetBuffer(0)?;
            self.context
                .UpdateSubresource(&buffer, 0, None, pixels.as_ptr().cast(), width * 4, 0);
        }
        self.end_frame()?;
        if let Some(layer) = &self.layer {
            canvas_result(layer.commit())?;
        }
        Ok(())
    }

    /// Canvas owns buffer binding and resizing; the renderer owns the draw bracket.
    pub fn begin_frame(&mut self, width: u32, height: u32) -> Result<ID2D1DeviceContext> {
        self.resize(width, height)?;
        Ok(self.drawing.clone())
    }

    /// Backpressure before rasterization, rather than drawing frames a full
    /// presentation queue cannot accept. The pending timer retains the redraw.
    pub fn try_begin_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Option<ID2D1DeviceContext>> {
        if let Some(at) = self.retry_at.get() {
            let remaining = at.saturating_duration_since(std::time::Instant::now());
            if !remaining.is_zero() {
                // A timer can run just before the deadline. Preserve a wakeup
                // even after that callback validated the previous paint request.
                self.schedule_retry(remaining.as_millis() as u32 + 1)?;
                return Ok(None);
            }
        }
        self.begin_frame(width, height).map(Some)
    }

    pub fn end_frame(&self) -> Result<()> {
        // DWM owns display synchronization. Never make the common UI thread wait
        // for every pane's vertical blank; a full queue retries the latest state.
        let result = unsafe { self.present.Present(0, DXGI_PRESENT_DO_NOT_WAIT) };
        if result.is_err() {
            crate::pane::render_debug::render_trace(format_args!("hwnd={:?} Present={result:?}", self.hwnd));
        }
        if result == DXGI_ERROR_WAS_STILL_DRAWING {
            self.schedule_retry(16)?;
            self.retry_at.set(Some(
                std::time::Instant::now() + std::time::Duration::from_millis(16),
            ));
            return Ok(());
        }
        self.retry_at.set(None);
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(self.hwnd.0, PRESENT_RETRY);
        }
        result.ok()
    }

    fn schedule_retry(&self, millis: u32) -> Result<()> {
        if unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(
                self.hwnd.0,
                PRESENT_RETRY,
                millis,
                Some(retry_present),
            )
        } == 0
        {
            return Err(Error::from_thread());
        }
        Ok(())
    }

    /// Read the current back buffer after drawing, before end_frame/Present.
    /// After Present, buffer zero may be the next frame rather than the one drawn.
    #[cfg(test)]
    pub fn readback(&self) -> Result<Vec<u8>> {
        unsafe {
            let swap: IDXGISwapChain1 = native_interface(self.swap.raw_swap_chain())?;
            let source: ID3D11Texture2D = swap.GetBuffer(0)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&raw mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.context.GetDevice()?.CreateTexture2D(
                &raw const desc,
                None,
                Some(&raw mut staging),
            )?;
            let staging = staging.unwrap();
            self.context.CopyResource(&staging, &source);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))?;
            let mut pixels = vec![0; (desc.Width * desc.Height * 4) as usize];
            for y in 0..desc.Height as usize {
                let row = std::slice::from_raw_parts(
                    mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                    desc.Width as usize * 4,
                );
                pixels[y * row.len()..(y + 1) * row.len()].copy_from_slice(row);
            }
            self.context.Unmap(&staging, 0);
            Ok(pixels)
        }
    }
}

const PRESENT_RETRY: usize = 0x4c50_4750;
unsafe extern "system" fn retry_present(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _: u32,
    id: usize,
    _: u32,
) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(hwnd, id);
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        crate::pane::render_debug::render_trace(format_args!("hwnd={:?} surface dropped", self.hwnd));
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(self.hwnd.0, PRESENT_RETRY);
        }
    }
}

#[cfg(test)]
pub(super) mod animation_tests {
    use super::*;
    #[test]
    fn system_policy_changes_invalidate_cached_material_and_restore_effects() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("System material fallback")
            .size(96, 64).style(WS_POPUP).ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .create().unwrap();
        let hwnd = HWND(window.hwnd().cast());
        let mut surface = Surface::new_settings(hwnd).unwrap();
        for dark in [false, true] {
            surface.theme(hwnd, dark);
            for material in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::MicaAlt] {
                surface.acrylic.as_ref().unwrap().set_effects_enabled_for_test(true);
                surface.material(hwnd, material);
                surface.acrylic.as_ref().unwrap().set_effects_enabled_for_test(false);
                surface.material(hwnd, material);
                let acrylic = surface.acrylic.as_ref().unwrap();
                acrylic.assert_solid_color(if dark { 0x202020 } else { 0xf3f3f3 }, 1.0);
                acrylic.assert_content_visible();
                acrylic.set_effects_enabled_for_test(true);
                surface.material(hwnd, material);
                surface.acrylic.as_ref().unwrap().assert_material_effect();
                assert_eq!(surface.material, Some(material), "policy must not change the requested material");
            }
        }
    }

    #[test]
    fn pane_surface_uses_requested_composition_tree() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Pane composition isolation")
            .size(240, 160).style(WS_POPUP).ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .create().unwrap();
        let hwnd = HWND(window.hwnd().cast());
        let mut surface = Surface::new_pane(hwnd).unwrap();
        assert_eq!(surface.layer.is_none(), crate::pane::render_debug::shared_pane_tree());
        surface.material(hwnd, Backdrop::Acrylic);
        surface.present(240, 160, &[255; 240 * 160 * 4]).unwrap();
        assert!(surface.native);
        surface.opacity(0.75).unwrap();
        assert_eq!(surface.acrylic.as_ref().unwrap().opacity_value().unwrap(), 0.75);
        if crate::pane::render_debug::shared_pane_tree() {
            surface.acrylic.as_ref().unwrap().assert_content_visible();
        }
    }
    #[test]
    fn flyout_material_and_content_share_the_rounded_clip() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Rounded flyout regression")
            .size(240, 160).style(WS_POPUP).ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .create().unwrap();
        let hwnd = HWND(window.hwnd().cast());
        let mut surface = Surface::new_flyout(hwnd, 0.0).unwrap();
        assert!(surface.layer.is_none());
        let scale = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd.0) } as f32 / 96.0;
        for material in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::Solid { color: 0x123456, opacity: 0.5 }, Backdrop::Translucent { opacity: 0.8 }] {
            surface.material(hwnd, material);
            surface.resize(240, 160).unwrap();
            let acrylic = surface.acrylic.as_ref().unwrap();
            acrylic.assert_content_visible();
            acrylic.assert_rounded_clip(240, 160, (super::super::menu::CORNER_RADIUS + 0.5) * scale);
            for opacity in [0.0, 0.5, 1.0] {
                surface.opacity(opacity).unwrap();
                assert_eq!(acrylic.opacity_value().unwrap(), opacity);
            }
        }
    }

    pub(crate) fn settings_content_survives_material_changes_and_resize() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Settings composition regression")
            .size(240, 160)
            .style(WS_POPUP)
            .ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .create()
            .unwrap();
        let hwnd = HWND(window.hwnd().cast());
        {
            let mut surface = Surface::new_settings(hwnd).unwrap();
            assert!(
                surface.layer.is_none(),
                "settings must use one composition target"
            );
            for material in [
                Backdrop::Acrylic,
                Backdrop::Mica,
                Backdrop::MicaAlt,
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 0.0,
                },
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 0.5,
                },
                Backdrop::Solid {
                    color: 0x123456,
                    opacity: 1.0,
                },
                Backdrop::Translucent { opacity: 0.8 },
                Backdrop::Acrylic,
            ] {
                surface.material(hwnd, material);
                assert_eq!(
                    surface.native,
                    !matches!(material, Backdrop::Translucent { .. })
                );
                surface.acrylic.as_ref().unwrap().assert_content_visible();
                if matches!(
                    material,
                    Backdrop::Acrylic | Backdrop::Mica | Backdrop::MicaAlt
                ) {
                    surface.acrylic.as_ref().unwrap().assert_material_effect();
                }
                if let Backdrop::Solid { color, opacity } = material {
                    surface
                        .acrylic
                        .as_ref()
                        .unwrap()
                        .assert_solid_color(color, opacity);
                }
                surface.resize(320, 200).unwrap();
            }
            for dark in [false, true] {
                surface.theme(hwnd, dark);
                for material in [Backdrop::Acrylic, Backdrop::Mica, Backdrop::MicaAlt] {
                    for strength in [0, 25, 50, 75, 100] {
                        let tuned = material.with_strength(strength);
                        surface.material(hwnd, tuned);
                        surface.acrylic.as_ref().unwrap().assert_material_effect();
                        surface
                            .acrylic
                            .as_ref()
                            .unwrap()
                            .assert_material_colors(tuned, dark);
                        surface.acrylic.as_ref().unwrap().assert_content_visible();
                    }
                }
            }
        }
        unsafe {
            DestroyWindow(hwnd.0);
        }
    }

    #[test]
    fn warp_surface_draws_resizes_and_defers_without_losing_the_wakeup() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("WARP rendering regression")
            .size(96, 64)
            .style(WS_POPUP)
            .ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .create()
            .unwrap();
        let hwnd = HWND(window.hwnd().cast());
        let mut surface =
            Surface::new_with_device(hwnd, 1.0, GpuDevice::new_warp().unwrap()).unwrap();
        // Simulate an early retry callback: try_begin_frame must rearm a wakeup.
        surface.retry_at.set(Some(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
        assert!(surface.try_begin_frame(96, 64).unwrap().is_none());
        unsafe {
            windows_sys::Win32::Graphics::Gdi::ValidateRect(hwnd.0, std::ptr::null());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let mut message = MSG::default();
            unsafe {
                while PeekMessageW(&raw mut message, hwnd.0, WM_TIMER, WM_TIMER, PM_REMOVE) != 0 {
                    DispatchMessageW(&message);
                }
                if windows_sys::Win32::Graphics::Gdi::GetUpdateRect(hwnd.0, std::ptr::null_mut(), 0)
                    != 0
                {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "retry lost the final redraw"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        surface.retry_at.set(None);
        for (width, height) in [(96, 64), (144, 96), (96, 64)] {
            let target = surface.try_begin_frame(width, height).unwrap().unwrap();
            super::super::canvas::draw(&target, 1.0, |frame| {
                frame.clear(windows_canvas::ColorF::new(1.0, 0.0, 0.0, 1.0));
                frame.finish()
            })
            .unwrap();
            let pixels = surface.readback().unwrap();
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0, 0, 255, 255])
            );
            surface.end_frame().unwrap();
            surface.retry_at.set(None);
        }
        // Destruction must cancel even a still-pending queue retry.
        surface.schedule_retry(16).unwrap();
        drop(surface);
        assert_eq!(unsafe { KillTimer(hwnd.0, PRESENT_RETRY) }, 0);
    }

    #[test]
    fn fade_applies_to_native_material_and_finishes_after_a_delayed_tick() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Fade integration")
            .size(240, 160)
            .style(windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP)
            .ex_style(windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP)
            .create()
            .unwrap();
        let hwnd = HWND(window.hwnd().cast());
        {
            let mut surface = Surface::new_with_opacity(hwnd, 0.0).unwrap();
            surface.material(hwnd, Backdrop::Acrylic);
            assert!(surface.native);
            // Material attachment and the first content upload must not expose
            // an opaque frame before the fade gets its first sample.
            assert_eq!(
                surface.acrylic.as_ref().unwrap().opacity_value().unwrap(),
                0.0
            );
            surface.present(240, 160, &[255; 240 * 160 * 4]).unwrap();
            assert_eq!(
                surface.acrylic.as_ref().unwrap().opacity_value().unwrap(),
                0.0
            );
            let fade =
                super::super::animation::Fade::new(std::time::Duration::from_millis(120)).unwrap();
            for (millis, expected) in [(0, 0.0), (60, 0.75), (800, 1.0)] {
                let opacity = fade
                    .sample(std::time::Duration::from_millis(millis))
                    .unwrap();
                surface.opacity(opacity).unwrap();
                let material = surface.acrylic.as_ref().unwrap().opacity_value().unwrap();
                assert!((material - expected).abs() < 0.001);
            }
        }
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd.0);
        }
    }
}
