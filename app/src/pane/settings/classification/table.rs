//! Layout for classification rules, the editor, and organization summaries.
use super::*;
use components::{text_height, text_width};
use std::borrow::Cow;

const CONTROL: f32 = Tokens::CONTROL_HEIGHT;
const GAP: f32 = Tokens::GAP;
const INSET: f32 = Tokens::INSET;
const SWITCH_WIDTH: f32 = 46.0;
const SWITCH_HEIGHT: f32 = 24.0;
const SWITCH_ACTION_GAP: f32 = INSET;
const BUTTON_GAP: f32 = GAP;
const HEADING_SIZE: f32 = 12.0;
const CONDITION_LABELS: [&str; 4] = [
    "classification-item-type",
    "classification-extensions",
    "classification-keyword",
    "classification-pattern",
];

const ITEM_TYPES: [Option<ClassificationItemType>; 4] = [
    None,
    Some(ClassificationItemType::File),
    Some(ClassificationItemType::Folder),
    Some(ClassificationItemType::Shortcut),
];

fn item_type_label(item_type: Option<ClassificationItemType>) -> &'static str {
    t(match item_type {
        None => "classification-item-type-any",
        Some(ClassificationItemType::File) => "classification-item-type-file",
        Some(ClassificationItemType::Folder) => "classification-item-type-folder",
        Some(ClassificationItemType::Shortcut) => "classification-item-type-shortcut",
    })
}

struct Columns {
    left: f32,
    widths: [f32; 4],
    gaps: [f32; 4],
}

impl Columns {
    fn new(left: f32, width: f32, enabled_label: &str, show_grip: bool) -> Self {
        let actions = CONTROL + CONTROL + BUTTON_GAP;
        let status = text_width(enabled_label, HEADING_SIZE).ceil().max(SWITCH_WIDTH);
        let grip = if show_grip { RuleRow::GRIP_WIDTH } else { 0.0 };
        let gaps = [if show_grip { GAP } else { 0.0 }, INSET, SWITCH_ACTION_GAP, 0.0];
        let target = (width - grip - actions - status - gaps.iter().sum::<f32>()).max(1.0);
        Self {
            left,
            widths: [grip, target, status, actions],
            gaps,
        }
    }

    fn cells(&self, top: f32, height: f32) -> [Rect; 4] {
        let mut left = self.left;
        std::array::from_fn(|index| {
            let width = self.widths[index];
            let cell = Rect::from_xywh(left, top, width, height);
            left += width + self.gaps[index];
            cell
        })
    }
}

pub(super) fn draw(
    scene: &mut Scene,
    bounds: Rect,
    panels: &[Panel],
    rules: &[ClassificationRule],
    editor: &Editor,
) {
    let left = bounds.left + INSET;
    let right = bounds.right - INSET;
    let width = right - left;
    let enabled_label = t("classification-enabled");
    let columns = Columns::new(left, width, enabled_label, rules.len() > 1);
    let header_top = bounds.top + GAP / 2.0;
    let [_, target, status, actions] = columns.cells(header_top, CONTROL);
    let actions_label = t("classification-actions");
    let header_cells = [
        summary_target(target),
        centered_label(status, enabled_label, status.right - SWITCH_WIDTH / 2.0),
        centered_label(actions, actions_label, (actions.left + actions.right) / 2.0),
    ];
    let labels = [
        t("classification-target"),
        enabled_label,
        actions_label,
    ];
    let heading = header_cells
        .iter()
        .zip(labels)
        .map(|(cell, label)| text_height(label, HEADING_SIZE, cell.right - cell.left))
        .fold(CONTROL, f32::max);
    for (mut cell, label) in header_cells.into_iter().zip(labels) {
        cell.bottom = cell.top + heading;
        scene.text(cell, label, 6);
    }
    let mut y = header_top + heading + GAP / 2.0;
    let now = Instant::now();
    for (index, rule) in rules.iter().enumerate() {
        let (controls, text, separators, clips) = (
            scene.controls.len(), scene.text.len(), scene.separators.len(), scene.disclosure_clips.len(),
        );
        let divider_left = if index == 0 { left } else { target.left };
        scene.separators.push(Rect::from_xywh(divider_left, y, right - divider_left, 1.0));
        let bottom = saved_row(
            scene,
            columns.cells(y + GAP, CONTROL),
            panels,
            rule,
            index,
            (editor.expanded == Some(index), editor.detail_fraction(index, now)),
        ) + GAP;
        scene.rule_rows.push(RuleRow {
            bounds: Rect::from_xywh(left, y, width, bottom - y),
            offset: 0.0,
            controls: controls..scene.controls.len(),
            text: text..scene.text.len(),
            separators: separators..scene.separators.len(),
            detail_clips: clips..scene.disclosure_clips.len(),
        });
        y = bottom;
    }
    scene.cards.push(Rect {
        bottom: y,
        ..bounds
    });
}

