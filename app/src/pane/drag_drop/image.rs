//! A nonactivating, per-pixel-alpha overlay follows the pointer across windows.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::wildcard_imports
)]
use crate::pane::assets::Pixels;
use windows_sys::Win32::{
    Foundation::{HWND, POINT, SIZE},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};

pub struct DragImage {
    hwnd: HWND,
    hotspot: POINT,
}

/// A list drag preview keeps the icon and name aligned with the source row.
pub fn list_item_pixels(
    image: &Pixels,
    name: &str,
    grid: crate::pane::layout::Grid,
    scale: f32,
) -> Option<Pixels> {
    use crate::pane::{canvas, native_graphics::{canvas_result, gpu_device}};
    use windows_canvas::{ColorF, Rect, TextFormat, ParagraphAlignment, WordWrapping};
    let width = (grid.cell_width * scale).round().max(1.0) as u32;
    let height = (grid.cell_height * scale).round().max(1.0) as u32;
    let device = gpu_device().ok()?;
    let bitmap = canvas::Offscreen::new(&device, width, height).ok()?;
    let format = TextFormat::new(&crate::pane::fonts::family(), 12.0).ok()?
        .with_paragraph_alignment(ParagraphAlignment::Center)
        .with_word_wrapping(WordWrapping::NoWrap);
    canvas::ellipsis(&format).ok()?;
    canvas::draw(&bitmap.target, scale, |frame| {
        frame.clear(ColorF::new(0.0, 0.0, 0.0, 0.0));
        if image.width > 0 && image.height > 0 {
            let ratio = grid.icon_size / image.width.max(image.height) as f32;
            let iw = image.width as f32 * ratio;
            let ih = image.height as f32 * ratio;
            let icon = canvas_result(frame.create_bitmap(&image.data, image.width, image.height))?;
            frame.draw_bitmap(&icon, &Rect::from_xywh(4.0, (grid.cell_height - ih) / 2.0, iw, ih), 1.0);
        }
        let ink = canvas_result(frame.create_solid_brush(ColorF::WHITE))?;
        frame.clipped_text(name, &format, &Rect::from_xywh(32.0, 0.0, (grid.cell_width - 40.0).max(1.0), grid.cell_height), &ink);
        frame.finish()
    }).ok()?;
    Some(Pixels { width, height, data: bitmap.pixels().ok()? })
}

/// Keep the drag preview in cell coordinates so dragging by the label does not jump.
pub fn item_pixels(
    image: &Pixels,
    name: &str,
    grid: crate::pane::layout::Grid,
    scale: f32,
) -> Option<Pixels> {
    if image.width == 0 || image.height == 0 {
        return None;
    }
    let width = (grid.cell_width * scale).round().max(1.0) as u32;
    let label = if grid.icon_only {
        None
    } else {
        Some(crate::pane::label::raster_scaled(name, width, (96.0 * scale).round() as u32, 2, grid.text_scale)?)
    };
    let label_y = ((grid.icon_size + crate::pane::layout::LABEL_OFFSET) * scale).round() as u32
        - label.as_ref().map_or(0, |label| label.padding);
    let height = label.as_ref().map_or_else(
        || (grid.cell_height * scale).ceil() as u32,
        |label| label_y + label.pixels.height,
    );
    let mut data = vec![0; (width * height * 4) as usize];
    let ratio = grid.icon_size * scale / image.width.max(image.height) as f32;
    let iw = (image.width as f32 * ratio).round().max(1.0) as u32;
    let ih = (image.height as f32 * ratio).round().max(1.0) as u32;
    let left = (width - iw) / 2;
    let top = (grid.icon_top() * scale + (grid.icon_size * scale - ih as f32) / 2.0).round() as u32;
    let icon = resize(image, iw, ih);
    for y in 0..ih {
        let start = (((top + y) * width + left) * 4) as usize;
        let source = (y * iw * 4) as usize;
        data[start..start + (iw * 4) as usize]
            .copy_from_slice(&icon[source..source + (iw * 4) as usize]);
    }
    if let Some(label) = label {
        for (source, destination) in label.pixels.data.chunks_exact(4)
            .zip(data[(label_y * width * 4) as usize..].chunks_exact_mut(4))
        {
            let remaining = 255 - u16::from(source[3]);
            for c in 0..4 {
                destination[c] =
                    (u16::from(source[c]) + (u16::from(destination[c]) * remaining + 127) / 255) as u8;
            }
        }
    }
    Some(Pixels {
        width,
        height,
        data,
    })
}

