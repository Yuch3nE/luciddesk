use luciddesk_core::ShellIdentity;
use std::{cell::RefCell, rc::Rc};
use windows::Win32::UI::Shell::{IFileOperationProgressSink, IFileOperationProgressSink_Impl};
use windows::core::{HRESULT, PCWSTR, Ref, implement};
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG, HWND, S_OK},
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::Shell::{
            COPYENGINE_S_COLLISIONRESOLVED, FileOperation, IFileOperation, IShellItem,
            SHCreateItemFromParsingName,
        },
    },
    core::{HSTRING, Result},
};

/// Rename through Shell so namespace rules and Explorer change notifications apply.
/// Returns false if the user cancels a Shell conflict or permission dialog.
/// # Errors
/// Returns Shell errors for invalid names, unavailable items, or failed operations.
pub fn rename_shell_identity(owner: HWND, identity: &ShellIdentity, name: &str) -> Result<bool> {
    Ok(commit_shell_rename(owner, identity, name)?.is_some())
}

/// Return the actual Shell result so the caller can replace its identity before
/// releasing a desktop presentation transaction. None means user cancellation.
pub fn rename_shell_item(
    owner: HWND,
    identity: &ShellIdentity,
    name: &str,
) -> Result<Option<crate::ShellEntry>> {
    commit_shell_rename(owner, identity, name)?
        .map(|renamed| renamed.entry(identity))
        .transpose()
}

/// The Shell confirmed the rename. The returned object can be queried again
/// on the same STA if its new identity cannot be read immediately.
#[derive(Clone)]
pub struct CommittedRename {
    item: Option<IShellItem>,
    actual_name: Option<String>,
}

impl CommittedRename {
    pub fn entry(&self, old: &ShellIdentity) -> Result<crate::ShellEntry> {
        if let Some(item) = &self.item
            && let Ok(entry) = crate::namespace::shell_entry_with_display_fallback(item, self.actual_name.as_deref())
        {
            return Ok(entry);
        }
        // The reported name may differ from the requested one. Trust a path
        // reconstructed from it only when it still has the original file ID.
        if let ShellIdentity::FileSystem { path, volume_id: Some(volume), file_id: Some(file) } = old
            && let Some(parent) = path.parent()
            && let Some(name) = self.actual_name.as_deref()
            && !name.contains(['\\', '/', ':', '\0'])
            && std::path::Path::new(name).file_name() == Some(std::ffi::OsStr::new(name))
        {
            let path = parent.join(name);
            if crate::namespace::stable_file_identity(&path) == Some((*volume, *file)) {
                return Ok(crate::ShellEntry {
                    identity: ShellIdentity::FileSystem { path, volume_id: Some(*volume), file_id: Some(*file) },
                    display_name: name.to_owned(),
                    attributes: Default::default(),
                    created: None,
                    modified: None,
                    size: None,
                });
            }
        }
        Err(windows::core::Error::new(E_FAIL, "Shell 改名已完成，暂时无法读取新项目身份"))
    }
}

/// Perform the rename and report commitment independently of identity reads.
pub fn commit_shell_rename(
    owner: HWND,
    identity: &ShellIdentity,
    name: &str,
) -> Result<Option<CommittedRename>> {
    if name.trim().is_empty() || name.contains(['\0', '/', '\\']) {
        return Err(windows::core::Error::new(
            E_INVALIDARG,
            "名称不能为空或包含路径分隔符",
        ));
    }
    let parsing_name = match identity {
        ShellIdentity::FileSystem { path, .. } => path.to_string_lossy().into_owned(),
        ShellIdentity::Namespace { parsing_name } => parsing_name.clone(),
    };
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None)?;
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
        reported.unwrap_or_else(|| Err(windows::core::Error::new(E_FAIL, "Shell 未返回改名结果")))
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
        let outcome = result.ok().map(|()| {
            (item.as_ref().is_some() || matches!(result, S_OK | COPYENGINE_S_COLLISIONRESOLVED)).then(|| CommittedRename {
                item: item.as_ref().cloned(),
                actual_name: (!name.is_null())
                    .then(|| unsafe { name.to_string().ok() })
                    .flatten()
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
                &HSTRING::from(before.to_string_lossy().as_ref()),
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