pub(super) fn draw_preview_summary(
    scene: &mut Scene,
    bounds: &mut Rect,
    preview: &hybrid::OrganizePreview,
    undo: Option<(&str, Action)>,
) {
    let moved = preview.move_count();
    let skipped = preview.entries.len() - moved;
    let changed = preview.changes.is_some();
    let title = t(if changed {
        "classification-preview-changed"
    } else {
        "classification-preview-title"
    });
    let note = if preview.entries.is_empty() {
        Some(t("classification-preview-empty"))
    } else if changed && moved > 0 {
        Some(t("organizer-review-changes"))
    } else {
        None
    };
    let counts = [(moved, t("organizer-to-group")), (skipped, t("organizer-skipped"))];
    let counts = if preview.entries.is_empty() { &[] } else { counts.as_slice() };
    bounds.top = draw_summary(scene, *bounds, title, None, note, counts, undo) + GAP;
}

pub(super) fn draw_result(
    scene: &mut Scene,
    mut bounds: Rect,
    result: &OrganizeResult,
    undo: Option<(&str, Action)>,
) {
    use luciddesk_window::control_style::{CHECK_MARK, INFO, WARNING};

    match *result {
        OrganizeResult::Applied(ref moves) => {
            let count = moves.len();
            bounds.top = draw_summary(
                scene, bounds, t("organizer-applied-heading"), Some(if count > 0 { CHECK_MARK } else { INFO }),
                None, &[(count, t("organizer-organized"))], undo,
            ) + GAP;
            draw_move_groups(
                scene, &mut bounds, moves.iter(), t("organizer-organized-icons"),
            );
        }
        OrganizeResult::Undone { restored, skipped } => {
            let (title, icon) = if skipped > 0 {
                (t("organizer-undo-summary"), WARNING)
            } else if restored > 0 {
                (t("organizer-undo-heading"), CHECK_MARK)
            } else {
                (t("organizer-undo-summary"), INFO)
            };
            draw_summary(
                scene, bounds, title, Some(icon), None,
                &[(restored, t("organizer-restored")), (skipped, t("organizer-skipped"))], undo,
            );
        }
    }
}

