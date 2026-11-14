//! OLE drop registration, drag previews, and temporary drop descriptions.
mod description;
pub(super) mod image;
pub(super) mod target;

use luciddesk_core::{PanelId, PointDip, ShellIdentity};
use std::{cell::RefCell, mem::DropGuard, rc::Rc};
use windows::{
    Win32::{Foundation::E_UNEXPECTED, System::Com::IDataObject},
    core::{Error, IUnknown, Interface, Result},
};

#[derive(Clone)]
pub(super) struct PaneDrag {
    pub source: PanelId,
    pub grabbed: ShellIdentity,
    pub items: Vec<ShellIdentity>,
    pub offset: PointDip,
}

thread_local! {
    static ACTIVE_DRAG: RefCell<Option<(IUnknown, Rc<PaneDrag>)>> = const { RefCell::new(None) };
}

/// Associate pane state with the exact Shell object for the duration of the OLE loop.
pub(super) fn with_pane_drag(
    data: &IDataObject,
    drag: PaneDrag,
    run: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let identity = data.cast::<IUnknown>()?;
    ACTIVE_DRAG.with(|slot| {
        let mut active = slot.borrow_mut();
        if active.is_some() {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        *active = Some((identity, Rc::new(drag)));
        Ok(())
    })?;
    let _reset = DropGuard::new((), |()| {
        let active = ACTIVE_DRAG.with(|slot| slot.borrow_mut().take());
        drop(active);
    });
    run()
}

fn pane_drag(data: &IDataObject) -> Option<Rc<PaneDrag>> {
    let (source, drag) = ACTIVE_DRAG.with(|slot| slot.borrow().clone())?;
    let identity = data.cast::<IUnknown>().ok()?;
    (source.as_raw() == identity.as_raw()).then_some(drag)
}

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
