//! Source-view pixels carried into the native Shell drag loop.
use windows::{
    Win32::{
        Foundation::{COLORREF, E_INVALIDARG, POINT, SIZE},
        Graphics::Gdi::HBITMAP,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject},
        UI::Shell::{
            CLSID_DragDropHelper, DSH_ALLOWDROPDESCRIPTIONTEXT, IDragSourceHelper2, SHDRAGIMAGE,
        },
    },
    core::Result,
};

/// A transparent source-view preview in premultiplied BGRA, in physical pixels.
#[derive(Clone, Debug)]
pub struct FileDragImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub hotspot: POINT,
}

impl FileDragImage {
    pub(super) fn initialize(&self, data: &IDataObject) -> Result<IDragSourceHelper2> {
        use windows_sys::Win32::Graphics::Gdi::*;
        let invalid = || windows::core::Error::from_hresult(E_INVALIDARG);
        let width = i32::try_from(self.width).map_err(|_| invalid())?;
        let height = i32::try_from(self.height).map_err(|_| invalid())?;
        let length = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(invalid)?;
        if width <= 0 || height <= 0 || length != self.pixels.len() {
            return Err(invalid());
        }
        unsafe {
            let helper: IDragSourceHelper2 =
                CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)?;
            helper.SetFlags(DSH_ALLOWDROPDESCRIPTIONTEXT.0 as u32)?;
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                std::ptr::null_mut(),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                std::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                return Err(windows::core::Error::from_thread());
            }
            let destination = std::slice::from_raw_parts_mut(bits.cast::<u8>(), length);
            destination.copy_from_slice(&self.pixels);
            // InitializeFromBitmap multiplies RGB by alpha itself.
            unpremultiply(destination);
            let image = SHDRAGIMAGE {
                sizeDragImage: SIZE {
                    cx: width,
                    cy: height,
                },
                ptOffset: self.hotspot,
                hbmpDragImage: HBITMAP(bitmap),
                crColorKey: COLORREF(u32::MAX),
            };
            if let Err(error) = helper.InitializeFromBitmap(&raw const image, data) {
                DeleteObject(bitmap);
                return Err(error);
            }
            // The drag-image manager owns the bitmap after successful initialization.
            set_boolean(data, windows::core::w!("IsShowingText"), true)?;
            set_boolean(data, windows::core::w!("UsingDefaultDragImage"), false)?;
            Ok(helper)
        }
    }
}

fn set_boolean(data: &IDataObject, name: windows::core::PCWSTR, value: bool) -> Result<()> {
    use windows::Win32::System::{
        Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL},
        DataExchange::RegisterClipboardFormatW,
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::ReleaseStgMedium,
    };
    unsafe {
        let format_id = RegisterClipboardFormatW(name);
        if format_id == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let format = FORMATETC {
            cfFormat: format_id as u16,
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
            ..Default::default()
        };
        let memory = GlobalAlloc(GMEM_MOVEABLE, size_of::<i32>())?;
        let mut medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: memory },
            ..Default::default()
        };
        let pointer = GlobalLock(memory).cast::<i32>();
        if pointer.is_null() {
            let error = windows::core::Error::from_thread();
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        pointer.write(i32::from(value));
        let _ = GlobalUnlock(memory);
        if let Err(error) = data.SetData(&format, &medium, true) {
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        Ok(())
    }
}

fn unpremultiply(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = if alpha == 0 {
                0
            } else {
                ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_conversion_preserves_transparency_and_edge_colors() {
        let mut pixels = [9, 8, 7, 0, 32, 64, 128, 128, 12, 34, 56, 255];
        unpremultiply(&mut pixels);
        assert_eq!(pixels, [0, 0, 0, 0, 64, 128, 255, 128, 12, 34, 56, 255]);
    }

    #[test]
    fn shell_data_object_carries_custom_image_after_initialization() {
        use windows::Win32::{
            System::{
                Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL},
                DataExchange::RegisterClipboardFormatW,
                Ole::ReleaseStgMedium,
            },
            UI::Shell::SHCreateDataObject,
        };
        let _apartment = crate::ShellApartment::initialize_sta().unwrap();
        unsafe {
            let data: IDataObject = SHCreateDataObject(None, None, None).unwrap();
            let image = FileDragImage {
                width: 2,
                height: 2,
                pixels: [32, 64, 128, 128].repeat(4),
                hotspot: POINT { x: 1, y: 1 },
            };
            let helper = image.initialize(&data).unwrap();
            let format = FORMATETC {
                cfFormat: RegisterClipboardFormatW(windows::core::w!("DragImageBits")) as u16,
                dwAspect: DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
                ..Default::default()
            };
            let mut medium = data
                .GetData(&format)
                .expect("Shell must retain the supplied preview");
            ReleaseStgMedium(&mut medium);
            drop(helper);
        }
    }
}
