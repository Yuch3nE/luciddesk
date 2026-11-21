//! Settings caption and resize hit testing.
use super::*;

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
pub(super) fn with_titlebar(scene: Scene, width: f32, maximized: bool) -> Scene {
    with_named_titlebar(scene, width, maximized, crate::i18n::text("ui-luciddesk-settings"), Tokens::NAV_LEADING)
}

pub(super) fn with_named_titlebar(
    mut scene: Scene, width: f32, maximized: bool, title: &str, icon_left: f32,
) -> Scene {
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
    for r in scene.cards.iter_mut().chain(&mut scene.separators)
        .chain(scene.section_headers.iter_mut().map(|header| &mut header.bounds))
        .chain(scene.destination_changes.iter_mut().map(|change| &mut change.bounds))
        .chain(scene.rule_rows.iter_mut().map(|row| &mut row.bounds))
        .chain(scene.disclosure_clips.iter_mut().map(|clip| &mut clip.bounds))
        .chain(scene.activity.iter_mut())
    {
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
    scene.caption_icon = Some(Rect::from_xywh(icon_left, (TITLE_HEIGHT - 18.0) * 0.5, 18.0, 18.0));
    let title_left = icon_left + 28.0;
    scene.text(
        // Keep localized titles clear of the caption buttons.
        Rect::from_xywh(title_left, 0.0, (width - title_left - 138.0 - 16.0).max(1.0), TITLE_HEIGHT),
        title,
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
    if msg == WM_CTLCOLOREDIT
        && let Some(brush) = classification::input_colors(wp as HDC, lp as _)
    {
        return brush;
    }
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