/// Merge selected cells in client-pixel coordinates, including gaps and rows.
/// The returned origin keeps the pointer anchored to the cell actually grabbed.
pub fn selection_pixels(cells: &[(Pixels, POINT)]) -> Option<(Pixels, POINT)> {
    let left = cells.iter().map(|(_, p)| p.x).min()?;
    let top = cells.iter().map(|(_, p)| p.y).min()?;
    let right = cells
        .iter()
        .map(|(image, p)| i64::from(p.x) + i64::from(image.width))
        .max()?;
    let bottom = cells
        .iter()
        .map(|(image, p)| i64::from(p.y) + i64::from(image.height))
        .max()?;
    let width = u32::try_from(right - i64::from(left)).ok()?;
    let height = u32::try_from(bottom - i64::from(top)).ok()?;
    let size = usize::try_from(width)
        .ok()?
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let mut data = Vec::new();
    data.try_reserve_exact(size).ok()?;
    data.resize(size, 0);
    for (image, point) in cells {
        let x = (i64::from(point.x) - i64::from(left)) as usize;
        let y = (i64::from(point.y) - i64::from(top)) as usize;
        for row in 0..image.height as usize {
            let source = row * image.width as usize * 4;
            let destination = ((y + row) * width as usize + x) * 4;
            for (src, dst) in image.data[source..source + image.width as usize * 4]
                .chunks_exact(4)
                .zip(data[destination..destination + image.width as usize * 4].chunks_exact_mut(4))
            {
                let remaining = 255 - u16::from(src[3]);
                for c in 0..4 {
                    dst[c] =
                        (u16::from(src[c]) + (u16::from(dst[c]) * remaining + 127) / 255) as u8;
                }
            }
        }
    }
    Some((
        Pixels {
            width,
            height,
            data,
        },
        POINT { x: left, y: top },
    ))
}

