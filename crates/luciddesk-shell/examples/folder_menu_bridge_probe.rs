//! Isolated exploration: pass a non-desktop selection to Explorer's menu site.
//! Default is read-only. --show opens a menu; cancel without executing commands.
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        System::{
            Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider},
            Variant::VARIANT,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{HSTRING, Interface},
};
#[path = "../src/native_menu/legacy_input.rs"]
mod input;
#[path = "../src/native_menu/lifetime.rs"]
mod lifetime;
#[path = "../src/native_menu/selection.rs"]
mod selection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _sta = luciddesk_shell::ShellApartment::initialize_sta()?;
    let path = std::env::args().nth(1).expect("absolute file path");
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        let array = SHCreateShellItemArrayFromShellItem::<_, IShellItemArray>(&item)?;
        let context: IContextMenu = array.BindToHandler(None, &BHID_SFUIObject)?;
        let menu = CreatePopupMenu()?;
        context
            .QueryContextMenu(menu, 0, 1, 0x7fff, CMF_NORMAL | CMF_CANRENAME)
            .ok()?;
        println!("external_context_commands={}", GetMenuItemCount(Some(menu)));
        DestroyMenu(menu)?;
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        if std::env::args().any(|arg| arg == "--folder-view") {
            let parent = item.GetParent()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let expected = parent.to_string()?;
            windows::Win32::System::Com::CoTaskMemFree(Some(parent.0.cast()));
            for i in 0..shell.Count()? {
                let Ok(dispatch) = shell.Item(&VARIANT::from(i)) else {
                    continue;
                };
                let Ok(provider) = dispatch.cast::<IServiceProvider>() else {
                    continue;
                };
                let Ok(browser) = provider.QueryService::<IShellBrowser>(&SID_STopLevelBrowser)
                else {
                    continue;
                };
                let Ok(view) = browser.QueryActiveShellView() else {
                    continue;
                };
                let Ok(folder) = view.cast::<IFolderView2>() else {
                    continue;
                };
                let Ok(current) = folder.GetFolder::<IShellItem>() else {
                    continue;
                };
                let Ok(raw_name) = current.GetDisplayName(SIGDN_FILESYSPATH) else {
                    continue;
                };
                let name = raw_name.to_string();
                windows::Win32::System::Com::CoTaskMemFree(Some(raw_name.0.cast()));
                if !name?.eq_ignore_ascii_case(&expected) {
                    continue;
                }
                println!(
                    "matching_folder_view=true; menu_site={}",
                    view.cast::<IContextMenuSite>().is_ok()
                );
                let raw_name = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
                let name = raw_name.to_string()?;
                windows::Win32::System::Com::CoTaskMemFree(Some(raw_name.0.cast()));
                let index = selection::resolve(&folder, &name)?;
                println!("target_resolved={index}");
                if std::env::args().any(|arg| arg == "--show") {
                    let restore = selection::RestoreSelection::capture(&folder)?;
                    folder.SelectItem(
                        index,
                        (SVSI_SELECT.0 | SVSI_FOCUSED.0 | SVSI_DESELECTOTHERS.0) as u32,
                    )?;
                    let hwnd = view.GetWindow()?;
                    let mut process = 0;
                    let thread = GetWindowThreadProcessId(hwnd, Some(&raw mut process));
                    let observer = lifetime::Observer::new(process, thread)?;
                    println!(
                        "foreground={:?}",
                        SetForegroundWindow(GetAncestor(hwnd, GA_ROOT))
                    );
                    view.UIActivate(SVUIA_ACTIVATE_FOCUS.0 as u32)?;
                    let shown = input::open_mouse(hwnd, POINT { x: 650, y: 450 })
                        .and_then(|()| observer.wait_for_close(None));
                    println!("folder_popup={shown:?}");
                    println!("selection_restored={:?}", restore.finish());
                }
                return Ok(());
            }
            println!("matching_folder_view=false");
            return Ok(());
        }
        let mut raw = 0;
        let dispatch = shell.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP as i32),
            &VARIANT::default(),
            SWC_DESKTOP,
            &raw mut raw,
            SWFO_NEEDDISPATCH,
        )?;
        let provider: IServiceProvider = dispatch.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view = browser.QueryActiveShellView()?;
        let site: IContextMenuSite = view.cast()?;
        println!("desktop_site_available=true; desktop_selection_unchanged=true");
        if std::env::args().any(|arg| arg == "--show") {
            let hwnd = view.GetWindow()?;
            let root = GetAncestor(hwnd, GA_ROOT);
            println!("foreground={:?}", SetForegroundWindow(root));
            view.UIActivate(SVUIA_ACTIVATE_FOCUS.0 as u32)?;
            println!(
                "external_popup={:?}",
                site.DoContextMenuPopup(
                    &context,
                    CMF_ITEMMENU | CMF_CANRENAME,
                    POINT { x: 650, y: 450 }
                )
            );
            let started = std::time::Instant::now();
            while started.elapsed().as_secs() < 20 {
                let mut message = MSG::default();
                while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = PostMessageW(
                Some(HWND(root.0)),
                WM_CANCELMODE,
                Default::default(),
                Default::default(),
            );
        }
    }
    Ok(())
}
