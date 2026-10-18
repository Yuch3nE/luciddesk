//! Interoperation between application bindings and the Canvas/generated graphics ABI.
use canvas_core::Interface as _;
pub use dwm::{
    DWMNCRP_DISABLED, DWMSBT_NONE, DWMWA_BORDER_COLOR, DWMWA_NCRENDERING_POLICY,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_HOSTBACKDROPBRUSH, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND, MARGINS,
};
use luciddesk_graphics::dwm;
use windows::Win32::Foundation::HWND;
use windows::core::{Error, HRESULT, IUnknown, Interface, Result};

thread_local! {
    static DEVICE: std::cell::RefCell<Option<windows_canvas::GpuDevice>> = const { std::cell::RefCell::new(None) };
}

/// Declare after the COM/OLE apartment and before any windows or renderers.
/// DXGI may unload helper DLLs on final release; doing that from a Windows TLS
/// destructor during process shutdown can raise DXGI_ERROR_INVALID_CALL.
pub(super) struct GraphicsLifetime;

impl Drop for GraphicsLifetime {
    fn drop(&mut self) {
        crate::pane::render_debug::render_trace(format_args!("shutdown: graphics caches begin"));
        crate::plugins::shutdown();
        super::acrylic::clear_thread_cache();
        luciddesk_graphics::clear_thread_cache();
        let device = DEVICE.with(|slot| slot.borrow_mut().take());
        drop(device);
        crate::pane::render_debug::render_trace(format_args!("shutdown: graphics caches released"));
    }
}

/// All pane and flyout contexts on the UI thread share one graphics device.
pub fn gpu_device() -> Result<windows_canvas::GpuDevice> {
    DEVICE.with(|slot| {
        let mut cached = slot.borrow_mut();
        if let Some(device) = cached.as_ref() {
            let native: windows::Win32::Graphics::Direct3D11::ID3D11Device =
                native_interface(device.d3d_device())?;
            if unsafe { native.GetDeviceRemovedReason() }.is_ok() {
                return Ok(device.clone());
            }
        }
        let device = if std::env::var("LUCIDDESK_RENDERER")
            .is_ok_and(|value| value.eq_ignore_ascii_case("warp"))
        {
            canvas_result(windows_canvas::GpuDevice::new_warp())?
        } else {
            canvas_result(windows_canvas::GpuDevice::new().or_else(|error| {
                luciddesk_diagnostics::log(
                    luciddesk_diagnostics::Level::Warn,
                    "pane.native_graphics",
                    &format!("Hardware rendering unavailable, using WARP: {error}"),
                );
                windows_canvas::GpuDevice::new_warp()
            }))?
        };
        *cached = Some(device.clone());
        Ok(device)
    })
}

/// # Safety
/// The attribute and value must match the DWM API's expected layout.
pub unsafe fn set_attribute<T>(hwnd: HWND, attribute: i32, value: &T) -> Result<()> {
    HRESULT(unsafe {
        dwm::DwmSetWindowAttribute(
            hwnd.0,
            attribute as u32,
            std::ptr::from_ref(value).cast(),
            size_of::<T>() as u32,
        )
    })
    .ok()
}

/// # Safety
/// The HWND must belong to a live window.
pub unsafe fn extend_frame(hwnd: HWND, margins: &MARGINS) -> Result<()> {
    HRESULT(unsafe { dwm::DwmExtendFrameIntoClientArea(hwnd.0, margins) }).ok()
}

pub fn create_layer(
    hwnd: HWND,
    dxgi: &windows::Win32::Graphics::Dxgi::IDXGIDevice,
    content: &windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    opacity: f32,
) -> Result<luciddesk_graphics::Layer> {
    let dxgi_ptr = dxgi.as_raw();
    let content_ptr = content.as_raw();
    canvas_result(unsafe {
        luciddesk_graphics::Layer::new(
            hwnd.0,
            canvas_core::IUnknown::from_raw_borrowed(&dxgi_ptr).unwrap(),
            canvas_core::IUnknown::from_raw_borrowed(&content_ptr).unwrap(),
            opacity,
        )
    })
}

/// QueryInterface returns an owned reference without transferring the source's ownership.
pub fn native_interface<T: Interface>(source: &impl canvas_core::Interface) -> Result<T> {
    let raw = source.as_raw();
    unsafe { IUnknown::from_raw_borrowed(&raw).unwrap().cast() }
}

pub fn canvas_result<T>(result: canvas_core::Result<T>) -> Result<T> {
    result.map_err(|error| Error::from_hresult(HRESULT(error.code().0)))
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[test]
    fn graphics_caches_release_before_apartment_and_process_exit() {
        const CHILD: &str = "LUCIDDESK_GRAPHICS_SHUTDOWN_TEST";
        if std::env::var_os(CHILD).is_some() {
            let _apartment = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
            let graphics = GraphicsLifetime;
            for shared in [false, true] {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                let window = windows_window::Window::new("Graphics shutdown regression")
                    .size(96, 64)
                    .style(WS_POPUP)
                    .ex_style(WS_EX_NOREDIRECTIONBITMAP)
                    .on_message(|_, msg, _, _| (msg == WM_DESTROY).then_some(0))
                    .create()
                    .unwrap();
                let hwnd = HWND(window.hwnd().cast());
                let mut surface = if shared {
                    super::super::composition::Surface::new_settings(hwnd)
                } else {
                    super::super::composition::Surface::new_pane(hwnd)
                }
                .unwrap();
                surface.material(hwnd, luciddesk_core::Backdrop::Acrylic);
                surface.present(96, 64, &[255; 96 * 64 * 4]).unwrap();
            }
            assert!(DEVICE.with(|slot| slot.borrow().is_some()));
            drop(graphics);
            assert!(DEVICE.with(|slot| slot.borrow().is_none()));
            return;
        }
        // Check the actual process exit: an in-process assertion cannot catch
        // a later crash from TLS destruction after the test has returned.
        for _ in 0..3 {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "pane::native_graphics::shutdown_tests::graphics_caches_release_before_apartment_and_process_exit",
                    "--test-threads=1", "--nocapture"])
                .env(CHILD, "1").env("LUCIDDESK_SHARED_PANE_TREE", "0")
                .output().unwrap();
            assert!(
                output.status.success(),
                "graphics shutdown failed: {:?}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
