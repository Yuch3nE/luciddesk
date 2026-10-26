//! Search input, results, and status drawing.
use super::*;

pub(super) struct Drawing {
    frame_cache: std::cell::RefCell<crate::pane::canvas::FrameCache>,
    brushes: crate::pane::canvas::Brushes<10>,
    file_icons: icons::Icons,
    family: String,
    language: &'static str,
    pub(super) surface: crate::pane::composition::Surface,
    name: windows_canvas::TextFormat,
    path: windows_canvas::TextFormat,
    icon: windows_canvas::TextFormat,
    action_icon: windows_canvas::TextFormat,
    placeholder: windows_canvas::TextFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_repaints_reuse_gpu_bitmap_and_clearing_releases_resources() {
        let _sta=crate::pane::test_support::apartment();
        let window=windows_window::Window::new("Search icon lifetime").size(320,220)
            .style(WS_POPUP).ex_style(WS_EX_NOREDIRECTIONBITMAP)
            .on_message(|_,msg,_,_|(msg==WM_DESTROY).then_some(0)).create().unwrap();
        let hwnd=window.hwnd().cast();
        let mut drawing=Drawing::new(hwnd).unwrap();
        let model=crate::pane::tests::test_model("Search");
        let mut state=Search::new();
        assert_eq!(drawing.file_icons.resources(),(0,0,false));
        for cycle in 1..=5 {
            state.query="folder".into();
            state.entries=vec![Entry {path:"C:/one.txt".into(),folder:false},Entry {path:"C:/two.txt".into(),folder:false}];
            let deadline=Instant::now()+Duration::from_secs(5);
            while drawing.file_icons.uploads<cycle {
                drawing.paint(hwnd,&model,&state).unwrap();
                assert!(Instant::now()<deadline,"icon worker did not finish");
                std::thread::sleep(Duration::from_millis(10));
            }
            for _ in 0..20 {drawing.paint(hwnd,&model,&state).unwrap();}
            assert_eq!(drawing.file_icons.uploads,cycle,"shared type uploaded more than once");
            assert_eq!(drawing.file_icons.resources(),(0,1,true),"CPU pixels should be released after upload");
            state.query.clear();state.entries.clear();
            drawing.clear_icons();
            drawing.paint(hwnd,&model,&state).unwrap();
            assert_eq!(drawing.file_icons.resources(),(0,0,false));
        }
    }

