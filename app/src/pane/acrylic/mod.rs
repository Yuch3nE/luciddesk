//! Host backdrop composition has no window-activation policy. Windows still controls
//! backdrop transparency through accessibility settings and power policy.
mod effects;
mod transition;
mod host;
mod runtime;
mod material;
use runtime::Runtime;

use std::{cell::RefCell, rc::Rc};
use windows::{
    UI::{
        Color,
        Composition::{
            CompositionRoundedRectangleGeometry, ContainerVisual,
            Desktop::DesktopWindowTarget,
        },
    },
    Win32::{
        Foundation::HWND,
        System::WinRT::Composition::ICompositorDesktopInterop,
    },
    core::{Interface, Result},
};
use windows_numerics::Vector2;

struct PowerNotification(windows_sys::Win32::System::Power::HPOWERNOTIFY);
impl Drop for PowerNotification {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { windows_sys::Win32::System::Power::UnregisterPowerSettingNotification(self.0); }
        }
    }
}

pub(super) fn clear_thread_cache() {
    runtime::clear_thread_cache();
}

pub struct Acrylic {
    hwnd: HWND,
    _power_notification: PowerNotification,
    ui_settings: Option<windows::UI::ViewManagement::UISettings>,
    #[cfg(test)]
    effects_override: std::cell::Cell<Option<bool>>,
    disable_backdrop: bool,
    material_brush: RefCell<Option<(bool, windows::UI::Composition::CompositionBrush)>>,
    host_backdrop: RefCell<Option<host::HostBackdrop>>,
    #[cfg(test)]
    unavailable_backdrops: std::cell::Cell<(bool, bool)>,
    runtime: Rc<Runtime>,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    backdrop: windows::UI::Composition::SpriteVisual,
    tint: windows::UI::Composition::SpriteVisual,
    transition: RefCell<Option<transition::Pending>>,
    transition_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    target_brush: RefCell<Option<windows::UI::Composition::CompositionBrush>>,
    #[cfg(test)]
    animations_override: std::cell::Cell<Option<bool>>,
    material_initialized: std::cell::Cell<bool>,
    rounded_clip: Option<(CompositionRoundedRectangleGeometry, (u32, u32, f32))>,
    content: Option<windows::UI::Composition::SpriteVisual>,
}

impl Acrylic {
    #[allow(dead_code)] // Used by the standalone backdrop probe.
    pub fn new(hwnd: HWND) -> Result<Self> {
        let material = Self::new_with_opacity(hwnd, 1.0)?;
        material.material(luciddesk_core::Backdrop::Acrylic, true)?;
        Ok(material)
    }

    pub fn new_with_opacity(hwnd: HWND, initial_opacity: f32) -> Result<Self> {
        Self::new_target(hwnd, initial_opacity, false)
    }

    pub fn new_with_content(
        hwnd: HWND,
        initial_opacity: f32,
        swap: &windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    ) -> Result<Self> {
        let mut material = Self::new_target(hwnd, initial_opacity, true)?;
        material.attach_content(swap)?;
        Ok(material)
    }

