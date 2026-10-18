use std::{
    collections::HashMap,
    sync::{
        LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};
const SOURCES: [&str; 7] = [
    include_str!("../locales/zh-CN.ftl"),
    include_str!("../locales/zh-TW.ftl"),
    include_str!("../locales/en-US.ftl"),
    include_str!("../locales/ja-JP.ftl"),
    include_str!("../locales/ko-KR.ftl"),
    include_str!("../locales/de-DE.ftl"),
    include_str!("../locales/ru-RU.ftl"),
];
static CATALOGS: LazyLock<Vec<HashMap<&'static str, &'static str>>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|s| s.lines().filter_map(|l| l.split_once(" = ")).collect())
        .collect()
});
static LANGUAGE: AtomicUsize = AtomicUsize::new(0);
pub fn set(language: &str) {
    LANGUAGE.store(
        [
            "zh-CN", "zh-TW", "en-US", "ja-JP", "ko-KR", "de-DE", "ru-RU",
        ]
        .iter()
        .position(|s| *s == language)
        .unwrap_or(2),
        Ordering::Relaxed,
    );
}
pub fn text(key: &'static str) -> &'static str {
    CATALOGS[LANGUAGE.load(Ordering::Relaxed)]
        .get(key)
        .copied()
        .unwrap_or(key)
}
