//! In-place editing and persistence for the ordered classification rules.
use super::*;
use luciddesk_core::{Classification, ClassificationError, ClassificationRule};
use luciddesk_storage::SettingValue;
use windows_sys::Win32::Foundation::HWND;

mod editor;
mod table;
use editor::{Draft, Inputs};
pub(super) use editor::{Field, input_colors};
pub(super) use hybrid::CLASSIFICATION_KEY as KEY;

fn t(key: &'static str) -> &'static str {
    crate::i18n::text(key)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Enable(bool),
    Add,
    AddTemplate(Template),
    Edit(usize),
    Delete(usize),
    UndoDelete,
    Toggle(usize),
    ToggleDetails(usize),
    Move(usize, isize),
    Run,
    ApplyPreview,
    CancelPreview,
    Input(Field),
    Target(PanelId),
    DraftEnabled,
    DraftRegex,
    Save,
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Template {
    Documents,
    Images,
    Archives,
    Shortcuts,
}

impl Template {
    const ALL: [Self; 4] = [
        Self::Documents,
        Self::Images,
        Self::Archives,
        Self::Shortcuts,
    ];

    fn label(self) -> &'static str {
        t(match self {
            Self::Documents => "classification-template-documents",
            Self::Images => "classification-template-images",
            Self::Archives => "classification-template-archives",
            Self::Shortcuts => "classification-template-shortcuts",
        })
    }

    fn extensions(self) -> Vec<String> {
        let values: &[&str] = match self {
            Self::Documents => &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "md", "csv",
                "odt", "ods", "odp",
            ],
            Self::Images => &[
                "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "tif", "tiff", "heic",
                "avif",
            ],
            Self::Archives => &["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "tgz", "cab"],
            Self::Shortcuts => &["lnk", "url", "website"],
        };
        values.iter().map(|value| (*value).to_owned()).collect()
    }
}

impl Command {
    fn action(self) -> Action {
        Action::Classification(self)
    }

    pub(super) fn is_control(self, action: &Action) -> bool {
        match action {
            Action::Classification(value) => *value == self,
            Action::Dropdown(options) if self == Self::Add => options
                .iter()
                .any(|(_, action, _)| matches!(action, Action::Classification(Self::Add))),
            _ => false,
        }
    }
}

fn load(s: &PaneApp) -> Result<Classification, String> {
    let values = s.store.settings().map_err(|e| e.to_string())?;
    decode(values.get(KEY))
}

fn decode(value: Option<&SettingValue>) -> Result<Classification, String> {
    match value {
        Some(SettingValue::Text(raw)) => serde_json::from_str(raw).map_err(|e| e.to_string()),
        _ => Err("Missing classification settings".into()),
    }
}

fn encode(config: &Classification) -> Result<String, String> {
    serde_json::to_string(config).map_err(|e| e.to_string())
}

fn save(s: &PaneApp, config: &Classification) -> Result<(), String> {
    config.compile().map_err(|error| match error {
        ClassificationError::InvalidRegex(_) => t("classification-regex-invalid").into(),
        error => error.to_string(),
    })?;
    s.store
        .save_settings(
            &[(KEY.into(), SettingValue::Text(encode(config)?))]
                .into_iter()
                .collect(),
        )
        .map_err(|e| e.to_string())
}

fn targets(panels: &[Panel]) -> impl Iterator<Item = &Panel> {
    panels
        .iter()
        .filter(|panel| panel.supports_tabs())
}

struct DeletedRule {
    index: usize,
    rule: ClassificationRule,
    after: String,
}

#[derive(Default)]
pub(super) struct Editor {
    draft: Option<Draft>,
    deleted: Option<DeletedRule>,
    expanded: Option<usize>,
    preview: Option<hybrid::OrganizePreview>,
    applying: Option<std::sync::mpsc::Receiver<Result<usize, String>>>,
    result: Option<usize>,
    error: Option<String>,
    reveal: Option<Command>,
}

impl Editor {
    pub fn editing(&self) -> bool {
        self.draft.is_some()
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    pub(super) fn poll_organize(&mut self) -> bool {
        use std::sync::mpsc::TryRecvError;
        let Some(receiver) = &self.applying else { return false; };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err(t("classification-desktop-unavailable").into()),
        };
        self.applying = None;
        match result {
            Ok(count) => self.result = Some(count),
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub fn take_reveal(&mut self) -> Option<Command> {
        self.reveal.take()
    }

    pub fn sync(&mut self, scene: &Scene, scale: f32, dark: bool) {
        if let Some(draft) = &mut self.draft {
            draft.inputs.sync(scene, scale, dark);
        }
    }

    pub fn discard(&mut self) {
        *self = Self::default();
    }

    pub fn resume(&mut self) {
        if self.editing() { self.reveal = Some(Command::Input(Field::Extensions)); }
    }

    pub fn suspend(&mut self) {
        if let Some(draft) = &mut self.draft { draft.inputs.hide(); }
        self.preview = None;
        self.reveal = None;
    }

    pub(super) fn collapse_details(&mut self) {
        self.expanded = None;
    }

    pub fn focus(&mut self, field: Field) {
        if let Some(draft) = &mut self.draft {
            draft.inputs.focus(field);
        }
    }

    pub fn field_for(&self, hwnd: HWND) -> Option<Field> {
        self.draft.as_ref()?.inputs.field_for(hwnd)
    }

    pub fn execute(&mut self, state: &Rc<RefCell<PaneApp>>, hwnd: HWND, command: Command) {
        if let Command::Input(field) = command {
            self.focus(field);
        } else {
            self.error = self.apply(state, hwnd, command).err();
            if command == Command::Save && self.error.is_some() && self.editing() {
                self.reveal = Some(Command::Input(Field::Extensions));
            }
        }
    }

    fn apply(
        &mut self,
        state: &Rc<RefCell<PaneApp>>,
        hwnd: HWND,
        command: Command,
    ) -> Result<(), String> {
        match command {
            Command::ApplyPreview | Command::CancelPreview => {
                if let Some(preview) = self.preview.take() {
                    self.reveal = Some(Command::Run);
                    if command == Command::ApplyPreview {
                        if unsafe { SetTimer(hwnd, ORGANIZE_TIMER, 100, None) } == 0 {
                            return Err(std::io::Error::last_os_error().to_string());
                        }
                        match hybrid::apply_organize(&mut state.borrow_mut(), preview) {
                            Ok(receiver) => self.applying = Some(receiver),
                            Err(error) => {
                                unsafe { KillTimer(hwnd, ORGANIZE_TIMER); }
                                return Err(error);
                            }
                        }
                    }
                }
                return Ok(());
            }
            _ if self.previewing() || self.applying.is_some() => return Ok(()),
            Command::Run if !self.editing() => {
                self.result = None;
                let preview = hybrid::preview_organize(&state.borrow())?;
                if preview.entries().len() == 0 {
                    self.result = Some(0);
                } else {
                    self.preview = Some(preview);
                    self.reveal = Some(Command::CancelPreview);
                }
                return Ok(());
            }
            Command::Target(id) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.target_panel = id.get();
                }
                return Ok(());
            }
            Command::DraftEnabled => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.enabled = !draft.rule.enabled;
                }
                return Ok(());
            }
            Command::DraftRegex => {
                if let Some(draft) = &mut self.draft {
                    draft.regex = !draft.regex;
                }
                return Ok(());
            }
            Command::Cancel => {
                self.finish();
                return Ok(());
            }
            _ => {}
        }
        let mut config = load(&state.borrow())?;
        let mut deleted = None;
        match command {
            Command::Add | Command::AddTemplate(_) | Command::Edit(_) if !self.editing() => {
                let (index, rule) = {
                    let state = state.borrow();
                    let mut targets = targets(state.workspace.panels());
                    let first = targets
                        .next()
                        .ok_or_else(|| t("classification-no-target"))?;
                    match command {
                        Command::Edit(index) => (
                            Some(index),
                            config
                                .rules
                                .get(index)
                                .ok_or_else(|| t("classification-conflict"))?
                                .clone(),
                        ),
                        _ => {
                            let (target_panel, extensions) = match command {
                                Command::AddTemplate(template) => (
                                    if targets.next().is_some() {
                                        0
                                    } else {
                                        first.id().get()
                                    },
                                    template.extensions(),
                                ),
                                _ => (first.id().get(), Vec::new()),
                            };
                            (
                                None,
                                ClassificationRule {
                                    target_panel,
                                    enabled: true,
                                    extensions,
                                    name_contains: None,
                                    name_regex: None,
                                },
                            )
                        }
                    }
                };
                let mut inputs = Inputs::new(hwnd, &rule)?;
                inputs.focus(Field::Extensions);
                self.collapse_details();
                self.draft = Some(Draft {
                    index,
                    original: encode(&config)?,
                    regex: rule.name_regex.is_some(),
                    rule,
                    inputs,
                });
                self.reveal = Some(Command::Input(Field::Extensions));
                return Ok(());
            }
            Command::Save => {
                let Some(draft) = &self.draft else {
                    return Ok(());
                };
                if encode(&config)? != draft.original {
                    return Err(t("classification-conflict").into());
                }
                if !targets(state.borrow().workspace.panels())
                    .any(|panel| panel.id().get() == draft.rule.target_panel)
                {
                    return Err(t("classification-no-target").into());
                }
                let rule = draft.edited_rule();
                if let Some(index) = draft.index {
                    *config
                        .rules
                        .get_mut(index)
                        .ok_or_else(|| t("classification-conflict"))? = rule;
                } else {
                    config.rules.push(rule);
                }
                save(&state.borrow(), &config)?;
                self.deleted = None;
                self.finish();
                return Ok(());
            }
            _ if self.editing() => return Ok(()),
            Command::ToggleDetails(index) if index < config.rules.len() => {
                self.expanded = (self.expanded != Some(index)).then_some(index);
                self.reveal = Some(command);
                return Ok(());
            }
            Command::Enable(value) => config.enabled = value,
            Command::Delete(i) if i < config.rules.len() => {
                deleted = Some((i, config.rules.remove(i)));
            }
            Command::UndoDelete => {
                let Some(deleted) = &self.deleted else { return Ok(()); };
                if encode(&config)? != deleted.after {
                    return Err(t("classification-conflict").into());
                }
                config.rules.insert(deleted.index, deleted.rule.clone());
            }
            Command::Toggle(i) => {
                if let Some(rule) = config.rules.get_mut(i) {
                    rule.enabled = !rule.enabled;
                }
            }
            Command::Move(i, delta) => {
                if let Some(to) = i
                    .checked_add_signed(delta)
                    .filter(|to| *to < config.rules.len() && i < config.rules.len())
                {
                    config.rules.swap(i, to);
                }
            }
            _ => return Ok(()),
        }
        save(&state.borrow(), &config)?;
        self.deleted = deleted.map(|(index, rule)| DeletedRule {
            index, rule, after: encode(&config).expect("validated classification serializes"),
        });
        if matches!(command, Command::Enable(_)) {
            hybrid::classification_settings_changed(&mut state.borrow_mut());
        }
        Ok(())
    }

    fn finish(&mut self) {
        self.reveal = self
            .draft
            .take()
            .map(|draft| draft.index.map_or(Command::Add, Command::Edit));
    }
}

