//! Read the computer's wall clock and active time zone for every captured event.

/// Human-readable computer-local time, including milliseconds and UTC offset.
/// A failed OS conversion is reported explicitly rather than inventing a timestamp.
#[must_use]
pub fn local_timestamp() -> String {
    local().unwrap_or_else(|| "Local time unavailable".into())
}

fn display(date_time: &str, milliseconds: u16, offset_minutes: i64) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let minutes = offset_minutes.unsigned_abs();
    format!(
        "{date_time}.{milliseconds:03} {sign}{:02}:{:02}",
        minutes / 60,
        minutes % 60
    )
}

#[cfg(windows)]
fn local() -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::{
            SystemInformation::GetSystemTime,
            Time::{SystemTimeToFileTime, SystemTimeToTzSpecificLocalTimeEx},
        },
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    let mut utc_file = FILETIME::default();
    let mut local_file = FILETIME::default();
    // Convert one captured instant, using Windows' active time zone and DST rules.
    // Computing the offset from this same instant avoids clock/zone sampling races.
    unsafe {
        GetSystemTime(&raw mut utc);
        if SystemTimeToTzSpecificLocalTimeEx(std::ptr::null(), &raw const utc, &raw mut local) == 0
            || SystemTimeToFileTime(&raw const utc, &raw mut utc_file) == 0
            || SystemTimeToFileTime(&raw const local, &raw mut local_file) == 0
        {
            return None;
        }
    }
    let ticks = |value: FILETIME| {
        (i128::from(value.dwHighDateTime) << 32) | i128::from(value.dwLowDateTime)
    };
    let offset = i64::try_from((ticks(local_file) - ticks(utc_file)) / 600_000_000).ok()?;
    let date_time = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    );
    Some(display(&date_time, local.wMilliseconds, offset))
}

#[cfg(unix)]
fn local() -> Option<String> {
    use std::{
        ffi::CStr,
        time::{SystemTime, UNIX_EPOCH},
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let seconds = libc::time_t::try_from(now.as_secs()).ok()?;
    let mut time = std::mem::MaybeUninit::<libc::tm>::uninit();
    let mut text: [libc::c_char; 64] = [0; 64];
    let mut zone: [libc::c_char; 16] = [0; 16];
    // localtime_r/strftime consult the host's current local time-zone configuration.
    unsafe {
        if libc::localtime_r(&raw const seconds, time.as_mut_ptr()).is_null() {
            return None;
        }
        if libc::strftime(
            text.as_mut_ptr(),
            text.len(),
            c"%Y-%m-%d %H:%M:%S".as_ptr(),
            time.as_ptr(),
        ) == 0
            || libc::strftime(zone.as_mut_ptr(), zone.len(), c"%z".as_ptr(), time.as_ptr()) == 0
        {
            return None;
        }
        let date_time = CStr::from_ptr(text.as_ptr()).to_str().ok()?;
        let offset = CStr::from_ptr(zone.as_ptr()).to_str().ok()?;
        let sign = match offset.as_bytes().first()? {
            b'-' => -1,
            b'+' => 1,
            _ => return None,
        };
        let hours: i64 = offset.get(1..3)?.parse().ok()?;
        let minutes: i64 = offset.get(3..5)?.parse().ok()?;
        Some(display(
            date_time,
            u16::try_from(now.subsec_millis()).ok()?,
            sign * (hours * 60 + minutes),
        ))
    }
}

#[cfg(not(any(windows, unix)))]
fn local() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_milliseconds_and_signed_half_hour_offsets_are_readable() {
        assert_eq!(
            display("2024-02-29 23:59:59", 7, 480),
            "2024-02-29 23:59:59.007 +08:00"
        );
        assert_eq!(
            display("2026-10-20 00:00:00", 123, -210),
            "2026-10-20 00:00:00.123 -03:30"
        );
        assert_eq!(
            display("2026-10-20 12:34:56", 999, 0),
            "2026-10-20 12:34:56.999 +00:00"
        );
    }
    #[test]
    fn computer_time_contains_a_calendar_date_and_an_explicit_zone() {
        let now = local_timestamp();
        assert_eq!(now.len(), 30, "{now}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], " ");
        assert_eq!(&now[19..20], ".");
        assert!(matches!(now.as_bytes()[24], b'+' | b'-'));
        assert_eq!(&now[27..28], ":");
    }
}