// Interpolate premultiplied BGRA without losing edge alpha.
fn resize(source: &Pixels, width: u32, height: u32) -> Vec<u8> {
    let mut output = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let sx = ((x as f32 + 0.5) * source.width as f32 / width as f32 - 0.5)
                .clamp(0.0, (source.width - 1) as f32);
            let sy = ((y as f32 + 0.5) * source.height as f32 / height as f32 - 0.5)
                .clamp(0.0, (source.height - 1) as f32);
            let (x0, y0) = (sx as u32, sy as u32);
            let (x1, y1) = (
                (x0 + 1).min(source.width - 1),
                (y0 + 1).min(source.height - 1),
            );
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            for c in 0..4 {
                let sample =
                    |x, y| f32::from(source.data[((y * source.width + x) * 4 + c) as usize]);
                let top = sample(x0, y0) * (1.0 - fx) + sample(x1, y0) * fx;
                let bottom = sample(x0, y1) * (1.0 - fx) + sample(x1, y1) * fx;
                output[((y * width + x) * 4 + c) as usize] =
                    (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    output
}

impl DragImage {
    pub fn new(
        owner: HWND,
        pixels: &Pixels,
        point: POINT,
        hotspot: POINT,
        size: SIZE,
    ) -> Option<Self> {
        if pixels.width == 0 || pixels.height == 0 || size.cx <= 0 || size.cy <= 0 {
            return None;
        }
        let data = resize(pixels, size.cx as u32, size.cy as u32);
        unsafe {
            let class: Vec<_> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                size.cx,
                size.cy,
                owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            let overlay = Self { hwnd, hotspot };
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return None;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size.cx,
                    biHeight: -size.cy,
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
            if bitmap.is_null() {
                DeleteDC(dc);
                return None;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), bits.cast(), data.len());
            let old = SelectObject(dc, bitmap);
            let destination = POINT {
                x: point.x - hotspot.x,
                y: point.y - hotspot.y,
            };
            let origin = POINT::default();
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 210,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let ok = UpdateLayeredWindow(
                hwnd,
                std::ptr::null_mut(),
                &raw const destination,
                &raw const size,
                dc,
                &raw const origin,
                0,
                &raw const blend,
                ULW_ALPHA,
            );
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            if ok == 0 {
                return None;
            }
            // Present alpha pixels before making the window visible to avoid a black flash.
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Some(overlay)
        }
    }
    pub fn move_to(&self, point: POINT) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                point.x - self.hotspot.x,
                point.y - self.hotspot.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
}
impl Drop for DragImage {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_preview_includes_label_at_each_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            {
                let grid = crate::pane::layout::Grid::system(400.0, 300.0, 48.0, (88.0, 96.0));
                for (width, height) in [(16, 16), (32, 16), (16, 32)] {
                    let source = Pixels {
                        width,
                        height,
                        data: [60, 120, 240, 255].repeat((width * height) as usize),
                    };
                    let preview = item_pixels(&source, "微信 Music", grid, scale)
                        .expect("icon and label preview");
                    let label_start = ((grid.icon_size + crate::pane::layout::LABEL_OFFSET) * scale)
                        .ceil() as u32;
                    assert!(
                        preview.data[(label_start * preview.width * 4) as usize..]
                            .chunks_exact(4)
                            .any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200),
                        "the dragged name must remain visible below the icon"
                    );
                    assert!(
                        preview
                            .data
                            .chunks_exact(4)
                            .any(|p| p == [60, 120, 240, 255])
                    );
                    assert_eq!(&preview.data[..4], &[0, 0, 0, 0]);
                    assert!(
                        preview
                            .data
                            .chunks_exact(4)
                            .all(|p| p[..3].iter().all(|c| *c <= p[3]))
                    );
                }
            }
        }
    }

    #[test]
    fn multi_selection_keeps_cells_gaps_and_pointer_anchor_at_each_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            let grid = crate::pane::layout::Grid::system(400.0, 300.0, 48.0, (88.0, 96.0));
            let icon = |color: [u8; 4]| Pixels {
                width: 16,
                height: 16,
                data: color.repeat(256),
            };
            let first = item_pixels(&icon([0, 0, 255, 255]), "First", grid, scale).unwrap();
            let second = item_pixels(&icon([0, 255, 0, 255]), "Second", grid, scale).unwrap();
            let third = item_pixels(&icon([255, 0, 0, 255]), "Third", grid, scale).unwrap();
            let origin = POINT {
                x: (12.0 * scale) as i32,
                y: (-40.0 * scale) as i32,
            };
            let right = POINT {
                x: origin.x + (grid.cell_width * scale * 2.0).round() as i32,
                y: origin.y,
            };
            let below = POINT {
                x: origin.x,
                y: origin.y + (140.0 * scale) as i32,
            };
            let (one, one_origin) = selection_pixels(&[(first.clone(), origin)]).unwrap();
            assert_eq!(one.data, first.data);
            assert_eq!((one_origin.x, one_origin.y), (origin.x, origin.y));
            let (all, all_origin) =
                selection_pixels(&[(first, origin), (second, right), (third, below)]).unwrap();
            assert_eq!((all_origin.x, all_origin.y), (origin.x, origin.y));
            for color in [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255]] {
                assert!(all.data.chunks_exact(4).any(|pixel| pixel == color));
            }
            let gap = ((grid.cell_width * scale * 1.5).round() as u32 * 4) as usize;
            assert_eq!(&all.data[gap..gap + 4], &[0, 0, 0, 0]);
            // Grabbing the right cell keeps its exact offset within the group.
            let grabbed = POINT {
                x: right.x + 15,
                y: right.y + 20,
            };
            let hotspot = POINT {
                x: grabbed.x - all_origin.x,
                y: grabbed.y - all_origin.y,
            };
            assert_eq!(grabbed.x - hotspot.x + right.x - all_origin.x, right.x);
            assert_eq!(grabbed.y - hotspot.y + right.y - all_origin.y, right.y);
            assert!(
                all.data
                    .chunks_exact(4)
                    .all(|p| p[..3].iter().all(|v| *v <= p[3]))
            );
        }
        assert!(selection_pixels(&[]).is_none());
    }

    #[test]
    fn scaling_preserves_transparent_pixels_and_premultiplied_edges() {
        let source = Pixels {
            width: 2,
            height: 1,
            data: vec![0, 0, 0, 0, 60, 120, 240, 255],
        };
        let output = resize(&source, 4, 2);
        assert_eq!(&output[..4], &[0, 0, 0, 0]);
        assert_eq!(&output[12..16], &[60, 120, 240, 255]);
        assert_eq!(&output[4..8], &[15, 30, 60, 64]);
        assert_eq!(&output[..16], &output[16..]);
    }
}
