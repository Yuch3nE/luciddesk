windows_link::link!("dwmapi.dll" "system" fn DwmExtendFrameIntoClientArea(hwnd : HWND, pmarinset : *const MARGINS) -> HRESULT);
windows_link::link!("dwmapi.dll" "system" fn DwmSetWindowAttribute(hwnd : HWND, dwattribute : u32, pvattribute : *const core::ffi::c_void, cbattribute : u32) -> HRESULT);
pub type DWMNCRENDERINGPOLICY = i32;
pub const DWMNCRP_DISABLED: DWMNCRENDERINGPOLICY = 1;
pub const DWMSBT_NONE: DWM_SYSTEMBACKDROP_TYPE = 1;
pub const DWMWA_BORDER_COLOR: DWMWINDOWATTRIBUTE = 34;
pub const DWMWA_NCRENDERING_POLICY: DWMWINDOWATTRIBUTE = 2;
pub const DWMWA_SYSTEMBACKDROP_TYPE: DWMWINDOWATTRIBUTE = 38;
pub const DWMWA_USE_HOSTBACKDROPBRUSH: DWMWINDOWATTRIBUTE = 17;
pub const DWMWA_USE_IMMERSIVE_DARK_MODE: DWMWINDOWATTRIBUTE = 20;
pub const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = 33;
pub const DWMWCP_DONOTROUND: DWM_WINDOW_CORNER_PREFERENCE = 1;
pub const DWMWCP_ROUND: DWM_WINDOW_CORNER_PREFERENCE = 2;
pub type DWMWINDOWATTRIBUTE = i32;
pub type DWM_SYSTEMBACKDROP_TYPE = i32;
pub type DWM_WINDOW_CORNER_PREFERENCE = i32;
pub type HRESULT = i32;
pub type HWND = *mut core::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MARGINS {
    pub cxLeftWidth: i32,
    pub cxRightWidth: i32,
    pub cyTopHeight: i32,
    pub cyBottomHeight: i32,
}