fn draw_summary(
    scene: &mut Scene,
    bounds: Rect,
    title: &str,
    icon: Option<&str>,
    note: Option<&str>,
    counts: &[(usize, &str)],
    undo: Option<(&str, Action)>,
) -> f32 {
    let icon_left = bounds.left + INSET;
    let left = icon_left + if icon.is_some() { Style::ICON_SLOT + GAP } else { 0.0 };
    let right = bounds.right - INSET;
    let width = right - left;
    let top = bounds.top + INSET;
    let button_width = undo.as_ref().map_or(0.0, |(label, _)| {
        (text_width(label, 14.0) + 2.0 * INSET)
            .max(112.0)
            .min(width)
    });
    let inline = undo.is_none() || width - button_width - INSET >= 246.0;
    let summary_width = if inline && undo.is_some() {
        width - button_width - INSET
    } else {
        width
    };
    let heading = text_height(title, 16.0, summary_width).max(24.0);
    if let Some(icon) = icon {
        scene.text(Rect::from_xywh(icon_left, top, Style::ICON_SLOT, 24.0), icon, 4);
    }
    scene.text(
        Rect::from_xywh(left, top, summary_width, heading),
        title,
        Tokens::SECTION_TITLE,
    );
    let mut bottom = top + heading;
    if let Some(note) = note {
        let line = text_height(note, 12.0, summary_width).max(20.0);
        scene.text(
            Rect::from_xywh(left, bottom + GAP / 2.0, summary_width, line),
            note,
            6,
        );
        bottom += line + GAP / 2.0;
    }
    if !counts.is_empty() {
        let column = (summary_width / counts.len() as f32).min(180.0);
        let top = bottom + GAP;
        for (index, &(count, label)) in counts.iter().enumerate() {
            let x = left + index as f32 * column;
            let value = count.to_string();
            let number_width = (text_width(&value, 20.0) + GAP).min(column / 2.0);
            let label_width = (column - number_width - GAP).max(1.0);
            let height = text_height(label, 12.0, label_width).max(28.0);
            scene.text(Rect::from_xywh(x, top, number_width, height), value, 2);
            scene.text(
                Rect::from_xywh(x + number_width, top, label_width, height),
                label,
                6,
            );
            bottom = bottom.max(top + height);
        }
    }
    if let Some((label, action)) = undo {
        let y = if inline {
            top + ((bottom - top - CONTROL) / 2.0).max(0.0)
        } else {
            bottom + GAP
        };
        scene.button(
            Rect::from_xywh(right - button_width, y, button_width, CONTROL),
            label,
            action,
            false,
        );
        bottom = bottom.max(y + CONTROL);
    }
    scene.cards.push(Rect {
        bottom: bottom + INSET,
        ..bounds
    });
    bottom + INSET
}

pub(super) fn draw_preview(
    scene: &mut Scene,
    mut bounds: Rect,
    preview: &hybrid::OrganizePreview,
    disclosure: (bool, f32),
) {
    if let Some(changes) = preview.changes.as_deref().filter(|changes| !changes.is_empty()) {
        draw_preview_changes(scene, &mut bounds, changes);
    }
    draw_move_groups(scene, &mut bounds, preview.moves(), t("classification-preview-moves"));
    let skipped = preview.entries.len() - preview.move_count();
    if skipped > 0 {
        draw_skipped(
            scene,
            bounds,
            skipped,
            preview
                .entries
                .iter()
                .filter(|entry| disclosure.1 > 0.0 && matches!(entry, hybrid::PreviewEntry::Skip { .. }))
                .map(preview_row),
            disclosure,
        );
    }
}

fn draw_move_groups<'a>(
    scene: &mut Scene,
    bounds: &mut Rect,
    moves: impl Iterator<Item = &'a hybrid::OrganizeMove>,
    title: &str,
) {
    let mut groups: Vec<Vec<&hybrid::OrganizeMove>> = Vec::new();
    let mut indices = HashMap::new();
    for entry in moves {
        let index = indices.entry(entry.target).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[*index].push(entry);
    }
    if groups.is_empty() {
        return;
    }

    let left = bounds.left + INSET;
    let width = bounds.right - bounds.left - 2.0 * INSET;
    let row_left = left + SectionHeader::TITLE_INSET;
    let row_width = width - SectionHeader::TITLE_INSET;
    let mut top = preview_heading(scene, left, bounds.top + GAP, width, title);
    for group in groups {
        scene.separators.push(Rect::from_xywh(left, top, width, 1.0));
        let header = SectionHeader::new(
            Rect::from_xywh(left, top + GAP / 2.0, width, 0.0),
            luciddesk_window::control_style::FOLDER,
            &group[0].target_name,
            icon_count(group.len()),
        );
        top = header.bounds.bottom;
        scene.section_headers.push(header);

        for entry in group {
            let rule = matched_rule(entry.rule);
            let rule_width = (text_width(&rule, 12.0).ceil() + 1.0).min(row_width / 3.0);
            let name_width = row_width - GAP - rule_width;
            let height = text_height(&entry.item_name, 14.0, name_width).max(24.0);
            scene.text(
                Rect::from_xywh(row_left, top, name_width, height),
                &entry.item_name, 1,
            );
            scene.text(
                Rect::from_xywh(row_left + name_width + GAP, top, rule_width, height),
                rule, Tokens::SECONDARY_VALUE,
            );
            top += height + GAP / 2.0;
        }
        top += GAP / 2.0;
    }
    scene.cards.push(Rect { bottom: top + GAP, ..*bounds });
    bounds.top = top + INSET;
}

