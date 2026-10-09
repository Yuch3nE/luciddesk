//! Hide the Shell's action badge while retaining its icon-and-label drag image.
use windows::{
    Win32::{
        System::{
            Com::{
                DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
            },
            DataExchange::RegisterClipboardFormatW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::ReleaseStgMedium,
        },
        UI::Shell::{CFSTR_DROPDESCRIPTION, DROPDESCRIPTION, DROPIMAGE_INVALID, DROPIMAGE_NOIMAGE},
    },
    core::Result,
};

fn format() -> Result<FORMATETC> {
    let id = unsafe { RegisterClipboardFormatW(CFSTR_DROPDESCRIPTION) };
    if id == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(FORMATETC {
        cfFormat: id as u16,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    })
}

fn read(data: &IDataObject) -> Option<DROPDESCRIPTION> {
    unsafe {
        let mut medium = data.GetData(&format().ok()?).ok()?;
        let description = if medium.tymed == TYMED_HGLOBAL.0 as u32
            && GlobalSize(medium.u.hGlobal) >= size_of::<DROPDESCRIPTION>()
        {
            let pointer = GlobalLock(medium.u.hGlobal).cast::<DROPDESCRIPTION>();
            if pointer.is_null() {
                None
            } else {
                let value = pointer.read();
                let _ = GlobalUnlock(medium.u.hGlobal);
                Some(value)
            }
        } else {
            None
        };
        ReleaseStgMedium(&mut medium);
        description
    }
}

fn write(data: &IDataObject, description: DROPDESCRIPTION) -> Result<()> {
    let format = format()?;
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, size_of::<DROPDESCRIPTION>())?;
        let mut medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: memory },
            ..Default::default()
        };
        let pointer = GlobalLock(memory).cast::<DROPDESCRIPTION>();
        if pointer.is_null() {
            let error = windows::core::Error::from_thread();
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        pointer.write(description);
        let _ = GlobalUnlock(memory);
        if let Err(error) = data.SetData(&format, &medium, true) {
            ReleaseStgMedium(&mut medium);
            return Err(error);
        }
        Ok(())
    }
}

pub(super) struct QuietDescription {
    data: IDataObject,
    previous: Option<DROPDESCRIPTION>,
}

impl QuietDescription {
    pub fn new(data: &IDataObject) -> Self {
        Self {
            data: data.clone(),
            previous: read(data),
        }
    }

    pub fn suppress(&self) {
        if let Some(current) = read(&self.data) {
            let message = current.szMessage;
            let insert = current.szInsert;
            if { current.r#type } == DROPIMAGE_NOIMAGE
                && message.iter().chain(&insert).all(|ch| *ch == 0)
            {
                return;
            }
        }
        // NOIMAGE hides the operation glyph; empty strings hide its caption.
        // INVALID would request the default "Link" badge instead.
        let _ = write(
            &self.data,
            DROPDESCRIPTION {
                r#type: DROPIMAGE_NOIMAGE,
                ..Default::default()
            },
        );
    }
}

impl Drop for QuietDescription {
    fn drop(&mut self) {
        let _ = write(
            &self.data,
            self.previous.take().unwrap_or(DROPDESCRIPTION {
                r#type: DROPIMAGE_INVALID,
                ..Default::default()
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Shell::{DROPIMAGE_LINK, SHCreateDataObject};

    #[test]
    fn hides_badge_and_caption_then_restores_source_description() {
        let _apartment = crate::pane::test_support::apartment();
        let data: IDataObject = unsafe { SHCreateDataObject(None, None, None).unwrap() };
        let mut original = DROPDESCRIPTION {
            r#type: DROPIMAGE_LINK,
            ..Default::default()
        };
        original.szMessage[0] = b'L' as u16;
        original.szInsert[0] = b'X' as u16;
        write(&data, original).unwrap();
        {
            let quiet = QuietDescription::new(&data);
            for _ in 0..2 {
                quiet.suppress();
                let hidden = read(&data).unwrap();
                assert_eq!({ hidden.r#type }, DROPIMAGE_NOIMAGE);
                let message = hidden.szMessage;
                let insert = hidden.szInsert;
                assert!(message.iter().chain(&insert).all(|ch| *ch == 0));
            }
        }
        let restored = read(&data).unwrap();
        assert_eq!({ restored.r#type }, DROPIMAGE_LINK);
        assert_eq!({ restored.szMessage[0] }, b'L' as u16);
        assert_eq!({ restored.szInsert[0] }, b'X' as u16);
    }
}