pub(super) fn draw(
    scene: &mut Scene,
    width: f32,
    panels: &[Panel],
    value: Option<&SettingValue>,
    editor: &Editor,
) {
    let mut form = SettingsForm::new(scene, width, t("classification-description"));
    let config = match decode(value) {
        Ok(config) => config,
        Err(error) => {
            form.info(t("classification-title"), &error);
            return;
        }
    };
    let editing = editor.editing();
    let idle = !editing && !editor.previewing() && editor.applying.is_none();
    form.toggle_enabled(
        t("classification-auto"),
        t("classification-auto-description"),
        config.enabled,
        idle,
        Command::Enable(!config.enabled).action(),
    );
    let choices = std::iter::once((
        t("classification-custom-rule").into(),
        Command::Add.action(),
        false,
    ))
    .chain(Template::ALL.map(|template| {
        (
            template.label().into(),
            Command::AddTemplate(template).action(),
            false,
        )
    }))
    .collect::<Vec<_>>();
    form.actions_enabled(
        t("classification-rules"),
        t("classification-priority"),
        vec![
            (t("classification-add"), Action::Dropdown(choices.into())),
            (t("classification-run"), Command::Run.action()),
        ],
        idle,
    );
    if let Some(preview) = &editor.preview {
        let entries = preview.entries();
        let summary = crate::i18n::format(
            "classification-preview-summary",
            &[("count", entries.len().to_string())],
        );
        form.actions(
            t("classification-preview-title"),
            &summary,
            vec![
                (t("dialog-cancel"), Command::CancelPreview.action()),
                (
                    t("classification-preview-apply"),
                    Command::ApplyPreview.action(),
                ),
            ],
        );
        let (scene, bounds) = form.into_content();
        table::draw_preview(scene, bounds, entries);
        return;
    }
    if editor.applying.is_some() {
        form.info(t("ui-loading-desktop-items"), "");
        return;
    }
    if let Some(error) = &editor.error
        && !editing
    {
        form.info(error, "");
    } else if !editing && let Some(count) = editor.result {
        let result = if count == 0 {
            t("classification-result-empty").to_owned()
        } else {
            crate::i18n::format("classification-result", &[("count", count.to_string())])
        };
        form.info(&result, "");
    }
    if let Some(deleted) = &editor.deleted {
        form.actions_enabled(
            t("classification-deleted"), "",
            vec![(t("classification-undo"), Command::UndoDelete.action())],
            idle && encode(&config).is_ok_and(|raw| raw == deleted.after),
        );
    }
    if config.rules.is_empty() && !editing {
        form.info(t("classification-empty"), t("classification-add-hint"));
    } else {
        let (scene, bounds) = form.into_content();
        table::draw(scene, bounds, panels, &config.rules, editor);
    }
}

#[cfg(test)]
mod tests;