fn preview_heading(scene: &mut Scene, left: f32, top: f32, width: f32, title: &str) -> f32 {
    let height = text_height(title, 16.0, width).max(CONTROL);
    scene.text(Rect::from_xywh(left, top, width, height), title, Tokens::SECTION_TITLE);
    top + height
}

fn draw_preview_changes(scene: &mut Scene, bounds: &mut Rect, changes: &[hybrid::PreviewChange]) {
    let left = bounds.left + INSET;
    let width = bounds.right - bounds.left - 2.0 * INSET;
    let mut top = preview_heading(
        scene, left, bounds.top + GAP, width, t("classification-preview-differences"),
    );
    for change in changes {
        scene.separators.push(Rect::from_xywh(left, top, width, 1.0));
        top += GAP;
        if let hybrid::PreviewChange::Changed {
            before: hybrid::PreviewEntry::Move(before),
            after: hybrid::PreviewEntry::Move(after),
        } = change
            && (before.target != after.target || before.target_name != after.target_name)
        {
            let name = transition(&before.item_name, &after.item_name);
            top = draw_change_title(scene, left, top, width, name, t("classification-diff-changed"));
            let pair = DestinationChange::new(
                Rect::from_xywh(left, top + GAP / 2.0, width, 0.0),
                (&before.target_name, matched_rule(before.rule)),
                (&after.target_name, matched_rule(after.rule)),
            );
            top = pair.bounds.bottom;
            scene.destination_changes.push(pair);
        } else {
            let (status, name, detail) = change_row(change);
            top = draw_change_title(scene, left, top, width, name, t(status));
            let height = text_height(&detail, 12.0, width).max(20.0);
            scene.text(Rect::from_xywh(left, top, width, height), detail, 6);
            top += height;
        }
        top += GAP;
    }
    scene.cards.push(Rect { bottom: top + GAP, ..*bounds });
    bounds.top = top + INSET;
}

fn draw_change_title(scene: &mut Scene, left: f32, top: f32, width: f32, name: String, status: &str) -> f32 {
    let status_width = (text_width(status, 12.0).ceil() + 1.0).min(width / 3.0);
    let name_width = width - status_width - INSET;
    let height = text_height(&name, 16.0, name_width).max(24.0);
    scene.text(Rect::from_xywh(left, top, name_width, height), name, Tokens::SECTION_TITLE);
    scene.text(
        Rect::from_xywh(left + width - status_width, top, status_width, 24.0),
        status, Tokens::SECONDARY_VALUE,
    );
    top + height
}

