use super::{Alignment, Bounds, Canvas, Scrollbar, Tone};
use crate::control_style::{self as style, Palette, colorref};
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{
        Controls::{STATE_SYSTEM_PRESSED, STATE_SYSTEM_UNAVAILABLE},
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

pub(crate) struct GdiCanvas<'a> {
    pub dc: HDC,
    pub scale: f32,
    pub palette: &'a Palette,
}

impl GdiCanvas<'_> {
    pub fn bounds(&self, rect: RECT) -> Bounds {
        Bounds::new(
            rect.left as f32 / self.scale,
            rect.top as f32 / self.scale,
            (rect.right - rect.left) as f32 / self.scale,
            (rect.bottom - rect.top) as f32 / self.scale,
        )
    }

    fn px(&self, dip: f32) -> i32 {
        (dip * self.scale).round() as i32
    }

    fn rect(&self, bounds: Bounds) -> RECT {
        RECT {
            left: self.px(bounds.x),
            top: self.px(bounds.y),
            right: self.px(bounds.x + bounds.width),
            bottom: self.px(bounds.y + bounds.height),
        }
    }

    fn round_rect(&self, bounds: Bounds, radius: f32, brush: HGDIOBJ, pen: HGDIOBJ) {
        let rect = self.rect(bounds);
        let diameter = self.px(radius * 2.0);
        unsafe {
            let old_brush = SelectObject(self.dc, brush);
            let old_pen = SelectObject(self.dc, pen);
            RoundRect(
                self.dc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                diameter,
                diameter,
            );
            SelectObject(self.dc, old_pen);
            SelectObject(self.dc, old_brush);
        }
    }

    fn fill_color(&self, bounds: Bounds, radius: f32, rgb: u32) {
        unsafe {
            let brush = CreateSolidBrush(colorref(rgb));
            self.round_rect(bounds, radius, brush, GetStockObject(NULL_PEN));
            DeleteObject(brush);
        }
    }

    pub(crate) fn icons<const N: usize>(&self, size: f32, icons: [(RECT, &str, u32); N]) {
        let characters = icons.map(|(_, icon, _)| icon.encode_utf16().next().unwrap());
        for family in [style::FLUENT_ICON_FONT, style::MDL2_ICON_FONT] {
            let face: Vec<_> = family.encode_utf16().chain(Some(0)).collect();
            unsafe {
                let font = CreateFontW(
                    -self.px(size),
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    ANTIALIASED_QUALITY as u32,
                    0,
                    face.as_ptr(),
                );
                if font.is_null() {
                    continue;
                }
                let old_font = SelectObject(self.dc, font);
                let mut glyphs = [0; N];
                let found = GetGlyphIndicesW(
                    self.dc,
                    characters.as_ptr(),
                    N as i32,
                    glyphs.as_mut_ptr(),
                    GGI_MARK_NONEXISTING_GLYPHS,
                ) != GDI_ERROR as u32
                    && !glyphs.contains(&u16::MAX);
                if found {
                    let old_mode = SetBkMode(self.dc, TRANSPARENT as i32);
                    let old_align = SetTextAlign(self.dc, TA_LEFT | TA_BASELINE);
                    for ((slot, _, rgb), glyph) in icons.into_iter().zip(glyphs) {
                        self.center_glyph(slot, glyph, rgb);
                    }
                    SetTextAlign(self.dc, old_align);
                    SetBkMode(self.dc, old_mode);
                }
                SelectObject(self.dc, old_font);
                DeleteObject(font);
                if found {
                    break;
                }
            }
        }
    }

    fn center_glyph(&self, bounds: RECT, glyph: u16, rgb: u32) {
        let mut metrics = GLYPHMETRICS::default();
        let one = FIXED { fract: 0, value: 1 };
        let transform = MAT2 {
            eM11: one,
            eM22: one,
            ..Default::default()
        };
        unsafe {
            if GetGlyphOutlineW(
                self.dc,
                u32::from(glyph),
                GGO_GLYPH_INDEX | GGO_METRICS,
                &raw mut metrics,
                0,
                null_mut(),
                &transform,
            ) == GDI_ERROR as u32
            {
                return;
            }
            let x = bounds.left + (bounds.right - bounds.left - metrics.gmBlackBoxX as i32) / 2
                - metrics.gmptGlyphOrigin.x;
            let y = bounds.top
                + (bounds.bottom - bounds.top - metrics.gmBlackBoxY as i32) / 2
                + metrics.gmptGlyphOrigin.y;
            let old_color = SetTextColor(self.dc, colorref(rgb));
            ExtTextOutW(
                self.dc,
                x,
                y,
                ETO_CLIPPED | ETO_GLYPH_INDEX,
                &bounds,
                &glyph,
                1,
                null(),
            );
            SetTextColor(self.dc, old_color);
        }
    }
}

