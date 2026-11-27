//! Diagnostic entry points that temporarily select items in the real desktop.
use super::*;
use luciddesk_core::ShellIdentity;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{
    CMF_CANRENAME, CMF_ITEMMENU, IContextMenu, IContextMenuSite, IFolderView2, IShellWindows,
    SVGIO_SELECTION, SVSI_DESELECTOTHERS, SVSI_FOCUSED, SVSI_SELECT, SVUIA_ACTIVATE_FOCUS,
    ShellWindows,
};
use windows::core::Interface;

/// Opens the real Explorer menu for a desktop item at physical screen coordinates.
/// Pumps UI messages until dismissal; callers must release model borrows first.
/// A missing desktop item is an error, never a request for a background menu.
///
/// # Errors
/// Returns an error when the Shell host, target, focus handoff or popup observation
/// is unavailable, another menu is active, or selection restoration fails.
pub fn show_desktop_item_menu(
    owner: HWND,
    identity: &ShellIdentity,
    point: POINT,
    invocation: MenuInvocation,
) -> Result<()> {
    show_desktop_items_menu(owner, std::slice::from_ref(identity), point, invocation)
}

/// Opens the Explorer menu for the complete desktop selection.
/// # Errors
/// Returns errors resolving any selected item or showing the Shell menu.
pub fn show_desktop_items_menu(
    owner: HWND,
    identities: &[ShellIdentity],
    point: POINT,
    invocation: MenuInvocation,
) -> Result<()> {
    if identities.is_empty() {
        return Ok(());
    }
    let _active = ActiveMenu::acquire()?;
    let mut timings = performance::Timings::new();
    unsafe {
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let view = crate::desktop::shell_view(&shell)?;
        let folder: IFolderView2 = view.cast()?;
        timings.mark("explorer-connected");
        let indices: Vec<_> = identities
            .iter()
            .map(|identity| {
                selection::resolve(&folder, identity.activation_name())
            })
            .collect::<Result<_>>()?;
        timings.mark("target-resolved");
        let site: IContextMenuSite = view.cast()?;
        let hwnd = view.GetWindow()?.0;
        let mut pid = 0;
        let thread = GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid == 0 || thread == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let observer = lifetime::Observer::new(pid, thread)?;
        timings.mark("observer-ready");
        let restore = if owner.0.is_null() {
            selection::RestoreSelection::capture(&folder)?
        } else {
            selection::RestoreSelection::deselect_on_close(&folder)
        };
        // Drop before RestoreSelection, including on error paths, so restoring
        // the user's selection does not briefly paint an active desktop highlight.
        let return_focus = ReturnFocus {
            owner,
            desktop: HWND(GetAncestor(hwnd, GA_ROOT)),
        };
        focus_trace("target-before", owner, return_focus.desktop, None);
        // Prepare the hidden menu target before activating the desktop, instead
        // of activating its old selection and immediately replacing it.
        for (at, index) in indices.into_iter().enumerate() {
            let flags = SVSI_SELECT.0
                | if at == 0 {
                    SVSI_FOCUSED.0 | SVSI_DESELECTOTHERS.0
                } else {
                    0
                };
            folder.SelectItem(index, flags.cast_unsigned())?;
        }
        timings.mark("target-selected");
        focus_trace("target-after", owner, return_focus.desktop, None);
        if AllowSetForegroundWindow(pid) == 0
            || SetForegroundWindow(GetAncestor(hwnd, GA_ROOT)) == 0
        {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "Explorer menu focus handoff failed",
            ));
        }
        view.UIActivate(SVUIA_ACTIVATE_FOCUS.0.cast_unsigned())?;
        timings.mark("focus-ready");
        focus_trace("menu-activated", owner, return_focus.desktop, None);
        timings.mark("context-ready");
        let result = match invocation {
            MenuInvocation::Mouse => legacy_input::open_mouse(HWND(hwnd), point),
            MenuInvocation::Keyboard => {
                let menu: IContextMenu = view.GetItemObject(SVGIO_SELECTION)?;
                site.DoContextMenuPopup(&menu, CMF_ITEMMENU | CMF_CANRENAME, point)
            }
        }
        .and_then(|()| observer.wait_for_close(None));
        if let Some(visible) = observer.first_visible() {
            timings.at("popup-first-observed", visible);
        }
        let desktop = return_focus.desktop;
        focus_trace("menu-closed", owner, desktop, None);
        drop(return_focus);
        let restored = restore.finish();
        focus_trace("selection-restored", owner, desktop, None);
        result.and(restored)
    }
}
