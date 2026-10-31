//! Everything IPC2. Queries run on a worker with its own reply window/message pump.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*, System::DataExchange::COPYDATASTRUCT, UI::WindowsAndMessaging::*,
};

pub(super) const PAGE_SIZE: u32 = 200;
const TOKEN: usize = 0x4c504556;
const REQUEST: u32 = 0x4; // FULL_PATH_AND_NAME

#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub folder: bool,
}
#[derive(Debug)]
pub(super) struct Page {
    pub total: u32,
    pub offset: u32,
    pub entries: Vec<Entry>,
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let data = bytes
        .get(offset..offset.checked_add(4).ok_or(crate::i18n::text("ui-data-overflow"))?)
        .ok_or(crate::i18n::text("ui-incomplete-data-from-everything"))?;
    Ok(u32::from_le_bytes(data.try_into().unwrap()))
}

fn parse(bytes: &[u8]) -> Result<Page, String> {
    let total = word(bytes, 0)?;
    let count = word(bytes, 4)?;
    let offset = word(bytes, 8)?;
    if word(bytes, 12)? != REQUEST
        || count > PAGE_SIZE
        || (count > 0 && offset.saturating_add(count) > total)
    {
        return Err(crate::i18n::text("ui-unsupported-everything-result-format").into());
    }
    let data_start = 20 + count as usize * 8;
    if bytes.len() < data_start {
        return Err(crate::i18n::text("ui-incomplete-data-from-everything").into());
    }
    let mut entries = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let flags = word(bytes, 20 + i * 8)?;
        let start = word(bytes, 24 + i * 8)? as usize;
        if start < data_start {
            return Err(crate::i18n::text("ui-invalid-offset-from-everything").into());
        }
        let length = word(bytes, start)? as usize;
        let end = start
            .checked_add(4)
            .and_then(|v| {
                length
                    .checked_add(1)
                    .and_then(|l| l.checked_mul(2))
                    .and_then(|l| v.checked_add(l))
            })
            .ok_or(crate::i18n::text("ui-path-from-everything-is-too-long"))?;
        let text = bytes
            .get(start + 4..end)
            .ok_or(crate::i18n::text("ui-incomplete-path-from-everything"))?;
        if text.len() < 2 || text[text.len() - 2..] != [0, 0] {
            return Err(crate::i18n::text("ui-invalid-path-from-everything").into());
        }
        let wide: Vec<u16> = text[..text.len() - 2]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if wide.contains(&0) {
            return Err(crate::i18n::text("ui-invalid-path-from-everything").into());
        }
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));
        if !path.is_absolute() {
            return Err(crate::i18n::text("ui-everything-returned-a-relative-path").into());
        }
        entries.push(Entry {
            path,
            folder: flags & 3 != 0,
        });
    }
    Ok(Page {
        total,
        offset,
        entries,
    })
}

unsafe extern "system" fn find_instance(hwnd: HWND, data: LPARAM) -> i32 {
    let mut class = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    let name = String::from_utf16_lossy(&class[..n.max(0) as usize]);
    if name.starts_with("EVERYTHING_TASKBAR_NOTIFICATION_(") {
        unsafe {
            *(data as *mut HWND) = hwnd;
        }
        return 0;
    }
    1
}

unsafe extern "system" fn receive(
    hwnd: HWND,
    message: u32,
    wp: usize,
    lp: isize,
    _: usize,
    reference: usize,
) -> LRESULT {
    if message == WM_COPYDATA && lp != 0 {
        let data = unsafe { &*(lp as *const COPYDATASTRUCT) };
        if data.dwData == TOKEN && !data.lpData.is_null() && data.cbData <= 32 * 1024 * 1024 {
            unsafe {
                // WM_COPYDATA owns these bytes until this callback returns.
                // Parse now and retain only owned paths, never a second full
                // wire buffer alongside the decoded result page.
                *(reference as *mut Option<Result<Page, String>>) = Some(parse(
                    std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize),
                ));
            }
            return 1;
        }
    }
    unsafe { windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wp, lp) }
}

fn instance() -> HWND {
    let mut target = unsafe {
        FindWindowW(
            windows_sys::w!("EVERYTHING_TASKBAR_NOTIFICATION"),
            std::ptr::null(),
        )
    };
    if target.is_null() {
        unsafe {
            EnumWindows(Some(find_instance), (&raw mut target) as isize);
        }
    }
    target
}