    fn new_target(hwnd: HWND, initial_opacity: f32, topmost: bool) -> Result<Self> {
        let runtime = Runtime::shared()?;
        let compositor = &runtime.compositor;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        // A standalone material uses the bottom target; a combined content tree
        // owns the top target so NOREDIRECTIONBITMAP windows remain visible.
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, topmost)? };
        let root = compositor.CreateContainerVisual()?;
        root.SetOpacity(initial_opacity)?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        let backdrop = compositor.CreateSpriteVisual()?;
        backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        backdrop.SetBrush(&compositor.CreateColorBrush()?)?;
        let tint = compositor.CreateSpriteVisual()?;
        tint.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        tint.SetBrush(&compositor.CreateColorBrushWithColor(Color {
            A: 120,
            R: 24,
            G: 27,
            B: 32,
        })?)?;
        root.Children()?.InsertAtBottom(&backdrop)?;
        root.Children()?.InsertAtTop(&tint)?;
        target.SetRoot(&root)?;
        Ok(Self {
            hwnd,
            _power_notification: PowerNotification(unsafe {
                windows_sys::Win32::System::Power::RegisterPowerSettingNotification(
                    hwnd.0,
                    &windows_sys::Win32::System::SystemServices::GUID_POWER_SAVING_STATUS,
                    windows_sys::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE,
                )
            }),
            ui_settings: windows::UI::ViewManagement::UISettings::new().ok(),
            #[cfg(test)]
            // Rendering tests control policy explicitly, independent of the
            // test machine's current transparency or battery-saver setting.
            effects_override: std::cell::Cell::new(Some(true)),
            disable_backdrop: crate::pane::render_debug::disable_backdrop(),
            material_brush: RefCell::new(None),
            host_backdrop: RefCell::new(None),
            #[cfg(test)]
            unavailable_backdrops: std::cell::Cell::new((false, false)),
            runtime,
            _target: target,
            root,
            backdrop,
            tint,
            transition: RefCell::new(None),
            transition_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            target_brush: RefCell::new(None),
            #[cfg(test)]
            animations_override: std::cell::Cell::new(Some(false)),
            material_initialized: std::cell::Cell::new(false),
            rounded_clip: None,
            content: None,
        })
    }

    pub fn round_corners(&mut self, width: u32, height: u32, radius: f32) -> Result<()> {
        let bounds = (width, height, radius);
        if self
            .rounded_clip
            .as_ref()
            .is_some_and(|(_, old)| *old == bounds)
        {
            return Ok(());
        }
        let geometry = if let Some((geometry, _)) = &self.rounded_clip {
            geometry.clone()
        } else {
            self.runtime.compositor.CreateRoundedRectangleGeometry()?
        };
        geometry.SetSize(Vector2 {
            X: width as f32,
            Y: height as f32,
        })?;
        geometry.SetCornerRadius(Vector2 {
            X: radius,
            Y: radius,
        })?;
        if self.rounded_clip.is_none() {
            let clip = self
                .runtime
                .compositor
                .CreateGeometricClipWithGeometry(&geometry)?;
            self.root.SetClip(&clip)?;
        }
        self.rounded_clip = Some((geometry, bounds));
        Ok(())
    }

    pub(super) fn effects_enabled(&self) -> bool {
        #[cfg(test)]
        if let Some(enabled) = self.effects_override.get() {
            return enabled;
        }
        let advanced = self.ui_settings.as_ref()
            .and_then(|settings| settings.AdvancedEffectsEnabled().ok())
            .unwrap_or(true);
        let mut power = windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS::default();
        let saver = unsafe {
            windows_sys::Win32::System::Power::GetSystemPowerStatus(&raw mut power) != 0
                && power.SystemStatusFlag == 1
        };
        advanced && !saver
    }

    #[cfg(test)]
    pub(super) fn set_effects_enabled_for_test(&self, enabled: bool) {
        self.effects_override.set(Some(enabled));
    }

    pub fn visible(&self, visible: bool) -> Result<()> {
        // Content can share this tree; switching to a plain background must
        // hide only the material, never the application's content visual.
        self.backdrop.SetIsVisible(visible)?;
        self.tint.SetIsVisible(visible)?;
        if !visible {
            self.clear_transition()?;
            self.material_initialized.set(false);
            self.host_backdrop.borrow_mut().take();
        }
        Ok(())
    }

    fn attach_content(
        &mut self,
        swap: &windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    ) -> Result<()> {
        use windows::Win32::System::WinRT::Composition::ICompositorInterop;
        let compositor = &self.runtime.compositor;
        let interop: ICompositorInterop = compositor.cast()?;
        let surface = unsafe { interop.CreateCompositionSurfaceForSwapChain(swap)? };
        let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        let visual = compositor.CreateSpriteVisual()?;
        visual.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        visual.SetBrush(&brush)?;
        self.root.Children()?.InsertAtTop(&visual)?;
        self.content = Some(visual);
        Ok(())
    }

    pub fn commit_ready(&self) -> Result<Box<dyn Fn() -> bool>> {
        let commit = self.runtime.compositor.RequestCommitAsync()?;
        Ok(Box::new(move || {
            commit.Status().map_or(true, |status| status.0 != 0)
        }))
    }

    pub fn opacity(&self, opacity: f32) -> Result<()> {
        self.root.SetOpacity(opacity)
    }

    pub fn fade_in(&self) -> Result<()> {
        let animation = self.runtime.compositor.CreateScalarKeyFrameAnimation()?;
        animation.InsertKeyFrame(0.0, 0.0)?;
        animation.InsertKeyFrame(1.0, 1.0)?;
        animation.SetDuration(windows::Foundation::TimeSpan {
            Duration: (super::animation::SETTINGS_DURATION.as_nanos() / 100) as i64,
        })?;
        self.root
            .StartAnimation(&windows::core::HSTRING::from("Opacity"), &animation)
    }
}

/// Shared material recipe for the compositor and the illustrative settings preview.
pub(super) fn material_colors(material: luciddesk_core::Backdrop, dark: bool) -> (Color, Color) {
    let colors = if matches!(
        material.base(),
        luciddesk_core::Backdrop::Mica | luciddesk_core::Backdrop::MicaAlt
    ) {
        effects::mica_palette(dark, material.base() == luciddesk_core::Backdrop::MicaAlt)
    } else {
        effects::acrylic_palette(dark)
    };
    effects::adjust_strength(colors.0, colors.1, material.strength().unwrap_or(50))
}

#[cfg(test)]
mod tests;
