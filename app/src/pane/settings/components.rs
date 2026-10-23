//! Reusable settings layout primitives; pages supply values and actions, never coordinates.
use super::*;

pub(super) struct Tokens;
impl Tokens {
    pub fn content_x() -> f32 {
        super::layout::navigation()
            .iter()
            .map(|entry| (text_width(&entry.label, Style::NAV_TEXT) + 108.0)
                .max(entry.section.map_or(0.0, |title| text_width(title, Style::NAV_HEADING) + 52.0)))
            .fold(248.0_f32, f32::max)
            .min(344.0)
    }
    pub const MARGIN: f32 = 24.0;
    pub const GAP: f32 = 8.0;
    pub const INSET: f32 = 16.0;
    pub const CARD_RADIUS: f32 = 8.0;
    pub const CONTROL_HEIGHT: f32 = Style::CONTROL_HEIGHT;
    pub const ROW_HEIGHT: f32 = 40.0;
    pub const MAX_WIDTH: f32 = 1000.0;
    pub const VALUE_TEXT: usize = 7;
    pub const STATUS_GOOD: usize = 10;
    pub const STATUS_ERROR: usize = 11;
    pub const TITLE_BADGE: usize = 12;
}

pub(super) struct Palette {
    pub background: u32,
    pub ink: u32,
    pub muted: u32,
    pub border: u32,
    pub accent: u32,
    pub slider_track: u32,
    pub slider_border: u32,
    pub scroll_thumb: u32,
}
impl Palette {
    pub fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                background: 0x202020,
                ink: 0xf5f5f5,
                muted: 0xadadad,
                border: 0x424242,
                accent: 0x76b9ed,
                slider_track: 0x424242,
                slider_border: 0x424242,
                scroll_thumb: 0xadadad,
            }
        } else {
            Self {
                background: 0xe9edf2,
                ink: 0x202833,
                muted: 0x566170,
                border: 0xb7c0cc,
                accent: 0x0969b8,
                slider_track: 0x8995a4,
                slider_border: 0xa3aebb,
                scroll_thumb: 0x758293,
            }
        }
    }
}

pub(super) struct SettingsForm<'a> {
    scene: &'a mut Scene,
    title_badge: Option<String>,
    x: f32,
    width: f32,
    y: f32,
}