fn draw_skipped(
    scene: &mut Scene,
    bounds: Rect,
    count: usize,
    entries: impl Iterator<Item = (String, String)>,
    (expanded, reveal): (bool, f32),
) {
    use luciddesk_window::controls::InlineControl;

    let left = bounds.left + INSET;
    let width = (bounds.right - bounds.left - 2.0 * INSET).max(1.0);
    let heading_top = bounds.top + GAP;
    let summary = icon_count(count);
    let summary_width = SectionHeader::measure_summary(&summary, width);
    let padding = InlineControl::SECTION_DISCLOSURE_PADDING;
    scene.control(
        ControlKind::SectionDisclosure { summary_width },
        Rect::from_xywh(left - padding, heading_top, width + 2.0 * padding, CONTROL),
        t("classification-preview-skipped"),
        Command::ToggleSkipped.action(),
        expanded,
    );
    scene.text(
        Rect::from_xywh(left + width - summary_width, heading_top, summary_width, CONTROL),
        summary,
        Tokens::SECONDARY_VALUE,
    );
    let rows_top = heading_top + CONTROL;
    let first_text = scene.text.len();
    let first_separator = scene.separators.len();
    let mut top = rows_top;
    let inset = InlineControl::DISCLOSURE_LABEL_INSET;
    let row_width = (width - inset).max(1.0);
    for (index, (name, detail)) in entries.enumerate() {
        if index > 0 {
            scene.separators.push(Rect::from_xywh(left + inset, top, row_width, 1.0));
        }
        let row = Rect::from_xywh(left + inset, top + GAP / 2.0, row_width, 0.0);
        top = preview_text_row(scene, row, name, detail) + GAP / 2.0;
    }
    let height = (top - rows_top) * reveal;
    if reveal > 0.0 && reveal < 1.0 {
        scene.disclosure_clips.push(DisclosureClip {
            text: first_text..scene.text.len(),
            separators: first_separator..scene.separators.len(),
            bounds: Rect::from_xywh(left, rows_top, width, height),
        });
    }
    scene.cards.push(Rect { bottom: rows_top + height + GAP, ..bounds });
}

fn icon_count(count: usize) -> String {
    crate::i18n::format("organizer-group-count", &[("count", count.to_string())])
}

fn preview_text_row(scene: &mut Scene, bounds: Rect, name: String, detail: String) -> f32 {
    let left = bounds.left;
    let width = bounds.right - left;
    let top = bounds.top;
    let detail_width = width * 0.41;
    let inline = width >= 520.0 && !detail.is_empty() && text_width(&detail, 12.0) <= detail_width;
    let name_width = if inline { width - detail_width - INSET } else { width };
    let height = text_height(&name, 14.0, name_width).max(24.0);
    scene.text(Rect::from_xywh(left, top, name_width, height), name, 1);
    let mut bottom = top + height;
    if inline {
        scene.text(
            Rect::from_xywh(left + name_width + INSET, top, detail_width, height),
            detail, Tokens::SECONDARY_VALUE,
        );
    } else if !detail.is_empty() {
        let height = text_height(&detail, 12.0, width).max(20.0);
        scene.text(Rect::from_xywh(left, bottom, width, height), detail, 6);
        bottom += height;
    }
    bottom
}

fn matched_rule(rule: usize) -> String {
    crate::i18n::format("organizer-matched-rule", &[("rule", rule.to_string())])
}

fn preview_row(entry: &hybrid::PreviewEntry) -> (String, String) {
    match entry {
        hybrid::PreviewEntry::Move(entry) => (
            entry.item_name.clone(),
            crate::i18n::format(
                "classification-preview-destination",
                &[
                    ("target", entry.target_name.clone()),
                    ("rule", entry.rule.to_string()),
                ],
            ),
        ),
        hybrid::PreviewEntry::Skip {
            item_name, reason, ..
        } => (item_name.clone(), skip_reason(*reason).into()),
    }
}

fn change_row(change: &hybrid::PreviewChange) -> (&'static str, String, String) {
    match change {
        hybrid::PreviewChange::Added(entry) => {
            let (name, detail) = preview_row(entry);
            ("classification-diff-added", name, detail)
        }
        hybrid::PreviewChange::Removed(entry) => {
            let (name, detail) = preview_row(entry);
            ("classification-diff-removed", name, detail)
        }
        hybrid::PreviewChange::Changed { before, after } => {
            let (old_name, old_detail) = preview_row(before);
            let (name, detail) = preview_row(after);
            (
                "classification-diff-changed",
                transition(&old_name, &name),
                transition(&old_detail, &detail),
            )
        }
    }
}

fn transition(before: &str, after: &str) -> String {
    if before == after {
        after.to_owned()
    } else {
        crate::i18n::format(
            "classification-diff-transition",
            &[("before", before.to_owned()), ("after", after.to_owned())],
        )
    }
}

