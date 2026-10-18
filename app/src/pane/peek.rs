//! Use the resident Peek Shell entry point for files and virtual desktop items.
use super::keyboard::{self, Modifiers};
use luciddesk_core::ShellIdentity;
use luciddesk_storage::WorkspaceStore;
use std::{cell::RefCell, path::PathBuf};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Provider {
    Peek,
    QuickLook,
}
impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Peek => "Peek",
            Self::QuickLook => "QuickLook",
        }
    }
    fn executable(self) -> &'static str {
        match self {
            Self::Peek => "PowerToys.Peek.UI.exe",
            Self::QuickLook => "QuickLook.exe",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Settings {
    pub enabled: bool,
    pub provider: Provider,
    pub quicklook_path: String,
    pub path: String,
    pub key: u16,
    pub modifiers: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: Provider::Peek,
            quicklook_path: String::new(),
            path: String::new(),
            key: VK_SPACE,
            modifiers: 0,
        }
    }
}
thread_local! { static SETTINGS: RefCell<Settings> = RefCell::new(Settings::default()); }
pub(super) fn settings() -> Settings {
    SETTINGS.with(|s| s.borrow().clone())
}
impl Settings {
    pub fn active_path(&self) -> &str {
        match self.provider {
            Provider::Peek => &self.path,
            Provider::QuickLook => &self.quicklook_path,
        }
    }
    pub fn set_path(&mut self, path: String) {
        match self.provider {
            Provider::Peek => self.path = path,
            Provider::QuickLook => self.quicklook_path = path,
        }
    }
}
fn encode(s: &Settings) -> String {
    format!(
        "v2\n{}\n{}\n{}\n{}\n{}\n{}",
        u8::from(s.enabled),
        s.key,
        s.modifiers,
        s.provider.name(),
        s.path,
        s.quicklook_path
    )
}
fn decode(raw: &str) -> Option<Settings> {
    let (provider, quicklook_path, legacy) = if let Some(raw) = raw.strip_prefix("v2\n") {
        let fields: Vec<_> = raw.splitn(6, '\n').collect();
        if fields.len() != 6 {
            return None;
        }
        let provider = match fields[3] {
            "Peek" => Provider::Peek,
            "QuickLook" => Provider::QuickLook,
            _ => return None,
        };
        (
            provider,
            fields[5].to_owned(),
            format!("{}\n{}\n{}\n{}", fields[0], fields[1], fields[2], fields[4]),
        )
    } else {
        (Provider::Peek, String::new(), raw.to_owned())
    };
    let mut lines = legacy.splitn(4, '\n');
    let enabled = match lines.next()? {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    let key = lines.next()?.parse().ok()?;
    let modifiers = lines.next()?.parse().ok()?;
    let path = lines.next()?.to_owned();
    valid_shortcut(key, modifiers).then_some(Settings {
        enabled,
        key,
        modifiers,
        path,
        provider,
        quicklook_path,
    })
}
pub(super) fn load(store: &WorkspaceStore) -> Result<(), String> {
    let mut value = store
        .preference("peek")
        .map_err(|e| e.to_string())?
        .and_then(|raw| decode(&raw))
        .unwrap_or_default();
    value.enabled &= resolved(&value).is_some();
    SETTINGS.with(|s| *s.borrow_mut() = value);
    Ok(())
}
pub(super) fn save(store: &WorkspaceStore, mut value: Settings) -> Result<(), String> {
    value.enabled &= resolved(&value).is_some();
    store
        .save_preference("peek", &encode(&value))
        .map_err(|e| e.to_string())?;
    SETTINGS.with(|s| *s.borrow_mut() = value);
    Ok(())
}
pub(super) fn modifier_bits(m: &Modifiers) -> u8 {
    u8::from(m.ctrl)
        | (u8::from(m.shift) << 1)
        | (u8::from(m.alt) << 2)
        | (u8::from(m.windows) << 3)
}
pub(super) fn valid_shortcut(key: u16, bits: u8) -> bool {
    if bits > 7
        || !(key == VK_SPACE
            || (0x30..=0x39).contains(&key)
            || (0x41..=0x5a).contains(&key)
            || (VK_F1..=VK_F24).contains(&key))
    {
        return false;
    }
    if bits & 4 != 0 && matches!(key, VK_F4 | VK_SPACE) {
        return false;
    }
    let mods = Modifiers {
        ctrl: bits & 1 != 0,
        shift: bits & 2 != 0,
        alt: bits & 4 != 0,
        windows: false,
    };
    keyboard::command(key, &mods, false).is_none()
}
pub(super) fn matches(key: u16, mods: &Modifiers, repeat: bool) -> bool {
    let s = settings();
    s.enabled
        && !repeat
        && key == s.key
        && modifier_bits(mods) == s.modifiers
        && resolved(&s).is_some()
}
pub(super) fn shortcut_label(s: &Settings) -> String {
    let mut parts = Vec::new();
    if s.modifiers & 1 != 0 {
        parts.push("Ctrl".to_owned());
    }
    if s.modifiers & 2 != 0 {
        parts.push("Shift".to_owned());
    }
    if s.modifiers & 4 != 0 {
        parts.push("Alt".to_owned());
    }
    parts.push(match s.key {
        VK_SPACE => "Space".into(),
        VK_F1..=VK_F24 => format!("F{}", s.key - VK_F1 + 1),
        key => char::from_u32(u32::from(key)).unwrap_or('?').to_string(),
    });
    parts.join(" + ")
}
pub(super) fn detect() -> Option<PathBuf> {
    for (var, base) in [
        ("ProgramFiles", "PowerToys"),
        ("LOCALAPPDATA", "PowerToys"),
        ("LOCALAPPDATA", "Programs\\PowerToys"),
    ] {
        if let Some(root) = std::env::var_os(var) {
            for relative in ["WinUI3Apps\\PowerToys.Peek.UI.exe", "PowerToys.Peek.UI.exe"] {
                let path = PathBuf::from(&root).join(base).join(relative);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}
fn running_quicklook() -> Option<PathBuf> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{ProcessStatus::EnumProcesses, Threading::*},
    };
    let mut ids = vec![0u32; 65536];
    let mut bytes = 0;
    unsafe {
        if EnumProcesses(ids.as_mut_ptr(), (ids.len() * 4) as u32, &raw mut bytes) == 0 {
            return None;
        }
        for id in &ids[..bytes as usize / 4] {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *id);
            if process.is_null() {
                continue;
            }
            let mut name = [0u16; 32768];
            let mut length = name.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &raw mut length);
            CloseHandle(process);
            if ok != 0 {
                let path = PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
                if path
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("QuickLook.exe"))
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

// QuickLook's per-user pipe accepts UTF-8 command|path|options lines.
fn quicklook_pipe(path: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::ConvertSidToStringSidW, *},
        System::Threading::*,
    };
    let pipe = unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut length = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut length);
        let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &raw mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &raw mut sid) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let name = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
        LocalFree(sid.cast());
        format!(r"\\.\pipe\QuickLook.App.Pipe.{name}")
    };
    let path = path.to_str().ok_or("QuickLook requires a Unicode path")?;
    if path.contains(['\r', '\n', '|']) {
        return Err("Invalid QuickLook path".into());
    }
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .open(pipe)
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("QuickLook.App.PipeMessages.Toggle|{path}|\n").as_bytes())
        .map_err(|e| e.to_string())
}

