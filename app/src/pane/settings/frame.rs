//! Settings caption, resize hit testing and toggle geometry.
use super::*;

pub(super) fn toggle_thumb(bounds: Rect, progress: f32) -> Ellipse {
    let height = bounds.bottom - bounds.top;
    let radius = (height / 2.0 - 4.0).max(1.0);
    let left = bounds.left + height / 2.0;
    let right = bounds.right - height / 2.0;
    Ellipse {
        center: Vector2 {
            x: left + (right - left) * progress.clamp(0.0, 1.0),
            y: (bounds.top + bounds.bottom) / 2.0,
        },
        radius_x: radius,
        radius_y: radius,
    }
}

pub(super) const TITLE_HEIGHT: f32 = 40.0;
pub(super) fn frame_hit(x: f32, y: f32, width: f32, height: f32, maximized: bool) -> u32 {
    if !maximized {
        let (left, right, top, bottom) = (x < 6.0, x >= width - 6.0, y < 6.0, y >= height - 6.0);
        match (left, right, top, bottom) {
            (true, _, true, _) => return HTTOPLEFT,
            (_, true, true, _) => return HTTOPRIGHT,
            (true, _, _, true) => return HTBOTTOMLEFT,
            (_, true, _, true) => return HTBOTTOMRIGHT,
            (true, _, _, _) => return HTLEFT,
            (_, true, _, _) => return HTRIGHT,
            (_, _, true, _) => return HTTOP,
            (_, _, _, true) => return HTBOTTOM,
            _ => {}
        }
    }
    if y < TITLE_HEIGHT && x < width - 138.0 {
        HTCAPTION
    } else {
        HTCLIENT
    }
}
pub(super) fn with_titlebar(mut scene: Scene, width: f32, maximized: bool) -> Scene {
    if let Some(viewport) = &mut scene.navigation_viewport {
        viewport.top += TITLE_HEIGHT;
        viewport.bottom += TITLE_HEIGHT;
    }
    if let Some(r) = &mut scene.app_icon {
        r.top += TITLE_HEIGHT;
        r.bottom += TITLE_HEIGHT;
    }
    for (r, _, _) in &mut scene.text {
        r.top += TITLE_HEIGHT;
        r.bottom += TITLE_HEIGHT;
    }
    for r in scene.cards.iter_mut().chain(&mut scene.separators) {
        r.top += TITLE_HEIGHT;
        r.bottom += TITLE_HEIGHT;
    }
    for (r, _) in &mut scene.previews {
        r.top += TITLE_HEIGHT;
        r.bottom += TITLE_HEIGHT;
    }
    for c in &mut scene.controls {
        c.bounds.top += TITLE_HEIGHT;
        c.bounds.bottom += TITLE_HEIGHT;
    }
    scene.caption_icon = Some(Rect::from_xywh(28.0, (TITLE_HEIGHT - 18.0) * 0.5, 18.0, 18.0));
    scene.text(
        // Align with the sidebar brand and keep localized titles clear of caption buttons.
        Rect::from_xywh(56.0, 0.0, (width - 56.0 - 138.0 - 16.0).max(1.0), TITLE_HEIGHT),
        crate::i18n::text("ui-luciddesk-settings"),
        Tokens::CAPTION_TEXT,
    );
    for (i, (glyph, command)) in [
        (crate::i18n::text("ui-minimize"), SC_MINIMIZE),
        (
            if maximized {
                crate::i18n::text("ui-restore")
            } else {
                crate::i18n::text("ui-maximize")
            },
            if maximized { SC_RESTORE } else { SC_MAXIMIZE },
        ),
        (crate::i18n::text("ui-close"), SC_CLOSE),
    ]
    .iter()
    .enumerate()
    {
        scene.control(
            ControlKind::Caption,
            Rect::from_xywh(width - 138.0 + i as f32 * 46.0, 0.0, 46.0, TITLE_HEIGHT),
            glyph,
            Action::Window(*command),
            false,
        );
    }
    scene
}

// Windows may calculate the frame synchronously while the main handler is
// detached. Keep the client-area decision independent of application state.
pub(super) unsafe extern "system" fn frame_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: usize,
    lp: isize,
    id: usize,
    _: usize,
) -> isize {
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    // Destruction may be nested inside WM_CLOSE while windows-window has
    // detached its callback. Closing settings must never post WM_QUIT.
    if msg == WM_DESTROY {
        return 0;
    }
    if msg == WM_NCCALCSIZE || msg == WM_NCPAINT {
        return 0;
    }
    if msg == WM_NCACTIVATE {
        return 1;
    }
    if msg == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(hwnd, Some(frame_proc), id);
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}
