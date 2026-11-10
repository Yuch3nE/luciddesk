//! Composition device caching and visual layer lifetime.
use crate::dcomp;
use windows_core::{IUnknown, Interface, Result};

pub struct Layer {
    device: dcomp::IDCompositionDevice,
    _target: dcomp::IDCompositionTarget,
    _visual: dcomp::IDCompositionVisual,
    opacity: dcomp::IDCompositionEffectGroup,
}
thread_local! {
    // Keep one composition device per UI thread and graphics device. Retaining
    // the COM identity prevents pointer reuse after graphics device recovery.
    static DEVICE: std::cell::RefCell<Option<(IUnknown, dcomp::IDCompositionDevice)>> = const { std::cell::RefCell::new(None) };
}

/// Release cached composition resources on the UI thread before COM teardown.
/// All layers must have been dropped first.
pub fn clear_thread_cache() {
    let cached = DEVICE.with(|cache| cache.borrow_mut().take());
    drop(cached);
}

unsafe fn composition_device(dxgi: &IUnknown) -> Result<dcomp::IDCompositionDevice> {
    let identity: IUnknown = dxgi.cast()?;
    DEVICE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((current, device)) = cache.as_ref()
            && current == &identity
        {
            return Ok(device.clone());
        }
        unsafe {
            let mut raw = core::ptr::null_mut();
            dcomp::DCompositionCreateDevice(
                dxgi.as_raw(),
                &dcomp::IDCompositionDevice::IID,
                &mut raw,
            )
            .ok()?;
            let device = dcomp::IDCompositionDevice::from_raw(raw);
            *cache = Some((identity, device.clone()));
            Ok(device)
        }
    })
}
impl Layer {
    /// Attaches a composition swap chain above or below the native child windows.
    ///
    /// # Safety
    /// `hwnd` must remain valid on the calling UI thread while this layer exists.
    /// `dxgi` must borrow an actual IDXGIDevice interface pointer, and `content`
    /// must refer to a composition-compatible swap chain from that device.
    /// COM must be initialized. The caller retains ownership of both arguments.
    pub unsafe fn new(
        hwnd: *mut core::ffi::c_void,
        dxgi: &IUnknown,
        content: &IUnknown,
        initial_opacity: f32,
        above_children: bool,
    ) -> Result<Self> {
        unsafe {
            let device = composition_device(dxgi)?;
            let target = device.CreateTargetForHwnd(hwnd, above_children)?;
            let visual = device.CreateVisual()?;
            let opacity = device.CreateEffectGroup()?;
            opacity.SetOpacity2(initial_opacity).ok()?;
            visual.SetEffect(&opacity).ok()?;
            visual.SetContent(content).ok()?;
            target.SetRoot(&visual).ok()?;
            device.Commit().ok()?;
            Ok(Self {
                device,
                _target: target,
                _visual: visual,
                opacity,
            })
        }
    }
    pub fn opacity(&self, value: f32) -> Result<()> {
        unsafe { self.opacity.SetOpacity2(value).ok() }
    }
    pub fn commit(&self) -> Result<()> {
        unsafe { self.device.Commit().ok() }
    }
}
