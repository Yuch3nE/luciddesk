//! Thread-bound OLE initialization.
use std::{cell::Cell, marker::PhantomData, rc::Rc};
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};

use crate::ShellError;

thread_local! {
    static ACTIVE_GUARDS: Cell<usize> = const { Cell::new(0) };
}

/// Keeps OLE initialized on the calling thread for Shell UI and drag/drop.
///
/// Keep this guard alive until all OLE-dependent resources have been released.
/// Each successful initialization is balanced by one uninitialization on drop.
/// The last guard on the thread persists any file clipboard owned by this crate.
/// The guard cannot be constructed directly, cloned, or sent to another thread.
///
/// ```no_run
/// let apartment = luciddesk_shell::ShellApartment::initialize_sta()?;
/// // Perform Shell operations while `apartment` remains alive.
/// drop(apartment);
/// # Ok::<(), luciddesk_shell::ShellError>(())
/// ```
///
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<luciddesk_shell::ShellApartment>();
/// ```
///
/// ```compile_fail
/// fn require_sync<T: Sync>() {}
/// require_sync::<luciddesk_shell::ShellApartment>();
/// ```
///
/// ```compile_fail
/// let apartment = luciddesk_shell::ShellApartment {};
/// ```
#[derive(Debug)]
#[must_use = "keep the guard alive while using OLE-dependent resources"]
pub struct ShellApartment {
    _thread_bound: PhantomData<Rc<()>>,
}

impl ShellApartment {
    /// Initializes OLE on the calling UI thread.
    ///
    /// # Errors
    /// Returns a COM error if OLE initialization fails, including when this
    /// thread already uses an incompatible apartment model.
    pub fn initialize_sta() -> Result<Self, ShellError> {
        // SAFETY: the reserved argument is null. Only a successful call creates
        // a guard, whose thread-bound ownership balances this call in Drop.
        unsafe { OleInitialize(None) }?;
        ACTIVE_GUARDS.set(ACTIVE_GUARDS.get() + 1);
        Ok(Self {
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for ShellApartment {
    fn drop(&mut self) {
        if ACTIVE_GUARDS.get() == 1 {
            // Keep this guard counted while OLE may dispatch reentrant calls.
            crate::file_command::flush_file_clipboard();
        }
        ACTIVE_GUARDS.set(ACTIVE_GUARDS.get() - 1);
        // SAFETY: safe code cannot move this guard off its initializing thread.
        unsafe { OleUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::ShellApartment;
    use windows::Win32::System::Com::{APTTYPE, APTTYPEQUALIFIER, CoGetApartmentType};

    #[test]
    fn nested_guards_balance_each_successful_initialization() {
        std::thread::spawn(|| {
            let mut apartment_type = APTTYPE::default();
            let mut qualifier = APTTYPEQUALIFIER::default();
            let first = ShellApartment::initialize_sta().unwrap();
            let second = ShellApartment::initialize_sta().unwrap();
            drop(first);
            assert!(
                unsafe { CoGetApartmentType(&raw mut apartment_type, &raw mut qualifier) }.is_ok()
            );
            drop(second);
            assert!(
                unsafe { CoGetApartmentType(&raw mut apartment_type, &raw mut qualifier) }.is_err()
            );
        })
        .join()
        .unwrap();
    }
}