fn skip_reason(reason: hybrid::SkipReason) -> &'static str {
    t(match reason {
        hybrid::SkipReason::DownloadOrLock => "classification-skip-download",
        hybrid::SkipReason::TemporaryFile => "classification-skip-temporary",
        hybrid::SkipReason::UnknownTime => "classification-skip-unknown-time",
        hybrid::SkipReason::VirtualIcon => "classification-skip-virtual",
        hybrid::SkipReason::MissingTarget => "classification-skip-target",
        hybrid::SkipReason::NoMatch => "classification-skip-no-match",
    })
}

fn centered_label(cell: Rect, label: &str, center: f32) -> Rect {
    let width = text_width(label, HEADING_SIZE).ceil().min(cell.right - cell.left);
    let left = (center - width / 2.0).clamp(cell.left, cell.right - width);
    Rect::from_xywh(left, cell.top, width, cell.bottom - cell.top)
}

fn summary_target(mut cell: Rect) -> Rect {
    cell.left += luciddesk_window::controls::InlineControl::DISCLOSURE_LABEL_INSET;
    cell
}

fn switch_bounds(cell: Rect) -> Rect {
    Rect::from_xywh(
        cell.right - SWITCH_WIDTH,
        (cell.top + cell.bottom - SWITCH_HEIGHT) / 2.0,
        SWITCH_WIDTH,
        SWITCH_HEIGHT,
    )
}

fn action_buttons(cell: Rect) -> [Rect; 2] {
    [
        Rect::from_xywh(cell.left, cell.top, CONTROL, CONTROL),
        Rect::from_xywh(cell.right - CONTROL, cell.top, CONTROL, CONTROL),
    ]
}

fn saved_row(
    scene: &mut Scene,
    [_, target, status, actions]: [Rect; 4],
    panels: &[Panel],
    rule: &ClassificationRule,
    index: usize,
    (expanded, expansion): (bool, f32),
) -> f32 {
    let destination = panels
        .iter()
        .find(|panel| panel.id().get() == rule.target_panel && panel.supports_tabs());
    let label = destination.map_or(t("classification-missing"), Panel::title);
    scene.control(
        ControlKind::RuleDisclosure { missing_target: destination.is_none() },
        target,
        label,
        Command::ToggleDetails(index).action(),
        expanded,
    );
    let target = summary_target(target);
    scene.toggle(
        switch_bounds(status),
        Command::Toggle(index).action(),
        rule.enabled,
        true,
    );
    let [edit, delete] = action_buttons(actions);
    scene.icon_button(
        edit,
        "\u{e70f}",
        t("classification-edit"),
        Command::Edit(index).action(),
        false,
        true,
    );
    scene.icon_button(
        delete,
        luciddesk_window::control_style::DELETE,
        t("classification-delete"),
        Command::Delete(index).action(),
        false,
        true,
    );
    if expansion == 0.0 {
        return actions.bottom;
    }
    let first_detail = scene.text.len();
    let full_bottom = draw_details(scene, target.left, actions.right, actions.bottom + INSET, rule);
    let bottom = actions.bottom + (full_bottom - actions.bottom) * expansion;
    if expansion < 1.0 {
        let clip = Rect::from_xywh(target.left, actions.bottom, actions.right - target.left, bottom - actions.bottom);
        scene.disclosure_clips.push(DisclosureClip {
            text: first_detail..scene.text.len(),
            separators: 0..0,
            bounds: clip,
        });
    }
    if bottom >= actions.bottom + GAP + 1.0 {
        scene.separators.push(Rect::from_xywh(
            target.left,
            actions.bottom + GAP,
            actions.right - target.left,
            1.0,
        ));
    }
    bottom
}

fn rule_conditions(rule: &ClassificationRule) -> impl Iterator<Item = (&'static str, Cow<'_, str>)> {
    CONDITION_LABELS
        .into_iter()
        .zip([
            rule.item_type.map(|value| Cow::Borrowed(item_type_label(Some(value)))),
            (!rule.extensions.is_empty()).then(|| Cow::Owned(rule.extensions.join(", "))),
            rule.name_contains.as_deref().map(Cow::Borrowed),
            rule.name_regex.as_deref().map(Cow::Borrowed),
        ])
        .filter_map(|(label, value)| value.map(|value| (t(label), value)))
}

