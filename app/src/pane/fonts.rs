//! Installed outline fonts with coverage for the active interface language, shared by GDI and DirectWrite.
use std::sync::RwLock;
use windows_sys::Win32::Graphics::Gdi::*;
const KEY: &str = "ui_font_family";
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) fn revision() -> u64 { REVISION.load(std::sync::atomic::Ordering::Relaxed) }
static FAMILY: RwLock<String> = RwLock::new(String::new());

const ICON_GLYPHS: &[u32] = &[
    0xe70d, 0xe70e, 0xe711, 0xe721, 0xe72b, 0xe73e, 0xe76c,
    0xe790, 0xe80f, 0xe81c, 0xe890, 0xe8a5, 0xe8b7, 0xe8bb,
    0xe8d2, 0xe916, 0xe921, 0xe922, 0xe923, 0xe946, 0xf0e2,
];

/// Private-use glyphs cannot rely on normal text fallback. Check the actual
/// DirectWrite family and glyph coverage before selecting the Windows 11 font.
pub(super) fn icon_family() -> &'static str {
    static ICON_FAMILY: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
        let family = choose_icon_family(|name| icon_coverage(name).unwrap_or(false));
        crate::pane::render_debug::render_trace(format_args!("icon_font={family}"));
        family
    });
    *ICON_FAMILY
}

fn choose_icon_family(supports: impl FnOnce(&str) -> bool) -> &'static str {
    if supports("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" }
}

