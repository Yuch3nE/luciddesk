//! Interface discovery by default. --show-first temporarily selects the first desktop item,
//! opens its system menu, then restores selection. Cancel the popup without choosing a command.
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, IContextMenuSite, IShellBrowser, IShellWindows, SID_STopLevelBrowser,
    SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::core::Interface;
#[path = "../src/native_menu/lifetime.rs"]
mod menu_lifetime;
#[path = "../src/native_menu/selection.rs"]
mod menu_selection;
#[path = "support/menu_test_window.rs"]
mod menu_test_window;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _apartment = luciddesk_shell::ShellApartment::initialize_sta()?;
    unsafe {
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut desktop_hwnd = 0;
        let dispatch = shell.FindWindowSW(
            &VARIANT::from(CSIDL_DESKTOP.cast_signed()),
            &VARIANT::default(),
            SWC_DESKTOP,
            &raw mut desktop_hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let provider: IServiceProvider = dispatch.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view = browser.QueryActiveShellView()?;
        if std::env::args().any(|arg| arg == "--benchmark-snapshot") {
            for sample in 0..3 {
                let start = std::time::Instant::now();
                let snapshot = luciddesk_shell::native_desktop_snapshot()?;
                println!("sample={sample} items={} snapshot_ms={:.3}", snapshot.items.len(), start.elapsed().as_secs_f64()*1000.0);
            }
            return Ok(());
        }
        if std::env::args().any(|arg| arg == "--benchmark-resolve") {
            use windows::Win32::UI::Shell::{IFolderView2, SVGIO_ALLVIEW, SIGDN_DESKTOPABSOLUTEPARSING};
            let folder: IFolderView2 = view.cast()?;
            let count = folder.ItemCount(SVGIO_ALLVIEW)?;
            for expected in [0, count / 2, count - 1].into_iter().filter(|i| *i >= 0 && *i < count) {
                let item = menu_selection::item_at(&folder, expected)?;
                let name = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
                let value = name.to_string();
                windows::Win32::System::Com::CoTaskMemFree(Some(name.0.cast()));
                let value = value?;
                let start = std::time::Instant::now();
                assert_eq!(menu_selection::find(&folder, &item)?, Some(expected));
                let scan = start.elapsed();
                menu_selection::update_hints(std::iter::once((value.clone(), expected)));
                let start = std::time::Instant::now();
                assert_eq!(menu_selection::resolve(&folder, &value)?, expected);
                println!("index={expected}/{count} full_scan_ms={:.3} validated_hint_ms={:.3}", scan.as_secs_f64()*1000.0, start.elapsed().as_secs_f64()*1000.0);
                menu_selection::update_hints(std::iter::once((value.clone(), (expected + 1) % count)));
                assert_eq!(menu_selection::resolve(&folder, &value)?, expected, "Stale hint must fall back to exact Shell identity");
            }
            println!("PASS: live first/middle/last identities and stale-index fallback; no selection or menu changes");
            return Ok(());
        }
        if std::env::args().any(|arg| arg == "--refresh-view") {
            println!("desktop_view_refresh={:?}", view.Refresh());
        }
        if let Some(name) = target_argument()? {
            let folder = view.cast()?;
            if std::env::args().any(|arg| arg == "--inspect-target") {
                inspect_target(&folder, &name)?;
                return Ok(());
            }
            let index = menu_selection::resolve(&folder, &name)?;
            println!("resolved_desktop_target_index={index}");
            if std::env::args().any(|arg| arg == "--resolve-only") {
                return Ok(());
            }
            if std::env::args().any(|arg| arg == "--selection-roundtrip") {
                use windows::Win32::UI::Shell::{SVSI_DESELECTOTHERS, SVSI_FOCUSED, SVSI_SELECT};
                let restore = menu_selection::RestoreSelection::capture(&folder)?;
                folder.SelectItem(
                    index,
                    (SVSI_SELECT.0 | SVSI_FOCUSED.0 | SVSI_DESELECTOTHERS.0).cast_unsigned(),
                )?;
                restore.finish()?;
                return Ok(());
            }
        }
        if std::env::args().any(|arg| arg == "--window") {
            menu_test_window::run(&view)?;
            return Ok(());
        }
        if std::env::args().any(|arg| arg == "--show-first") {
            show_at(&view, windows::Win32::Foundation::POINT { x: 600, y: 450 })?;
            return Ok(());
        }
        println!("desktop_shell_view_available=true");
        println!(
            "desktop_view_IContextMenuSite={:?}",
            view.cast::<IContextMenuSite>().map(|_| ())
        );
        println!(
            "desktop_browser_IContextMenuSite={:?}",
            browser.cast::<IContextMenuSite>().map(|_| ())
        );
        println!(
            "desktop_view_IServiceProvider={:?}",
            view.cast::<IServiceProvider>().map(|_| ())
        );
        for (name, source) in [
            ("browser", browser.cast::<IServiceProvider>()),
            ("view", view.cast::<IServiceProvider>()),
        ] {
            if let Ok(source) = source {
                let service =
                    windows::core::GUID::from_u128(0xb306c5b1_b4f2_473c_b6ff_701b246ce2d2);
                let iid = windows::core::GUID::from_u128(0x37a472f7_63cf_4ccf_a88b_5231a3c7d8b6);
                let mut raw = std::ptr::null_mut();
                let result = (source.vtable().QueryService)(
                    source.as_raw(),
                    &raw const service,
                    &raw const iid,
                    &raw mut raw,
                );
                println!("desktop_{name}_presenter_service={result:?}");
                if result.is_ok() && !raw.is_null() {
                    drop(windows::core::IUnknown::from_raw(raw));
                }
            }
        }
    }
    Ok(())
}