// Sidebar interaction surfaces have their own contrast requirements; pane-tab
// fills are too subtle here, especially on dark acrylic and light solid colors.
pub(super) fn navigation_colors(material: Backdrop, dark: bool) -> [ColorF; 3] {
    let (selected, hovered) = match (material.base(), dark) {
        (Backdrop::Acrylic, true) => (0.14, 0.07),
        (Backdrop::Mica, true) => (0.10, 0.05),
        (Backdrop::MicaAlt, true) => (0.12, 0.06),
        (_, true) => (0.12, 0.06),
        (Backdrop::Acrylic, false) => (0.14, 0.055),
        (Backdrop::Mica, false) => (0.10, 0.035),
        (Backdrop::MicaAlt, false) => (0.12, 0.045),
        (_, false) => (0.12, 0.045),
    };
    // Keep all states legible at either end of the material-strength slider.
    let strength = f32::from(material.strength().unwrap_or(50)) / 100.0;
    let factor = 0.9 + strength * 0.2;
    let fill = |rgb: u32, alpha: f32| ColorF {
        r: ((rgb >> 16) & 255) as f32 / 255.0,
        g: ((rgb >> 8) & 255) as f32 / 255.0,
        b: (rgb & 255) as f32 / 255.0,
        a: alpha * factor,
    };
    let selection = Palette::for_theme(dark).accent;
    [
        fill(selection, selected),
        fill(if dark { 0xffffff } else { 0x000000 }, hovered),
        fill(selection, selected * 1.25),
    ]
}
impl<'a> SettingsForm<'a> {
    pub fn continuation(scene: &'a mut Scene, width: f32, y: f32) -> Self {
        Self {
            scene,
            title_badge: None,
            x: Tokens::content_x(),
            width: (width - Tokens::content_x() - Tokens::MARGIN).min(Tokens::MAX_WIDTH),
            y,
        }
    }
    pub fn new(scene: &'a mut Scene, width: f32, description: &str) -> Self {
        let width = (width - Tokens::content_x() - Tokens::MARGIN).min(Tokens::MAX_WIDTH);
        let description_height = text_height(description, 12.0, width).max(36.0);
        scene.text(
            Rect::from_xywh(Tokens::content_x(), 76.0, width, description_height),
            description,
            6,
        );
        Self {
            scene,
            title_badge: None,
            x: Tokens::content_x(),
            width,
            y: 84.0 + description_height,
        }
    }
    /// Annotate the next setting, reserving space for the badge in its title row.
    pub fn title_badge(&mut self, label: &str) -> &mut Self {
        self.title_badge = Some(label.to_owned());
        self
    }
    /// Group related setting rows in one card with inset separators.
    pub fn group(&mut self, content: impl FnOnce(&mut Self)) {
        let start = self.scene.cards.len();
        content(self);
        if self.scene.cards.len() <= start + 1 { return; }
        let bottom = self.scene.cards.last().unwrap().bottom;
        for card in &self.scene.cards[start + 1..] {
            self.scene.separators.push(Rect::from_xywh(
                card.left + Tokens::INSET, card.top - Tokens::GAP / 2.0,
                card.right - card.left - 2.0 * Tokens::INSET, 1.0));
        }
        self.scene.cards.truncate(start + 1);
        self.scene.cards[start].bottom = bottom;
    }
    /// A secondary link belongs to the preceding setting, without another card.
    pub fn footer_link(&mut self, label: &str, action: Action) {
        let card = self.scene.cards.last_mut().unwrap();
        let height = text_height(label, 12.0, self.width - 64.0).max(32.0);
        let top = card.bottom - 8.0;
        card.bottom = top + height + Tokens::INSET;
        self.y = card.bottom + Tokens::GAP;
        self.scene.control(ControlKind::FooterLink,
            Rect::from_xywh(self.x + 8.0, top, self.width - 16.0, height),
            label, action, false);
    }
    pub fn section(&mut self, title: &str) {
        if self.y > 120.0 {
            self.y += 16.0;
        }
        self.scene
            .text(Rect::from_xywh(self.x, self.y, self.width, 28.0), title, 1);
        self.y += 36.0;
    }
    /// Reserve a common trailing column. Long descriptions use the full card width.
    fn card(&mut self, title: &str, description: &str, action_width: f32) -> Rect {
        self.card_layout(title, description, action_width, false)
    }
    fn card_layout(
        &mut self,
        title: &str,
        description: &str,
        action_width: f32,
        force_stacked: bool,
    ) -> Rect {
        let column = action_width.max(232.0);
        let stacked =
            force_stacked || self.width < column + 260.0 || description.chars().count() > 60;
        let text_width = if stacked || action_width == 0.0 {
            self.width - 2.0 * Tokens::INSET
        } else {
            self.width - column - 3.0 * Tokens::INSET
        };
        let badge = self.title_badge.take();
        let title_width = super::components::text_width(title, 14.0);
        let badge_width = badge.as_ref().map_or(0.0, |label|
            (super::components::text_width(label, 11.0) + 36.0).min(text_width));
        let badge_inline = title_width + Tokens::GAP + badge_width <= text_width;
        let title_line_height = text_height(title, 14.0, text_width).max(24.0);
        let badge_height = badge.as_ref().map_or(0.0, |label|
            text_height(label, 11.0, (badge_width - 36.0).max(1.0)).max(16.0) + 4.0);
        let title_height = title_line_height + if badge.is_some() && !badge_inline {
            badge_height + 4.0
        } else { 0.0 };
        let description_height = if description.is_empty() {
            0.0
        } else {
            text_height(description, 12.0, text_width).max(20.0)
        };
        let text_height = title_height
            + if description.is_empty() {
                0.0
            } else {
                4.0 + description_height
            };
        let height = (text_height
            + 2.0 * Tokens::INSET
            + if stacked && action_width > 0.0 {
                12.0 + Tokens::CONTROL_HEIGHT
            } else {
                0.0
            })
        .max(72.0);
        self.scene
            .cards
            .push(Rect::from_xywh(self.x, self.y, self.width, height));
        self.scene.text(
            Rect::from_xywh(
                self.x + Tokens::INSET,
                self.y
                    + if description.is_empty() && !stacked {
                        (height - title_height) / 2.0
                    } else {
                        Tokens::INSET
                    },
                text_width,
                title_line_height,
            ),
            title,
            1,
        );
        if let Some(label) = badge {
            let top = self.y + if description.is_empty() && !stacked {
                (height - title_height) / 2.0
            } else { Tokens::INSET };
            self.scene.text(Rect::from_xywh(
                self.x + Tokens::INSET + if badge_inline { title_width + Tokens::GAP } else { 0.0 },
                top + if badge_inline { (title_line_height - badge_height) / 2.0 } else { title_line_height + 4.0 },
                badge_width, badge_height,
            ), label, Tokens::TITLE_BADGE);
        }
        if !description.is_empty() {
            self.scene.text(
                Rect::from_xywh(
                    self.x + Tokens::INSET,
                    self.y + Tokens::INSET + title_height + 4.0,
                    text_width,
                    description_height,
                ),
                description,
                6,
            );
        }
        let action = Rect::from_xywh(
            self.x + self.width - Tokens::INSET - action_width,
            self.y
                + if stacked {
                    height - Tokens::INSET - Tokens::CONTROL_HEIGHT
                } else {
                    (height - Tokens::CONTROL_HEIGHT) / 2.0
                },
            action_width,
            Tokens::CONTROL_HEIGHT,
        );
        self.y += height + Tokens::GAP;
        action
    }
    pub fn info(&mut self, title: &str, description: &str) {
        self.card(title, description, 0.0);
    }
    pub fn toggle_enabled(
        &mut self,
        title: &str,
        description: &str,
        selected: bool,
        enabled: bool,
        action: Action,
    ) {
        self.toggle(title, description, selected, action);
        self.scene.controls.last_mut().unwrap().enabled = enabled;
    }
    pub fn actions(&mut self, title: &str, description: &str, actions: Vec<(&str, Action)>) {
        self.action_group(title, description, actions, false);
    }
    pub fn desktop_connection(&mut self, connected: bool, copied: bool) {
        let title = crate::i18n::text("ui-desktop-connection");
        let actions = vec![
            (crate::i18n::text("ui-reconnect"), Action::Change(Event::RetryDesktop)),
            (crate::i18n::text(if copied { "ui-copied" } else { "ui-copy-diagnostics" }), Action::CopyDiagnostics),
        ];
        let inner = self.width - 2.0 * Tokens::INSET;
        let left = self.x + Tokens::INSET;
        let top = self.y;
        let summary = crate::i18n::text(if connected { "desktop-status-connected" } else { "desktop-status-unavailable" });
        let status_style = if connected { Tokens::STATUS_GOOD } else { Tokens::STATUS_ERROR };
        let title_width = text_width(title, 14.0);
        let cell = actions.iter().map(|(label, action)| {
            let label = if matches!(action, Action::CopyDiagnostics) {
                crate::i18n::text("ui-copy-diagnostics")
            } else { label };
            text_width(label, 14.0) + 32.0
        }).fold(112.0_f32, f32::max).min(inner);
        let action_width = actions.len() as f32 * (cell + Tokens::GAP) - Tokens::GAP;
        if title_width + 16.0 + text_width(summary, 14.0) + 24.0 + action_width <= inner {
            let height = 72.0;
            let center = top + height / 2.0;
            self.scene.cards.push(Rect::from_xywh(self.x, top, self.width, height));
            self.scene.text(Rect::from_xywh(left, center - 12.0, title_width, 24.0), title, 1);
            self.scene.text(Rect::from_xywh(left + title_width + 16.0, center - 12.0,
                inner - title_width - 16.0 - action_width - 24.0, 24.0), summary, status_style);
            for (index, (label, action)) in actions.into_iter().enumerate() {
                self.scene.button(Rect::from_xywh(left + inner - action_width + index as f32 * (cell + Tokens::GAP),
                    center - Tokens::CONTROL_HEIGHT / 2.0, cell, Tokens::CONTROL_HEIGHT), label, action, false);
            }
            self.y = top + height + Tokens::GAP;
            return;
        }
        let inline = title_width + 16.0 + text_width(summary, 14.0) <= inner;
        let title_height = text_height(title, 14.0, inner).max(24.0);
        self.scene.text(Rect::from_xywh(left, top + Tokens::INSET,
            if inline { title_width } else { inner }, title_height), title, 1);
        let status_width = if inline { inner - title_width - 16.0 } else { inner };
        let status_height = text_height(summary, 14.0, status_width).max(24.0);
        let status_top = top + Tokens::INSET + if inline { 0.0 } else { title_height + 4.0 };
        self.scene.text(Rect::from_xywh(
            if inline { left + title_width + 16.0 } else { left },
            status_top, status_width, status_height,
        ), summary, status_style);
        let mut y = (top + Tokens::INSET + title_height).max(status_top + status_height);
        let columns = ((inner + Tokens::GAP) / (cell + Tokens::GAP)).floor().max(1.0) as usize;
        let columns = columns.min(actions.len());
        let rows = actions.len().div_ceil(columns);
        y += 12.0;
        for (index, (label, action)) in actions.into_iter().enumerate() {
            let column = index % columns;
            let row = index / columns;
            self.scene.button(Rect::from_xywh(
                self.x + self.width - Tokens::INSET - columns as f32 * (cell + Tokens::GAP)
                    + Tokens::GAP + column as f32 * (cell + Tokens::GAP),
                y + row as f32 * (Tokens::CONTROL_HEIGHT + Tokens::GAP), cell, Tokens::CONTROL_HEIGHT,
            ), label, action, false);
        }
        let bottom = y + rows as f32 * (Tokens::CONTROL_HEIGHT + Tokens::GAP) - Tokens::GAP + Tokens::INSET;
        self.scene.cards.push(Rect::from_xywh(self.x, top, self.width, bottom - top));
        self.y = bottom + Tokens::GAP;
    }
    /// Keep related actions and a compact switch in one card, including when
    /// long button labels require the actions to wrap onto separate rows.
    pub fn actions_with_toggle(
        &mut self,
        title: &str,
        description: &str,
        actions: Vec<(&str, Action)>,
        toggle: (&str, Action, bool),
        highlight: bool,
    ) {
        let first_card = self.scene.cards.len();
        let top = self.y;
        let first_text = self.scene.text.len();
        let first_control = self.scene.controls.len();
        self.actions(title, description, actions);
        if highlight {
            if let Some((_, _, style)) = self.scene.text.get_mut(first_text) {
                *style = Tokens::STATUS_GOOD;
            }
            if let Some(control) = self.scene.controls.get_mut(first_control) {
                control.selected = true;
            }
        }
        let footer_top = self.y - Tokens::GAP;
        let label_width = self.width - 2.0 * Tokens::INSET - 46.0 - 16.0;
        let label_height = text_height(toggle.0, 14.0, label_width).max(24.0);
        let footer_height = label_height + 2.0 * Tokens::INSET;
        self.scene.cards.truncate(first_card);
        self.scene.cards.push(Rect::from_xywh(
            self.x, top, self.width, footer_top + footer_height - top,
        ));
        self.scene.separators.push(Rect::from_xywh(
            self.x + Tokens::INSET, footer_top, self.width - 2.0 * Tokens::INSET, 1.0,
        ));
        self.scene.text(Rect::from_xywh(
            self.x + Tokens::INSET, footer_top + Tokens::INSET, label_width, label_height,
        ), toggle.0, 1);
        self.scene.toggle(Rect::from_xywh(
            self.x + self.width - Tokens::INSET - 46.0,
            footer_top + (footer_height - 24.0) / 2.0, 46.0, 24.0,
        ), toggle.1, toggle.2, true);
        self.y = footer_top + footer_height + Tokens::GAP;
    }
    pub fn path(&mut self, title: &str, path: &str, actions: Vec<(&str, Action)>) {
        self.action_group(title, path, actions, true);
    }
    fn action_group(
        &mut self,
        title: &str,
        description: &str,
        actions: Vec<(&str, Action)>,
        stacked: bool,
    ) {
        let cell = actions
            .iter()
            .map(|(label, _)| text_width(label, 14.0) + 32.0)
            .fold(112.0_f32, f32::max);
        let width = (cell + Tokens::GAP) * actions.len() as f32 - Tokens::GAP;
        if width > self.width - 32.0 {
            self.info(title, description);
            for (label, action) in actions {
                self.option(label, action, false);
            }
            return;
        }
        let r = self.card_layout(title, description, width, stacked);
        let count = actions.len();
        let cell = (width - Tokens::GAP * (count - 1) as f32) / count as f32;
        for (i, (label, action)) in actions.into_iter().enumerate() {
            self.scene.button(
                Rect::from_xywh(
                    r.left + i as f32 * (cell + Tokens::GAP),
                    r.top,
                    cell,
                    Tokens::CONTROL_HEIGHT,
                ),
                label,
                action,
                false,
            );
        }
    }
    pub fn shortcut(
        &mut self,
        title: &str,
        description: &str,
        label: &str,
        action: Action,
        reset: Action,
    ) {
        let r = self.card(title, description, 352.0);
        self.scene.button(
            Rect::from_xywh(r.left, r.top, 232.0, 32.0),
            label,
            action,
            false,
        );
        self.scene.button(
            Rect::from_xywh(r.left + 240.0, r.top, 112.0, 32.0),
            crate::i18n::text("ui-reset"),
            reset,
            false,
        );
    }
    pub fn link(&mut self, title: &str, description: &str, action: Action) {
        let r = self.card(title, description, 112.0);
        self.scene.forward_row(
            r.left,
            r.top,
            r.right - r.left,
            crate::i18n::text("ui-open"),
            action,
        );
        self.scene.controls.last_mut().unwrap().bounds = r;
    }
    pub fn combo(&mut self, title: &str, description: &str, label: &str, action: Action) {
        let r = self.card(title, description, 232.0);
        self.scene
            .control(ControlKind::Combo, r, label, action, false);
    }
    pub fn dropdown(&mut self, title: &str, description: &str, options: Vec<(&str, Action, bool)>) {
        let label = options
            .iter()
            .find(|(_, _, selected)| *selected)
            .or_else(|| options.first())
            .map_or("", |(label, _, _)| *label);
        let enabled = !options.is_empty();
        let action = Action::Dropdown(
            options
                .iter()
                .map(|(label, action, selected)| ((*label).to_owned(), action.clone(), *selected))
                .collect(),
        );
        self.combo(title, description, label, action);
        self.scene.controls.last_mut().unwrap().enabled = enabled;
    }
    pub fn reorder(&mut self, title: &str, values: Vec<(String, Option<Action>, Option<Action>)>) {
        if values.is_empty() {
            return;
        }
        self.section(title);
        let height = values.len() as f32 * 44.0 + 16.0;
        self.scene
            .cards
            .push(Rect::from_xywh(self.x, self.y, self.width, height));
        for (index, (label, up, down)) in values.into_iter().enumerate() {
            let top = self.y + 8.0 + index as f32 * 44.0;
            self.scene.text(
                Rect::from_xywh(self.x + 16.0, top, self.width - 112.0, 36.0),
                format!("{} · {label}", index + 1),
                1,
            );
            for (offset, label, action) in [(84.0, "↑", up), (44.0, "↓", down)] {
                let enabled = action.is_some();
                self.scene.button(
                    Rect::from_xywh(self.x + self.width - offset, top + 2.0, 32.0, 32.0),
                    label,
                    action.unwrap_or(Action::Page(15)),
                    false,
                );
                self.scene.controls.last_mut().unwrap().enabled = enabled;
            }
        }
        self.y += height + Tokens::GAP;
    }
    pub fn back(&mut self, _label: &str, action: Action) {
        self.scene.control(
            ControlKind::BackButton,
            Rect::from_xywh(self.x + self.width - 86.0, 34.0, 86.0, 32.0),
            crate::i18n::text("ui-back"),
            action,
            false,
        );
    }
    pub fn option(&mut self, label: &str, action: Action, selected: bool) {
        self.scene.row(self.x, self.y, self.width, label, action);
        let control = self.scene.controls.last_mut().unwrap();
        control.selected = selected;
        control.bounds.bottom = self.y + Tokens::ROW_HEIGHT;
        self.y += 48.0;
    }
    pub fn font_summary(&mut self, selected: &str) {
        self.button(crate::i18n::text("ui-current-font"), selected,
            crate::i18n::text("ui-reset-font"), Action::Font(crate::i18n::default_font().into()));
        self.scene.controls.last_mut().unwrap().enabled = selected != crate::i18n::default_font();
        let card = self.scene.cards.last_mut().unwrap();
        let top = card.bottom - 8.0;
        let inner = self.width - 2.0 * Tokens::INSET;
        let sample = crate::i18n::text("font-preview");
        let sample_height = text_height(sample, 20.0, inner).max(32.0);
        let fallback = format!("{}: {}", crate::i18n::text("font-fallback"), crate::i18n::default_font());
        let fallback_height = text_height(&fallback, 12.0, inner).max(20.0);
        card.bottom = top + sample_height + 4.0 + fallback_height + Tokens::INSET;
        self.y = card.bottom + Tokens::GAP;
        self.scene.text(Rect::from_xywh(self.x + Tokens::INSET, top, inner, sample_height), sample, 2);
        self.scene.text(Rect::from_xywh(self.x + Tokens::INSET, top + sample_height + 4.0, inner, fallback_height), fallback, 6);
    }
    pub fn font_list_hint(&mut self) {
        let label = crate::i18n::text("font-list-hint");
        let height = text_height(label, 12.0, self.width).max(20.0);
        self.scene.text(Rect::from_xywh(self.x, self.y, self.width, height), label, 6);
        self.y += height + 4.0;
    }
    pub fn font_option(&mut self, name: &str, selected: bool) {
        self.option(name, Action::Font(name.into()), selected);
        self.y -= 8.0;
    }
    pub fn pager(&mut self, label: &str, previous: (Action, bool), next: (Action, bool)) {
        let r = self.card(crate::i18n::text("ui-pages"), label, 232.0);
        for (i, (label, (action, enabled))) in [
            (crate::i18n::text("ui-previous"), previous),
            (crate::i18n::text("ui-next"), next),
        ]
        .into_iter()
        .enumerate()
        {
            self.scene.button(
                Rect::from_xywh(r.left + i as f32 * 120.0, r.top, 112.0, 32.0),
                label,
                action,
                false,
            );
            self.scene.controls.last_mut().unwrap().enabled = enabled;
        }
    }
    pub fn brand(&mut self) {
        self.scene.cards.push(Rect::from_xywh(self.x, self.y, self.width, 104.0));
        self.scene.app_icon = Some(Rect::from_xywh(self.x + 20.0, self.y + 20.0, 64.0, 64.0));
        self.scene.text(
            Rect::from_xywh(self.x + 104.0, self.y + 16.0, self.width - 120.0, 32.0),
            "LucidDesk",
            2,
        );
        self.scene.text(
            Rect::from_xywh(self.x + 104.0, self.y + 54.0, self.width - 120.0, 28.0),
            concat!("v", env!("CARGO_PKG_VERSION"), " (", env!("LUCIDDESK_BUILD_REVISION"), ") · ", env!("CARGO_PKG_AUTHORS"), " · ", env!("CARGO_PKG_LICENSE")),
            6,
        );
        self.y += 112.0;
    }
    pub fn colors(&mut self, color: u32) {
        let r = self.card(
            crate::i18n::text("ui-color-presets"),
            crate::i18n::text("ui-preview-colors-as-you-select-them"),
            352.0,
        );
        for (i, value) in [
            0x181b20, 0xf5f6f8, 0x24364b, 0x32463d, 0x51405c, 0x5b3838, 0x745839, 0x416c78,
        ]
        .into_iter()
        .enumerate()
        {
            self.scene.button(
                Rect::from_xywh(r.left + i as f32 * 45.0, r.top, 37.0, 32.0),
                "",
                Action::ColorPreset(value),
                color == value,
            );
        }
    }
    pub fn toggle(&mut self, title: &str, description: &str, selected: bool, action: Action) {
        let r = self.card(title, description, 46.0);
        self.scene.toggle(
            Rect::from_xywh(r.left, r.top + 4.0, 46.0, 24.0),
            action,
            selected,
            true,
        );
    }
    pub fn toggle_group(&mut self, title: &str, items: Vec<(&str, Action, bool)>) {
        let min_width = items
            .iter()
            .map(|(label, _, _)| text_width(label, 14.0) + 94.0)
            .fold(180.0_f32, f32::max);
        let columns = if self.width >= min_width * 2.0 + Tokens::GAP {
            2
        } else {
            1
        };
        let cell = (self.width - Tokens::GAP * (columns - 1) as f32) / columns as f32;
        self.scene.text(
            Rect::from_xywh(self.x, self.y + 8.0, self.width, 28.0),
            title,
            1,
        );
        self.y += 44.0;
        let rows = items.len().div_ceil(columns);
        for (index, (label, action, selected)) in items.into_iter().enumerate() {
            let x = self.x + (index % columns) as f32 * (cell + Tokens::GAP);
            let y = self.y + (index / columns) as f32 * 56.0;
            self.scene.cards.push(Rect::from_xywh(x, y, cell, 48.0));
            self.scene.text(
                Rect::from_xywh(x + 16.0, y + 12.0, cell - 94.0, 24.0),
                label,
                1,
            );
            self.scene.toggle(
                Rect::from_xywh(x + cell - 62.0, y + 12.0, 46.0, 24.0),
                action,
                selected,
                true,
            );
        }
        self.y += rows as f32 * 56.0;
    }
    pub fn diagnostics(&mut self) {
        use luciddesk_diagnostics::{Level, level};
        let current = level();
        self.section(crate::i18n::text("diagnostics-section"));
        self.dropdown(
            crate::i18n::text("diagnostics-level"),
            crate::i18n::text("diagnostics-description"),
            [
                Level::Error,
                Level::Warn,
                Level::Info,
                Level::Debug,
                Level::Trace,
            ]
            .into_iter()
            .map(|value| (crate::i18n::text(match value {
                Level::Error => "log-level-error", Level::Warn => "log-level-warn",
                Level::Info => "log-level-info", Level::Debug => "log-level-debug",
                Level::Trace => "log-level-trace",
            }), Action::LogLevel(value), value == current))
            .collect(),
        );
    }
    /// Independent checkboxes share a card and wrap on narrow windows.
    pub fn checkboxes(&mut self, title: &str, description: &str, options: Vec<(&str, Action, bool)>) {
        if options.is_empty() { return; }
        self.card(title, description, 0.0);
        let card = *self.scene.cards.last().unwrap();
        let inner = self.width - 2.0 * Tokens::INSET;
        let min_cell = options.iter().map(|(label, _, _)| text_width(label, 14.0) + 40.0)
            .fold(160.0_f32, f32::max);
        let columns = ((inner + Tokens::GAP) / (min_cell + Tokens::GAP)).floor().max(1.0) as usize;
        let columns = columns.min(options.len());
        let cell = (inner - Tokens::GAP * (columns - 1) as f32) / columns as f32;
        let rows = options.len().div_ceil(columns);
        let top = card.bottom - 8.0;
        for (index, (label, action, selected)) in options.into_iter().enumerate() {
            let x = self.x + Tokens::INSET + (index % columns) as f32 * (cell + Tokens::GAP);
            let y = top + (index / columns) as f32 * 40.0;
            self.scene.checkbox(Rect::from_xywh(x, y, cell, 32.0), label, action, selected, true);
        }
        let bottom = top + rows as f32 * 40.0 + 8.0;
        self.scene.cards.last_mut().unwrap().bottom = bottom;
        self.y = bottom + Tokens::GAP;
    }
    pub fn choices(&mut self, title: &str, description: &str, choices: Vec<(&str, Action, bool)>) {
        let cell = choices
            .iter()
            .map(|(label, _, _)| text_width(label, 14.0) + 32.0)
            .fold(80.0_f32, f32::max);
        let needed = (cell + Tokens::GAP) * choices.len() as f32 - Tokens::GAP;
        if needed > self.width - 32.0 {
            self.info(title, description);
            for (label, action, selected) in choices {
                self.option(label, action, selected);
            }
            return;
        }
        let r = self.card(title, description, needed);
        let width =
            (r.right - r.left - Tokens::GAP * (choices.len() - 1) as f32) / choices.len() as f32;
        for (index, (label, action, selected)) in choices.into_iter().enumerate() {
            self.scene.button(
                Rect::from_xywh(
                    r.left + index as f32 * (width + Tokens::GAP),
                    r.top,
                    width,
                    32.0,
                ),
                label,
                action,
                selected,
            );
        }
    }
    pub fn slider(
        &mut self,
        title: &str,
        description: &str,
        value: Slider,
        label: &str,
        action: Action,
    ) {
        let r = self.card(title, description, 232.0);
        // Reserve only the space needed by the value's format. Keep it stable
        // while dragging so changing digit counts cannot move the slider endpoint.
        let value_width = if matches!(action, Action::GridSize(_) | Action::Opacity(_)) {
            40.0
        } else {
            32.0
        };
        let gap = 4.0;
        self.scene.slider(
            Rect::from_xywh(r.left, r.top, r.right - r.left - value_width - gap, 32.0),
            value,
            action,
        );
        self.scene.text(
            Rect::from_xywh(r.right - value_width, r.top, value_width, 32.0),
            label,
            Tokens::VALUE_TEXT,
        );
    }
    pub fn button(&mut self, title: &str, description: &str, label: &str, action: Action) {
        let r = self.card(
            title,
            description,
            (text_width(label, 14.0) + 32.0)
                .max(112.0)
                .min(self.width - 32.0),
        );
        self.scene.button(r, label, action, false);
    }
    pub fn preview(&mut self, name: &str, color: u32, opacity: f32) {
        self.material_preview(name, Backdrop::Solid { color, opacity });
    }
    pub fn color_button(&mut self, title: &str, value: u32) {
        let r = self.card(title, &format!("#{value:06X}"), 176.0);
        self.scene.button(r, crate::i18n::text("ui-edit-colors"), Action::SolidColor, false);
        self.scene.control(ControlKind::Button,
            Rect::from_xywh(r.left - 36.0, r.top + 2.0, 28.0, 28.0),
            "", Action::SolidColor, false);
    }
    pub fn material_preview(&mut self, name: &str, material: Backdrop) {
        self.scene
            .cards
            .push(Rect::from_xywh(self.x, self.y, self.width, 152.0));
        self.scene.previews.push((
            Rect::from_xywh(self.x + 16.0, self.y + 16.0, 200.0, 120.0),
            material,
        ));
        self.scene.text(
            Rect::from_xywh(self.x + 232.0, self.y + 16.0, self.width - 248.0, 28.0),
            name,
            1,
        );
        let description = match material.base() {
            Backdrop::Mica => crate::i18n::text("ui-soft-wallpaper-tones-keep-content-clear"),
            Backdrop::MicaAlt => {
                crate::i18n::text("ui-stronger-wallpaper-tones-with-a-deeper-base")
            }
            Backdrop::Solid { .. } => crate::i18n::text("ui-choose-a-color-and-opacity"),
            _ => crate::i18n::text("ui-frosted-glass-reveals-depth-in-the-background"),
        };
        self.scene.text(
            Rect::from_xywh(self.x + 232.0, self.y + 48.0, self.width - 248.0, 40.0),
            description,
            6,
        );
        self.scene.text(
            Rect::from_xywh(self.x + 232.0, self.y + 92.0, self.width - 248.0, 44.0),
            crate::i18n::text("ui-illustration-actual-appearance-depends-on-your-wallpaper"),
            6,
        );
        self.y += 160.0;
    }
}

