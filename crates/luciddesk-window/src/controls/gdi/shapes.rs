//! Antialiased shape rendering for native dialog controls.
use crate::control_style::ElevationGradient;
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::RECT,
    Graphics::{
        Gdi::HDC,
        GdiPlus::{
            FillModeWinding, GdipAddPathArc, GdipAddPathRectangle, GdipClosePathFigure,
            GdipCreateFromHDC, GdipCreateLineBrush, GdipCreatePath, GdipCreatePen1,
            GdipCreatePen2, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics,
            GdipDeletePath, GdipDeletePen, GdipDrawEllipse, GdipDrawPath, GdipFillEllipse,
            GdipFillPath, GdipSetLinePresetBlend, GdipSetPageUnit, GdipSetPixelOffsetMode,
            GdipSetSmoothingMode, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput,
            GpGraphics, GpPath, Ok as SUCCESS, PixelOffsetModeHalf, PointF,
            SmoothingModeAntiAlias, UnitPixel, WrapModeTileFlipXY,
        },
    },
};

pub(crate) struct Antialiasing(usize);

impl Antialiasing {
    pub(crate) fn start() -> Option<Self> {
        let mut token = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        (unsafe { GdiplusStartup(&raw mut token, &input, null_mut()) } == SUCCESS)
            .then(|| Self(token))
    }
}

impl Drop for Antialiasing {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.0);
        }
    }
}

fn argb([r, g, b, a]: [f32; 4]) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(a) << 24) | (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

pub(super) fn paint(dc: HDC, rect: RECT, radius: f32, rgb: u32, stroke: f32) -> bool {
    paint_argb(dc, rect, radius, 0xff00_0000 | rgb, stroke)
}

pub(super) fn paint_rgba(dc: HDC, rect: RECT, radius: f32, rgba: [f32; 4], stroke: f32) -> bool {
    paint_argb(dc, rect, radius, argb(rgba), stroke)
}

fn paint_argb(dc: HDC, rect: RECT, radius: f32, color: u32, stroke: f32) -> bool {
    let mut graphics = null_mut();
    if unsafe { GdipCreateFromHDC(dc, &raw mut graphics) } != SUCCESS {
        return false;
    }
    unsafe {
        let ready = GdipSetPageUnit(graphics, UnitPixel) == SUCCESS
            && GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias) == SUCCESS
            && GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf) == SUCCESS;
        let mut path = null_mut();
        let painted = if ready && GdipCreatePath(FillModeWinding, &raw mut path) == SUCCESS {
            let painted =
                rounded_path(path, rect, radius, stroke) && paint_path(graphics, path, color, stroke);
            GdipDeletePath(path);
            painted
        } else {
            false
        };
        GdipDeleteGraphics(graphics);
        painted
    }
}

pub(super) fn paint_elevation(
    dc: HDC,
    rect: RECT,
    radius: f32,
    stroke: f32,
    gradient: ElevationGradient,
) -> bool {
    if rect.right <= rect.left || rect.bottom <= rect.top || stroke <= 0.0 {
        return false;
    }
    let mut graphics = null_mut();
    if unsafe { GdipCreateFromHDC(dc, &raw mut graphics) } != SUCCESS {
        return false;
    }
    unsafe {
        let ready = GdipSetPageUnit(graphics, UnitPixel) == SUCCESS
            && GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias) == SUCCESS
            && GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf) == SUCCESS;
        let mut path = null_mut();
        let painted = if ready && GdipCreatePath(FillModeWinding, &raw mut path) == SUCCESS {
            let painted = rounded_path(path, rect, radius, stroke) && {
                let start = PointF {
                    X: rect.left as f32,
                    Y: rect.top as f32,
                };
                let end = PointF {
                    X: rect.left as f32,
                    Y: rect.bottom as f32,
                };
                let top = argb(gradient.top);
                let mid = argb(gradient.mid);
                let bottom = argb(gradient.bottom);
                let mut brush = null_mut();
                if GdipCreateLineBrush(
                    &raw const start,
                    &raw const end,
                    top,
                    bottom,
                    WrapModeTileFlipXY,
                    &raw mut brush,
                ) == SUCCESS
                {
                    let colors = [top, mid, bottom];
                    let positions = [0.0_f32, gradient.stop.clamp(0.01, 0.99), 1.0_f32];
                    let blend_ok = GdipSetLinePresetBlend(
                        brush,
                        colors.as_ptr(),
                        positions.as_ptr(),
                        3,
                    ) == SUCCESS;
                    let mut pen = null_mut();
                    let drawn = blend_ok
                        && GdipCreatePen2(brush.cast(), stroke, UnitPixel, &raw mut pen) == SUCCESS
                        && {
                            let ok = GdipDrawPath(graphics, pen, path) == SUCCESS;
                            GdipDeletePen(pen);
                            ok
                        };
                    GdipDeleteBrush(brush.cast());
                    drawn
                } else {
                    false
                }
            };
            GdipDeletePath(path);
            painted
        } else {
            false
        };
        GdipDeleteGraphics(graphics);
        painted
    }
}