fn inspect_target(
    folder: &windows::Win32::UI::Shell::IFolderView2,
    name: &str,
) -> windows::core::Result<()> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SIGDN_DESKTOPABSOLUTEPARSING, SVGIO_ALLVIEW};
    unsafe {
        println!("desktop_view_count={}", folder.ItemCount(SVGIO_ALLVIEW)?);
        let expected = std::path::Path::new(name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let mut matches = 0;
        for index in 0..folder.ItemCount(SVGIO_ALLVIEW)? {
            let item = menu_selection::item_at(folder, index)?;
            let raw = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)?;
            let parsing = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            if parsing?.contains(expected.as_ref()) {
                println!("target_name_present_at_index={index}");
                matches += 1;
            }
        }
        println!("target_name_matches={matches}");
    }
    Ok(())
}

fn show_at(
    view: &windows::Win32::UI::Shell::IShellView,
    point: windows::Win32::Foundation::POINT,
) -> windows::core::Result<()> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        CMF_ITEMMENU, IContextMenu, IFolderView2, SIGDN_NORMALDISPLAY, SVGIO_SELECTION,
        SVSI_DESELECTOTHERS, SVSI_FOCUSED, SVSI_SELECT,
    };
    if std::env::args().any(|arg| arg == "--bridge") {
        let parsing_name = target_argument()?.ok_or_else(|| {
            windows::core::Error::new(
                windows::Win32::Foundation::E_INVALIDARG,
                "--bridge requires --target",
            )
        })?;
        return luciddesk_shell::show_desktop_item_menu(
            windows::Win32::Foundation::HWND::default(),
            &luciddesk_shell::ShellIdentity::Namespace { parsing_name },
            point,
            luciddesk_shell::MenuInvocation::Mouse,
        );
    }
    unsafe {
        let folder: IFolderView2 = view.cast()?;
        let shell_hwnd = view.GetWindow()?.0;
        let list = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW(
            shell_hwnd,
            std::ptr::null_mut(),
            windows_sys::w!("SysListView32"),
            std::ptr::null(),
        );
        println!(
            "menu_host_visible={}, native_icon_list_visible={}",
            windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(shell_hwnd),
            windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(list)
        );
        let mut shell_pid = 0;
        let shell_thread = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            view.GetWindow()?.0,
            &raw mut shell_pid,
        );
        let observer = menu_lifetime::Observer::new(shell_pid, shell_thread)?;
        let index = match target_argument()? {
            Some(name) => menu_selection::resolve(&folder, &name)?,
            None => 0,
        };
        let item = menu_selection::item_at(&folder, index)?;
        let restore = menu_selection::RestoreSelection::capture(&folder)?;
        let name = item.GetDisplayName(SIGDN_NORMALDISPLAY)?;
        println!("menu_target={}", name.to_string()?);
        CoTaskMemFree(Some(name.0.cast()));
        folder.SelectItem(
            index,
            (SVSI_SELECT.0 | SVSI_FOCUSED.0 | SVSI_DESELECTOTHERS.0).cast_unsigned(),
        )?;
        let menu: IContextMenu = view.GetItemObject(SVGIO_SELECTION)?;
        let site: IContextMenuSite = view.cast()?;
        println!("calling_Explorer_menu_site=true (cancel without executing a command)");
        let flags = CMF_ITEMMENU
            | if std::env::args().any(|arg| arg == "--can-rename") {
                windows::Win32::UI::Shell::CMF_CANRENAME
            } else {
                0
            };
        let mut result = site.DoContextMenuPopup(&menu, flags, point);
        println!("Explorer_menu_site_returned={result:?}");
        if result.is_ok() && std::env::args().any(|arg| arg == "--hold") {
            result = observer.wait_for_close(None);
        }
        let restored = restore.finish();
        println!("desktop_selection_restored={restored:?}");
        result.and(restored)
    }
}

fn target_argument() -> windows::core::Result<Option<String>> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == "--target" {
            return args
                .next()
                .filter(|name| !name.starts_with("--"))
                .map(Some)
                .ok_or_else(|| {
                    windows::core::Error::new(
                        windows::Win32::Foundation::E_INVALIDARG,
                        "--target requires a Shell parsing name or absolute path",
                    )
                });
        }
    }
    Ok(None)
}
