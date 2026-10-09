use luciddesk_core::ShellIdentity;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ, HPALETTE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapUsePremultipliedAlpha,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::{Interface, PCWSTR};

#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Windows Shell parsing name for the virtual Recycle Bin, not a filesystem path.
/// Its CLSID is fixed across Windows versions, display languages, and architectures.
pub(super) const RECYCLE_BIN_PARSING_NAME: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";

pub fn load(identity: &ShellIdentity, size: i32) -> windows::core::Result<Pixels> {
    let name = match identity {
        ShellIdentity::FileSystem { path, .. } => path.to_string_lossy().into_owned(),
        ShellIdentity::Namespace { parsing_name } => parsing_name.clone(),
    };
    if name.eq_ignore_ascii_case(RECYCLE_BIN_PARSING_NAME) {
        return recycle_icon(size);
    }
    let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)?;
        if item
            .GetAttributes(windows::Win32::System::SystemServices::SFGAO_LINK)
            .is_ok_and(|a| a.0 != 0)
            && let Ok(pixels) = link_icon(&item, size)
        {
            return Ok(pixels);
        }
        // Folder previews and executable thumbnails can contain a baked-in
        // frame or an opaque background. Use their actual Shell icon instead.
        let icon_only = identity.file_system_path().is_some_and(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        }) || item
            .GetAttributes(windows::Win32::System::SystemServices::SFGAO_FOLDER)
            .is_ok_and(|attributes| attributes.0 != 0);
        if icon_only {
            return shell_icon(&item, size);
        }
        let factory: IShellItemImageFactory = item.cast()?;
        let bitmap = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_BIGGERSIZEOK)?;
        let result = (|| {
            let imaging: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let image = imaging.CreateBitmapFromHBITMAP(
                bitmap,
                HPALETTE::default(),
                WICBitmapUsePremultipliedAlpha,
            )?;
            let (mut width, mut height) = (0, 0);
            image.GetSize(&raw mut width, &raw mut height)?;
            let mut data = vec![0; (width * height * 4) as usize];
            image.CopyPixels(std::ptr::null(), width * 4, &mut data)?;
            Ok(Pixels {
                width,
                height,
                data,
            })
        })();
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        result
    }
}

// The namespace image factory may keep returning an empty-bin bitmap even when
// a fresh Shell query reports items (observed with files recycled from C:).
// Query all drives on the existing notification worker and extract the explicit
// stock state at the requested resolution, bypassing that dynamic image cache.
fn recycle_icon(size: i32) -> windows::core::Result<Pixels> {
    use windows::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        SHQueryRecycleBinW(PCWSTR::null(), &raw mut info)?;
    }
    if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace) {
        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "pane.assets",
            "recycle-state items={} bytes={}",
            info.i64NumItems, info.i64Size
        );
    }
    // An empty file is still an item in the Recycle Bin.
    recycle_state_icon(info.i64NumItems > 0, size)
}

fn recycle_state_icon(full: bool, size: i32) -> windows::core::Result<Pixels> {
    use windows::Win32::UI::{
        Shell::{
            SHDefExtractIconW, SHGSI_ICONLOCATION, SHGetStockIconInfo, SHSTOCKICONINFO,
            SIID_RECYCLER, SIID_RECYCLERFULL,
        },
        WindowsAndMessaging::{DestroyIcon, HICON},
    };
    let mut info = SHSTOCKICONINFO {
        cbSize: size_of::<SHSTOCKICONINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        SHGetStockIconInfo(
            if full {
                SIID_RECYCLERFULL
            } else {
                SIID_RECYCLER
            },
            SHGSI_ICONLOCATION,
            &raw mut info,
        )?;
        let mut icon = HICON::default();
        let extracted = SHDefExtractIconW(
            PCWSTR(info.szPath.as_ptr()),
            info.iIcon,
            0,
            Some(&raw mut icon),
            None,
            size.clamp(1, 65535) as u32,
        )
        .ok();
        let result = extracted.and_then(|()| icon_pixels(icon));
        if !icon.is_invalid() {
            let _ = DestroyIcon(icon);
        }
        result
    }
}

// Request the shortcut's own base icon, without an overlay. This preserves custom
// shortcut icons while hiding the arrow in LucidDesk only.
#[allow(clippy::wildcard_imports)]
fn link_icon(item: &IShellItem, size: i32) -> windows::core::Result<Pixels> {
    if let Ok(pixels) = extract_link_icon(item, size) {
        return Ok(pixels);
    }
    shell_icon(item, size)
}