pub(super) fn paint_circle(
    dc: HDC,
    cx: f32,
    cy: f32,
    radius: f32,
    rgba: [f32; 4],
    stroke: f32,
) -> bool {
    if radius <= 0.0 {
        return false;
    }
    let mut graphics = null_mut();
    if unsafe { GdipCreateFromHDC(dc, &raw mut graphics) } != SUCCESS {
        return false;
    }
    unsafe {
        let ready = GdipSetPageUnit(graphics, UnitPixel) == SUCCESS
            && GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias) == SUCCESS
            && GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf) == SUCCESS;
        let x = cx - radius;
        let y = cy - radius;
        let diameter = radius * 2.0;
        let color = argb(rgba);
        let painted = if !ready {
            false
        } else if stroke > 0.0 {
            let mut pen = null_mut();
            if GdipCreatePen1(color, stroke, UnitPixel, &raw mut pen) == SUCCESS {
                let ok = GdipDrawEllipse(graphics, pen, x, y, diameter, diameter) == SUCCESS;
                GdipDeletePen(pen);
                ok
            } else {
                false
            }
        } else {
            let mut brush = null_mut();
            if GdipCreateSolidFill(color, &raw mut brush) == SUCCESS {
                let ok = GdipFillEllipse(graphics, brush.cast(), x, y, diameter, diameter) == SUCCESS;
                GdipDeleteBrush(brush.cast());
                ok
            } else {
                false
            }
        };
        GdipDeleteGraphics(graphics);
        painted
    }
}

unsafe fn rounded_path(path: *mut GpPath, rect: RECT, radius: f32, stroke: f32) -> bool {
    // Draw the stroke inside the same outer bounds as the fill.
    let inset = stroke / 2.0;
    let left = rect.left as f32 + inset;
    let top = rect.top as f32 + inset;
    let width = (rect.right - rect.left) as f32 - stroke;
    let height = (rect.bottom - rect.top) as f32 - stroke;
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let diameter = (2.0 * (radius - inset)).clamp(0.0, width.min(height));
    unsafe {
        if diameter == 0.0 {
            return GdipAddPathRectangle(path, left, top, width, height) == SUCCESS;
        }
        let right = left + width - diameter;
        let bottom = top + height - diameter;
        [
            (left, top, 180.0),
            (right, top, 270.0),
            (right, bottom, 0.0),
            (left, bottom, 90.0),
        ]
        .into_iter()
        .all(|(x, y, start)| GdipAddPathArc(path, x, y, diameter, diameter, start, 90.0) == SUCCESS)
            && GdipClosePathFigure(path) == SUCCESS
    }
}

unsafe fn paint_path(
    graphics: *mut GpGraphics,
    path: *mut GpPath,
    color: u32,
    stroke: f32,
) -> bool {
    unsafe {
        if stroke > 0.0 {
            let mut pen = null_mut();
            if GdipCreatePen1(color, stroke, UnitPixel, &raw mut pen) != SUCCESS {
                return false;
            }
            let status = GdipDrawPath(graphics, pen, path);
            GdipDeletePen(pen);
            status == SUCCESS
        } else {
            let mut brush = null_mut();
            if GdipCreateSolidFill(color, &raw mut brush) != SUCCESS {
                return false;
            }
            let status = GdipFillPath(graphics, brush.cast(), path);
            GdipDeleteBrush(brush.cast());
            status == SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control_style::{Palette, colorref},
        controls::{Bounds, Canvas, Tone, gdi::GdiCanvas},
    };
    use std::mem::size_of;
    use windows_sys::Win32::Graphics::Gdi::*;

    #[test]
    fn native_button_fills_and_borders_blend_at_rounded_corners() {
        let _antialiasing = Antialiasing::start().expect("GDI+ must initialize");
        let surface = RECT {
            left: 0,
            top: 0,
            right: 220,
            bottom: 84,
        };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: surface.right,
                biHeight: -surface.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let mut pixels = null_mut();
            let bitmap =
                CreateDIBSection(dc, &info, DIB_RGB_COLORS, &raw mut pixels, null_mut(), 0);
            assert!(!bitmap.is_null());
            let old = SelectObject(dc, bitmap);
            for dark in [false, true] {
                let palette = Palette::for_theme(dark);
                let background = colorref(palette.page_background);
                let brush = CreateSolidBrush(background);
                for scale in [1.0, 1.25, 1.5, 2.0] {
                    let mut canvas = GdiCanvas {
                        dc,
                        scale,
                        palette: &palette,
                    };
                    let bounds = Bounds::new(4.0, 4.0, 96.0, 32.0);
                    let rect = canvas.rect(bounds);
                    let radius = canvas.px(6.0);
                    let blended_corner = |solid| {
                        (rect.top..rect.top + radius).any(|y| {
                            (rect.left..rect.left + radius).any(|x| {
                                let pixel = GetPixel(dc, x, y);
                                pixel != background && pixel != solid && pixel != u32::MAX
                            })
                        })
                    };
                    FillRect(dc, &surface, brush);
                    canvas.fill(bounds, 6.0, Tone::Accent);
                    assert!(blended_corner(colorref(palette.accent)), "fill at {scale}×");
                    FillRect(dc, &surface, brush);
                    canvas.stroke(bounds, 6.0, 1.0, Tone::Border);
                    assert!(
                        blended_corner(colorref(palette.border)),
                        "border at {scale}×"
                    );
                }
                let mut canvas = GdiCanvas {
                    dc,
                    scale: 1.0,
                    palette: &palette,
                };
                let bounds = Bounds::new(4.0, 4.0, 96.0, 32.0);
                FillRect(dc, &surface, brush);
                canvas.fill(bounds, 4.0, Tone::Surface);
                canvas.elevation_border(
                    bounds,
                    4.0,
                    crate::control_style::ElevationKind::Control,
                    0.0,
                    0.0,
                );
                let top_px = GetPixel(dc, 52, 4);
                let bottom_px = GetPixel(dc, 52, 35);
                assert_ne!(
                    top_px, bottom_px,
                    "elevation border should shade top and bottom edges differently (dark={dark})"
                );
                DeleteObject(brush);
            }
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
    }
}