pub(super) fn resolved(s: &Settings) -> Option<PathBuf> {
    if !s.active_path().is_empty() {
        return Some(PathBuf::from(s.active_path())).filter(|p| {
            p.is_file()
                && p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(s.provider.executable()))
        });
    }
    if s.provider == Provider::Peek {
        return detect();
    }
    for (var, relative) in [
        ("LOCALAPPDATA", "Programs/QuickLook/QuickLook.exe"),
        ("LOCALAPPDATA", "QuickLook/QuickLook.exe"),
        ("ProgramFiles", "QuickLook/QuickLook.exe"),
        ("ProgramFiles(x86)", "QuickLook/QuickLook.exe"),
    ] {
        if let Some(root) = std::env::var_os(var) {
            let path = PathBuf::from(root).join(relative);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    running_quicklook()
}
pub(super) fn browse(owner: isize, provider: Provider) -> Result<Option<String>, String> {
    super::executable_picker::browse(
        owner,
        match provider { Provider::Peek => "PowerToys Peek", Provider::QuickLook => "QuickLook" },
        provider.executable(),
        "ui-could-not-select-preview-app",
        &crate::i18n::format("ui-select", &[("arg0", provider.executable().into())]),
    )
}

struct Signal(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Signal {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
fn ensure_running(path: &std::path::Path) -> Result<Vec<Signal>, String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{ProcessStatus::EnumProcesses, Threading::*},
    };
    let mut ids = vec![0u32; 65536];
    let mut bytes = 0;
    if unsafe { EnumProcesses(ids.as_mut_ptr(), (ids.len() * 4) as u32, &raw mut bytes) } == 0 {
        return Err(crate::i18n::text("ui-could-not-inspect-peek-process").into());
    }
    for id in &ids[..bytes as usize / 4] {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *id);
            if process.is_null() {
                continue;
            }
            let mut name = [0u16; 32768];
            let mut length = name.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &raw mut length);
            CloseHandle(process);
            if ok != 0
                && String::from_utf16_lossy(&name[..length as usize])
                    .eq_ignore_ascii_case(&path.to_string_lossy())
            {
                return Ok(Vec::new());
            }
        }
    }
    // Keep auto-reset events alive until the newly started listener consumes the request.
    let mut signals = Vec::new();
    for name in [
        windows_sys::w!("Local\\ShowPeekEvent"),
        windows_sys::w!("Local\\TerminatePeekEvent"),
    ] {
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        signals.push(Signal(handle));
    }
    std::process::Command::new(path)
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| crate::i18n::format("ui-could-not-start-peek", &[("e", format!("{}", e))]))?;
    Ok(signals)
}

