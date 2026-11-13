//! OLE drop registration, drag previews, and temporary drop descriptions.
mod description;
pub(super) mod image;
pub(super) mod target;

/// Pane sorting and desktop membership use internal drag handling; other
/// applications need an OLE data object to receive the dragged files.
pub(super) fn over_external_window(point: windows_sys::Win32::Foundation::POINT) -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetClassNameW, GetWindowThreadProcessId, WindowFromPoint,
    };
    unsafe {
        let target = GetAncestor(WindowFromPoint(point), GA_ROOT);
        if target.is_null() {
            return false;
        }
        let mut process = 0;
        GetWindowThreadProcessId(target, &mut process);
        if process == GetCurrentProcessId() {
            return false;
        }
        let mut class = [0u16; 64];
        let length = GetClassNameW(target, class.as_mut_ptr(), class.len() as i32);
        let class = &class[..length as usize];
        !class.iter().copied().eq("Progman".encode_utf16())
            && !class.iter().copied().eq("WorkerW".encode_utf16())
    }
}