impl Scene {
    pub fn reveal_navigation(&self, offset: &mut f32) {
        if let (Some(viewport), Some(control)) = (self.navigation_viewport,
            self.controls.iter().find(|c| matches!(c.kind, ControlKind::Navigation) && c.selected)) {
            if control.bounds.top - *offset < viewport.top { *offset = control.bounds.top - viewport.top; }
            if control.bounds.bottom - *offset > viewport.bottom { *offset = control.bounds.bottom - viewport.bottom; }
        }
    }
    pub fn scroll_navigation(&mut self, offset: &mut f32) {
        let Some(viewport) = self.navigation_viewport else { return; };
        let bottom = self.controls.iter().filter(|c| matches!(c.kind, ControlKind::Navigation))
            .map(|c| c.bounds.bottom).fold(viewport.top, f32::max);
        self.navigation_max = (bottom + 8.0 - viewport.bottom).max(0.0);
        *offset = offset.clamp(0.0, self.navigation_max);
        self.navigation_offset = *offset;
        for c in &mut self.controls {
            if matches!(c.kind, ControlKind::Navigation) { c.bounds.top -= *offset; c.bounds.bottom -= *offset; }
        }
        for index in self.navigation_text.clone() {
            self.text[index].0.top -= *offset;
            self.text[index].0.bottom -= *offset;
        }
    }
    pub fn navigation_thumb(&self) -> Option<Rect> {
        let viewport = self.navigation_viewport?;
        if self.navigation_max <= 0.0 { return None; }
        let track = viewport.bottom - viewport.top - 16.0;
        let height = (track * track / (track + self.navigation_max)).max(28.0);
        let top = viewport.top + 8.0 + (track-height)*self.navigation_offset/self.navigation_max;
        Some(Rect::from_xywh(viewport.right-10.0, top, 4.0, height))
    }
    pub fn fixed_list(&self) -> bool {
        self.controls
            .iter()
            .any(|c| matches!(c.action, Action::FontSearch))
    }
    pub fn list_row(c: &Control) -> bool {
        matches!(c.action, Action::Font(_)) && c.kind.is_row()
    }
    pub fn scroll_to(&mut self, width: f32, height: f32, offset: &mut f32) {
        if self.fixed_list() {
            let top = self
                .controls
                .iter()
                .filter(|c| Self::list_row(c))
                .map(|c| c.bounds.top)
                .fold(height - Tokens::MARGIN, f32::min);
            let bottom = (height - Tokens::MARGIN).max(top + Tokens::ROW_HEIGHT);
            let content_bottom = self
                .controls
                .iter()
                .filter(|c| Self::list_row(c))
                .map(|c| c.bounds.bottom)
                .fold(top, f32::max);
            self.viewport = Some(Rect::from_xywh(
                Tokens::content_x(),
                top,
                (width - Tokens::content_x() - Tokens::MARGIN).min(Tokens::MAX_WIDTH),
                bottom - top,
            ));
            self.scroll_max = (content_bottom - bottom).max(0.0);
            *offset = offset.clamp(0.0, self.scroll_max);
            self.scroll_offset = *offset;
            for c in self.controls.iter_mut().filter(|c| Self::list_row(c)) {
                c.bounds.top -= *offset;
                c.bounds.bottom -= *offset;
                c.bounds.right -= 16.0;
            }
            return;
        }
        let viewport = Rect::from_xywh(
            Tokens::content_x(),
            TITLE_HEIGHT,
            width - Tokens::content_x(),
            height - TITLE_HEIGHT,
        );
        let bottom = self
            .text
            .iter()
            .map(|(r, _, _)| r)
            .chain(self.cards.iter())
            .chain(
                self.controls
                    .iter()
                    .filter(|c| !matches!(c.kind, ControlKind::Caption))
                    .map(|c| &c.bounds),
            )
            .filter(|r| r.left >= Tokens::content_x())
            .map(|r| r.bottom)
            .chain(self.app_icon.iter().map(|r| r.bottom))
            .fold(viewport.top, f32::max);
        self.scroll_max = (bottom + Tokens::MARGIN - viewport.bottom).max(0.0);
        *offset = offset.clamp(0.0, self.scroll_max);
        self.scroll_offset = *offset;
        self.viewport = Some(viewport);
        let translate = |r: &mut Rect| {
            if r.left >= Tokens::content_x() {
                r.top -= *offset;
                r.bottom -= *offset;
            }
        };
        for (r, _, _) in &mut self.text {
            translate(r);
        }
        for r in self.cards.iter_mut().chain(self.separators.iter_mut()) {
            translate(r);
        }
        for (r, _) in &mut self.previews {
            translate(r);
        }
        if let Some(r) = &mut self.app_icon {
            translate(r);
        }
        for c in &mut self.controls {
            if !matches!(c.kind, ControlKind::Caption) {
                translate(&mut c.bounds);
            }
        }
    }
    pub fn scroll_thumb(&self) -> Option<Rect> {
        let viewport = self.viewport?;
        if self.scroll_max <= 0.0 {
            return None;
        }
        let track = viewport.bottom - viewport.top - 16.0;
        let height = (track * track / (track + self.scroll_max)).max(28.0);
        let top = viewport.top + 8.0 + (track - height) * self.scroll_offset / self.scroll_max;
        Some(Rect::from_xywh(viewport.right - 10.0, top, 4.0, height))
    }
    pub fn accepts_pointer(&self, c: &Control, x: f32, y: f32) -> bool {
        if matches!(c.kind, ControlKind::Navigation) {
            return contains(&c.bounds, x, y) && self.navigation_viewport.is_none_or(|v| contains(&v, x, y));
        }
        contains(&c.bounds, x, y)
            && (matches!(c.kind, ControlKind::Caption)
                || c.bounds.left < Tokens::content_x()
                || (self.fixed_list() && !Self::list_row(c))
                || self.viewport.is_none_or(|v| contains(&v, x, y)))
    }
}

