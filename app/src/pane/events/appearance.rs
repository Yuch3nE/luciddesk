//! Appearance gestures and atomic option saves with rollback.
use super::*;

/// Preview only; the settings gesture commits its final value separately.
pub(in crate::pane) fn preview_grid(state: &mut PaneApp, value: f32) {
    if !value.is_finite() {
        return;
    }
    let range = luciddesk_core::PaneOptions::GRID_SCALE_RANGE;
    let value = value.clamp(range.0, range.1);
    let old = state.workspace.pane_options();
    let mut options = old;
    options.grid_scale = value;
    if options == state.workspace.pane_options() {
        return;
    }
    state.workspace.set_pane_options(options);
    for view in &state.views {
        let mut model = view.model.borrow_mut();
        model.apply_options(options, old, model::GridScroll::PreserveHorizontal);
        drop(model);
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
}

pub(in crate::pane) fn commit_grid(state: &mut PaneApp, original: f32) -> Result<(), String> {
    let options = state.workspace.pane_options();
    if options.grid_scale == original {
        return Ok(());
    }
    if let Err(error) = state.store.save_pane_options(options) {
        preview_grid(state, original);
        return Err(error.to_string());
    }
    Ok(())
}

pub(in crate::pane) fn preview_radius(state: &mut PaneApp, radius: f32) {
    if !radius.is_finite() {
        return;
    }
    let radius = radius.clamp(0.0, luciddesk_core::PaneOptions::MAX_CORNER_RADIUS);
    let mut options = state.workspace.pane_options();
    if options.corner_radius == radius {
        return;
    }
    options.corner_radius = radius;
    state.workspace.set_pane_options(options);
    for view in &state.views {
        view.model.borrow_mut().options = options;
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
}

pub(in crate::pane) fn commit_radius(state: &mut PaneApp, original: f32) -> Result<(), String> {
    let options = state.workspace.pane_options();
    if options.corner_radius == original {
        return Ok(());
    }
    if let Err(error) = state.store.save_pane_options(options) {
        preview_radius(state, original);
        return Err(error.to_string());
    }
    Ok(())
}

/// Update only memory and visuals during a continuous material gesture.
pub(in crate::pane) fn preview_material(state: &mut PaneApp, backdrop: luciddesk_core::Backdrop) {
    if state
        .workspace
        .appearance()
        .is_some_and(|(_, old)| old == backdrop)
    {
        return;
    }
    let theme = state
        .workspace
        .appearance()
        .map_or(luciddesk_core::PanelTheme::System, |v| v.0);
    state.workspace.set_appearance(theme, backdrop);
    for view in &state.views {
        view.model.borrow_mut().backdrop = backdrop;
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    if let Some(window) = &state.settings {
        unsafe {
            InvalidateRect(window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
}

pub(in crate::pane) fn commit_material(
    state: &mut PaneApp,
    original: luciddesk_core::Backdrop,
) -> Result<(), String> {
    if let Err(error) = save(state) {
        preview_material(state, original);
        return Err(error);
    }
    Ok(())
}

pub(super) fn options(state: &Rc<RefCell<PaneApp>>, event: Event) -> Result<bool, String> {
    let mut s = state.borrow_mut();
    let old = s.workspace.pane_options();
    let mut options = old;
    match event {
        Event::SetCornerRadius(radius) => {
            if !radius.is_finite() {
                return Ok(false);
            }
            options.corner_radius =
                radius.clamp(0.0, luciddesk_core::PaneOptions::MAX_CORNER_RADIUS)
        }
        Event::SetIconGrid(value) => {
            if !value.is_finite() {
                return Ok(false);
            }
            let range = luciddesk_core::PaneOptions::GRID_SCALE_RANGE;
            let value = value.round().clamp(range.0, range.1);
            options.grid_scale = value;
        }
        Event::ToggleBorder => options.border = !options.border,
        Event::SetPanelText(text) => options.text = text,
        Event::ToggleTextProtection => options.text_protection = !options.text_protection,
        Event::ToggleSnap => options.snap = !options.snap,
        Event::ResetPaneOptions => options = luciddesk_core::PaneOptions::default(),
        _ => unreachable!(),
    }
    s.workspace.set_pane_options(options);
    if options == old {
        return Ok(false);
    }
    if let Err(error) = s
        .store
        .save_pane_options(options)
        .map_err(|e| e.to_string())
    {
        s.workspace.set_pane_options(old);
        return Err(error);
    }
    for view in &s.views {
        let mut model = view.model.borrow_mut();
        model.apply_options(options, old, model::GridScroll::PreserveHorizontal);
        drop(model);
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    if let Some(window) = &s.settings {
        unsafe {
            InvalidateRect(window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    return Ok(false);
}
pub(super) fn material(state: &Rc<RefCell<PaneApp>>, event: Event) -> Result<bool, String> {
    let mut s = state.borrow_mut();
    let old = s.workspace.clone();
    let (mut theme, mut backdrop) = s
        .workspace
        .appearance()
        .or_else(|| {
            s.workspace
                .panels()
                .first()
                .map(|p| (p.theme(), p.backdrop()))
        })
        .unwrap_or((
            luciddesk_core::PanelTheme::System,
            luciddesk_core::Backdrop::Mica,
        ));
    match event {
        Event::Theme(value) => theme = value,
        Event::Material(value) => backdrop = value,
        _ => unreachable!(),
    }
    s.workspace.set_appearance(theme, backdrop);
    if let Err(error) = save(&mut s) {
        s.workspace = old;
        return Err(error);
    }
    for view in &s.views {
        {
            let mut model = view.model.borrow_mut();
            model.theme = theme;
            model.dark = self::theme::is_dark(theme);
            model.backdrop = backdrop;
        }
        unsafe {
            InvalidateRect(view.window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    if let Some(window) = &s.settings {
        unsafe {
            InvalidateRect(window.hwnd().cast(), std::ptr::null(), 0);
        }
    }
    return Ok(false);
}
