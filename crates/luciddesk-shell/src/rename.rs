use luciddesk_core::ShellIdentity;
use std::{
    cell::RefCell, ffi::{OsStr, OsString}, os::windows::ffi::{OsStrExt, OsStringExt}, rc::Rc,
};
use windows::Win32::UI::Shell::{IFileOperationProgressSink, IFileOperationProgressSink_Impl};
use windows::core::{HRESULT, PCWSTR, Ref, implement};
use windows::{
    Win32::{
        Foundation::{E_FAIL, HWND, S_OK},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::Shell::{
            COPYENGINE_S_COLLISIONRESOLVED, FileOperation, IFileOperation, IShellItem,
            SHCreateItemFromParsingName,
        },
    },
    core::{HSTRING, Result},
};

#[derive(Debug)]
pub enum RenameError {
    InvalidName,
    MissingResult,
    Shell(windows::core::Error),
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => formatter.write_str("The rename name is empty or contains a path separator"),
            Self::MissingResult => formatter.write_str("The Shell did not report a rename result"),
            Self::Shell(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Shell(error) => Some(error),
            _ => None,
        }
    }
}

impl From<windows::core::Error> for RenameError {
    fn from(error: windows::core::Error) -> Self {
        Self::Shell(error)
    }
}

/// Rename through Shell so namespace rules and Explorer change notifications apply.
/// Returns false if the user cancels a Shell conflict or permission dialog.
/// # Errors
/// Returns Shell errors for invalid names, unavailable items, or failed operations.
pub fn rename_shell_identity(
    owner: HWND,
    identity: &ShellIdentity,
    name: &str,
) -> std::result::Result<bool, RenameError> {
    Ok(commit_shell_rename(owner, identity, name)?.is_some())
}

/// Return the actual Shell result so the caller can replace its identity before
/// releasing a desktop presentation transaction. None means user cancellation.
pub fn rename_shell_item(
    owner: HWND,
    identity: &ShellIdentity,
    name: &str,
) -> std::result::Result<Option<crate::ShellEntry>, RenameError> {
    commit_shell_rename(owner, identity, name)?
        .map(|renamed| renamed.entry(identity))
        .transpose()
        .map_err(Into::into)
}

/// The Shell confirmed the rename. The returned object can be queried again
/// on the same STA if its new identity cannot be read immediately.
#[derive(Clone)]
pub struct CommittedRename {
    item: Option<IShellItem>,
    actual_name: Option<OsString>,
}

impl CommittedRename {
    pub fn entry(&self, old: &ShellIdentity) -> Result<crate::ShellEntry> {
        use crate::namespace::{
            shell_entry_from_attributes, shell_entry_with_display_fallback, stable_file_identity,
        };
        let reported_display = self.actual_name.as_deref().map(OsStr::to_string_lossy);
        if let Some(item) = &self.item
            && let Ok(entry) = shell_entry_with_display_fallback(item, reported_display.as_deref())
        {
            return Ok(entry);
        }
        // The reported name may differ from the requested one. Trust a path
        // reconstructed from it only when it still has the original file ID.
        if let ShellIdentity::FileSystem {
            path,
            volume_id: Some(volume),
            file_id: Some(file),
        } = old
            && let Some(parent) = path.parent()
            && let Some(name) = self.actual_name.as_deref()
            && !name.encode_wide().any(|unit| b"\\/:\0".iter().any(|&byte| u16::from(byte) == unit))
            && std::path::Path::new(name).file_name() == Some(name)
        {
            let path = parent.join(name);
            let expected_id = Some((*volume, *file));
            if stable_file_identity(&path) == expected_id {
                let item: Result<IShellItem> = unsafe {
                    SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)
                };
                if let Ok(item) = item
                    && let Ok(entry) = shell_entry_with_display_fallback(&item, reported_display.as_deref())
                    && matches!(&entry.identity, ShellIdentity::FileSystem {
                        volume_id: Some(actual_volume), file_id: Some(actual_file), ..
                    } if (actual_volume, actual_file) == (volume, file))
                {
                    return Ok(entry);
                }
                let entry = shell_entry_from_attributes(
                    ShellIdentity::FileSystem {
                        path: path.clone(),
                        volume_id: Some(*volume),
                        file_id: Some(*file),
                    },
                    name.to_string_lossy().into_owned(),
                    Some(&path),
                    0,
                );
                if stable_file_identity(&path) == expected_id {
                    return Ok(entry);
                }
            }
        }
        Err(windows::core::Error::new(E_FAIL, "The Shell committed the rename but the updated identity is unavailable"))
    }
}

/// Perform the rename and report commitment independently of identity reads.
pub fn commit_shell_rename(
    owner: HWND,
    identity: &ShellIdentity,
    name: &str,
) -> std::result::Result<Option<CommittedRename>, RenameError> {
    if name.trim().is_empty() || name.contains(['\0', '/', '\\']) {
        return Err(RenameError::InvalidName);
    }
    let parsing_name = HSTRING::from(identity.activation_name());
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&parsing_name, None)?;
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        operation.SetOwnerWindow(owner)?;
        let outcome = Rc::new(RefCell::new(None));
        let sink: IFileOperationProgressSink = RenameResult(outcome.clone()).into();
        operation.RenameItem(&item, &HSTRING::from(name), &sink)?;
        let performed = operation.PerformOperations();
        let reported = outcome.borrow_mut().take();
        if let Some(Ok(Some(renamed))) = reported {
            return Ok(Some(renamed));
        }
        if !crate::file_command::command_completed(performed)? {
            return Ok(None);
        }
        if operation.GetAnyOperationsAborted()?.as_bool() {
            return Ok(None);
        }
        reported.ok_or(RenameError::MissingResult)?.map_err(Into::into)
    }
}

