//! In-place editing and persistence for the ordered classification rules.
use super::*;
use luciddesk_core::{Classification, ClassificationError, ClassificationItemType, ClassificationRule};
use luciddesk_storage::SettingValue;
use windows_sys::Win32::Foundation::HWND;

mod editor;
mod host;
mod table;
use editor::{Draft, Inputs};
pub(super) use editor::{Field, input_colors};
pub(super) use hybrid::CLASSIFICATION_KEY as KEY;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Rules,
    Organize,
}

const TAB_TOP: f32 = Tokens::GAP;
const BODY_TOP: f32 = TAB_TOP + NavigationItem::HEIGHT + Tokens::INSET;
const MIN_WINDOW_HEIGHT: f32 = 380.0;
const MAX_PREVIEW_HEIGHT: f32 = 800.0;

pub(in crate::pane) fn show_organize_preview(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    host::show(state, Tab::Organize)
}

pub(super) fn show_rules(state: &Rc<RefCell<PaneApp>>) -> Result<(), String> {
    host::show(state, Tab::Rules)
}

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
    ToggleSkipped,
    UndoOrganize,
    Input(Field),
    Target(PanelId),
    DraftItemType(Option<ClassificationItemType>),
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

    fn tab(self) -> Tab {
        match self {
            Self::Run | Self::ApplyPreview | Self::CancelPreview | Self::ToggleSkipped | Self::UndoOrganize => Tab::Organize,
            _ => Tab::Rules,
        }
    }

    pub(super) fn is_footer(self) -> bool {
        matches!(self, Self::Run | Self::ApplyPreview | Self::CancelPreview | Self::Save | Self::Cancel)
    }

    pub(super) fn is_control(self, action: &Action) -> bool {
        matches!(action, Action::Classification(value) if *value == self)
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

enum OrganizeResult {
    Applied(Vec<hybrid::OrganizeMove>),
    Undone { restored: usize, skipped: usize },
}

struct PendingOrganize {
    command: Command,
    result: std::sync::mpsc::Receiver<Result<hybrid::OrganizeOutcome, String>>,
}

#[derive(Default)]
pub(super) struct Editor {
    draft: Option<Draft>,
    deleted: Option<DeletedRule>,
    expanded: Option<usize>,
    preview: Option<hybrid::OrganizePreview>,
    show_skipped: Option<bool>,
    applying: Option<PendingOrganize>,
    result: Option<OrganizeResult>,
    error: Option<String>,
    organize_error: Option<String>,
    reveal: [Option<Command>; 2],
}

impl Editor {
    pub fn editing(&self) -> bool {
        self.draft.is_some()
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    fn skipped_expanded(&self, preview: &hybrid::OrganizePreview) -> bool {
        self.show_skipped.unwrap_or(preview.move_count() == 0)
    }

    pub(super) fn poll_organize(&mut self) -> bool {
        use std::sync::mpsc::TryRecvError;
        let Some(pending) = &self.applying else { return false; };
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err(t("classification-desktop-unavailable").into()),
        };
        self.applying = None;
        match result {
            Ok(hybrid::OrganizeOutcome::Applied(moves)) => {
                self.result = Some(OrganizeResult::Applied(moves));
                self.reveal(Command::UndoOrganize);
            }
            Ok(hybrid::OrganizeOutcome::Reconfirm(preview)) => {
                self.preview = Some(preview);
                self.reveal(Command::CancelPreview);
            }
            Ok(hybrid::OrganizeOutcome::Undone { restored, skipped }) => {
                self.result = Some(OrganizeResult::Undone { restored, skipped });
                self.reveal(Command::Run);
            }
            Err(error) => {
                self.organize_error = Some(error);
                self.reveal(Command::Run);
            }
        }
        true
    }

    fn start_organize(
        &mut self,
        hwnd: HWND,
        command: Command,
        start: impl FnOnce() -> Result<std::sync::mpsc::Receiver<Result<hybrid::OrganizeOutcome, String>>, String>,
    ) -> Result<(), String> {
        if unsafe { SetTimer(hwnd, ORGANIZE_TIMER, 100, None) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        match start() {
            Ok(receiver) => {
                self.applying = Some(PendingOrganize { command, result: receiver });
                self.result = None;
                Ok(())
            }
            Err(error) => {
                unsafe { KillTimer(hwnd, ORGANIZE_TIMER); }
                Err(error)
            }
        }
    }

    fn reveal(&mut self, command: Command) {
        self.reveal[command.tab() as usize] = Some(command);
    }

    fn take_reveal(&mut self, tab: Tab) -> Option<Command> {
        self.reveal[tab as usize].take()
    }

    pub fn sync(&mut self, scene: &Scene, scale: f32, dark: bool) {
        if let Some(draft) = &mut self.draft {
            draft.inputs.sync(scene, scale, dark, draft.regex);
        }
    }

    pub fn discard(&mut self) {
        *self = Self::default();
    }

    fn hide_inputs(&mut self) {
        if let Some(draft) = &mut self.draft { draft.inputs.hide(); }
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
        } else if command.tab() == Tab::Organize {
            self.organize_error = self.apply(state, hwnd, command).err();
        } else {
            self.error = self.apply(state, hwnd, command).err();
            if command == Command::Save && self.editing() {
                let field = match self.error.as_deref() {
                    Some(error) if error == t("classification-regex-invalid") => Field::Keyword,
                    _ => Field::Extensions,
                };
                self.focus(field);
                self.reveal(Command::Input(field));
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
            Command::CancelPreview => {
                self.preview = None;
                self.show_skipped = None;
                self.reveal(Command::Run);
                return Ok(());
            }
            Command::ApplyPreview if self.applying.is_none() => {
                if let Some(preview) = self.preview.take() {
                    self.reveal(Command::Run);
                    if preview.move_count() > 0 {
                        return self.start_organize(hwnd, command, || hybrid::apply_organize(&mut state.borrow_mut(), preview));
                    }
                }
                return Ok(());
            }
            Command::Run if self.applying.is_none() => {
                let preview = hybrid::preview_organize(&state.borrow())?;
                self.result = None;
                self.show_skipped = None;
                self.preview = Some(preview);
                self.reveal(Command::CancelPreview);
                return Ok(());
            }
            Command::ToggleSkipped => {
                if let Some(preview) = &self.preview {
                    self.show_skipped = Some(!self.skipped_expanded(preview));
                    self.reveal(command);
                }
                return Ok(());
            }
            Command::UndoOrganize if self.applying.is_none() => {
                self.start_organize(hwnd, command, || hybrid::undo_organize(&mut state.borrow_mut()))?;
                self.preview = None;
                self.show_skipped = None;
                return Ok(());
            }
            Command::Target(id) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.target_panel = id.get();
                }
                return Ok(());
            }
            Command::DraftItemType(item_type) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.item_type = item_type;
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
                            let (target_panel, extensions, item_type) = match command {
                                Command::AddTemplate(template) => (
                                    if targets.next().is_some() {
                                        0
                                    } else {
                                        first.id().get()
                                    },
                                    template.extensions(),
                                    (template == Template::Documents).then_some(ClassificationItemType::File),
                                ),
                                _ => (first.id().get(), Vec::new(), None),
                            };
                            (
                                None,
                                ClassificationRule {
                                    target_panel,
                                    enabled: true,
                                    item_type,
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
                self.reveal(Command::Input(Field::Extensions));
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
                self.reveal(command);
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
                    self.reveal(Command::Edit(to));
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
        if let Some(draft) = self.draft.take() {
            self.reveal(draft.index.map_or(Command::Add, Command::Edit));
        }
    }
}

pub(super) fn settings_entry(scene: &mut Scene, width: f32) {
    let mut form = SettingsForm::new(scene, width, t("classification-description"));
    form.primary_button(t("organizer-window-title"), "", t("organizer-open"), Action::OpenOrganizer);
}

fn window_scene(
    (width, height): (f32, f32),
    backdrop: Backdrop,
    tab: Tab,
    panels: &[Panel],
    value: Option<&SettingValue>,
    editor: &Editor,
    undo_available: bool,
) -> Scene {
    let mut scene = Scene::empty(backdrop, Tokens::MARGIN, TITLE_HEIGHT + BODY_TOP - Tokens::GAP);
    let labels = [
        (Tab::Rules, t("organizer-rules"), "\u{e8fd}"),
        (Tab::Organize, t("classification-run"), "\u{e8b7}"),
    ];
    let widths = labels.map(|(_, label, _)| {
        let text = components::text_width(label, Style::TEXT_SIZE).ceil();
        (text + NavigationItem::LABEL_INSET + NavigationItem::END_INSET).max(112.0)
    });
    let available = (width - 2.0 * Tokens::MARGIN - Tokens::GAP).max(2.0);
    let scale = (available / widths.iter().sum::<f32>()).min(1.0);
    let mut left = Tokens::MARGIN;
    for ((target, label, icon), preferred_width) in labels.into_iter().zip(widths) {
        let tab_width = preferred_width * scale;
        scene.control(ControlKind::Tab(icon), Rect::from_xywh(left, TAB_TOP, tab_width, NavigationItem::HEIGHT),
            label, Action::OrganizeTab(target), target == tab);
        left += tab_width + Tokens::GAP;
    }
    let form = SettingsForm::continuation(&mut scene, width, BODY_TOP);
    match tab {
        Tab::Rules => draw_rules(form, panels, value, editor),
        Tab::Organize => draw_organize(form, editor, undo_available),
    }
    let preview = editor.preview.as_ref();
    let footer = match tab {
        Tab::Rules if editor.editing() => vec![
            (t("classification-cancel"), Command::Cancel, false),
            (t("classification-save"), Command::Save, true),
        ],
        Tab::Organize if editor.applying.is_none() => match preview {
            Some(preview) => {
                let can_apply = preview.move_count() > 0;
                let mut actions = vec![
                    (t("organizer-refresh-preview"), Command::Run, false),
                    (t("dialog-cancel"), Command::CancelPreview, false),
                ];
                if can_apply {
                    let apply = t(if preview.changes.is_some() {
                        "classification-preview-confirm"
                    } else {
                        "classification-preview-apply"
                    });
                    actions.push((apply, Command::ApplyPreview, true));
                }
                actions
            }
            None => {
                let label = if matches!(editor.result, Some(OrganizeResult::Undone { .. })) {
                    "organizer-return-to-preview"
                } else {
                    "organizer-generate-preview"
                };
                vec![(t(label), Command::Run, true)]
            }
        },
        _ => Vec::new(),
    };
    draw_footer(&mut scene, width, height, &footer);
    scene
}

fn preferred_preview_height(scene: &Scene) -> f32 {
    (scene.content_bottom() + Tokens::MARGIN + scene.footer_height)
        .clamp(MIN_WINDOW_HEIGHT, MAX_PREVIEW_HEIGHT)
}

fn draw_footer(scene: &mut Scene, width: f32, height: f32, actions: &[(&str, Command, bool)]) {
    if actions.is_empty() {
        return;
    }
    let width = (width - 2.0 * Tokens::MARGIN).min(Tokens::MAX_WIDTH);
    scene.footer_height = Tokens::CONTROL_HEIGHT + 2.0 * Tokens::INSET;
    let top = height - TITLE_HEIGHT - scene.footer_height + Tokens::INSET;
    let gap = luciddesk_window::control_style::BUTTON_GAP;
    let padding = luciddesk_window::control_style::BUTTON_PADDING;
    let gaps = (actions.len() - 1) as f32 * gap;
    let widths: Vec<_> = actions.iter()
        .map(|(label, action, primary)| {
            let refresh = *action == Command::Run && !primary;
            (components::text_width(label, Style::TEXT_SIZE).ceil() + padding * 2.0 + if refresh { 20.0 } else { 0.0 }).max(96.0)
        })
        .collect();
    let total = widths.iter().sum::<f32>();
    let scale = ((width - gaps) / total).min(1.0);
    let mut left = Tokens::MARGIN + width - gaps - total * scale;
    for (&(label, action, primary), width) in actions.iter().zip(widths) {
        let width = width * scale;
        let refresh = action == Command::Run && !primary;
        let kind = if primary { ControlKind::PrimaryButton } else if refresh {
            ControlKind::QuietButton("\u{e72c}")
        } else { ControlKind::Button };
        scene.control(
            kind,
            Rect::from_xywh(if refresh { Tokens::MARGIN } else { left }, top, width, Tokens::CONTROL_HEIGHT),
            label,
            action.action(),
            false,
        );
        left += width + gap;
    }
}

fn draw_rules(
    mut form: SettingsForm<'_>,
    panels: &[Panel],
    value: Option<&SettingValue>,
    editor: &Editor,
) {
    let config = match decode(value) {
        Ok(config) => config,
        Err(error) => {
            form.status(&error, Tokens::STATUS_ERROR, None);
            return;
        }
    };
    if let Some(draft) = &editor.draft {
        let (scene, bounds) = form.into_content();
        table::draw_editor(scene, bounds, panels, draft, editor.error.as_deref());
        return;
    }
    let choices = Template::ALL.map(|template| {
        (
            template.label().into(),
            Command::AddTemplate(template).action(),
            false,
        )
    });
    let has_targets = targets(panels).next().is_some();
    form.group(|form| {
        form.toggle(
            t("classification-auto"),
            t("classification-auto-description"),
            config.enabled,
            Command::Enable(!config.enabled).action(),
        );
        form.primary_actions(
            t("classification-rules"),
            t(if has_targets { "classification-priority" } else { "classification-no-target" }),
            (t("organizer-rule-templates"), Action::Dropdown(choices.into())),
            (t("classification-add"), Command::Add.action()),
            has_targets,
        );
    });
    if let Some(error) = &editor.error {
        form.status(error, Tokens::STATUS_ERROR, None);
    }
    if let Some(deleted) = &editor.deleted {
        form.actions_enabled(
            t("classification-deleted"), "",
            vec![(t("classification-undo"), Command::UndoDelete.action())],
            encode(&config).is_ok_and(|raw| raw == deleted.after),
        );
    }
    if config.rules.is_empty() {
        form.info(t("classification-empty"), if has_targets { t("classification-add-hint") } else { "" });
    } else {
        let (scene, bounds) = form.into_content();
        table::draw(scene, bounds, panels, &config.rules, editor);
    }
}

fn draw_organize(mut form: SettingsForm<'_>, editor: &Editor, undo_available: bool) {
    let undo = undo_available.then(|| (t("classification-undo-organize"), Command::UndoOrganize.action()));
    if let Some(preview) = &editor.preview {
        if let Some(error) = &editor.organize_error {
            form.status(error, Tokens::STATUS_ERROR, None);
        }
        let (scene, mut bounds) = form.into_content();
        table::draw_preview_summary(scene, &mut bounds, preview, undo);
        table::draw_preview(scene, bounds, preview, editor.skipped_expanded(preview));
        return;
    }
    if let Some(pending) = &editor.applying {
        let operation = t(if pending.command == Command::UndoOrganize {
            "classification-undo-in-progress"
        } else {
            "classification-apply-in-progress"
        });
        form.info(operation, t("ui-loading-desktop-items"));
        return;
    }
    if let Some(error) = &editor.organize_error {
        form.status(error, Tokens::STATUS_ERROR, None);
    }
    if let Some(result) = &editor.result {
        let (scene, bounds) = form.into_content();
        table::draw_result(scene, bounds, result, undo);
    } else if let Some(undo) = undo {
        form.actions(t("classification-preview-title"), t("organizer-preview-description"), vec![undo]);
    } else {
        form.info(t("classification-preview-title"), t("organizer-preview-description"));
    }
}

#[cfg(test)]
mod tests;
