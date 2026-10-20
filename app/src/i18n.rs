//! Embedded Fluent resources. Catalogs stay alive while the active language can change at runtime.
use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use std::{
    collections::HashMap,
    sync::{LazyLock, atomic::{AtomicUsize, Ordering}},
};

pub const LANGUAGES: [(&str, &str); 8] = [
    ("system", "System"),
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("en-US", "English"),
    ("ja-JP", "日本語"),
    ("ko-KR", "한국어"),
    ("de-DE", "Deutsch"),
    ("ru-RU", "Русский"),
];
const SOURCES: [&str; 7] = [
    include_str!("../locales/zh-CN.ftl"),
    include_str!("../locales/zh-TW.ftl"),
    include_str!("../locales/en-US.ftl"),
    include_str!("../locales/ja-JP.ftl"),
    include_str!("../locales/ko-KR.ftl"),
    include_str!("../locales/de-DE.ftl"),
    include_str!("../locales/ru-RU.ftl"),
];
static ACTIVE: AtomicUsize = AtomicUsize::new(usize::MAX);
// Keep separate from pane visibility's WM_APP + 196..=198 lifecycle messages.
pub const CHANGED: u32 = 0x8000 + 200;
struct Catalog {
    bundle: FluentBundle<FluentResource>,
    text: HashMap<String, String>,
    wide: HashMap<String, Vec<u16>>,
}
static CATALOGS: [LazyLock<Catalog>; 7] = [
    LazyLock::new(|| catalog(0)),
    LazyLock::new(|| catalog(1)),
    LazyLock::new(|| catalog(2)),
    LazyLock::new(|| catalog(3)),
    LazyLock::new(|| catalog(4)),
    LazyLock::new(|| catalog(5)),
    LazyLock::new(|| catalog(6)),
];
fn catalog(i: usize) -> Catalog {
    let source = SOURCES[i];
    let resource = FluentResource::try_new(source.to_owned()).expect("validated Fluent resource");
    let mut bundle = FluentBundle::new_concurrent(vec![LANGUAGES[i + 1].0.parse().unwrap()]);
    bundle.set_use_isolating(false);
    bundle.add_resource(resource).expect("unique message IDs");
    let mut text = HashMap::new();
    let mut wide = HashMap::new();
    for line in source
        .lines()
        .filter(|line| line.contains(" = ") && !line.starts_with('#'))
    {
        let id = line.split(" = ").next().unwrap();
        if line.contains("{ $") {
            continue;
        }
        let value = bundle.get_message(id).unwrap().value().unwrap();
        let mut errors = Vec::new();
        let rendered = bundle.format_pattern(value, None, &mut errors).into_owned();
        assert!(errors.is_empty(), "{id}: {errors:?}");
        wide.insert(
            id.to_owned(),
            rendered.encode_utf16().chain(Some(0)).collect(),
        );
        text.insert(id.to_owned(), rendered);
    }
    Catalog { bundle, text, wide }
}

pub fn resolve(language: &str) -> usize {
    let tag = language.replace('_', "-").to_ascii_lowercase();
    let parts: Vec<_> = tag.split('-').collect();
    match parts[0] {
        "zh" if parts.contains(&"hant")
            || (!parts.contains(&"hans")
                && parts.iter().any(|v| matches!(*v, "tw" | "hk" | "mo"))) =>
        {
            1
        }
        "zh" => 0,
        "ja" => 3,
        "ko" => 4,
        "de" => 5,
        "ru" => 6,
        _ => 2,
    }
}
fn system_language() -> String {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(locale: u32, name: *mut u16, count: i32, flags: u32) -> i32;
    }
    let mut name = [0u16; 85];
    let count = unsafe {
        LCIDToLocaleName(
            u32::from(GetUserDefaultUILanguage()),
            name.as_mut_ptr(),
            85,
            0,
        )
    };
    if count > 1 {
        String::from_utf16_lossy(&name[..count as usize - 1])
    } else {
        "en-US".into()
    }
}
pub fn initialize(store: &luciddesk_storage::WorkspaceStore) -> Result<bool, String> {
    let selected = store
        .preference("language")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "system".into());
    let language = if selected == "system" {
        system_language()
    } else {
        selected
    };
    let next = resolve(&language);
    #[cfg(test)]
    if TEST_LOCALE.with(|value| value.get().is_some()) {
        return Ok(TEST_LOCALE.with(|value| value.replace(Some(next))) != Some(next));
    }
    Ok(ACTIVE.swap(next, Ordering::Relaxed) != next)
}
fn active() -> usize {
    #[cfg(test)]
    if let Some(locale) = TEST_LOCALE.with(std::cell::Cell::get) {
        return locale;
    }
    // Unit tests that construct isolated UI models keep their original Chinese fixtures.
    let current = ACTIVE.load(Ordering::Relaxed);
    if current != usize::MAX { return current; }
    let initial = if cfg!(test) { 0 } else { resolve(&system_language()) };
    match ACTIVE.compare_exchange(usize::MAX, initial, Ordering::Relaxed, Ordering::Relaxed) {
        Ok(_) => initial,
        Err(value) => value,
    }

}
pub fn language() -> &'static str { LANGUAGES[active() + 1].0 }