#[implement(IFileOperationProgressSink)]
struct RenameResult(Rc<RefCell<Option<Result<Option<CommittedRename>>>>>);
impl IFileOperationProgressSink_Impl for RenameResult_Impl {
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        name: &PCWSTR,
        result: HRESULT,
        item: Ref<IShellItem>,
    ) -> Result<()> {
        let outcome = crate::file_command::command_completed(result.ok()).map(|completed| {
            let committed = completed
                && (item.as_ref().is_some()
                    || matches!(result, S_OK | COPYENGINE_S_COLLISIONRESOLVED));
            committed.then(|| CommittedRename {
                item: item.as_ref().cloned(),
                actual_name: (!name.is_null())
                    .then(|| OsString::from_wide(unsafe { name.as_wide() }))
                    .filter(|name| !name.is_empty()),
            })
        });
        *self.0.borrow_mut() = Some(outcome);
        Ok(())
    }
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _: HRESULT) -> Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Ref<IShellItem>) -> Result<()> {
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rename_callback_reports_explicit_cancellation() {
        let outcome = Rc::new(RefCell::new(None));
        let sink: IFileOperationProgressSink = RenameResult(outcome.clone()).into();
        unsafe {
            sink.PostRenameItem(
                0,
                None::<&IShellItem>,
                PCWSTR::null(),
                windows::Win32::UI::Shell::COPYENGINE_E_USER_CANCELLED,
                None::<&IShellItem>,
            )
        }
        .unwrap();
        assert!(matches!(outcome.borrow_mut().take(), Some(Ok(None))));
    }

    #[test]
    fn committed_rename_can_recover_identity_after_callback_lookup_fails() {
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("luciddesk-rename-recovery-{suffix}"));
        std::fs::create_dir(&root).unwrap();
        let before = root.join("before.txt");
        let after = root.join("reported.txt");
        std::fs::write(&before, b"rename recovery").unwrap();
        let (volume, file) = crate::namespace::stable_file_identity(&before).unwrap();
        let old = ShellIdentity::FileSystem { path: before.clone(), volume_id: Some(volume), file_id: Some(file) };
        let outcome = Rc::new(RefCell::new(None));
        let sink: IFileOperationProgressSink = RenameResult(outcome.clone()).into();
        unsafe { sink.PostRenameItem(0, None::<&IShellItem>, windows::core::w!("reported.txt"), S_OK, None::<&IShellItem>) }.unwrap();
        let committed = outcome.borrow_mut().take().unwrap().unwrap().unwrap();
        std::fs::write(&after, b"unrelated file").unwrap();
        assert!(committed.entry(&old).is_err(), "do not capture another file at the reported path");
        std::fs::remove_file(&after).unwrap();
        std::fs::rename(&before, &after).unwrap();
        let entry = committed.entry(&old).unwrap();
        assert_eq!(entry.identity.file_system_path(), Some(after.as_path()));
        assert!(entry.identity.equivalent_to(&old));
        assert!(entry.attributes.file_system);
        assert_eq!(entry.attributes.filesystem_directory, Some(false));
        assert_eq!(entry.size, Some(b"rename recovery".len() as u64));
        assert!(entry.modified.is_some());
        std::fs::remove_file(after).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn shell_rename_keeps_the_same_file_identity() {
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("luciddesk-shell-rename-{suffix}"));
        std::fs::create_dir(&root).unwrap();
        let before = root.join("旧名字.txt");
        let after = root.join("新名字.txt");
        std::fs::write(&before, b"rename fixture").unwrap();
        let file_id = crate::namespace::stable_file_identity(&before).unwrap();
        let identity = ShellIdentity::FileSystem {
            path: before.clone(),
            volume_id: Some(file_id.0),
            file_id: Some(file_id.1),
        };
        unsafe {
            use windows::{
                Win32::UI::{
                    Shell::{
                        BHID_SFUIObject, CMF_CANRENAME, CMF_ITEMMENU, GCS_VERBA, IContextMenu,
                    },
                    WindowsAndMessaging::{CreatePopupMenu, DestroyMenu},
                },
                core::PSTR,
            };
            let item: IShellItem = SHCreateItemFromParsingName(
                &HSTRING::from(before.as_os_str()),
                None,
            )
            .unwrap();
            let context: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject).unwrap();
            let menu = CreatePopupMenu().unwrap();
            let populated =
                context.QueryContextMenu(menu, 0, 1, 0x7fff, CMF_ITEMMENU | CMF_CANRENAME);
            populated.ok().unwrap();
            let mut found = false;
            for command in 0..(populated.0 as u32 & 0xffff) {
                let mut verb = [0u8; 256];
                if context
                    .GetCommandString(
                        command as usize,
                        GCS_VERBA,
                        None,
                        PSTR(verb.as_mut_ptr()),
                        256,
                    )
                    .is_ok()
                {
                    found |= verb.starts_with(b"rename\0");
                }
            }
            DestroyMenu(menu).unwrap();
            assert!(found, "Shell menu did not expose the native rename verb");
        }
        let renamed = rename_shell_item(HWND::default(), &identity, "新名字.txt")
            .unwrap()
            .unwrap();
        assert_eq!(renamed.identity.file_system_path(), Some(after.as_path()));
        assert!(renamed.identity.equivalent_to(&identity));
        let again = rename_shell_item(HWND::default(), &renamed.identity, "再次改名.txt")
            .unwrap()
            .unwrap();
        let final_path = root.join("再次改名.txt");
        assert_eq!(
            again.identity.file_system_path(),
            Some(final_path.as_path())
        );
        assert!(!before.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"rename fixture");
        assert_eq!(
            crate::namespace::stable_file_identity(&final_path).unwrap(),
            file_id
        );
        std::fs::remove_file(final_path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
