//! Color only the emoji cells of the native title EDIT. Its own character
//! positions remain authoritative for selection, scrolling, caret and IME.
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        },
    },
    core::Result,
};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, GetSysColor, GetTextMetricsW, HDC, LOGFONTW,
        SelectObject, TEXTMETRICW,
    },
    UI::{Controls::*, WindowsAndMessaging::*},
};

pub(super) struct Painter {
    factory: IDWriteFactory,
    format: IDWriteTextFormat,
    target: ID2D1DCRenderTarget,
}

struct EmojiCell {
    layout: IDWriteTextLayout,
    x: f32,
    y: f32,
    width: f32,
    metrics: DWRITE_TEXT_METRICS,
    selected: bool,
}

impl Painter {
    pub(super) fn new(font: &LOGFONTW) -> Result<Self> {
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            // Pixel units, just like EM_POSFROMCHAR. Do not scale a second time
            // at 125/150/200% DPI; LOGFONT already contains the scaled size.
            let format = factory.CreateTextFormat(
                windows::core::w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT(font.lfWeight),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font.lfHeight.unsigned_abs() as f32,
                windows::core::w!(""),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self {
                factory,
                format,
                target,
            })
        }
    }

    pub(super) unsafe fn paint(&self, edit: HWND, dc: HDC, text: &str, dark: bool) -> Result<()> {
        if !text.chars().any(super::super::canvas::emoji_character) {
            return Ok(());
        }
        let wide: Vec<_> = text.encode_utf16().collect();
        unsafe {
            let mut client = windows_sys::Win32::Foundation::RECT::default();
            let mut clip = client;
            GetClientRect(edit, &raw mut client);
            SendMessageW(edit, EM_GETRECT, 0, (&raw mut clip) as isize);
            let mut start = 0u32;
            let mut end = 0u32;
            SendMessageW(
                edit,
                EM_GETSEL,
                (&raw mut start) as usize,
                (&raw mut end) as isize,
            );
            let line_height = line_height(edit, dc);
            self.target.BindDC(
                windows::Win32::Graphics::Gdi::HDC(dc),
                &RECT {
                    left: 0,
                    top: 0,
                    right: client.right,
                    bottom: client.bottom,
                },
            )?;
            let foreground = self.target.CreateSolidColorBrush(
                &color(if dark { 0x00f4_f4f4 } else { 0x0020_2020 }),
                None,
            )?;
            let background = self.target.CreateSolidColorBrush(
                &color(if dark { 0x002b_2b2b } else { 0x00ff_ffff }),
                None,
            )?;
            let highlight = self
                .target
                .CreateSolidColorBrush(&color(GetSysColor(COLOR_HIGHLIGHT)), None)?;
            let selected_text = self
                .target
                .CreateSolidColorBrush(&color(GetSysColor(COLOR_HIGHLIGHTTEXT)), None)?;
            // Prepare every fallible layout operation before BeginDraw so errors
            // cannot leave a render target in an unfinished drawing session.
            let cells = self.cells(edit, &wide, start, end)?;
            self.target.BeginDraw();
            self.target.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: clip.left.max(0) as f32,
                    top: clip.top.max(0) as f32,
                    right: clip.right.min(client.right) as f32,
                    bottom: clip.bottom.min(client.bottom) as f32,
                },
                D2D1_ANTIALIAS_MODE_ALIASED,
            );
            for EmojiCell {
                layout: probe,
                x,
                y,
                width,
                metrics,
                selected,
            } in cells
            {
                let cell = D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + width,
                    bottom: y + line_height,
                };
                self.target
                    .PushAxisAlignedClip(&raw const cell, D2D1_ANTIALIAS_MODE_ALIASED);
                self.target.FillRectangle(
                    &raw const cell,
                    if selected { &highlight } else { &background },
                );
                // Fit the shaped sequence into the cell allocated by EDIT. This
                // also covers flags, skin tones, keycaps and joined families.
                let scale = (width / metrics.widthIncludingTrailingWhitespace.max(1.0)).min(1.0);
                self.target.SetTransform(&windows_numerics::Matrix3x2 {
                    M11: scale,
                    M22: 1.0,
                    M31: x,
                    M32: y + (line_height - metrics.height) * 0.5,
                    ..Default::default()
                });
                self.target.DrawTextLayout(
                    windows_numerics::Vector2::zero(),
                    &probe,
                    if selected {
                        &selected_text
                    } else {
                        &foreground
                    },
                    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                );
                self.target
                    .SetTransform(&windows_numerics::Matrix3x2::identity());
                self.target.PopAxisAlignedClip();
            }
            self.target.PopAxisAlignedClip();
            self.target.EndDraw(None, None)
        }
    }
    unsafe fn cells(
        &self,
        edit: HWND,
        wide: &[u16],
        start: u32,
        end: u32,
    ) -> Result<Vec<EmojiCell>> {
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(wide, &self.format, 100_000.0, 4096.0)?;
            let mut clusters = vec![DWRITE_CLUSTER_METRICS::default(); wide.len()];
            let mut count = 0;
            layout.GetClusterMetrics(Some(&mut clusters), &raw mut count)?;
            let mut cells = Vec::new();
            let mut at = 0;
            for cluster in &clusters[..count as usize] {
                let next = at + usize::from(cluster.length);
                if super::super::canvas::emoji_cluster(&wide[at..next]) {
                    let position = SendMessageW(edit, EM_POSFROMCHAR, at, 0);
                    if position != -1 {
                        let (x, y) = point(position);
                        let following = SendMessageW(edit, EM_POSFROMCHAR, next, 0);
                        let probe = self.factory.CreateTextLayout(
                            &wide[at..next],
                            &self.format,
                            100_000.0,
                            4096.0,
                        )?;
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        probe.GetMetrics(&raw mut metrics)?;
                        let (right, next_y) = point(following);
                        let width = if following != -1 && y == next_y && right > x {
                            (right - x) as f32
                        } else {
                            metrics.widthIncludingTrailingWhitespace.ceil()
                        };
                        let selected = (at as u32) < end && (next as u32) > start;
                        cells.push(EmojiCell {
                            layout: probe,
                            x: x as f32,
                            y: y as f32,
                            width,
                            metrics,
                            selected,
                        });
                    }
                }
                at = next;
            }
            Ok(cells)
        }
    }
}