pub(super) struct ContentClip<'a, 'b> {
    pass: &'a canvas::DrawPass<'b>,
    active: bool,
}
impl<'a, 'b> ContentClip<'a, 'b> {
    pub fn navigation(pass: &'a canvas::DrawPass<'b>, scene: &Scene) -> Self {
        if let Some(viewport) = &scene.navigation_viewport { pass.push_clip(viewport); }
        Self { pass, active: scene.navigation_viewport.is_some() }
    }
    pub fn new(pass: &'a canvas::DrawPass<'b>, scene: &Scene, content: bool) -> Self {
        let active = content && scene.viewport.is_some();
        if active {
            pass.push_clip(&scene.viewport.unwrap());
        }
        Self { pass, active }
    }
}
impl Drop for ContentClip<'_, '_> {
    fn drop(&mut self) {
        if self.active {
            self.pass.pop_clip();
        }
    }
}

fn text_width(text: &str, size: f32) -> f32 {
    measured_text(text, size, 10000.0).0
}
fn text_height(text: &str, size: f32, width: f32) -> f32 {
    measured_text(text, size, width).1.ceil()
}
type MetricsKey = (String, String, String, u32, u32);
thread_local! { static METRICS: std::cell::RefCell<std::collections::HashMap<MetricsKey, (f32, f32)>> = std::cell::RefCell::new(std::collections::HashMap::new()); }