#[allow(clippy::wildcard_imports)]
fn shell_icon(item: &IShellItem, size: i32) -> windows::core::Result<Pixels> {
    use windows::Win32::UI::{Controls::IImageList, Shell::*, WindowsAndMessaging::DestroyIcon};
    unsafe {
        let pidl = SHGetIDListFromObject(item)?;
        let mut info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR(pidl.cast()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&raw mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_PIDL | SHGFI_SYSICONINDEX,
        );
        windows::Win32::System::Com::CoTaskMemFree(Some(pidl.cast()));
        if !info.hIcon.is_invalid() {
            let _ = DestroyIcon(info.hIcon);
        }
        if result == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let mut lists = Vec::new();
        for kind in [SHIL_SMALL, SHIL_LARGE, SHIL_EXTRALARGE, SHIL_JUMBO] {
            if let Ok(list) = SHGetImageList::<IImageList>(kind.cast_signed()) {
                let (mut w, mut h) = (0, 0);
                if list.GetIconSize(&raw mut w, &raw mut h).is_ok() {
                    lists.push((w, list));
                }
            }
        }
        lists.sort_by_key(|(w, _)| {
            if *w >= size {
                (0, *w - size)
            } else {
                (1, size - *w)
            }
        });
        for (_, list) in lists {
            let Ok(icon) = list.GetIcon(info.iIcon, 0) else {
                continue;
            };
            let pixels = icon_pixels(icon);
            let _ = DestroyIcon(icon);
            if let Ok(pixels) = pixels
                && pixels.data.chunks_exact(4).any(|pixel| pixel[3] != 0)
                && !is_padded_jumbo(&pixels)
            {
                return Ok(pixels);
            }
        }
        Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ))
    }
}

// Shell can put a legacy icon in the upper-left of a jumbo slot instead of
// providing high-resolution artwork. Reject that slot and use the next list;
// do not crop normal centered artwork or its intentional transparent margins.
fn is_padded_jumbo(pixels: &Pixels) -> bool {
    if pixels.width < 256 || pixels.height < 256 {
        return false;
    }
    let mut visible = false;
    for (index, pixel) in pixels.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        visible = true;
        let x = index as u32 % pixels.width;
        let y = index as u32 / pixels.width;
        if x >= pixels.width / 2 || y >= pixels.height / 2 {
            return false;
        }
    }
    visible
}

#[cfg(test)]
mod padding_tests {
    use super::*;

    #[test]
    #[ignore = "Read-only diagnostic for the reported Downloads executable"]
    fn reported_executable_has_visible_icon() {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
        let pixels = load(&ShellIdentity::FileSystem {
            path: std::path::PathBuf::from(r"C:\Users\Yuchen\Downloads\Fences6_setup.exe"),
            volume_id: None, file_id: None,
        }, 128).unwrap();
        let visible = pixels.data.chunks_exact(4).filter(|p| p[3] != 0).count();
        eprintln!("Fences icon {}x{}, visible pixels {visible}", pixels.width, pixels.height);
        assert!(visible > 0);
    }

    #[test]
    #[ignore = "Read-only icon diagnostic; set LUCIDDESK_TEST_FOLDER and LUCIDDESK_ICON_OUTPUT"]
    fn downloads_icons_preserve_transparency_and_fill_the_canvas() {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
        let root = std::path::PathBuf::from(std::env::var_os("LUCIDDESK_TEST_FOLDER").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("LUCIDDESK_ICON_OUTPUT").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        for name in [
            "openfences.exe",
            "wireguard-installer.exe",
            "VC_redist.x64.exe",
            "ArmouryCrateInstallTool",
        ] {
            let pixels = load(
                &ShellIdentity::FileSystem {
                    path: root.join(name),
                    volume_id: None,
                    file_id: None,
                },
                128,
            )
            .unwrap();
            eprintln!(
                "{name}: transparent pixels {}",
                pixels
                    .data
                    .chunks_exact(4)
                    .filter(|pixel| pixel[3] == 0)
                    .count()
            );
            if name != "openfences.exe" {
                assert!(pixels.data.chunks_exact(4).any(|pixel| pixel[3] == 0));
            }
            assert!(!is_padded_jumbo(&pixels), "{name}: padded jumbo icon");
            let mut bytes = pixels.width.to_le_bytes().to_vec();
            bytes.extend(pixels.height.to_le_bytes());
            bytes.extend(&pixels.data);
            std::fs::write(output.join(format!("{name}.bgra")), bytes).unwrap();
            eprintln!("{name}: {}x{}", pixels.width, pixels.height);
        }
    }

    #[test]
    fn recycle_states_have_distinct_high_resolution_pixels() {
        let _sta = luciddesk_shell::ShellApartment::initialize_sta().unwrap();
        let empty = recycle_state_icon(false, 128).unwrap();
        let full = recycle_state_icon(true, 128).unwrap();
        assert_eq!((empty.width, empty.height), (128, 128));
        assert_eq!((full.width, full.height), (128, 128));
        assert_ne!(empty.data, full.data);
        assert!(empty.data.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(full.data.chunks_exact(4).any(|pixel| pixel[3] == 255));
    }

    fn image(size: u32, start: u32, end: u32) -> Pixels {
        let mut pixels = Pixels {
            width: size,
            height: size,
            data: vec![0; (size * size * 4) as usize],
        };
        for y in start..end {
            for x in start..end {
                pixels.data[((y * size + x) * 4 + 3) as usize] = 255;
            }
        }
        pixels
    }

    #[test]
    fn rejects_legacy_icon_in_jumbo_slot_without_cropping_centered_artwork() {
        assert!(is_padded_jumbo(&image(256, 2, 48)));
        assert!(!is_padded_jumbo(&image(256, 8, 248)));
        assert!(!is_padded_jumbo(&image(256, 104, 152)));
        assert!(!is_padded_jumbo(&image(48, 2, 46)));
        assert!(!is_padded_jumbo(&image(256, 0, 0)));
    }
}

#[cfg(test)]
pub const UI_FONT: &str = "Microsoft YaHei UI";

pub fn use_ui_font(font: &mut windows_sys::Win32::Graphics::Gdi::LOGFONTW) {
    font.lfFaceName.fill(0);
    for (out, unit) in font.lfFaceName.iter_mut().zip(super::fonts::family().encode_utf16()) {
        *out = unit;
    }
}

static FONT_SIZE: std::sync::Mutex<Option<f32>> = std::sync::Mutex::new(None);

pub(super) fn invalidate_font() {
    *FONT_SIZE.lock().unwrap() = None;
}

pub fn font() -> (String, f32) {
    let size = *FONT_SIZE.lock().unwrap().get_or_insert_with(system_font_size);
    (super::fonts::family(), size)
}

fn system_font_size() -> f32 {
    use windows_sys::Win32::Graphics::Gdi::LOGFONTW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETICONTITLELOGFONT, SystemParametersInfoW,
    };
    let mut font = LOGFONTW::default();
    if unsafe {
        SystemParametersInfoW(
            SPI_GETICONTITLELOGFONT,
            size_of::<LOGFONTW>() as u32,
            (&raw mut font).cast(),
            0,
        )
    } != 0
    {
        let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() }.max(96);
        #[allow(clippy::cast_precision_loss)]
        let size = (font.lfHeight.unsigned_abs() as f32 * 96.0 / dpi as f32).max(11.0);
        size
    } else {
        12.0
    }
}

