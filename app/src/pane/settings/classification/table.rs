//! Column layout shared by saved rules and the active edit row.
use super::*;
use components::{text_height, text_width};

const CONTROL: f32 = Tokens::CONTROL_HEIGHT;
const GAP: f32 = Tokens::GAP;
const INSET: f32 = Tokens::INSET;
const SWITCH_WIDTH: f32 = 46.0;
const SWITCH_HEIGHT: f32 = 24.0;
const SWITCH_ACTION_GAP: f32 = INSET;
const BUTTON_GAP: f32 = GAP;
const ROW_PADDING: f32 = 12.0;
const ROW_GAP: f32 = GAP;

const COLUMN_GAPS: [f32; 4] = [INSET, INSET, SWITCH_ACTION_GAP, 0.0];

struct Columns {
    left: f32,
    widths: [f32; 4],
}

impl Columns {
    fn new(left: f32, width: f32, enabled_label: &str) -> Self {
        let actions = CONTROL + CONTROL + BUTTON_GAP;
        let status = text_width(enabled_label, 14.0).ceil().max(SWITCH_WIDTH);
        let order = text_width(t("classification-order"), 14.0)
            .max(text_width("256", 14.0))
            .ceil()
            .max(28.0);
        let target = (width - order - actions - status - COLUMN_GAPS.iter().sum::<f32>()).max(1.0);
        Self {
            left,
            widths: [order, target, status, actions],
        }
    }