fn draw_details(
    scene: &mut Scene,
    left: f32,
    right: f32,
    mut top: f32,
    rule: &ClassificationRule,
) -> f32 {
    let width = (right - left).max(1.0);
    let label_width = CONDITION_LABELS
        .iter()
        .map(|key| text_width(t(key), 12.0))
        .fold(0.0, f32::max)
        .ceil()
        .min(width * 0.35);
    let value_left = left + label_width + INSET;
    for (label, value) in rule_conditions(rule) {
        let value = value.as_ref();
        let height = text_height(value, 14.0, (right - value_left).max(1.0))
            .max(text_height(label, 12.0, label_width))
            .max(24.0);
        scene.text(Rect::from_xywh(left, top, label_width, height), label, 6);
        scene.text(
            Rect::from_xywh(value_left, top, (right - value_left).max(1.0), height),
            value,
            1,
        );
        top += height + GAP;
    }
    top
}

pub(super) fn draw_editor(
    scene: &mut Scene,
    bounds: Rect,
    panels: &[Panel],
    draft: &Draft,
    error: Option<&str>,
) {
    let left = bounds.left + INSET;
    let right = bounds.right - INSET;
    let header_top = bounds.top + INSET;
    let form_width = right - left;
    let heading = t(if draft.index.is_some() {
        "classification-edit"
    } else {
        "classification-add"
    });
    let enabled = t("classification-enabled");
    let switch_left = right - SWITCH_WIDTH;
    let label_width = text_width(enabled, 12.0).ceil().min(form_width / 3.0);
    let label_left = switch_left - GAP - label_width;
    let title_width = (label_left - left - INSET).max(1.0);
    let header_height = text_height(heading, 16.0, title_width)
        .max(text_height(enabled, 12.0, label_width))
        .max(CONTROL);
    scene.text(
        Rect::from_xywh(left, header_top, title_width, header_height),
        heading,
        Tokens::SECTION_TITLE,
    );
    scene.text(
        Rect::from_xywh(label_left, header_top, label_width, header_height),
        enabled,
        6,
    );
    scene.toggle(
        switch_bounds(Rect::from_xywh(
            switch_left,
            header_top,
            SWITCH_WIDTH,
            header_height,
        )),
        Command::DraftEnabled.action(),
        draft.rule.enabled,
        true,
    );
    let separator = header_top + header_height + GAP;
    scene
        .separators
        .push(Rect::from_xywh(left, separator, form_width, 1.0));

    let top = separator + INSET;
    let target_label = t("classification-target");
    let type_label = t("classification-item-type");
    let side_by_side = form_width >= 360.0;
    let field_width = if side_by_side {
        (form_width - INSET) / 2.0
    } else {
        form_width
    };
    let target_height = text_height(target_label, 12.0, field_width).max(20.0);
    let type_height = text_height(type_label, 12.0, field_width).max(20.0);
    let label_height = if side_by_side {
        target_height.max(type_height)
    } else {
        target_height
    };
    let target = labeled_field(
        scene,
        Rect::from_xywh(left, top, field_width, label_height),
        target_label,
    );
    let type_bounds = if side_by_side {
        Rect::from_xywh(target.right + INSET, top, field_width, label_height)
    } else {
        Rect::from_xywh(left, target.bottom + INSET, field_width, type_height)
    };
    let item_type = labeled_field(scene, type_bounds, type_label);
    let extension_label = t(Field::Extensions.label_key(draft.regex));
    let name_label = t(Field::Keyword.label_key(draft.regex));
    let extensions = labeled_field(
        scene,
        Rect::from_xywh(
            left,
            item_type.bottom + GAP,
            form_width,
            text_height(extension_label, 12.0, form_width).max(20.0),
        ),
        extension_label,
    );
    let regex_label = t("classification-regex");
    let regex_width = text_width(regex_label, 14.0).ceil() + Tokens::CHECKBOX_INSET;
    let regex_inline = text_width(name_label, 12.0) + INSET + regex_width <= form_width;
    let name_top = extensions.bottom + GAP;
    let (name_width, name_height) = if regex_inline {
        (form_width - regex_width - INSET, CONTROL)
    } else {
        (
            form_width,
            text_height(name_label, 12.0, form_width).max(20.0),
        )
    };
    scene.text(
        Rect::from_xywh(left, name_top, name_width, name_height),
        name_label,
        6,
    );
    let keyword = Rect::from_xywh(left, name_top + name_height + GAP, form_width, CONTROL);
    let regex_bounds = if regex_inline {
        Rect::from_xywh(right - regex_width, name_top, regex_width, CONTROL)
    } else {
        Rect::from_xywh(left, keyword.bottom + GAP, form_width, CONTROL)
    };
    let current = draft.rule.target_panel;
    let mut label = t(if current == 0 {
        "classification-choose-target"
    } else {
        "classification-missing"
    });
    let options: Vec<_> = targets(panels)
        .map(|panel| {
            let selected = panel.id().get() == current;
            if selected {
                label = panel.title();
            }
            (
                panel.title().to_owned(),
                Command::Target(panel.id()).action(),
                selected,
            )
        })
        .collect();
    let has_targets = !options.is_empty();
    scene.control(
        ControlKind::Combo,
        target,
        label,
        Action::Dropdown(options.into()),
        false,
    );
    scene.controls.last_mut().unwrap().enabled = has_targets;
    let type_options = ITEM_TYPES.map(|value| {
        (
            item_type_label(value).into(),
            Command::DraftItemType(value).action(),
            value == draft.rule.item_type,
        )
    });
    scene.control(
        ControlKind::Combo,
        item_type,
        item_type_label(draft.rule.item_type),
        Action::Dropdown(type_options.into()),
        false,
    );
    scene.button(
        extensions,
        "",
        Command::Input(Field::Extensions).action(),
        false,
    );
    scene.button(keyword, "", Command::Input(Field::Keyword).action(), false);

    scene.checkbox(
        regex_bounds,
        regex_label,
        Command::DraftRegex.action(),
        draft.regex,
        true,
    );
    let help = error.unwrap_or(if has_targets {
        t(if draft.regex {
            "classification-regex-hint"
        } else {
            "classification-extensions-hint"
        })
    } else {
        t("classification-no-target")
    });
    let help_height = text_height(help, if error.is_some() { 14.0 } else { 12.0 }, form_width);
    let help_top = regex_bounds.bottom.max(keyword.bottom) + GAP;
    scene.text(
        Rect::from_xywh(left, help_top, form_width, help_height),
        help,
        if error.is_some() {
            Tokens::STATUS_ERROR
        } else {
            6
        },
    );
    scene.cards.push(Rect {
        bottom: help_top + help_height + INSET,
        ..bounds
    });
}

fn labeled_field(scene: &mut Scene, label_bounds: Rect, label: &str) -> Rect {
    scene.text(label_bounds, label, 6);
    Rect::from_xywh(
        label_bounds.left,
        label_bounds.bottom + GAP,
        label_bounds.right - label_bounds.left,
        CONTROL,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_action_spacing_stays_fixed_with_long_headers() {
        for (width, enabled_label) in [(360.0, "启用"), (620.0, "Enabled for this rule")] {
            let left = 16.0;
            let columns = Columns::new(left, width, enabled_label, true);
            let [grip, target, status, actions] = columns.cells(100.0, CONTROL);
            let toggle = switch_bounds(status);
            let [first, second] = action_buttons(actions);
            let close = |actual: f32, expected: f32| {
                assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
            };

            assert!(grip.right < target.left);
            assert!(target.right <= toggle.left);
            close(first.left - toggle.right, SWITCH_ACTION_GAP);
            close(second.left - first.right, BUTTON_GAP);
            close(second.right, left + width);
            close(first.right - first.left, CONTROL);
            close(
                (toggle.top + toggle.bottom) / 2.0,
                (first.top + first.bottom) / 2.0,
            );
        }
    }
}
