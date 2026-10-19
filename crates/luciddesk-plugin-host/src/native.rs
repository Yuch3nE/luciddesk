//! The only platform loader boundary. Rejected DLLs are owned; accepted ones are pinned.
use luciddesk_plugin_api::{ABI_VERSION, ENTRY_NAME, Entry, Invoke, read_entry};
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::HMODULE,
    System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    },
};

pub(super) struct Module(HMODULE);

impl Module {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // An absolute path and restricted dependency search avoid CWD/PATH resolution.
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self(module))
    }

    pub(super) fn entry(&self) -> Result<Invoke, String> {
        // The module owns the code and table until accepted or rejected by the runtime.
        unsafe {
            let entry = GetProcAddress(self.0, ENTRY_NAME.as_ptr().cast())
                .ok_or("Missing plugin entry point")?;
            let entry: Entry = std::mem::transmute(entry);
            read_entry(entry(ABI_VERSION)).map_err(|e| e.to_string())
        }
    }

    pub(super) fn pin(self) {
        // No hot unload: even a failed config_open/shutdown may have left native code running.
        std::mem::forget(self);
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        // Only entry/manifest calls, which cannot create resources, precede rejection.
        unsafe { windows_sys::Win32::Foundation::FreeLibrary(self.0) };
    }
}