fn icon_coverage(name: &str) -> windows::core::Result<bool> {
    use windows::Win32::Graphics::DirectWrite::*;
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection = None;
        factory.GetSystemFontCollection(&raw mut collection, false)?;
        let collection = collection.unwrap();
        let mut index = 0;
        let mut exists = windows::core::BOOL(0);
        collection.FindFamilyName(&windows::core::HSTRING::from(name), &raw mut index, &raw mut exists)?;
        if !exists.as_bool() { return Ok(false); }
        let font = collection.GetFontFamily(index)?.GetFirstMatchingFont(
            DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        )?;
        let face = font.CreateFontFace()?;
        let mut glyphs = vec![0u16; ICON_GLYPHS.len()];
        face.GetGlyphIndices(ICON_GLYPHS.as_ptr(), ICON_GLYPHS.len() as u32, glyphs.as_mut_ptr())?;
        Ok(glyphs.iter().all(|glyph| *glyph != 0))
    }
}
pub(super) fn family() -> String { with_family(str::to_owned) }
pub(super) fn with_family<T>(read: impl FnOnce(&str) -> T) -> T {
    let value = FAMILY.read().unwrap();
    read(if value.is_empty() { crate::i18n::default_font() } else { &value })
}
fn set(value: String) {
    *FAMILY.write().unwrap() = value;
    REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

unsafe extern "system" fn collect(
    font: *const LOGFONTW,
    _: *const TEXTMETRICW,
    kind: u32,
    data: isize,
) -> i32 {
    if kind & TRUETYPE_FONTTYPE != 0 {
        let font = unsafe { &*font };
        let len = font
            .lfFaceName
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(font.lfFaceName.len());
        let name = String::from_utf16_lossy(&font.lfFaceName[..len]);
        if regular_face(font)
            && !name.starts_with('@')
            && !name.is_empty()
            && font.lfCharSet != SYMBOL_CHARSET
        {
            unsafe { &mut *(data as *mut Vec<String>) }.push(name);
        }
    }
    1
}
fn regular_face(font: &LOGFONTW) -> bool {
    font.lfWeight == FW_NORMAL as i32 && font.lfItalic == 0
}
// Reuse a single DC and glyph buffer for one scan; no native handles are cached.
struct CoverageProbe {
    dc: HDC,
    sample: Vec<u16>,
    glyphs: Vec<u16>,
}
impl CoverageProbe {
    fn new(sample: &str) -> Option<Self> {
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if dc.is_null() { return None; }
        let sample: Vec<u16> = sample.encode_utf16().collect();
        let glyphs = vec![0; sample.len()];
        Some(Self { dc, sample, glyphs })
    }
    fn supports(&mut self, name: &str) -> bool {
        if name.encode_utf16().count() >= 32 || name.contains('\0') { return false; }
        unsafe {
            let mut lf = LOGFONTW { lfHeight: -16, lfWeight: FW_NORMAL as i32, ..Default::default() };
            for (out, unit) in lf.lfFaceName.iter_mut().zip(name.encode_utf16()) { *out = unit; }
            let font = CreateFontIndirectW(&lf);
            if font.is_null() { return false; }
            let old = SelectObject(self.dc, font);
            self.glyphs.fill(0xffff);
            let count = GetGlyphIndicesW(self.dc, self.sample.as_ptr(), self.sample.len() as i32,
                self.glyphs.as_mut_ptr(), GGI_MARK_NONEXISTING_GLYPHS);
            SelectObject(self.dc, old);
            DeleteObject(font);
            count != u32::MAX && self.glyphs.iter().all(|g| *g != 0xffff && *g != 0)
        }
    }
}
impl Drop for CoverageProbe {
    fn drop(&mut self) { unsafe { DeleteDC(self.dc); } }
}
#[cfg(test)]
fn readable(name: &str) -> bool { CoverageProbe::new(crate::i18n::font_sample()).is_some_and(|mut probe| probe.supports(name)) }

#[cfg(test)]
pub(super) fn installed() -> Vec<String> {
    enumerate_candidates(crate::i18n::font_sample(), crate::i18n::default_font(), &std::sync::atomic::AtomicBool::new(false))
}

pub(super) struct CandidateLoad {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    receiver: std::sync::mpsc::Receiver<Vec<String>>,
}
impl CandidateLoad {
    pub(super) fn start() -> Result<Self, String> {
        let sample = crate::i18n::font_sample();
        let default = crate::i18n::default_font();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = cancelled.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("font-candidates".into()).spawn(move || {
            let names = enumerate_candidates(sample, default, &stop);
            if !stop.load(std::sync::atomic::Ordering::Relaxed) { let _ = sender.send(names); }
        }).map_err(|error| error.to_string())?;
        Ok(Self { cancelled, receiver })
    }
    pub(super) fn poll(&self) -> Result<Vec<String>, std::sync::mpsc::TryRecvError> { self.receiver.try_recv() }
}
impl Drop for CandidateLoad {
    fn drop(&mut self) { self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed); }
}
fn enumerate_candidates(sample: &str, default: &str, cancelled: &std::sync::atomic::AtomicBool) -> Vec<String> {
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) { return Vec::new(); }
    let mut names = Vec::<String>::new();
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return names;
        }
        let lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        EnumFontFamiliesExW(
            dc,
            &lf,
            Some(collect),
            (&raw mut names) as isize,
            0,
        );
        DeleteDC(dc);
    }
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    let Some(mut probe) = CoverageProbe::new(sample) else { return Vec::new(); };
    let mut supported = Vec::new();
    for name in names {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) { return Vec::new(); }
        if probe.supports(&name) { supported.push(name); }
    }
    let mut names = supported;
    if let Some(index) = names.iter().position(|name| name == default) {
        let default = names.remove(index);
        names.insert(0, default);
    }
    names
}
// Validate only the selected family; saving or startup never scans all fonts.
pub(super) fn available(name: &str) -> bool {
    if name.starts_with('@') || name.encode_utf16().count() >= 32 || name.contains('\0') { return false; }
    let Some(mut probe) = CoverageProbe::new(crate::i18n::font_sample()) else { return false; };
    let mut lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
    for (out, unit) in lf.lfFaceName.iter_mut().zip(name.encode_utf16()) { *out = unit; }
    let mut names = Vec::<String>::new();
    unsafe { EnumFontFamiliesExW(probe.dc, &lf, Some(collect), (&raw mut names) as isize, 0); }
    names.iter().any(|candidate| candidate == name) && probe.supports(name)
}
pub(super) fn load(store: &luciddesk_storage::WorkspaceStore) -> Result<(), String> {
    let saved = store
        .preference(KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if saved.is_empty() {
        set(String::new());
    } else {
        set(if available(&saved) { saved } else { String::new() });
    }
    Ok(())
}
pub(super) fn save(store: &luciddesk_storage::WorkspaceStore, name: &str) -> Result<(), String> {
    if name != crate::i18n::default_font() && !available(name) {
        return Err(crate::i18n::text("font-unavailable").into());
    }
    let saved = if name == crate::i18n::default_font() { "" } else { name };
    store.save_preference(KEY, saved).map_err(|e| e.to_string())?;
    set(saved.into());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_load_uses_requested_language_and_can_be_cancelled() {
        let expected = crate::i18n::with_locale(5, installed);
        let load = crate::i18n::with_locale(5, || CandidateLoad::start().unwrap());
        let names = load.receiver.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(names, expected);
        let cancelled = load.cancelled.clone();
        drop(load);
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        assert!(enumerate_candidates("Aa", "Segoe UI", &cancelled).is_empty());
    }

    #[test]
    fn candidates_follow_language_and_prefer_its_default() {
        for locale in 0..7 {
            crate::i18n::with_locale(locale, || {
                let names = installed();
                assert!(!names.is_empty());
                assert!(names.iter().all(|name| readable(name)));
                if names.iter().any(|name| name == crate::i18n::default_font()) {
                    assert_eq!(names[0], crate::i18n::default_font());
                }
            });
        }
    }

    #[test]
    fn downlevel_icon_font_covers_all_application_symbols() {
        assert_eq!(choose_icon_family(|_| false), "Segoe MDL2 Assets");
        assert_eq!(choose_icon_family(|_| true), "Segoe Fluent Icons");
        assert!(icon_coverage("Segoe MDL2 Assets").unwrap());
        assert!(!icon_coverage("LucidDesk nonexistent icon font").unwrap());
        assert!(icon_coverage(icon_family()).unwrap());
    }
    #[test]
    fn regular_faces_exclude_weight_variants_and_italics() {
        for weight in [100, 200, 300, 400, 500, 600, 700, 800, 900] {
            for italic in [0, 1] {
                let font = LOGFONTW {
                    lfWeight: weight,
                    lfItalic: italic,
                    ..Default::default()
                };
                assert_eq!(regular_face(&font), weight == 400 && italic == 0);
            }
        }
    }
    #[test]
    fn font_candidates_exclude_symbols_vertical_faces_and_missing_glyphs() {
        let names = installed();
        assert!(
            names
                .iter()
                .any(|name| name == super::super::assets::UI_FONT)
        );
        assert!(
            names
                .iter()
                .all(|name| !name.starts_with('@') && readable(name))
        );
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        assert!(save(&store, "LucidDesk nonexistent font 82947").is_err());
        assert!(store.preference(KEY).unwrap().is_none());
    }
    #[test]
    fn font_switch_updates_live_layout_persists_and_missing_fonts_fall_back() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(String);
        impl Drop for Restore {
            fn drop(&mut self) {
                set(self.0.clone());
            }
        }
        let _restore = Restore(family());
        set(crate::i18n::default_font().into());
        let names = installed();
        let alternative = names
            .iter()
            .find(|name| name.as_str() == "SimSun")
            .or_else(|| {
                names
                    .iter()
                    .find(|name| name.as_str() != super::super::assets::UI_FONT)
            })
            .expect("another installed Chinese font");
        let mut state = super::super::tests::test_state();
        state
            .workspace
            .set_appearance(luciddesk_core::PanelTheme::Dark, luciddesk_core::Backdrop::Mica);
        let model = super::super::create_model(&state, luciddesk_core::PanelId::new(1)).unwrap();
        let mut renderer = super::super::render::Renderer::new().unwrap();
        let before = renderer.pixels(420, 300, 1.0, &model).unwrap();
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        save(&store, alternative).unwrap();
        assert_eq!(family(), *alternative);
        assert_ne!(before, renderer.pixels(420, 300, 1.0, &model).unwrap());
        assert_eq!(
            store.preference(KEY).unwrap().as_deref(),
            Some(alternative.as_str())
        );
        set(String::new());
        load(&store).unwrap();
        assert_eq!(family(), *alternative);
        store.save_preference(KEY, "Removed font 928471").unwrap();
        load(&store).unwrap();
        assert_eq!(family(), super::super::assets::UI_FONT);
    }
}