impl Canvas for GdiCanvas<'_> {
    type Error = std::convert::Infallible;

    fn fill(&mut self, bounds: Bounds, radius: f32, tone: Tone) {
        self.fill_color(bounds, radius, tone.rgb(self.palette));
    }

    fn fill_rgb(&mut self, bounds: Bounds, radius: f32, rgb: [f32; 3]) -> Result<(), Self::Error> {
        let [r, g, b] = rgb.map(|value| (value * 255.0).round() as u32);
        self.fill_color(bounds, radius, (r << 16) | (g << 8) | b);
        Ok(())
    }

    fn stroke(&mut self, bounds: Bounds, radius: f32, thickness: f32, tone: Tone) {
        unsafe {
            let pen = CreatePen(
                PS_INSIDEFRAME,
                self.px(thickness),
                colorref(tone.rgb(self.palette)),
            );
            self.round_rect(bounds, radius, GetStockObject(HOLLOW_BRUSH), pen);
            DeleteObject(pen);
        }
    }

    fn text(&mut self, bounds: Bounds, label: &str, alignment: Alignment, tone: Tone) {
        if label.is_empty() {
            return;
        }
        let mut rect = self.rect(bounds);
        if matches!(alignment, Alignment::Icon | Alignment::ActionIcon) {
            let size = if matches!(alignment, Alignment::Icon) {
                style::ICON_SIZE
            } else {
                style::ICON_SLOT
            };
            self.icons(size, [(rect, label, tone.rgb(self.palette))]);
            return;
        }
        let text: Vec<_> = label.encode_utf16().collect();
        let horizontal = if matches!(alignment, Alignment::Leading | Alignment::Supporting) {
            DT_LEFT
        } else {
            DT_CENTER
        };
        unsafe {
            let old_mode = SetBkMode(self.dc, TRANSPARENT as i32);
            let old_color = SetTextColor(self.dc, colorref(tone.rgb(self.palette)));
            DrawTextW(
                self.dc,
                text.as_ptr(),
                text.len() as i32,
                &raw mut rect,
                horizontal | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
            SetTextColor(self.dc, old_color);
            SetBkMode(self.dc, old_mode);
        }
    }
}

/// Paint over the edit's native nonclient bar. The static buddy is transparent
/// to hit testing, so Win32 still owns scrolling, selection and accessibility.
pub(crate) unsafe fn add_edit_scrollbar(parent: HWND, edit: HWND, id: i32, dark: bool) {
    unsafe {
        let bar = CreateWindowExW(
            WS_EX_TRANSPARENT,
            windows_sys::w!("STATIC"),
            null(),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            parent,
            id as usize as _,
            null_mut(),
            null(),
        );
        if !bar.is_null()
            && SetWindowSubclass(
                bar,
                Some(edit_scrollbar_proc),
                usize::from(dark),
                edit as usize,
            ) == 0
        {
            DestroyWindow(bar);
        }
    }
}

fn native_scrollbar(edit: HWND) -> Option<SCROLLBARINFO> {
    let mut info = SCROLLBARINFO {
        cbSize: size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    (unsafe { GetScrollBarInfo(edit, OBJID_VSCROLL, &raw mut info) } != 0).then_some(info)
}

pub(crate) unsafe fn position_edit_scrollbar(parent: HWND, edit: HWND, bar: HWND) {
    if bar.is_null() {
        return;
    }
    let Some(info) = native_scrollbar(edit) else {
        return;
    };
    unsafe {
        let mut rect = info.rcScrollBar;
        MapWindowPoints(null_mut(), parent, (&raw mut rect).cast(), 2);
        SetWindowPos(
            bar,
            HWND_TOP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn edit_scrollbar_proc(
    hwnd: HWND,
    message: u32,
    wp: usize,
    lp: isize,
    dark: usize,
    edit: usize,
) -> isize {
    unsafe {
        match message {
            WM_NCHITTEST => HTTRANSPARENT as isize,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &raw mut paint);
                paint_edit_scrollbar(hwnd, edit as HWND, dc, dark != 0);
                EndPaint(hwnd, &paint);
                0
            }
            WM_PRINTCLIENT => {
                paint_edit_scrollbar(hwnd, edit as HWND, wp as HDC, dark != 0);
                0
            }
            _ => DefSubclassProc(hwnd, message, wp, lp),
        }
    }
}

unsafe fn paint_edit_scrollbar(hwnd: HWND, edit: HWND, dc: HDC, dark: bool) {
    unsafe {
        let palette = Palette::for_theme(dark);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &raw mut rect);
        let background = CreateSolidBrush(colorref(palette.page_background));
        FillRect(dc, &rect, background);
        DeleteObject(background);
        let Some(bar) = native_scrollbar(edit) else {
            return;
        };
        let mut scroll = SCROLLINFO {
            cbSize: size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            ..Default::default()
        };
        if GetScrollInfo(edit, SB_VERT, &raw mut scroll) == 0 {
            return;
        }
        let maximum = i64::from(scroll.nMax) - i64::from(scroll.nPage.saturating_sub(1));
        if maximum <= i64::from(scroll.nMin) {
            return;
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let center = rect.right / 2;
        let width = (Scrollbar::THUMB_WIDTH * scale).round() as i32;
        let mut canvas = GdiCanvas {
            dc,
            scale,
            palette: &palette,
        };
        let thumb = canvas.bounds(RECT {
            left: center - width / 2,
            top: bar.xyThumbTop,
            right: center - width / 2 + width,
            bottom: bar.xyThumbBottom,
        });
        if thumb.height > 0.0 {
            Scrollbar::paint_thumb(&mut canvas, thumb);
        }
        let tones = [
            (bar.rgstate[1], scroll.nPos > scroll.nMin),
            (bar.rgstate[5], i64::from(scroll.nPos) < maximum),
        ]
        .map(|(native_state, can_scroll)| {
            if !can_scroll || native_state & STATE_SYSTEM_UNAVAILABLE != 0 {
                Tone::Disabled
            } else if native_state & STATE_SYSTEM_PRESSED != 0 {
                Tone::AccentInk
            } else {
                Tone::Ink
            }
        });
        let slots = [
            RECT {
                bottom: bar.dxyLineButton,
                ..rect
            },
            RECT {
                top: rect.bottom - bar.dxyLineButton,
                ..rect
            },
        ];
        canvas.icons(
            style::ICON_SLOT,
            [
                (slots[0], style::CHEVRON_UP, tones[0].rgb(&palette)),
                (slots[1], style::CHEVRON_DOWN, tones[1].rgb(&palette)),
            ],
        );
    }
}
