//! HWND host-backdrop setup. Keep the downlevel accent ABI behind this boundary.
use super::super::native_graphics::{DWMWA_USE_HOSTBACKDROPBRUSH, set_attribute};
use windows::{
    Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, HWND},
    core::{Error, Result},
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

#[repr(C)]
struct AccentPolicy {
    state: i32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct AttributeData {
    attribute: i32,
    data: *const AccentPolicy,
    size: usize,
}

type SetAttribute =
    unsafe extern "system" fn(windows_sys::Win32::Foundation::HWND, *const AttributeData) -> i32;

fn legacy_setter() -> Result<SetAttribute> {
    let module = unsafe { GetModuleHandleW(windows_sys::w!("user32.dll")) };
    let address = if module.is_null() {
        None
    } else {
        unsafe { GetProcAddress(module, windows_sys::s!("SetWindowCompositionAttribute")) }
    };
    let address = address.ok_or_else(|| Error::from_hresult(E_NOTIMPL))?;
    // user32 remains loaded for the lifetime of our HWNDs. The export's
    // signature and SIZE_T layout match AttributeData above.
    Ok(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, SetAttribute>(address)
    })
}

pub(super) struct HostBackdrop {
    hwnd: HWND,
    legacy: Option<SetAttribute>,
}

impl HostBackdrop {
    pub(super) fn enable(hwnd: HWND) -> Result<Self> {
        match unsafe { set_attribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &1i32) } {
            Ok(()) => {
                crate::pane::render_debug::render_trace(format_args!("hwnd={hwnd:?} host_backdrop=public"));
                Ok(Self { hwnd, legacy: None })
            }
            Err(error) if matches!(error.code(), E_INVALIDARG | E_NOTIMPL) => {
                Self::enable_legacy(hwnd, legacy_setter()?)
            }
            Err(error) => Err(error),
        }
    }

    fn enable_legacy(hwnd: HWND, set: SetAttribute) -> Result<Self> {
        let policy = Self {
            hwnd,
            legacy: Some(set),
        };
        if policy.set_legacy(5) {
            crate::pane::render_debug::render_trace(format_args!("hwnd={hwnd:?} host_backdrop=legacy_accent"));
            // ACCENT_ENABLE_HOSTBACKDROP
            Ok(policy)
        } else {
            Err(Error::from_hresult(E_NOTIMPL))
        }
    }

    fn set_legacy(&self, state: i32) -> bool {
        let policy = AccentPolicy {
            state,
            flags: 0,
            color: 0,
            animation: 0,
        };
        let data = AttributeData {
            attribute: 19, // WCA_ACCENT_POLICY
            data: std::ptr::from_ref(&policy),
            size: size_of::<AccentPolicy>(),
        };
        unsafe { self.legacy.unwrap()(self.hwnd.0, std::ptr::from_ref(&data)) != 0 }
    }
}

impl Drop for HostBackdrop {
    fn drop(&mut self) {
        if self.legacy.is_some() {
            // A hidden material must not leave whole-window accent rendering behind.
            let _ = self.set_legacy(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    thread_local! { static STATES: std::cell::RefCell<Vec<i32>> = const { std::cell::RefCell::new(Vec::new()) }; }

    unsafe extern "system" fn record(
        _: windows_sys::Win32::Foundation::HWND,
        data: *const AttributeData,
    ) -> i32 {
        let data = unsafe { &*data };
        if data.attribute != 19 || data.size != 16 {
            return 0;
        }
        let policy = unsafe { &*data.data };
        if (policy.flags, policy.color, policy.animation) != (0, 0, 0) {
            return 0;
        }
        STATES.with(|states| states.borrow_mut().push(policy.state));
        1
    }

    #[test]
    fn legacy_host_policy_releases_its_accent_with_the_correct_abi() {
        STATES.with(|states| states.borrow_mut().clear());
        let policy = HostBackdrop::enable_legacy(HWND(std::ptr::null_mut()), record).unwrap();
        STATES.with(|states| assert_eq!(*states.borrow(), [5]));
        drop(policy);
        STATES.with(|states| assert_eq!(*states.borrow(), [5, 0]));
        assert_eq!(size_of::<AttributeData>(), 3 * size_of::<usize>());
    }

    #[test]
    fn legacy_host_entry_accepts_a_native_window() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Host backdrop compatibility")
            .size(96, 64)
            .style(windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP)
            .ex_style(windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP)
            .create()
            .unwrap();
        let policy =
            HostBackdrop::enable_legacy(HWND(window.hwnd().cast()), legacy_setter().unwrap())
                .unwrap();
        drop(policy);
    }
}