fn point(packed: isize) -> (i32, i32) {
    (i32::from(packed as i16), i32::from((packed >> 16) as i16))
}

unsafe fn line_height(edit: HWND, dc: HDC) -> f32 {
    unsafe {
        let previous_font = SelectObject(dc, SendMessageW(edit, WM_GETFONT, 0, 0) as _);
        let mut metrics = TEXTMETRICW::default();
        GetTextMetricsW(dc, &raw mut metrics);
        SelectObject(dc, previous_font);
        metrics.tmHeight.max(1) as f32
    }
}

fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: (rgb & 255) as f32 / 255.0,
        g: ((rgb >> 8) & 255) as f32 / 255.0,
        b: ((rgb >> 16) & 255) as f32 / 255.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::*;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn native_edit_keeps_utf16_selection_and_renders_color_sequences() {
        unsafe {
            for size in [16, 20, 24, 32] {
                let mut font = LOGFONTW {
                    lfHeight: -size,
                    lfWeight: 400,
                    ..Default::default()
                };
                let family: Vec<_> = "Segoe UI\0".encode_utf16().collect();
                font.lfFaceName[..family.len()].copy_from_slice(&family);
                let painter = Painter::new(&font).unwrap();
                let native_font = CreateFontIndirectW(&raw const font);
                let edit = CreateWindowExW(
                    0,
                    windows_sys::w!("EDIT"),
                    windows_sys::w!(""),
                    WS_POPUP | ES_MULTILINE as u32 | ES_AUTOHSCROLL as u32,
                    0,
                    0,
                    320,
                    64,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                assert!(!edit.is_null());
                SendMessageW(edit, WM_SETFONT, native_font as usize, 0);
                let dc = CreateCompatibleDC(std::ptr::null_mut());
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: 320,
                        biHeight: -64,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                let bitmap = CreateDIBSection(
                    dc,
                    &raw const info,
                    DIB_RGB_COLORS,
                    &raw mut bits,
                    std::ptr::null_mut(),
                    0,
                );
                assert!(!bitmap.is_null());
                let old = SelectObject(dc, bitmap);
                for label in ["🎮 游戏", "👩🏽‍💻 工作", "🇨🇳 中文", "1️⃣ 分类", "末尾🎮"]
                {
                    let value: Vec<_> = label.encode_utf16().chain([0]).collect();
                    SetWindowTextW(edit, value.as_ptr());
                    for selected in [false, true] {
                        SendMessageW(edit, EM_SETSEL, 0, if selected { -1 } else { 0 });
                        let mut before = (0u32, 0u32);
                        SendMessageW(
                            edit,
                            EM_GETSEL,
                            (&raw mut before.0) as usize,
                            (&raw mut before.1) as isize,
                        );
                        let background = CreateSolidBrush(0x002b_2b2b);
                        FillRect(
                            dc,
                            &windows_sys::Win32::Foundation::RECT {
                                left: 0,
                                top: 0,
                                right: 320,
                                bottom: 64,
                            },
                            background,
                        );
                        DeleteObject(background);
                        painter.paint(edit, dc, label, true).unwrap();
                        GdiFlush();
                        let pixels = std::slice::from_raw_parts(bits.cast::<u32>(), 320 * 64);
                        let highlight = GetSysColor(COLOR_HIGHLIGHT);
                        let highlight_rgb =
                            ((highlight & 255) << 16) | (highlight & 0xff00) | (highlight >> 16);
                        let colors = pixels
                            .iter()
                            .filter(|pixel| {
                                let p = **pixel & 0x00ff_ffff;
                                let r = (p >> 16) & 255;
                                let g = (p >> 8) & 255;
                                let b = p & 255;
                                p != highlight_rgb && r.max(g).max(b) - r.min(g).min(b) > 40
                            })
                            .count();
                        // Windows' system emoji font displays country flags as
                        // regional letters. Preserve that system fallback too.
                        if !label.starts_with('🇨') {
                            assert!(
                                colors > 5,
                                "color glyph missing: {label}, size={size}, selected={selected}: {colors}"
                            );
                        }
                        let mut after = (0u32, 0u32);
                        SendMessageW(
                            edit,
                            EM_GETSEL,
                            (&raw mut after.0) as usize,
                            (&raw mut after.1) as isize,
                        );
                        assert_eq!(before, after, "painting must not move the caret/selection");
                        assert_eq!(super::super::text(edit), label);
                    }
                }
                SelectObject(dc, old);
                DeleteObject(bitmap);
                DeleteDC(dc);
                DestroyWindow(edit);
                DeleteObject(native_font);
            }
        }
    }
}
