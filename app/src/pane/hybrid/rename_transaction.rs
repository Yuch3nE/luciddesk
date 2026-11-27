//! Publish a renamed identity before allowing the desktop to paint again.
use super::*;

struct Update {
    state: std::rc::Weak<RefCell<PaneApp>>,
    hook: Rc<FilterSession>,
    finished: bool,
}
impl Update {
    fn finish(&mut self) -> Result<(), String> {
        let result = self.hook.finish_update();
        self.finished = result.is_ok();
        if let Some(state) = self.state.upgrade() {
            if let Some(h) = state.borrow().session.as_ref().filter(|h| Rc::ptr_eq(&h.hook, &self.hook)) {
                h.menu_active.set(false);
                h.dirty.set(true);
                h.wake.notify();
            }
        }
        result
    }
}
impl Drop for Update {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

pub(in crate::pane) fn commit(
    state: &Rc<RefCell<PaneApp>>,
    owner: windows_sys::Win32::Foundation::HWND,
    old: &ShellIdentity,
    name: &str,
) -> Result<bool, String> {
    let hook = {
        let s = state.borrow();
        if s.session.is_some()
            && s.workspace.desktop_items().iter().any(|item| {
                item.identity().equivalent_to(old)
                    && matches!(item.placement(), DesktopPlacement::Pane { .. })
            })
        {
            Some(begin_item_menu(&s)?)
        } else {
            None
        }
    };
    let Some(hook) = hook else {
        return luciddesk_shell::rename_shell_identity(
            windows::Win32::Foundation::HWND(owner),
            old,
            name,
        )
        .map_err(crate::pane::rename::shell_rename_error);
    };
    if let Err(error) = hook.begin_update() {
        // Also release a late acknowledged freeze after an IPC timeout.
        let _ = hook.finish_update();
        end_item_menu(&state.borrow());
        return Err(error);
    }
    let mut update = Update {
        state: Rc::downgrade(state),
        hook,
        finished: false,
    };
    let renamed =
        luciddesk_shell::commit_shell_rename(windows::Win32::Foundation::HWND(owner), old, name)
            .map_err(crate::pane::rename::shell_rename_error)?;
    let Some(renamed) = renamed else {
        update.finish()?;
        return Ok(false);
    };
    let renamed = match renamed.entry(old) {
        Ok(entry) => entry,
        Err(error) => {
            queue_recovery(&mut state.borrow_mut(), old.clone(), renamed);
            log_recovery("read new identity", &error);
            if let Err(error) = update.finish() {
                log_recovery("release", &error);
            }
            return Ok(true);
        }
    };
    let hook = update.hook.clone();
    reconcile_committed(
        state,
        old,
        renamed,
        |new_name| hook.replace_identity(&old.activation_name().to_string_lossy(), new_name),
        |names| hook.set_hidden(names),
        || update.finish(),
    );
    Ok(true)
}

const RETRY_DELAY: Duration = Duration::from_secs(1);

pub(in crate::pane) struct PendingRename {
    old: ShellIdentity,
    renamed: luciddesk_shell::CommittedRename,
    retry_at: Option<Instant>,
}

impl PendingRename {
    fn matches(&self, identity: &ShellIdentity) -> bool {
        match (&self.old, identity) {
            (
                ShellIdentity::FileSystem { volume_id: Some(old_volume), file_id: Some(old_file), .. },
                ShellIdentity::FileSystem { volume_id: Some(volume), file_id: Some(file), .. },
            ) => (old_volume, old_file) == (volume, file),
            _ => self.old.equivalent_to(identity),
        }
    }
}

pub(in crate::pane) fn awaiting_identity(s: &PaneApp, identity: &ShellIdentity) -> bool {
    s.pending_desktop_renames.iter().any(|pending| {
        pending.matches(identity)
            && pending.old.activation_name().eq_ignore_ascii_case(identity.activation_name())
    })
}

pub(super) fn retain_unresolved(s: &PaneApp, incoming: &mut Vec<DesktopItem>) {
    for pending in &s.pending_desktop_renames {
        // Preserve the old placement if neither the authoritative new identity
        // nor the old row appeared in this snapshot. Ignore reuse of the old path.
        incoming.retain(|item| !pending.old.equivalent_to(item.identity()) || pending.matches(item.identity()));
        if !incoming.iter().any(|item| pending.matches(item.identity()))
            && let Some(old) = s.workspace.desktop_items().iter().find(|item| pending.matches(item.identity()))
        {
            incoming.push(old.clone());
        }
    }
}

fn queue_recovery(s: &mut PaneApp, old: ShellIdentity, renamed: luciddesk_shell::CommittedRename) {
    s.pending_desktop_renames.push(PendingRename { old, renamed, retry_at: Some(Instant::now()) });
    if let Some(h) = &s.session {
        h.dirty.set(true);
    }
    refresh_views(s);
    s.wake.notify();
}

pub(in crate::pane) fn recovery_deadline(s: &PaneApp) -> Option<Instant> {
    s.session.as_ref().filter(|h| is_alive(h) && !h.menu_active.get())?;
    s.pending_desktop_renames.iter().filter_map(|pending| pending.retry_at).min()
}

pub(in crate::pane) fn retry_pending(state: &Rc<RefCell<PaneApp>>) {
    let now = Instant::now();
    let (old, renamed) = {
        let mut s = state.borrow_mut();
        if recovery_deadline(&s).is_none_or(|due| due > now) {
            return;
        }
        let Some(pending) = s.pending_desktop_renames.iter_mut().find(|pending| pending.retry_at.is_some_and(|due| due <= now)) else { return; };
        // Prevent a nested message loop from retrying the same COM object.
        pending.retry_at = None;
        (pending.old.clone(), pending.renamed.clone())
    };
    let entry = renamed.entry(&old);
    let mut s = state.borrow_mut();
    let Some(index) = s.pending_desktop_renames.iter().position(|pending| pending.old == old) else { return; };
    let Some(entry) = entry.ok().filter(|_| s.session.is_some()) else {
        s.pending_desktop_renames[index].retry_at = Some(Instant::now() + RETRY_DELAY);
        return;
    };
    let pending = s.pending_desktop_renames.remove(index);
    let current = s.workspace.desktop_items().iter().find(|item| pending.matches(item.identity()))
        .map(|item| item.identity().clone());
    if let Some(current) = current {
        replace_item(&mut s, &current, entry);
        let h = s.session.as_mut().unwrap();
        h.pending_workspace_save = true;
        h.save_due = None;
        h.audit.invalidate_snapshot();
        h.dirty.set(true);
        h.wake.notify();
    }
    refresh_views(&mut s);
}

fn log_recovery(stage: &str, error: &impl std::fmt::Display) {
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.hybrid.rename_transaction", &format!("Rename committed; {stage} recovery pending: {error}"));
}

fn reconcile_committed(
    state: &Rc<RefCell<PaneApp>>,
    old: &ShellIdentity,
    renamed: luciddesk_shell::ShellEntry,
    replace: impl FnOnce(&str) -> Result<(), String>,
    publish: impl FnOnce(&[String]) -> Result<(), String>,
    release: impl FnOnce() -> Result<(), String>,
) {
    // The filesystem operation is committed already. Preserve its actual result
    // before fallible IPC so a recovery/retry cannot resurrect the old path.
    let new_name = renamed
        .identity
        .activation_name()
        .to_string_lossy()
        .into_owned();
    let (names, saved) = {
        let mut s = state.borrow_mut();
        replace_item(&mut s, old, renamed);
        let names = hidden_names(&s);
        let saved = {
            let s = &mut *s;
            s.store
                .save_workspace(&s.workspace)
                .map_err(|e| e.to_string())
        };
        refresh_views(&mut s);
        (names, saved)
    };
    // IPC pumps UI messages. Keep model/state borrows out of this scope.
    let replaced = replace(&new_name);
    // Publishing the complete set also repairs a failed identity handoff.
    let published = publish(&names);
    if let Some(h) = &state.borrow().session {
        if published.is_ok() {
            *h.published.borrow_mut() = Some(names);
        } else {
            *h.published.borrow_mut() = None;
        }
        h.dirty.set(true);
    }
    if saved.is_err() {
        if let Some(h) = state.borrow_mut().session.as_mut() {
            h.pending_workspace_save = true;
        }
    }
    let released = release();
    // Do not offer to rename the old, nonexistent path after a successful Shell
    // operation. The next audit/sync uses the new identity and retries repairs.
    for (stage, result) in [
        ("identity", replaced),
        ("filter", published),
        ("release", released),
        ("save", saved),
    ] {
        if let Err(error) = result {
            log_recovery(stage, &error);
        }
    }
}

fn replace_item(s: &mut PaneApp, old: &ShellIdentity, renamed: luciddesk_shell::ShellEntry) {
    if !old.equivalent_to(&renamed.identity) && s.workspace.desktop_item(&renamed.identity).is_some() {
        // A desktop audit may already have seen the renamed item separately.
        // Preserve the old entry's placement and remove the duplicate incoming row.
        let items: Vec<_> = s.workspace.desktop_items().iter()
            .filter(|item| !item.identity().equivalent_to(&renamed.identity)).cloned().collect();
        s.workspace.reconcile_desktop_items(items);
    }
    for item in s.workspace.desktop_items_mut() {
        if item.identity().equivalent_to(old) {
            let placement = item.placement().clone();
            let position = item.pane_position();
            *item = DesktopItem::new(renamed.identity.clone(), renamed.display_name.clone());
            item.set_placement(placement);
            item.set_pane_position(position);
        }
    }
    let old_key = old.persistent_key();
    let new_key = renamed.identity.persistent_key();
    if old_key != new_key {
        if let Some(image) = s.images.remove(&old_key) {
            s.images.insert(new_key, image);
        }
    }
    if let Some(h) = s.session.as_mut() {
        if !old.equivalent_to(&renamed.identity)
            && h.snapshot.items.iter().any(|item| item.identity.equivalent_to(old))
        {
            h.snapshot.items.retain(|item| !item.identity.equivalent_to(&renamed.identity));
        }
        for item in &mut h.snapshot.items {
            if item.identity.equivalent_to(old) {
                *item = renamed.clone();
            }
        }
        h.last_reconcile = Instant::now() - Duration::from_secs(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_rename_is_saved_before_failed_ipc_and_still_attempts_repair_and_release() {
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        let old = state.borrow().workspace.desktop_items()[0]
            .identity()
            .clone();
        let placement = state.borrow().workspace.desktop_items()[0]
            .placement()
            .clone();
        state.borrow_mut().images.insert(
            old.persistent_key(),
            Arc::new(assets::Pixels {
                width: 1,
                height: 1,
                data: vec![0, 0, 0, 255],
            }),
        );
        let identity = ShellIdentity::Namespace {
            parsing_name: "test:renamed".into(),
        };
        let renamed = luciddesk_shell::ShellEntry {
            identity: identity.clone(),
            display_name: "Renamed".into(),
            attributes: Default::default(),
            created: None,
            modified: None,
            size: None,
        };
        let repaired = Cell::new(false);
        let released = Cell::new(false);
        reconcile_committed(
            &state,
            &old,
            renamed,
            |name| {
                assert_eq!(name, "test:renamed");
                let s = state.borrow();
                let saved = s.store.load_workspace().unwrap();
                assert!(
                    saved
                        .desktop_items()
                        .iter()
                        .any(|i| i.identity() == &identity && i.placement() == &placement)
                );
                assert!(!saved.desktop_items().iter().any(|i| i.identity() == &old));
                assert!(
                    s.workspace
                        .desktop_items()
                        .iter()
                        .any(|i| i.identity() == &identity && i.placement() == &placement)
                );
                Err("simulated identity transport failure".into())
            },
            |names| {
                assert!(names.iter().any(|n| n == "test:renamed"));
                assert!(
                    !names
                        .iter()
                        .any(|n| n == &old.activation_name().to_string_lossy())
                );
                repaired.set(true);
                Err("simulated publish failure".into())
            },
            || {
                released.set(true);
                Ok(())
            },
        );
        assert!(repaired.get() && released.get());
        assert!(
            !state
                .borrow()
                .workspace
                .desktop_items()
                .iter()
                .any(|i| i.identity() == &old)
        );
    }
    #[test]
    fn renaming_preserves_free_coordinates() {
        let mut s=crate::pane::tests::test_state();
        let old=s.workspace.desktop_items()[0].identity().clone();
        let point=luciddesk_core::PointDip::new(23.5,67.25);
        s.workspace.desktop_item_mut(&old).unwrap().set_pane_position(Some(point));
        let renamed=luciddesk_shell::ShellEntry {
            identity: ShellIdentity::Namespace { parsing_name:"test:renamed".into() },
            display_name:"Renamed".into(),attributes:Default::default(),created:None,modified:None,size:None,
        };
        let identity=renamed.identity.clone();
        replace_item(&mut s,&old,renamed);
        assert_eq!(s.workspace.desktop_item_mut(&identity).unwrap().pane_position(),Some(point));
    }

    #[test]
    fn recovered_identity_replaces_an_audited_duplicate_and_keeps_placement() {
        let mut s = crate::pane::tests::test_state();
        let old = s.workspace.desktop_items()[0].identity().clone();
        let identity = ShellIdentity::Namespace { parsing_name: "test:recovered".into() };
        let point = luciddesk_core::PointDip::new(12.0, 42.0);
        let original = s.workspace.desktop_item_mut(&old).unwrap();
        original.set_pane_position(Some(point));
        let placement = original.placement().clone();
        let mut inventory = s.workspace.desktop_items().to_vec();
        inventory.push(DesktopItem::new(identity.clone(), "Recovered"));
        s.workspace.reconcile_desktop_items(inventory);

        replace_item(&mut s, &old, luciddesk_shell::ShellEntry {
            identity: identity.clone(),
            display_name: "Recovered".into(),
            attributes: Default::default(),
            created: None,
            modified: None,
            size: None,
        });

        assert!(s.workspace.desktop_item(&old).is_none());
        let mut entries = s.workspace.desktop_items().iter().filter(|item| item.identity() == &identity);
        let recovered = entries.next().unwrap();
        assert_eq!(recovered.placement(), &placement);
        assert_eq!(recovered.pane_position(), Some(point));
        assert!(entries.next().is_none());
    }

}
