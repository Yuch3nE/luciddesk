//! Settings rule management and an accessible native rule editor.
use super::*;
use luciddesk_core::{Classification, ClassificationRule};
use luciddesk_storage::SettingValue;
mod editor;
const KEY: &str = "desktop.classification";
fn t(key: &'static str) -> &'static str { crate::i18n::text(key) }

#[derive(Clone)]
pub(super) enum Command { Enable(bool), Add, Edit(usize), Delete(usize), Toggle(usize), Move(usize, isize), Run }

fn load(s: &PaneApp) -> Result<Classification, String> {
    let values = s.store.settings().map_err(|e| e.to_string())?;
    match values.get(KEY) {
        Some(SettingValue::Text(raw)) => serde_json::from_str(raw).map_err(|e| e.to_string()),
        _ => Err("Missing classification settings".into()),
    }
}

pub(super) fn draw(scene: &mut Scene, width: f32, s: &PaneApp) {
    let mut form = SettingsForm::new(scene, width, t("classification-description"));
    let config = match load(s) { Ok(v) => v, Err(e) => { form.info(t("classification-title"), &e); return; } };
    form.toggle(t("classification-auto"), t("classification-auto-description"), config.enabled, Action::Classification(Command::Enable(!config.enabled)));
    form.actions(t("classification-rules"), t("classification-priority"), vec![
        (t("classification-add"), Action::Classification(Command::Add)),
        (t("classification-run"), Action::Classification(Command::Run)),
    ]);
    if config.rules.is_empty() { form.info(t("classification-empty"), t("classification-add-hint")); }
    for (index, rule) in config.rules.iter().enumerate() {
        let target = s.workspace.panel(PanelId::new(rule.target_panel));
        let title = format!("{}. {}", index + 1, target.map_or(t("classification-missing"), Panel::title));
        let detail = format!("{}: {} · {}: {}", t("classification-extensions"), rule.extensions.join(", "), t("classification-name"), rule.name_contains.as_deref().unwrap_or("—"));
        form.toggle(&title, &detail, rule.enabled, Action::Classification(Command::Toggle(index)));
        let mut actions = vec![(t("classification-edit"), Action::Classification(Command::Edit(index))), (t("classification-delete"), Action::Classification(Command::Delete(index)))];
        if index > 0 { actions.push((t("classification-up"), Action::Classification(Command::Move(index, -1)))); }
        if index + 1 < config.rules.len() { actions.push((t("classification-down"), Action::Classification(Command::Move(index, 1)))); }
        form.actions("", "", actions);
    }
}

pub(super) fn execute(state: &Rc<RefCell<PaneApp>>, hwnd: windows_sys::Win32::Foundation::HWND, command: Command) -> Result<(), String> {
    if matches!(command, Command::Run) {
        let count = hybrid::organize_now(&mut state.borrow_mut())?;
        crate::error_report::show(hwnd as isize, t("classification-run"), &format!("{}: {count}", t("classification-result")), luciddesk_window::MessageKind::Information);
        return Ok(());
    }
    let mut config = load(&state.borrow())?;
    let original = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    match command {
        Command::Enable(value) => config.enabled = value,
        Command::Add | Command::Edit(_) => {
            let panels: Vec<_> = state.borrow().workspace.panels().iter().filter(|p| p.supports_tabs() && !p.locked()).cloned().collect();
            if panels.is_empty() { return Err(t("classification-no-target").into()); }
            let index = match command { Command::Edit(i) => Some(i), _ => None };
            let initial = index.and_then(|i| config.rules.get(i)).cloned().unwrap_or(ClassificationRule { target_panel: panels[0].id().get(), enabled: true, extensions: vec![], name_contains: None });
            let Some(rule) = editor::show(hwnd, &panels, initial)? else { return Ok(()); };
            if let Some(index) = index { config.rules[index] = rule; } else { config.rules.push(rule); }
        }
        Command::Delete(i) => { if i < config.rules.len() { config.rules.remove(i); } }
        Command::Toggle(i) => { if let Some(rule) = config.rules.get_mut(i) { rule.enabled = !rule.enabled; } }
        Command::Move(i, delta) => { let to = i.checked_add_signed(delta); if let Some(to) = to.filter(|to| *to < config.rules.len() && i < config.rules.len()) { config.rules.swap(i, to); } }
        Command::Run => unreachable!(),
    }
    config.validate()?;
    let s = state.borrow();
    if serde_json::to_string(&load(&s)?).map_err(|e| e.to_string())? != original { return Err(t("classification-conflict").into()); }
    s.store.save_settings(&[(KEY.into(), SettingValue::Text(serde_json::to_string(&config).map_err(|e| e.to_string())?))].into_iter().collect()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_controls_persist_order_toggle_delete_and_enable() {
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        let directory = tempfile::tempdir().unwrap();
        state.borrow_mut().store = WorkspaceStore::open(&directory.path().join("workspace.db")).unwrap();
        let config = Classification { enabled: false, rules: ["pdf", "png"].into_iter().map(|ext| ClassificationRule { target_panel: 1, enabled: true, extensions: vec![ext.into()], name_contains: None }).collect() };
        state.borrow().store.save_settings(&[(KEY.into(), SettingValue::Text(serde_json::to_string(&config).unwrap()))].into_iter().collect()).unwrap();
        let hwnd = std::ptr::null_mut();
        execute(&state, hwnd, Command::Move(1, -1)).unwrap();
        execute(&state, hwnd, Command::Toggle(0)).unwrap();
        execute(&state, hwnd, Command::Enable(true)).unwrap();
        let saved = load(&state.borrow()).unwrap();
        assert!(saved.enabled);
        assert_eq!(saved.rules[0].extensions, ["png"]);
        assert!(!saved.rules[0].enabled);
        execute(&state, hwnd, Command::Delete(0)).unwrap();
        assert_eq!(load(&state.borrow()).unwrap().rules[0].extensions, ["pdf"]);
        let before = state.borrow().workspace.clone();
        assert!(hybrid::organize_now(&mut state.borrow_mut()).is_err());
        assert_eq!(state.borrow().workspace, before);
    }
}