pub(super) fn release_text_metrics() {
    // Replace rather than clear: closing settings also returns the map's capacity.
    let _ = METRICS.try_with(|cache| *cache.borrow_mut() = std::collections::HashMap::new());
}

fn measured_text(text: &str, size: f32, width: f32) -> (f32, f32) {
    let family = super::super::fonts::family();
    let key = (
        family.clone(),
        crate::i18n::default_font().into(),
        text.to_owned(),
        size.to_bits(),
        width.to_bits(),
    );
    if let Some(metrics) = METRICS.with(|cache| cache.borrow().get(&key).copied()) {
        return metrics;
    }
    let Ok(format) = windows_canvas::TextFormat::new(&family, size) else {
        return (text.chars().count() as f32 * size, size * 2.0);
    };
    let _ = super::super::canvas::apply_fallback(&format);
    let format = format.with_word_wrapping(windows_canvas::WordWrapping::Wrap);
    let Ok(layout) = windows_canvas::TextLayout::new(text, &format, width.max(1.0), 10000.0) else {
        return (text.chars().count() as f32 * size, size * 2.0);
    };
    let metrics = layout.metrics();
    let metrics = (metrics.width, metrics.height);
    METRICS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 2048 {
            cache.clear();
        }
        cache.insert(key, metrics);
    });
    metrics
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn dropping_settings_painter_releases_text_metrics_and_capacity() {
        let _sta = crate::pane::test_support::apartment();
        let painter = super::super::Painter::new().unwrap();
        measured_text("Settings cache lifetime", 14.0, 300.0);
        assert!(METRICS.with(|cache| !cache.borrow().is_empty()));
        drop(painter);
        METRICS.with(|cache| {
            let cache = cache.borrow();
            assert!(cache.is_empty());
            assert_eq!(cache.capacity(), 0);
        });
        assert!(measured_text("Reopened settings", 14.0, 300.0).0 > 0.0);
        release_text_metrics();
    }
}