pub fn text(id: &'static str) -> &'static str {
    CATALOGS[active()]
        .text
        .get(id)
        .or_else(|| CATALOGS[2].text.get(id))
        .map_or(id, String::as_str)
}
pub fn wide(id: &'static str) -> *const u16 {
    CATALOGS[active()]
        .wide
        .get(id)
        .or_else(|| CATALOGS[2].wide.get(id))
        .expect("static translated text")
        .as_ptr()
}
pub fn format(id: &str, values: &[(&str, String)]) -> String {
    format_locale(active(), id, values)
}
fn format_locale(locale: usize, id: &str, values: &[(&str, String)]) -> String {
    let mut args = FluentArgs::new();
    for (name, value) in values {
        args.set(*name, value.as_str());
    }
    for index in [locale, 2] {
        let bundle = &CATALOGS[index].bundle;
        if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
            let mut errors = Vec::new();
            let output = bundle.format_pattern(pattern, Some(&args), &mut errors);
            if errors.is_empty() {
                return output.into_owned();
            }
        }
    }
    id.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_selection_refreshes_text_and_keeps_borrowed_strings_valid() {
        with_locale(0, || {
            let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
            let original = text("ui-about");
            let original_wide = wide("ui-about");
            for (code, expected) in [("en-US", "About"), ("de-DE", "Info"), ("zh-CN", "关于")] {
                store.save_preference("language", code).unwrap();
                assert!(initialize(&store).unwrap());
                assert_eq!(language(), code);
                if code != "de-DE" { assert_eq!(text("ui-about"), expected); }
                assert!(!initialize(&store).unwrap());
                assert_eq!(original, "关于");
                assert_eq!(unsafe { *original_wide }, '关' as u16);
            }
            assert_eq!(store.preference("language").unwrap().as_deref(), Some("zh-CN"));
        });
    }

    #[test]
    fn system_locale_matching() {
        for (name, expected) in [
            ("zh-HK", 1),
            ("zh-Hans-TW", 0),
            ("zh-Hant", 1),
            ("en-GB", 2),
            ("ja", 3),
            ("ko-KR", 4),
            ("de-AT", 5),
            ("ru-RU", 6),
            ("fr-FR", 2),
        ] {
            assert_eq!(resolve(name), expected, "{name}");
        }
    }
    #[test]
    fn error_descriptions_preserve_diagnostics_and_separate_sections() {
        let error = "Access denied (0x80070005)\nAdditional COM detail";
        let path = "C:\\Users\\测试\\A & B\\diagnostic.log";
        for locale in 0..SOURCES.len() {
            let log = format_locale(locale, "ui-desktop-log-saved", &[("path", path.into())]);
            let warning = format_locale(locale, "ui-desktop-connection-warning", &[
                ("error", error.into()), ("log", log.clone()),
            ]);
            assert!(warning.contains(&format!("\n{error}\n\n{log}")), "{locale}: {warning}");
            assert_eq!(warning.matches(path).count(), 1);
            let opening = format_locale(locale, "error-open-item", &[
                ("path", path.into()), ("error", error.into()),
            ]);
            assert!(opening.contains(&format!("\n{path}\n\n")), "{locale}: {opening}");
            assert!(opening.ends_with(error));
            let log_failure = format_locale(locale, "ui-desktop-log-failed", &[("error", error.into())]);
            assert!(log_failure.ends_with(error));
        }
        let rollback = format_locale(0, "ui-restore-failed-rollback-failed-previous-backup", &[
            ("error", "restore HRESULT=0x80070005".into()),
            ("e", "rollback HRESULT=0x80070020".into()), ("arg0", path.into()),
        ]);
        assert!(rollback.contains("\nrestore HRESULT=0x80070005\n\n"));
        assert!(rollback.contains("\nrollback HRESULT=0x80070020\n\n"));
        assert!(rollback.ends_with(path));
    }

    #[test]
    fn all_messages_format_in_all_languages() {
        let ids: Vec<_> = SOURCES[0]
            .lines()
            .filter(|l| !l.starts_with('#') && l.contains(" = "))
            .map(|l| l.split(" = ").next().unwrap())
            .collect();
        for (index, source) in SOURCES.iter().enumerate() {
            let other: Vec<_> = source
                .lines()
                .filter(|l| !l.starts_with('#') && l.contains(" = "))
                .map(|l| l.split(" = ").next().unwrap())
                .collect();
            assert_eq!(ids, other);
            for line in source
                .lines()
                .filter(|l| !l.starts_with('#') && l.contains(" = "))
            {
                let id = line.split(" = ").next().unwrap();
                let mut args = FluentArgs::new();
                for part in line.split("{ $").skip(1) {
                    args.set(part.split(' ').next().unwrap(), "example");
                }
                let bundle = &CATALOGS[index].bundle;
                let pattern = bundle.get_message(id).unwrap().value().unwrap();
                let mut errors = Vec::new();
                let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
                assert!(errors.is_empty(), "{index}/{id}: {errors:?}");
                assert!(!text.is_empty());
            }
        }
    }
}

pub fn default_font() -> &'static str {
    match active() {
        0 => "Microsoft YaHei UI",
        1 => "Microsoft JhengHei UI",
        3 => "Yu Gothic UI",
        4 => "Malgun Gothic",
        _ => "Segoe UI",
    }
}
pub fn font_sample() -> &'static str {
    match active() {
        0 => "中文标签文件设置AaZz0123456789",
        1 => "中文標籤檔案設定AaZz0123456789",
        3 => "日本語設定あいうアイウAaZz0123456789",
        4 => "한국어설정파일AaZz0123456789",
        5 => "ÄÖÜäöüßAaZz0123456789",
        6 => "РусскийязыкЁёAaZz0123456789",
        _ => "AaZz0123456789",
    }
}

#[cfg(test)]
thread_local! { static TEST_LOCALE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub fn with_locale<T>(locale: usize, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_LOCALE.set(self.0);
        }
    }
    let _restore = Restore(TEST_LOCALE.replace(Some(locale)));
    run()
}