pub(super) fn query(search: &str, offset: u32) -> Result<Page, String> {
    if search.encode_utf16().count() > 16_384 || search.contains('\0') {
        return Err(crate::i18n::text("ui-search-query-is-too-long-or-invalid").into());
    }
    let target = instance();
    if target.is_null() {
        return Err(crate::i18n::text("ui-start-everything-enable-ipc-then-refresh").into());
    }
    let mut reply: Option<Result<Page, String>> = None;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            windows_sys::w!("STATIC"),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    struct ReplyWindow(HWND);
    impl Drop for ReplyWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    let _window = ReplyWindow(hwnd);
    unsafe {
        if windows_sys::Win32::UI::Shell::SetWindowSubclass(
            hwnd,
            Some(receive),
            1,
            (&raw mut reply) as usize,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut());
    }
    let mut payload = Vec::new();
    for value in [
        hwnd as usize as u32,
        TOKEN as u32,
        0,
        offset,
        PAGE_SIZE,
        REQUEST,
        1,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    for value in search.encode_utf16().chain(Some(0)) {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let data = COPYDATASTRUCT {
        dwData: 18,
        cbData: payload.len() as u32,
        lpData: payload.as_ptr().cast_mut().cast(),
    };
    let mut accepted = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            target,
            WM_COPYDATA,
            hwnd as usize,
            (&raw const data) as isize,
            SMTO_ABORTIFHUNG,
            1500,
            &raw mut accepted,
        )
    };
    if sent == 0 || accepted == 0 {
        return Err(
            crate::i18n::text("ui-everything-did-not-respond-enable-ipc-in-everything-1-4-or-newer")
                .into(),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(result) = reply.take() {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(crate::i18n::text("ui-everything-search-timed-out-refresh-shortly").into());
        }
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 25, QS_ALLINPUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copydata_reply_owns_paths_without_retaining_the_wire_buffer() {
        let mut bytes = packet("C:\\搜索\\owned-result.txt");
        let mut reply: Option<Result<Page, String>> = None;
        let data = COPYDATASTRUCT {
            dwData: TOKEN,
            cbData: bytes.len() as u32,
            lpData: bytes.as_mut_ptr().cast(),
        };
        assert_eq!(
            unsafe {
                receive(
                    std::ptr::null_mut(),
                    WM_COPYDATA,
                    0,
                    (&raw const data) as isize,
                    1,
                    (&raw mut reply) as usize,
                )
            },
            1
        );
        bytes.fill(0);
        drop(bytes);
        assert_eq!(
            reply.take().unwrap().unwrap().entries[0].path,
            PathBuf::from("C:\\搜索\\owned-result.txt")
        );

        let mut malformed = packet("C:\\bad.txt");
        malformed.truncate(20);
        let data = COPYDATASTRUCT {
            dwData: TOKEN,
            cbData: malformed.len() as u32,
            lpData: malformed.as_mut_ptr().cast(),
        };
        unsafe {
            receive(
                std::ptr::null_mut(),
                WM_COPYDATA,
                0,
                (&raw const data) as isize,
                1,
                (&raw mut reply) as usize,
            );
        }
        assert!(reply.take().unwrap().is_err());
    }

    #[test]
    fn measure_result_page_payload_and_owned_storage() {
        for path_chars in [240, 4000] {
            let count = PAGE_SIZE as usize;
            let mut bytes = Vec::new();
            for value in [PAGE_SIZE, PAGE_SIZE, 0, REQUEST, 1] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.resize(20 + count * 8, 0);
            for i in 0..count {
                let offset = bytes.len() as u32;
                bytes[24 + i * 8..28 + i * 8].copy_from_slice(&offset.to_le_bytes());
                let path = format!("C:\\memory\\{i}\\{}", "x".repeat(path_chars));
                bytes.extend((path.encode_utf16().count() as u32).to_le_bytes());
                for unit in path.encode_utf16().chain(Some(0)) {
                    bytes.extend(unit.to_le_bytes());
                }
            }
            let page = parse(&bytes).unwrap();
            let owned = page.entries.capacity() * size_of::<Entry>()
                + page
                    .entries
                    .iter()
                    .map(|e| e.path.capacity())
                    .sum::<usize>();
            println!(
                "paths={count} suffix_chars={path_chars} removed_wire_copy_bytes={} owned_result_bytes={owned}",
                bytes.len()
            );
            assert_eq!(page.entries.len(), count);
        }
    }

    fn packet(path: &str) -> Vec<u8> {
        let mut data = Vec::new();
        for v in [
            1u32,
            1,
            0,
            REQUEST,
            1,
            0,
            28,
            path.encode_utf16().count() as u32,
        ] {
            data.extend(v.to_le_bytes());
        }
        for c in path.encode_utf16().chain(Some(0)) {
            data.extend(c.to_le_bytes());
        }
        data
    }
    #[test]
    fn parses_unicode_and_rejects_truncated_packets() {
        let bytes = packet("C:\\测试\\文档.txt");
        let page = parse(&bytes).unwrap();
        assert_eq!(page.entries[0].path, PathBuf::from("C:\\测试\\文档.txt"));
        assert!(!page.entries[0].folder);
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_err(), "end={end}");
        }
    }
    #[test]
    fn rejects_untrusted_offsets_and_lengths() {
        let original = packet("C:\\a.txt");
        for (offset, value) in [
            (4, 201u32),
            (12, 0),
            (24, 0),
            (24, u32::MAX),
            (28, u32::MAX),
        ] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(parse(&bytes).is_err());
        }
    }
    #[test]
    #[ignore = "requires a running Everything instance with this repository indexed"]
    fn live_everything_query() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let page = query(&format!("path:\"{}\" Cargo.toml", root.display()), 0).unwrap();
        assert!(
            page.entries
                .iter()
                .any(|e| e.path == root.join("Cargo.toml")),
            "{page:?}"
        );
        let files = query(
            &format!("file: <path:\"{}\" Cargo.toml>", root.display()),
            0,
        )
        .unwrap();
        assert!(files.entries.iter().all(|e| !e.folder));
        let folders = query(&format!("folder: path:\"{}\"", root.display()), 0).unwrap();
        assert!(!folders.entries.is_empty());
        assert!(folders.entries.iter().all(|e| e.folder));
        let first = query("file:", 0).unwrap();
        if first.total > PAGE_SIZE {
            let second = query("file:", PAGE_SIZE).unwrap();
            assert_eq!(second.offset, PAGE_SIZE);
            assert!(!second.entries.is_empty());
            assert!(
                second
                    .entries
                    .iter()
                    .all(|entry| first.entries.iter().all(|old| old.path != entry.path))
            );
        }
        assert_eq!(
            query("luciddesk-no-match-7af4d18cb03e", 0).unwrap().total,
            0
        );
    }
}