    #[test]
    fn deferred_search_frame_keeps_pending_redraw_until_it_is_painted() {
        let _sta = crate::pane::test_support::apartment();
        let window = windows_window::Window::new("Search frame retry").size(320, 200)
            .style(WS_POPUP).ex_style(WS_EX_NOREDIRECTIONBITMAP).create().unwrap();
        let hwnd = window.hwnd().cast();
        let mut drawing = Drawing::new(hwnd).unwrap();
        let model = crate::pane::tests::test_model("Search");
        let search = Search::new();
        drawing.surface.defer_frame_for_test(Duration::from_secs(5));
        assert!(!drawing.paint(hwnd, &model, &search).unwrap());
        assert!(drawing.surface.try_begin_frame(320, 200).unwrap().is_none());
        drawing.surface.defer_frame_for_test(Duration::ZERO);
        assert!(drawing.paint(hwnd, &model, &search).unwrap());
    }
}
impl Drawing {
    pub(super) fn clear_icons(&mut self) { self.file_icons.clear(); }
    pub(super) fn new(hwnd: HWND) -> Result<Self, String> {
        use windows_canvas::{ParagraphAlignment, TextFormat, WordWrapping};
        let name = TextFormat::new(&crate::pane::fonts::family(), 14.0)
            .map_err(|e| e.to_string())?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        let path = TextFormat::new(&crate::pane::fonts::family(), 11.0)
            .map_err(|e| e.to_string())?
            .with_paragraph_alignment(ParagraphAlignment::Center)
            .with_word_wrapping(WordWrapping::NoWrap);
        crate::pane::canvas::ellipsis_delimiter(&name, '.' as u32).map_err(|e| e.to_string())?;
        crate::pane::canvas::ellipsis_delimiter(&path, '\\' as u32).map_err(|e| e.to_string())?;
        Ok(Self {
            frame_cache: Default::default(),
            brushes: Default::default(),
            file_icons: icons::Icons::new(hwnd),
            family: crate::pane::fonts::family(),
            language: crate::i18n::language(),
            surface: crate::pane::composition::Surface::new_pane(windows::Win32::Foundation::HWND(
                hwnd,
            ))
            .map_err(|e| e.to_string())?,
            name,
            path,
            placeholder: TextFormat::new(&crate::pane::fonts::family(), crate::pane::control_style::TEXT)
                .map_err(|e| e.to_string())?
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            action_icon: TextFormat::new(crate::pane::fonts::icon_family(), 16.0)
                .map_err(|e| e.to_string())?
                .with_alignment(windows_canvas::TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
            icon: TextFormat::new(crate::pane::fonts::icon_family(), 20.0)
                .map_err(|e| e.to_string())?
                .with_alignment(windows_canvas::TextAlignment::Center)
                .with_paragraph_alignment(ParagraphAlignment::Center)
                .with_word_wrapping(WordWrapping::NoWrap),
        })
    }
    pub(super) fn paint(
        &mut self,
        hwnd: HWND,
        model: &GroupModel,
        state: &Search,
    ) -> Result<bool, String> {
        if !self.draw_frame(hwnd, model, state)? { return Ok(false); }
        self.surface.end_frame().map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Rasterize without presenting so pixel tests can inspect the completed
    /// back buffer before DXGI rotates it to the next frame.
    pub(super) fn draw_frame(
        &mut self,
        hwnd: HWND,
        model: &GroupModel,
        state: &Search,
    ) -> Result<bool, String> {
        use crate::pane::native_graphics::canvas_result;
        let family = crate::pane::fonts::family();
        if family != self.family || self.language != crate::i18n::language() {
            use windows_canvas::{ParagraphAlignment, TextFormat, WordWrapping};
            let make = |size| {
                TextFormat::new(&family, size)
                    .map(|format| {
                        format
                            .with_paragraph_alignment(ParagraphAlignment::Center)
                            .with_word_wrapping(WordWrapping::NoWrap)
                    })
                    .map_err(|e| e.to_string())
            };
            let name = make(14.0)?;
            let path = make(11.0)?;
            let placeholder = make(crate::pane::control_style::TEXT)?;
            crate::pane::canvas::ellipsis_delimiter(&name, '.' as u32)
                .map_err(|e| e.to_string())?;
            crate::pane::canvas::ellipsis_delimiter(&path, '\\' as u32)
                .map_err(|e| e.to_string())?;
            self.name = name;
            self.path = path;
            self.placeholder = placeholder;
            self.family = family;
            self.language = crate::i18n::language();
        }

        use windows_canvas::{ColorF, Rect, RoundedRect};
        let s = scale(hwnd);
        let mut r = RECT::default();
        unsafe {
            GetClientRect(hwnd, &raw mut r);
        }
        self.surface
            .theme(windows::Win32::Foundation::HWND(hwnd), model.dark);
        self.surface
            .material(windows::Win32::Foundation::HWND(hwnd), model.backdrop).map_err(|e| e.to_string())?;
        self.surface.pane_border = model.options.border;
        self.surface.pane_corner_radius = crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius);
        let Some(target) = self
            .surface
            .try_begin_frame(r.right.max(1) as u32, r.bottom.max(1) as u32)
            .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        let native = self.surface.native;
        let w = r.right as f32 / s;
        let h = r.bottom as f32 / s;
        let icon_size = (24.0*s).round() as i32;
        let start = state.scroll.min(state.entries.len());
        let end = (start+state.visible_rows).min(state.entries.len());
        self.file_icons.refresh(&state.entries[start..end], icon_size);
        let context = &target;
        crate::pane::canvas::draw_cached(&target, s, &self.frame_cache, |target| {
            target.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
            let contrast = crate::pane::theme::panel_contrast(
                model.backdrop,
                model.dark,
                model.options.text,
                native,
            );
            let ink = contrast.ink();
            let base = contrast.base();
            let [background, text, dim, line, outline, selected, hovered, accent, folder_icon, action_hover] =
                canvas_result(self.brushes.get(context, &target, [
                    ColorF::new(base, base, base, if !native { 1.0 } else if model.options.text_protection { contrast.scrim } else { 0.0 }),
                    ColorF::new(ink, ink, ink, 1.0),
                    ColorF::new(ink, ink, ink, 0.58),
                    crate::pane::theme::panel_divider(model.dark, model.backdrop),
                    crate::pane::theme::panel_border(model.dark, model.backdrop),
                    ColorF::new(ink, ink, ink, if contrast.light_text { 0.14 } else { 0.10 }),
                    ColorF::new(ink, ink, ink, 0.06),
                    if model.dark { ColorF::new(0.48, 0.74, 1.0, 1.0) } else { ColorF::new(0.0, 0.40, 0.76, 1.0) },
                    ColorF::new(0.95, 0.66, 0.18, 1.0),
                    ColorF::new(ink, ink, ink, if contrast.light_text { 0.12 } else { 0.08 }),
                ]))?;
            let shape = RoundedRect {
                rect: Rect::from_xywh(0.5, 0.5, w - 1.0, h - 1.0),
                radius_x: crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius),
                radius_y: crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius),
            };
            target.fill_rounded_rect(&shape, background);
            if model.options.border && !crate::pane::interface_flags::PANEL_SHADOW.get() {
                let stroke = 1.0 / s;
                let inset = stroke * 0.5;
                let radius = (crate::pane::interface_flags::pane_corner_radius(model.options.corner_radius) - inset).max(0.0);
                target.draw_rounded_rect(
                    &RoundedRect {
                        rect: Rect::from_xywh(inset, inset, w - stroke, h - stroke),
                        radius_x: radius,
                        radius_y: radius,
                    },
                    outline,
                    stroke,
                );
            }
            target.clipped_icon(
                if state.busy { "\u{e916}" } else { "\u{e721}" },
                &self.icon,
                &Rect::from_xywh(12.0, 0.0, 28.0, TOP),
                dim,
            );
            if unsafe { GetWindowTextLengthW(edit(hwnd)) } == 0 {
                target.clipped_text(
                    crate::i18n::text("ui-search-local-files"),
                    &self.placeholder,
                    &Rect::from_xywh(44.0, 0.0, (w - 90.0).max(0.0), TOP),
                    dim,
                );
            }
            if unsafe { GetWindowTextLengthW(edit(hwnd)) } > 0 {
                if let Some(r) = action_rect(Action::Clear, w) {
                    let rect = Rect::from_xywh(r.x, r.y, r.width, r.height);
                    if state.hovered_action == Some(Action::Clear) {
                        target.fill_rounded_rect(&RoundedRect { rect, radius_x: 6.0, radius_y: 6.0 }, hovered);
                    }
                    target.clipped_icon("\u{e711}", &self.icon, &rect,
                        if state.hovered_action == Some(Action::Clear) { text } else { dim });
                }
            }
            if !state.query.is_empty() {
                target.fill_rect(&Rect::from_xywh(12.0, TOP, w - 24.0, 1.0), line);
                if state.entries.is_empty() {
                    let title = if state.failed {
                        crate::i18n::text("ui-search-unavailable")
                    } else if state.busy {
                        crate::i18n::text("ui-searching")
                    } else {
                        crate::i18n::text("ui-no-matching-files")
                    };
                    let detail = if state.failed {
                        state.status.as_deref().unwrap_or(crate::i18n::text("ui-please-try-again"))
                    } else if state.busy {
                        crate::i18n::text("ui-searching-the-local-file-index")
                    } else {
                        crate::i18n::text("ui-try-shorter-keywords-or-check-your-filters")
                    };
                    target.clipped_text(
                        title,
                        &self.name,
                        &Rect::from_xywh(18.0, TOP + 10.0, (w - 36.0).max(0.0), 24.0),
                        text,
                    );
                    target.clipped_text(
                        detail,
                        &self.path,
                        &Rect::from_xywh(18.0, TOP + 36.0, (w - 36.0).max(0.0), 20.0),
                        dim,
                    );
                    if state.failed {
                        for (action, label) in [
                            (Action::Retry, crate::i18n::text("ui-retry-f5")),
                            (Action::Launch, crate::i18n::text("ui-start-everything")),
                        ] {
                            let Some(r) = action_rect(action, w) else { continue; };
                            let (x, width) = (r.x, r.width);
                            let rect = Rect::from_xywh(r.x, r.y, r.width, r.height);
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect,
                                    radius_x: 5.0,
                                    radius_y: 5.0,
                                },
                                if state.hovered_action == Some(action) { selected } else { hovered },
                            );
                            target.clipped_text(
                                label,
                                &self.path,
                                &Rect::from_xywh(x + 8.0, TOP + 60.0, width - 16.0, 28.0),
                                accent,
                            );
                        }
                    }
                }
                for (row, entry) in state
                    .entries
                    .iter()
                    .enumerate()
                    .skip(state.scroll)
                    .take(state.visible_rows)
                {
                    let y = TOP + ROW_INSET + (row - state.scroll) as f32 * ROW;
                    if state.selection.contains(&row) || state.hovered == Some(row) {
                        target.fill_rounded_rect(
                            &RoundedRect {
                                rect: Rect::from_xywh(6.0, y, w - 12.0, ROW - 2.0),
                                radius_x: 6.0,
                                radius_y: 6.0,
                            },
                            if state.selection.contains(&row) {
                                selected
                            } else {
                                hovered
                            },
                        );
                        if state.focused == Some(row) && state.selection.contains(&row) {
                            target.fill_rounded_rect(
                                &RoundedRect {
                                    rect: Rect::from_xywh(7.0, y + 12.0, 3.0, ROW - 26.0),
                                    radius_x: 1.5,
                                    radius_y: 1.5,
                                },
                                accent,
                            );
                        }
                    }
                    // Ctrl+navigation moves focus without changing selection.
                    // Keep that keyboard target visible, including unselected rows.
                    if state.focused == Some(row) && unsafe { GetFocus() } == hwnd {
                        target.draw_rounded_rect(&RoundedRect {
                            rect: Rect::from_xywh(7.0, y + 1.0, (w - 14.0).max(0.0), ROW - 4.0),
                            radius_x: crate::pane::control_style::RADIUS,
                            radius_y: crate::pane::control_style::RADIUS,
                        }, accent, crate::pane::control_style::FOCUS_STROKE);
                    }
                    if !state.replacing && (state.hovered == Some(row) || state.focused == Some(row)) {
                        for button in [RowAction::Location, RowAction::More] {
                            let Some(r) = row_action_rect(button, w, y) else { continue; };
                            let rect = Rect::from_xywh(r.x, r.y, r.width, r.height);
                            let active = state.hovered_row_action == Some((row, button));
                            if active { target.fill_rounded_rect(&RoundedRect { rect, radius_x: 5.0, radius_y: 5.0 }, action_hover); }
                            target.clipped_icon(if button == RowAction::More { "\u{e712}" } else { "\u{e8b7}" },
                                &self.action_icon, &rect, if active { text } else { dim });
                        }
                    }
                    let name = entry
                        .path
                        .file_name()
                        .unwrap_or(entry.path.as_os_str())
                        .to_string_lossy();
                    let parent = entry.path.parent().unwrap_or(&entry.path).to_string_lossy();
                    let path = parent;
                    let slot = Rect::from_xywh(16.0, y + 10.0, 24.0, 24.0);
                    if let Some(bitmap) = self.file_icons.bitmap(entry, icon_size, |pixels|
                        canvas_result(target.create_bitmap(&pixels.data, pixels.width, pixels.height)))? {
                        target.draw_bitmap(bitmap, &slot, 1.0);
                    } else {
                        target.clipped_icon(if entry.folder { "\u{e8b7}" } else { "\u{e8a5}" },
                            &self.icon, &slot, if entry.folder {folder_icon} else {accent});
                    }
                    target.clipped_text(
                        &name,
                        &self.name,
                        &Rect::from_xywh(50.0, y + 2.0, (w - 50.0 - if w >= 400.0 { 82.0 } else { 48.0 }).max(0.0), 22.0),
                        if state.replacing { dim } else { text },
                    );
                    target.clipped_text(
                        &path,
                        &self.path,
                        &Rect::from_xywh(50.0, y + 24.0, (w - 50.0 - if w >= 400.0 { 82.0 } else { 48.0 }).max(0.0), 18.0),
                        dim,
                    );
                }
                if !state.entries.is_empty() {
                    let y = h - FOOTER;
                    target.fill_rect(&Rect::from_xywh(12.0, y, (w - 24.0).max(0.0), 1.0), line);
                    let hint_width = if w >= 480.0 && !state.replacing && state.selection.len() == 1 { 210.0 } else { 0.0 };
                    if hint_width > 0.0 {
                        let preview = crate::pane::peek::settings();
                        let hint = if preview.enabled {
                            format!("Enter {} · {} {}", crate::i18n::text("ui-open"), crate::pane::peek::shortcut_label(&preview), crate::i18n::text("ui-preview"))
                        } else { format!("Enter {}", crate::i18n::text("ui-open")) };
                        target.clipped_text(&hint, &self.path, &Rect::from_xywh(w - hint_width - 16.0, y + 1.0, hint_width, FOOTER - 2.0), dim);
                    }
                    target.clipped_text(
                        &state.footer(),
                        &self.path,
                        &Rect::from_xywh(16.0, y + 1.0, (w - 32.0 - hint_width).max(0.0), FOOTER - 2.0),
                        dim,
                    );
                }
                if state.entries.len() > state.visible_rows {
                    let track = (h - TOP - FOOTER - 10.0).max(0.0);
                    let thumb =
                        (track * state.visible_rows as f32 / state.entries.len() as f32).max(12.0);
                    let y = TOP
                        + 5.0
                        + (track - thumb) * state.scroll as f32
                            / (state.entries.len() - state.visible_rows) as f32;
                    target.fill_rounded_rect(
                        &RoundedRect {
                            rect: Rect::from_xywh(w - 5.0, y, 2.0, thumb),
                            radius_x: 1.0,
                            radius_y: 1.0,
                        },
                        dim,
                    );
                }
            }
            target.finish()
        })
        .map_err(|e| e.to_string())?;
        Ok(true)
    }
}
