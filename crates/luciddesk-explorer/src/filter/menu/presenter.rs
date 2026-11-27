//! Explorer's unpublished presenter COM contract. QueryInterface gates use of
//! this ABI; no module offsets, code patches or object-layout fields are used.
//! This still requires compatibility testing when Windows changes the contract.
use std::{cell::Cell, ffi::c_void};
use windows::{
    Win32::{
        Foundation::{E_NOINTERFACE, E_POINTER, HWND},
        System::Com::{
            CLSCTX_INPROC_SERVER, CoCreateInstance, IServiceProvider, IServiceProvider_Impl,
        },
        UI::Shell::IShellView,
    },
    core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Result, implement},
};
use windows_core::ComObjectInner;
mod input;
const SERVICE: GUID = GUID::from_u128(0xb306c5b1_b4f2_473c_b6ff_701b246ce2d2);
windows_core::define_interface!(
    Presenter,
    PresenterVtbl,
    0x37a472f7_63cf_4ccf_a88b_5231a3c7d8b6
);
#[repr(C)]
pub struct PresenterVtbl {
    base: IUnknown_Vtbl,
    initialize: unsafe extern "system" fn(*mut c_void, i32, *mut c_void, HWND, i32) -> HRESULT,
    // Prepare, DoContextMenu; signatures are forwarded by the input adapter.
    _menu_methods: [usize; 2],
    dismiss: unsafe extern "system" fn(*mut c_void, *const u16),
    _is_open: usize,
    invoke: unsafe extern "system" fn(*mut c_void, u32),
}
#[implement(IServiceProvider)]
struct Site {
    presenter: Presenter,
    input: IUnknown,
    keyboard: std::rc::Rc<Cell<bool>>,
    // The native presenter stores a borrowed callback. Retain it through Close.
    _callback: IUnknown,
    closed: Cell<bool>,
    close_complete: Cell<bool>,
    invoke_message: u32,
    cancelled: std::rc::Rc<dyn Fn() -> bool>,
}
impl IServiceProvider_Impl for Site_Impl {
    fn QueryService(
        &self,
        service: *const GUID,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        unsafe {
            if service.is_null() || iid.is_null() || out.is_null() {
                return Err(E_POINTER.into());
            }
            *out = std::ptr::null_mut();
            if *service != SERVICE {
                return Err(E_NOINTERFACE.into());
            }
            (self.input.vtable().QueryInterface)(self.input.as_raw(), iid, out).ok()
        }
    }
}
impl Site {
    fn close(&self) -> Result<()> {
        self.closed.set(true);
        if !self.close_complete.get() {
            self.presenter
                .cast::<windows::Foundation::IClosable>()?
                .Close()?;
            self.close_complete.set(true);
        }
        Ok(())
    }
}
impl Drop for Site {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[derive(Clone)]
pub struct NativePresenter {
    site: windows::core::ComObject<Site>,
}
impl NativePresenter {
    pub fn create(
        view: &IShellView,
        hwnd: HWND,
        desktop: HWND,
        first: std::rc::Rc<Cell<Option<u32>>>,
        cancelled: std::rc::Rc<dyn Fn() -> bool>,
    ) -> Result<Self> {
        unsafe {
            let invoke_message =
                windows_sys::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(
                    windows_sys::w!("FILE_EXPLORER_CONTEXTMENU_INVOKEMENUITEM"),
                );
            if invoke_message == 0 {
                return Err(windows::core::Error::from_thread());
            }
            let presenter: Presenter = CoCreateInstance(
                &GUID::from_u128(0x86ca1aa0_34aa_4e8b_a509_50c905bae2a2),
                None,
                CLSCTX_INPROC_SERVER,
            )?;
            let callback = super::callback::create(view, hwnd, desktop, first, cancelled.clone())?;
            let keyboard = std::rc::Rc::new(Cell::new(false));
            let input =
                input::wrap_cancellable(presenter.cast()?, keyboard.clone(), cancelled.clone())?;
            let site = Site {
                presenter,
                input,
                keyboard,
                _callback: callback,
                closed: Cell::new(false),
                close_complete: Cell::new(false),
                invoke_message,
                cancelled,
            }
            .into_object();
            (site.presenter.vtable().initialize)(
                site.presenter.as_raw(),
                1,
                site._callback.as_raw(),
                hwnd,
                0,
            )
            .ok()?;
            Ok(Self { site })
        }
    }
    pub fn service(&self) -> IServiceProvider {
        self.site.to_interface()
    }
    pub fn set_keyboard_invocation(&self, keyboard: bool, _hwnd: isize) {
        #[cfg(any(debug_assertions, feature = "menu-diagnostics"))]
        input::trace_host(_hwnd);
        self.site.keyboard.set(keyboard);
    }
    pub fn close(&self) -> Result<()> {
        self.site.close()
    }
    pub fn dismiss(&self) {
        if !self.site.closed.get() {
            unsafe {
                (self.site.presenter.vtable().dismiss)(self.site.presenter.as_raw(), windows_sys::w!("LucidDesk.Cancel"));
            }
        }
    }
    pub fn handle_command_message(&self, message: u32, command: usize) -> bool {
        if message != self.site.invoke_message {
            return false;
        }
        if !self.site.closed.get() && !(self.site.cancelled)() {
            // XAML posts a command ID to the view. A full Explorer browser
            // forwards it to its presenter; IExplorerBrowser does not. Perform
            // that missing host step for this isolated view only. Invoke also
            // closes the flyout before calling the view's native command target.
            let presenter = self.site.presenter.clone();
            unsafe {
                (presenter.vtable().invoke)(presenter.as_raw(), command as u32);
            }
        }
        true
    }
}