// Ask the shortcut's icon handler for the requested size before falling back to
// fixed system-image-list sizes. No shortcut overlay is added by this path.
fn extract_link_icon(item: &IShellItem, size: i32) -> windows::core::Result<Pixels> {
    use windows::Win32::UI::{
        Shell::*,
        WindowsAndMessaging::{DestroyIcon, HICON},
    };
    unsafe {
        let extractor: IExtractIconW = item.BindToHandler(None, &BHID_SFUIObject)?;
        let mut path = [0u16; 32768];
        let (mut index, mut flags) = (0, 0);
        extractor.GetIconLocation(0, &mut path, &raw mut index, &raw mut flags)?;
        let mut icon = HICON::default();
        let result = extractor.Extract(
            PCWSTR(path.as_ptr()),
            index as u32,
            Some(&raw mut icon),
            None,
            size as u32,
        );
        if result.is_err() || icon.is_invalid() {
            if !icon.is_invalid() {
                let _ = DestroyIcon(icon);
                icon = HICON::default();
            }
            if flags & GIL_NOTFILENAME == 0 {
                SHDefExtractIconW(
                    PCWSTR(path.as_ptr()),
                    index,
                    0,
                    Some(&raw mut icon),
                    None,
                    size as u32,
                )
                .ok()?;
            }
        }
        if icon.is_invalid() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        let pixels = icon_pixels(icon);
        let _ = DestroyIcon(icon);
        pixels
    }
}

pub(super) fn icon_pixels(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> windows::core::Result<Pixels> {
    use windows::Win32::Graphics::Imaging::*;
    unsafe {
        let imaging: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bitmap = imaging.CreateBitmapFromHICON(icon)?;
        let converter = imaging.CreateFormatConverter()?;
        converter.Initialize(
            &bitmap,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut width, mut height) = (0, 0);
        converter.GetSize(&raw mut width, &raw mut height)?;
        let mut data = vec![0; (width * height * 4) as usize];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut data)?;
        Ok(Pixels {
            width,
            height,
            data,
        })
    }
}

/// Resample once in premultiplied BGRA; cache the resulting physical-pixel bitmap.
pub fn resample(source: &Pixels, width: u32, height: u32) -> windows::core::Result<std::borrow::Cow<'_, Pixels>> {
    use windows::Win32::Graphics::Imaging::*;
    if (source.width, source.height) == (width, height) {
        return Ok(std::borrow::Cow::Borrowed(source));
    }
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let image = factory.CreateBitmapFromMemory(
            source.width,
            source.height,
            &GUID_WICPixelFormat32bppPBGRA,
            source.width * 4,
            &source.data,
        )?;
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(
            &image,
            width,
            height,
            if width < source.width || height < source.height {
                WICBitmapInterpolationModeFant
            } else {
                WICBitmapInterpolationModeHighQualityCubic
            },
        )?;
        let mut data = vec![0; (width * height * 4) as usize];
        scaler.CopyPixels(std::ptr::null(), width * 4, &mut data)?;
        // Cubic kernels can overshoot. Enforce the premultiplied contract at edges.
        for p in data.chunks_exact_mut(4) {
            for c in 0..3 {
                p[c] = p[c].min(p[3]);
            }
        }
        Ok(std::borrow::Cow::Owned(Pixels {
            width,
            height,
            data,
        }))
    }
}
