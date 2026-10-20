//! Blocking `WinHTTP` transport, used only by the update worker.
use std::{
    ffi::c_void,
    ptr::null,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
    WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_AUTOLOGON_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};

struct Handle(*mut c_void);
impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, String> {
        if raw.is_null() {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn check(result: i32) -> Result<(), String> {
    if result == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn explicit_proxy() -> Option<Vec<u16>> {
    std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .ok()
        .and_then(|s| s.strip_prefix("http://").map(str::to_owned))
        .map(|s| s.trim_end_matches('/').to_owned())
        .filter(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".:-_[]".contains(&b))
        })
        .map(|s| wide(&s))
}

pub(super) fn get(cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Update canceled".into());
    }
    let url = super::API;
    let (host, path) = url
        .strip_prefix("https://")
        .and_then(|u| u.split_once('/'))
        .ok_or("Invalid HTTPS release API URL")?;
    if host != "api.github.com" || url.contains(['\r', '\n', '\0']) {
        return Err("Invalid update host".into());
    }
    // Honor an explicitly configured command-line proxy; otherwise use Windows proxy settings.
    let proxy = explicit_proxy();
    let session = Handle::new(unsafe {
        WinHttpOpen(
            wide(concat!("LucidDesk/", env!("CARGO_PKG_VERSION"))).as_ptr(),
            if proxy.is_some() {
                WINHTTP_ACCESS_TYPE_NAMED_PROXY
            } else {
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
            },
            proxy.as_ref().map_or(null(), Vec::as_ptr),
            null(),
            0,
        )
    })?;
    check(unsafe { WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 15_000) })?;
    let connection =
        Handle::new(unsafe { WinHttpConnect(session.0, wide(host).as_ptr(), 443, 0) })?;
    let request = Handle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            wide("GET").as_ptr(),
            wide(&format!("/{path}")).as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        )
    })?;
    let redirects = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
    check(unsafe {
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&raw const redirects).cast(),
            4,
        )
    })?;
    // Never use ambient Windows credentials for a public release check.
    let logon = WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH;
    check(unsafe {
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            (&raw const logon).cast(),
            4,
        )
    })?;
    let headers =
        wide("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");
    check(unsafe { WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, null(), 0, 0, 0) })?;
    check(unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) })?;
    let mut status = 0u32;
    let mut size = 4u32;
    check(unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&raw mut status).cast(),
            &raw mut size,
            std::ptr::null_mut(),
        )
    })?;
    if status != 200 {
        return Err(format!("GitHub HTTP {status}"));
    }
    let mut output = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Update canceled".into());
        }
        let mut read = 0u32;
        check(unsafe {
            WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), 65_536, &raw mut read)
        })?;
        if read == 0 {
            break;
        }
        if output.len() + read as usize > 2 * 1024 * 1024 {
            return Err("Update response exceeds size limit".into());
        }
        output.extend_from_slice(&buffer[..read as usize]);
    }
    Ok(output)
}
