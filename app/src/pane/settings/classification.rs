//! In-place editing and persistence for the ordered classification rules.
use super::*;
use super::super::animation::{Motion, SETTINGS_DURATION};
use luciddesk_core::{Classification, ClassificationError, ClassificationItemType, ClassificationRule};
use luciddesk_storage::SettingValue;
use std::collections::BTreeMap;
use std::time::Instant;
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
const SIDE_MARGIN: f32 = Tokens::INSET;
const BODY_TOP: f32 = TAB_TOP + NavigationItem::HEIGHT + Tokens::INSET;
const DEFAULT_WINDOW_WIDTH: f32 = 780.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 620.0;
const MIN_WINDOW_HEIGHT: f32 = 380.0;
#[cfg(test)]
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
        matches!(self, Self::Run | Self::ApplyPreview | Self::CancelPreview | Self::Save | Self::Cancel | Self::UndoDelete)
    }

    pub(super) fn is_control(self, action: &Action) -> bool {
        matches!(action, Action::Classification(value) if *value == self)
            || (self == Self::Target(PanelId::new(0))
                && matches!(action, Action::Dropdown(options) if options.iter().any(|(_, item, _)| {
                    matches!(item, Action::Classification(Self::Target(_)))
                })))
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

fn rule_feedback(error: &ClassificationError) -> (&'static str, Option<Field>) {
    match error {
        ClassificationError::TooManyRules => ("classification-rule-limit", None),
        ClassificationError::MissingTarget => ("classification-no-target", None),
        ClassificationError::MissingCondition => ("classification-condition-required", Some(Field::Extensions)),
        ClassificationError::ConflictingKeywords => ("classification-keyword-conflict", Some(Field::Keyword)),
        ClassificationError::BlankKeyword => ("classification-keyword-blank", Some(Field::Keyword)),
        ClassificationError::InvalidExtension => ("classification-extension-invalid", Some(Field::Extensions)),
        ClassificationError::InvalidRegex(_) => ("classification-regex-invalid", Some(Field::Keyword)),
    }
}

#[derive(Debug)]
enum SaveError {
    Rule(ClassificationError),
    Other(String),
}

impl SaveError {
    fn message(self) -> String {
        match self {
            Self::Rule(error) => t(rule_feedback(&error).0).into(),
            Self::Other(error) => error,
        }
    }
}

fn save(s: &PaneApp, config: &Classification) -> Result<(), SaveError> {
    config.compile().map_err(SaveError::Rule)?;
    s.store
        .save_settings(
            &[(KEY.into(), SettingValue::Text(encode(config).map_err(SaveError::Other)?))]
                .into_iter()
                .collect(),
        )
        .map_err(|e| SaveError::Other(e.to_string()))
}

fn targets(panels: &[Panel]) -> impl Iterator<Item = &Panel> {
    panels
        .iter()
        .filter(|panel| panel.supports_tabs())
}

#[derive(Default)]
struct DeletedRules {
    rules: BTreeMap<usize, ClassificationRule>,
    after: String,
}

impl DeletedRules {
    fn original_index(&self, mut index: usize) -> usize {
        for &removed in self.rules.keys() {
            if removed > index {
                break;
            }
            index += 1;
        }
        index
    }
}

enum RulePresenceRows {
    Leaving { index: usize, original: usize, details: f32 },
    Restoring { indices: Vec<usize>, partial: Option<(usize, f32)> },
}

struct RulePresence {
    rows: RulePresenceRows,
    expected: String,
    motion: Motion,
}

impl RulePresence {
    fn progress(&self, now: Instant) -> Option<f32> {
        self.motion.sample(now).ok().filter(|&value| value != self.motion.to)
    }

    fn partial_removal(&self, now: Instant) -> Option<(usize, f32)> {
        let RulePresenceRows::Leaving { original, .. } = &self.rows else { return None; };
        Some((*original, self.progress(now)?))
    }
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
    deleted: Option<DeletedRules>,
    rule_presence: Option<RulePresence>,
    expanded: Option<usize>,
    detail_motions: HashMap<usize, Motion>,
    preview: Option<hybrid::OrganizePreview>,
    show_skipped: Option<bool>,
    skipped_motion: Option<Motion>,
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

    fn skipped_expanded(&self) -> bool {
        self.show_skipped.unwrap_or(false)
    }

    fn skipped_fraction(&self, expanded: bool, now: Instant) -> f32 {
        self.skipped_motion.as_ref().and_then(|motion| motion.sample(now).ok())
            .unwrap_or(if expanded { 1.0 } else { 0.0 })
            .clamp(0.0, 1.0)
    }

    fn animate_skipped(&mut self, previous: bool, now: Instant, animate: bool) -> bool {
        if !animate {
            self.skipped_motion = None;
            return false;
        }
        let to = if self.show_skipped == Some(true) { 1.0 } else { 0.0 };
        let motion = self.skipped_motion.get_or_insert_with(|| {
            Motion::settled(if previous { 1.0 } else { 0.0 }, now)
        });
        motion.retarget_with_duration(to, now, true, SETTINGS_DURATION) != to
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
        self.skipped_motion = None;
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
        start: impl FnOnce(&mut Option<hybrid::OrganizePreview>) -> Result<std::sync::mpsc::Receiver<Result<hybrid::OrganizeOutcome, String>>, String>,
    ) -> Result<(), String> {
        if unsafe { SetTimer(hwnd, ORGANIZE_TIMER, 100, None) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        match start(&mut self.preview) {
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
        self.detail_motions.clear();
    }

    fn animate_details(&mut self, previous: Option<usize>, now: Instant) {
        for (index, target) in [(previous, 0.0), (self.expanded, 1.0)] {
            if let Some(index) = index {
                self.detail_motions.entry(index).or_insert_with(|| Motion::settled(1.0 - target, now))
                    .retarget_with_duration(target, now, true, SETTINGS_DURATION);
            }
        }
    }

    fn detail_fraction(&self, index: usize, now: Instant) -> f32 {
        self.detail_motions.get(&index).and_then(|motion| motion.sample(now).ok())
            .unwrap_or_else(|| if self.expanded == Some(index) { 1.0 } else { 0.0 })
            .clamp(0.0, 1.0)
    }

    fn tick_disclosure_animations(&mut self, now: Instant) -> bool {
        self.detail_motions.retain(|_, motion| motion.sample(now).is_ok_and(|value| value != motion.to));
        if self.skipped_motion.as_ref().is_some_and(|motion| {
            !motion.sample(now).is_ok_and(|value| value != motion.to)
        }) {
            self.skipped_motion = None;
        }
        !self.detail_motions.is_empty() || self.skipped_motion.is_some()
    }

    fn finish_disclosure_animations(&mut self) {
        self.detail_motions.clear();
        self.skipped_motion = None;
    }

    fn update_rule_presence(&mut self, now: Instant) -> bool {
        if self.rule_presence.as_ref().is_some_and(|row| row.progress(now).is_none()) {
            self.rule_presence = None;
        }
        self.rule_presence.is_some()
    }

    fn rule_presence(&self, value: Option<&SettingValue>, now: Instant) -> Option<(&RulePresence, f32)> {
        let row = self.rule_presence.as_ref()?;
        if !matches!(value, Some(SettingValue::Text(raw)) if raw == &row.expected) {
            return None;
        }
        Some((row, row.progress(now)?))
    }

    fn animate_rule_rows(&mut self, rows: RulePresenceRows, expected: &str, hwnd: HWND, now: Instant) {
        self.rule_presence = None;
        if hwnd.is_null() || !scrollbar::animations_enabled() {
            return;
        }
        let target = if matches!(rows, RulePresenceRows::Leaving { .. }) { 0.0 } else { 1.0 };
        let mut motion = Motion::settled(1.0 - target, now);
        if motion.retarget_with_duration(target, now, true, SETTINGS_DURATION) != target {
            self.rule_presence = Some(RulePresence { rows, expected: expected.to_owned(), motion });
        }
    }

    fn drop_rule(&mut self, state: &PaneApp, from: usize, to: usize, expected: &SettingValue) -> bool {
        self.error = (|| {
            let settings = state.store.settings().map_err(|error| error.to_string())?;
            let current = settings.get(KEY);
            if current != Some(expected) {
                return Err(t("classification-conflict").into());
            }
            self.move_rule(state, &mut decode(current)?, from, to)
        })().err();
        self.error.is_none()
    }

    fn move_rule(
        &mut self,
        state: &PaneApp,
        config: &mut Classification,
        from: usize,
        to: usize,
    ) -> Result<(), String> {
        if from == to || from >= config.rules.len() || to >= config.rules.len() {
            return Ok(());
        }
        let rule = config.rules.remove(from);
        config.rules.insert(to, rule);
        save(state, config).map_err(SaveError::message)?;
        self.deleted = None;
        Ok(())
    }

    pub fn focus(&mut self, field: Field) {
        if let Some(draft) = &mut self.draft {
            draft.inputs.focus(field);
        }
    }

    fn report_rule_error(&mut self, error: &ClassificationError) -> String {
        let (key, field) = rule_feedback(error);
        if let Some(field) = field {
            self.focus(field);
            self.reveal(Command::Input(field));
        } else if matches!(error, ClassificationError::MissingTarget) {
            self.reveal(Command::Target(PanelId::new(0)));
        }
        t(key).into()
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
                self.skipped_motion = None;
                self.reveal(Command::Run);
                return Ok(());
            }
            Command::ApplyPreview if self.applying.is_none() => {
                if let Some(preview) = &self.preview {
                    if preview.move_count() > 0 {
                        self.start_organize(hwnd, command, |preview| {
                            hybrid::apply_organize(&mut state.borrow_mut(), preview)
                        })?;
                    }
                    self.preview = None;
                    self.skipped_motion = None;
                    self.reveal(Command::Run);
                }
                return Ok(());
            }
            Command::Run if self.applying.is_none() => {
                let preview = hybrid::preview_organize(&state.borrow())?;
                self.result = None;
                self.show_skipped = None;
                self.skipped_motion = None;
                self.preview = Some(preview);
                self.reveal(Command::CancelPreview);
                return Ok(());
            }
            Command::ToggleSkipped => {
                if self.preview.is_some() {
                    self.show_skipped = Some(!self.skipped_expanded());
                    self.reveal(command);
                }
                return Ok(());
            }
            Command::UndoOrganize if self.applying.is_none() => {
                self.start_organize(hwnd, command, |_| hybrid::undo_organize(&mut state.borrow_mut()))?;
                self.preview = None;
                self.show_skipped = None;
                self.skipped_motion = None;
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
        let mut restored = None;
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
                    self.reveal(Command::Target(PanelId::new(0)));
                    return Err(t("classification-no-target").into());
                }
                let rule = draft.edited_rule();
                let index = draft.index;
                rule.validate_extensions().map_err(|error| self.report_rule_error(&error))?;
                if let Some(index) = index {
                    *config
                        .rules
                        .get_mut(index)
                        .ok_or_else(|| t("classification-conflict"))? = rule;
                } else {
                    config.rules.push(rule);
                }
                save(&state.borrow(), &config).map_err(|error| match error {
                    SaveError::Rule(error) => self.report_rule_error(&error),
                    SaveError::Other(message) => message,
                })?;
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
                let previous = if let Some(batch) = &self.deleted && batch.after == encode(&config)? {
                    Some(batch)
                } else { None };
                let original = previous.map_or(i, |batch| batch.original_index(i));
                deleted = Some((i, original, config.rules.remove(i), previous.is_some()));
            }
            Command::UndoDelete => {
                let Some(deleted) = &self.deleted else { return Ok(()); };
                if encode(&config)? != deleted.after {
                    return Err(t("classification-conflict").into());
                }
                for (&index, rule) in &deleted.rules {
                    config.rules.insert(index, rule.clone());
                }
                restored = Some(deleted.rules.keys().copied().collect());
            }
            Command::Toggle(i) => {
                if let Some(rule) = config.rules.get_mut(i) {
                    rule.enabled = !rule.enabled;
                }
            }
            _ => return Ok(()),
        }
        save(&state.borrow(), &config).map_err(SaveError::message)?;
        let previous = self.deleted.take();
        if let Some((index, original, rule, continues)) = deleted {
            let mut batch = previous.filter(|_| continues).unwrap_or_default();
            batch.rules.insert(original, rule);
            batch.after = encode(&config).expect("validated classification serializes");
            let now = Instant::now();
            self.animate_rule_rows(RulePresenceRows::Leaving { index, original, details: self.detail_fraction(index, now) },
                &batch.after, hwnd, now);
            self.deleted = Some(batch);
        } else if let Some(indices) = restored {
            let now = Instant::now();
            let partial = self.rule_presence.as_ref()
                .filter(|row| previous.as_ref().is_some_and(|batch| batch.after == row.expected))
                .and_then(|row| row.partial_removal(now));
            let after = encode(&config).expect("validated classification serializes");
            self.animate_rule_rows(RulePresenceRows::Restoring { indices, partial }, &after, hwnd, now);
        } else {
            self.rule_presence = None;
        }
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

fn organizer_titlebar(scene: Scene, width: f32, maximized: bool) -> Scene {
    with_named_titlebar(scene, width, maximized, t("organizer-window-title"), SIDE_MARGIN)
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
    let mut scene = Scene::empty(backdrop, SIDE_MARGIN, TITLE_HEIGHT + BODY_TOP - Tokens::GAP);
    let labels = [
        (Tab::Rules, t("organizer-rules"), "\u{e8fd}"),
        (Tab::Organize, t("classification-run"), "\u{e8b7}"),
    ];
    let widths = labels.map(|(_, label, _)| {
        let text = components::text_width(label, Style::TEXT_SIZE).ceil();
        (text + NavigationItem::LABEL_INSET + NavigationItem::END_INSET).max(112.0)
    });
    let available = (width - 2.0 * SIDE_MARGIN - Tokens::GAP).max(2.0);
    let scale = (available / widths.iter().sum::<f32>()).min(1.0);
    let mut left = SIDE_MARGIN;
    for ((target, label, icon), preferred_width) in labels.into_iter().zip(widths) {
        let tab_width = preferred_width * scale;
        scene.control(ControlKind::Tab(icon), Rect::from_xywh(left, TAB_TOP, tab_width, NavigationItem::HEIGHT),
            label, Action::OrganizeTab(target), target == tab);
        left += tab_width + Tokens::GAP;
    }
    let form = SettingsForm::continuation_with_margin(&mut scene, width, BODY_TOP, SIDE_MARGIN);
    match tab {
        Tab::Rules => draw_rules(form, panels, value, editor),
        Tab::Organize => draw_organize(form, value, editor, undo_available),
    }
    let preview = editor.preview.as_ref();
    let deletion = if tab == Tab::Rules && !editor.editing() {
        editor.deleted.as_ref().and_then(|deleted| decode(value).ok().map(|config|
            (deleted.rules.len(), encode(&config).is_ok_and(|raw| raw == deleted.after))))
    } else { None };
    let footer = match tab {
        Tab::Rules if editor.editing() => vec![
            (t("classification-cancel"), Command::Cancel, false),
            (t("classification-save"), Command::Save, true),
        ],
        Tab::Rules if deletion.is_some() => vec![(t("classification-undo"), Command::UndoDelete, true)],
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
    if let Some((count, enabled)) = deletion && let Some(undo) = scene.controls.last_mut() {
        undo.enabled = enabled;
        let bounds = undo.bounds;
        let label = if count == 1 { t("classification-deleted").to_owned() } else {
            crate::i18n::format("classification-deleted-count", &[("count", count.to_string())])
        };
        let width = (components::text_width(&label, Style::TEXT_SIZE).ceil() + 48.0)
            .min(bounds.left - SIDE_MARGIN - Tokens::GAP).max(1.0);
        scene.footer_status = Some((Rect::from_xywh(SIDE_MARGIN, bounds.top,
            width, Tokens::CONTROL_HEIGHT), label));
    }
    scene
}

#[cfg(test)]
fn preferred_preview_height(scene: &Scene) -> f32 {
    (scene.content_bottom() + Tokens::MARGIN + scene.footer_height)
        .clamp(MIN_WINDOW_HEIGHT, MAX_PREVIEW_HEIGHT)
}

fn draw_footer(scene: &mut Scene, width: f32, height: f32, actions: &[(&str, Command, bool)]) {
    if actions.is_empty() {
        return;
    }
    let width = (width - 2.0 * SIDE_MARGIN).min(Tokens::MAX_WIDTH);
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
    let mut left = SIDE_MARGIN + width - gaps - total * scale;
    for (&(label, action, primary), width) in actions.iter().zip(widths) {
        let width = width * scale;
        let refresh = action == Command::Run && !primary;
        let kind = if primary { ControlKind::PrimaryButton } else if refresh {
            ControlKind::QuietButton("\u{e72c}")
        } else { ControlKind::Button };
        scene.control(
            kind,
            Rect::from_xywh(if refresh { SIDE_MARGIN } else { left }, top, width, Tokens::CONTROL_HEIGHT),
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
    let presence = editor.rule_presence(value, Instant::now());
    if config.rules.is_empty() && presence.is_none() {
        form.info(t("classification-empty"), if has_targets { t("classification-add-hint") } else { "" });
    } else {
        let (scene, bounds) = form.into_content();
        table::draw(scene, bounds, panels, &config.rules, editor, presence);
    }
}

fn draw_organize(
    mut form: SettingsForm<'_>,
    value: Option<&SettingValue>,
    editor: &Editor,
    undo_available: bool,
) {
    let undo = undo_available.then(|| (t("classification-undo-organize"), Command::UndoOrganize.action()));
    if let Some(preview) = &editor.preview {
        if let Some(error) = &editor.organize_error {
            form.status(error, Tokens::STATUS_ERROR, None);
        }
        let (scene, mut bounds) = form.into_content();
        table::draw_preview_summary(scene, &mut bounds, preview, preview.is_stale(value), undo);
        let expanded = editor.skipped_expanded();
        let reveal = editor.skipped_fraction(expanded, Instant::now());
        table::draw_preview(scene, bounds, preview, (expanded, reveal));
        return;
    }
    if let Some(pending) = &editor.applying {
        let operation = t(if pending.command == Command::UndoOrganize {
            "classification-undo-in-progress"
        } else {
            "classification-apply-in-progress"
        });
        form.busy(operation, t("ui-loading-desktop-items"));
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