    fn cells(&self, top: f32, height: f32) -> [Rect; 4] {
        let mut left = self.left;
        std::array::from_fn(|index| {
            let width = self.widths[index];
            let cell = Rect::from_xywh(left, top, width, height);
            left += width + COLUMN_GAPS[index];
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
    let columns = Columns::new(left, width, enabled_label);
    let header_top = bounds.top;
    let [order, target, status, actions] = columns.cells(header_top, CONTROL);
    let target = summary_target(target);
    let header_cells = [order, target, switch_label(status, enabled_label), actions];
    let labels = [
        t("classification-order"),
        t("classification-target"),
        enabled_label,
        t("classification-actions"),
    ];
    let heading = header_cells
        .iter()
        .zip(labels)
        .map(|(cell, label)| text_height(label, 14.0, cell.right - cell.left))
        .fold(CONTROL, f32::max);
    for (mut cell, label) in header_cells.into_iter().zip(labels) {
        cell.bottom = cell.top + heading;
        scene.text(cell, label, 6);
    }
    let mut y = header_top + heading + GAP;
    let draft_index = editor.draft.as_ref().map(|draft| {
        // A removed row leaves its draft visible at the end until cancelled.
        draft.index.unwrap_or(rules.len()).min(rules.len())
    });
    for index in 0..rules.len() + usize::from(draft_index == Some(rules.len())) {
        let row_top = y;
        y += ROW_PADDING;
        if draft_index == Some(index) {
            let draft = editor.draft.as_ref().unwrap();
            let cells = columns.cells(y, CONTROL);
            y = edit_row(scene, cells, panels, draft, index, editor.error.as_deref());
        } else {
            let cells = columns.cells(y, CONTROL);
            y = saved_row(
                scene,
                cells,
                panels,
                &rules[index],
                index,
                rules.len(),
                editor,
            );
        }
        y += ROW_PADDING;
        scene.cards.push(Rect::from_xywh(
            bounds.left,
            row_top,
            bounds.right - bounds.left,
            y - row_top,
        ));
        y += ROW_GAP;
    }
}

pub(super) fn draw_preview<'a>(
    scene: &mut Scene,
    bounds: Rect,
    entries: impl IntoIterator<Item = (&'a str, usize, &'a str)>,
) {
    let left = bounds.left + INSET;
    let width = (bounds.right - bounds.left - 2.0 * INSET).max(1.0);
    let mut top = bounds.top + GAP;
    for (index, (item, rule, target)) in entries.into_iter().enumerate() {
        if index > 0 {
            scene
                .separators
                .push(Rect::from_xywh(left, top, width, 1.0));
        }
        let text = crate::i18n::format(
            "classification-preview-row",
            &[
                ("item", item.into()),
                ("rule", rule.to_string()),
                ("target", target.into()),
            ],
        );
        let height = text_height(&text, 14.0, width).max(24.0);
        scene.text(Rect::from_xywh(left, top + GAP, width, height), text, 1);
        top += height + 2.0 * GAP;
    }
    scene.cards.push(Rect::from_xywh(
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        top + GAP - bounds.top,
    ));
}

fn switch_label(cell: Rect, label: &str) -> Rect {
    let width = text_width(label, 14.0).ceil().min(cell.right - cell.left);
    let switch_center = cell.right - SWITCH_WIDTH / 2.0;
    let left = (switch_center - width / 2.0).clamp(cell.left, cell.right - width);
    Rect::from_xywh(left, cell.top, width, cell.bottom - cell.top)
}

fn summary_target(mut cell: Rect) -> Rect {
    cell.left += 28.0;
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
    [order, target, status, actions]: [Rect; 4],
    panels: &[Panel],
    rule: &ClassificationRule,
    index: usize,
    count: usize,
    editor: &Editor,
) -> f32 {
    let enabled = !editor.editing();
    let expanded = editor.expanded == Some(index);
    let label = panels
        .iter()
        .find(|panel| panel.id().get() == rule.target_panel)
        .map_or(t("classification-missing"), Panel::title);
    scene.text(order, (index + 1).to_string(), 6);
    scene.control(
        ControlKind::Disclosure,
        target,
        label,
        Command::ToggleDetails(index).action(),
        expanded,
    );
    scene.controls.last_mut().unwrap().enabled = enabled;
    let target = summary_target(target);
    scene.toggle(
        switch_bounds(status),
        Command::Toggle(index).action(),
        rule.enabled,
        enabled,
    );
    let mut choices = Vec::new();
    if index > 0 {
        choices.push((
            t("classification-up").into(),
            Command::Move(index, -1).action(),
            false,
        ));
    }
    if index + 1 < count {
        choices.push((
            t("classification-down").into(),
            Command::Move(index, 1).action(),
            false,
        ));
    }
    choices.push((
        t("classification-delete").into(),
        Command::Delete(index).action(),
        false,
    ));

    let [edit, menu] = action_buttons(actions);
    scene.icon_button(
        edit,
        "\u{e70f}",
        t("classification-edit"),
        Command::Edit(index).action(),
        false,
        enabled,
    );
    scene.icon_button(
        menu,
        "\u{e712}",
        t("ui-search-more-actions"),
        Action::Dropdown(choices.into()),
        false,
        enabled,
    );
    if expanded {
        draw_details(
            scene,
            target.left,
            actions.right,
            actions.bottom + ROW_PADDING,
            rule,
        )
    } else {
        actions.bottom
    }
}

fn draw_details(
    scene: &mut Scene,
    left: f32,
    right: f32,
    mut top: f32,
    rule: &ClassificationRule,
) -> f32 {
    let extensions = (!rule.extensions.is_empty()).then(|| rule.extensions.join(", "));
    let labels = [
        "classification-extensions",
        "classification-keyword",
        "classification-pattern",
    ];
    let width = (right - left).max(1.0);
    let label_width = labels
        .iter()
        .map(|key| text_width(t(key), 12.0))
        .fold(0.0, f32::max)
        .ceil()
        .min(width * 0.35);
    let value_left = left + label_width + INSET;
    for (label, value) in labels.into_iter().zip([
        extensions.as_deref(),
        rule.name_contains.as_deref(),
        rule.name_regex.as_deref(),
    ]) {
        let Some(value) = value else {
            continue;
        };
        let height = text_height(value, 14.0, (right - value_left).max(1.0))
            .max(text_height(t(label), 12.0, label_width))
            .max(24.0);
        scene.text(Rect::from_xywh(left, top, label_width, height), t(label), 6);
        scene.text(
            Rect::from_xywh(value_left, top, (right - value_left).max(1.0), height),
            value,
            1,
        );
        top += height + GAP;
    }
    top
}

fn edit_row(
    scene: &mut Scene,
    cells: [Rect; 4],
    panels: &[Panel],
    draft: &Draft,
    index: usize,
    error: Option<&str>,
) -> f32 {
    let [order, target, status, actions] = cells;
    scene.text(order, (index + 1).to_string(), 6);
    let left = target.left;
    let right = actions.right;
    let form_width = right - left;
    let label_height =
        text_height(t("classification-target"), 12.0, status.left - left - INSET).max(20.0);
    scene.text(
        Rect::from_xywh(left, target.top, status.left - left - INSET, label_height),
        t("classification-target"),
        6,
    );
    scene.text(
        switch_label(status, t("classification-enabled")),
        t("classification-enabled"),
        6,
    );
    let top = target.top + label_height + GAP;
    let target = Rect::from_xywh(left, top, status.left - left - INSET, CONTROL);
    let status = Rect::from_xywh(status.left, top, status.right - status.left, CONTROL);
    let mut y = top + CONTROL + INSET;
    let mut field = |key: &'static str| {
        let height = text_height(t(key), 12.0, form_width).max(20.0);
        scene.text(Rect::from_xywh(left, y, form_width, height), t(key), 6);
        y += height + GAP;
        let bounds = Rect::from_xywh(left, y, form_width, CONTROL);
        y += CONTROL + INSET;
        bounds
    };
    let extensions = field("classification-extensions");
    let keyword = field("classification-keyword");
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
    scene.button(
        extensions,
        "",
        Command::Input(Field::Extensions).action(),
        false,
    );
    scene.button(keyword, "", Command::Input(Field::Keyword).action(), false);

    let regex_top = keyword.bottom + GAP;
    scene.checkbox(
        Rect::from_xywh(
            keyword.left,
            regex_top,
            actions.right - keyword.left,
            CONTROL,
        ),
        t("classification-regex"),
        Command::DraftRegex.action(),
        draft.regex,
        true,
    );
    scene.toggle(
        switch_bounds(status),
        Command::DraftEnabled.action(),
        draft.rule.enabled,
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
    let help_top = regex_top + CONTROL + GAP;
    scene.text(
        Rect::from_xywh(left, help_top, form_width, help_height),
        help,
        if error.is_some() {
            Tokens::STATUS_ERROR
        } else {
            6
        },
    );
    let footer_top = help_top + help_height + INSET;
    let button_width = ["classification-save", "classification-cancel"]
        .into_iter()
        .map(|key| text_width(t(key), 14.0) + 2.0 * INSET)
        .fold(72.0, f32::max);
    scene.button(
        Rect::from_xywh(
            right - 2.0 * button_width - GAP,
            footer_top,
            button_width,
            CONTROL,
        ),
        t("classification-cancel"),
        Command::Cancel.action(),
        false,
    );
    scene.button(
        Rect::from_xywh(right - button_width, footer_top, button_width, CONTROL),
        t("classification-save"),
        Command::Save.action(),
        true,
    );
    footer_top + CONTROL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_action_spacing_stays_fixed_with_long_headers() {
        for (width, enabled_label) in [(360.0, "启用"), (620.0, "Enabled for this rule")] {
            let left = 16.0;
            let columns = Columns::new(left, width, enabled_label);
            let [order, target, status, actions] = columns.cells(100.0, CONTROL);
            let toggle = switch_bounds(status);
            let [first, second] = action_buttons(actions);
            let close = |actual: f32, expected: f32| {
                assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
            };

            assert!(order.right < target.left);
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