pub(super) fn open(owner: isize, identity: &ShellIdentity) -> Result<(), String> {
    let s = settings();
    if !s.enabled || resolved(&s).is_none() {
        return Ok(());
    }
    if s.provider == Provider::QuickLook {
        return open_path(identity);
    }
    let path = resolved(&s)
        .filter(|path| path.is_file())
        .ok_or(crate::i18n::text("ui-peek-not-found-select-powertoys-peek-ui-exe-in-settings"))?;
    let _signals = ensure_running(&path)?;
    luciddesk_shell::peek_desktop_item(windows::Win32::Foundation::HWND(owner as _), identity)
        .map_err(|error| crate::i18n::format("ui-could-not-open-powertoys-peek", &[("error", format!("{}", error))]))
}

pub(super) fn open_path(identity: &ShellIdentity) -> Result<(), String> {
    let value = settings();
    if !value.enabled || resolved(&value).is_none() {
        return Ok(());
    }
    let path = if value.provider == Provider::QuickLook {
        // QuickLook accepts Shell parsing names (such as ::{CLSID}) over its pipe.
        PathBuf::from(identity.activation_name())
    } else {
        identity
            .file_system_path()
            .ok_or(crate::i18n::text("ui-this-item-has-no-file-path"))?
            .to_path_buf()
    };
    if value.provider == Provider::QuickLook && quicklook_pipe(&path).is_ok() {
        return Ok(());
    }
    let executable = resolved(&value)
        .filter(|p| p.is_file())
        .ok_or_else(|| crate::i18n::format("ui-not-found-select-its-path-in-settings", &[("arg0", format!("{}", value.provider.name()))]))?;
    if value.provider == Provider::QuickLook {
        std::process::Command::new(executable)
            .arg("/autorun")
            .spawn()
            .map_err(|e| e.to_string())?;
        // Startup is asynchronous. Keep UI messages flowing while waiting for the listener.
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if quicklook_pipe(&path).is_ok() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(crate::i18n::text("ui-quicklook-started-but-its-preview-service-is-unavailable").into());
            }
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if message.message == WM_QUIT {
                        PostQuitMessage(message.wParam as i32);
                        return Err(crate::i18n::text("ui-preview-canceled").into());
                    }
                    TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
                MsgWaitForMultipleObjectsEx(
                    0,
                    std::ptr::null(),
                    25,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
            }
        }
    }
    std::process::Command::new(executable)
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_availability_tracks_configured_path() {
        let dir = std::env::temp_dir().join(format!(
            "luciddesk-availability-PowerToys.Peek.UI.exe-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("PowerToys.Peek.UI.exe");
        let value = Settings {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        assert!(resolved(&value).is_none());
        std::fs::write(&path, b"availability fixture").unwrap();
        assert_eq!(resolved(&value), Some(path.clone()));
        std::fs::remove_file(&path).unwrap();
        assert!(resolved(&value).is_none());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn quicklook_keeps_namespace_parsing_names() {
        let item = ShellIdentity::Namespace {
            parsing_name: "::{645FF040-5081-101B-9F08-00AA002F954E}".into(),
        };
        let path = PathBuf::from(item.activation_name());
        assert_eq!(
            path.to_str().unwrap(),
            "::{645FF040-5081-101B-9F08-00AA002F954E}"
        );
    }
    #[test]
    #[ignore = "requires running QuickLook on the interactive desktop"]
    fn live_quicklook_recycle_bin_preview() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        save(
            &store,
            Settings {
                enabled: true,
                provider: Provider::QuickLook,
                ..Default::default()
            },
        )
        .unwrap();
        let item = ShellIdentity::Namespace {
            parsing_name: "::{645FF040-5081-101B-9F08-00AA002F954E}".into(),
        };
        let result = open(0, &item);
        save(&store, Default::default()).unwrap();
        result.unwrap();
    }
    #[test]
    fn legacy_preview_preferences_preserve_peek_and_provider_paths() {
        let mut value = decode("1\n32\n0\nC:/Peek.exe").unwrap();
        assert_eq!(value.provider, Provider::Peek);
        value.provider = Provider::QuickLook;
        value.set_path("C:/QuickLook.exe".into());
        assert_eq!(value.path, "C:/Peek.exe");
        assert_eq!(decode(&encode(&value)), Some(value));
    }
    #[test]
    #[ignore = "requires running QuickLook on the interactive desktop"]
    fn live_quicklook_path_preview() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../README.md");
        assert!(running_quicklook().is_some());
        quicklook_pipe(&path.canonicalize().unwrap()).unwrap();
    }
    #[test]
    fn preferences_round_trip_and_shortcut_conflicts() {
        let store = WorkspaceStore::open_in_memory().unwrap();
        let value = Settings {
            enabled: false,
            provider: Provider::QuickLook,
            quicklook_path: "C:/QuickLook/QuickLook.exe".into(),
            path: "C:\\应用\\PowerToys.Peek.UI.exe".into(),
            key: 0x50,
            modifiers: 3,
        };
        save(&store, value.clone()).unwrap();
        load(&store).unwrap();
        assert_eq!(settings(), value);
        assert!(!matches(
            0x50,
            &Modifiers {
                ctrl: true,
                shift: true,
                ..Modifiers::default()
            },
            false
        ));
        assert!(valid_shortcut(VK_SPACE, 0));
        assert!(!valid_shortcut(VK_SPACE, 1));
        assert!(!valid_shortcut(0x43, 1));
        assert!(!valid_shortcut(VK_F4, 4));
        assert!(decode("broken").is_none());
        save(
            &store,
            Settings {
                enabled: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches(VK_SPACE, &Modifiers::default(), false));
        assert!(!matches(VK_SPACE, &Modifiers::default(), true));
    }
}
