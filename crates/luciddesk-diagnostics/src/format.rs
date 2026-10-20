//! Shared, single-line diagnostic records. No I/O or external dependencies.
use crate::Level;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn header(report: &str) -> String {
    let report: String = report.chars().take(16_384).collect();
    format!("# format=luciddesk-log-v1 report={report:?}\n")
}

pub(crate) fn entry(
    level: Level,
    component: &str,
    message: &str,
    version: &str,
    build: &str,
) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let component: String = component.chars().take(128).collect();
    let mut characters = message.chars();
    let mut message: String = characters.by_ref().take(16_384).collect();
    if characters.next().is_some() {
        message.push_str(" [message truncated after 16384 characters]");
    }
    let version: String = version.chars().take(128).collect();
    let build: String = build.chars().take(128).collect();
    format!(
        "timestamp_unix_ms={timestamp} level={} component={component:?} message={message:?} pid={} version={version:?} build={build:?}",
        level.label(),
        std::process::id()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fields_stay_on_one_line_and_preserve_unicode() {
        let text = entry(
            Level::Error,
            "a\nb",
            "错误\r\n\"quoted\"\t",
            "v\n1",
            "rev\r2",
        );
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("错误\\r\\n\\\"quoted\\\"\\t"));
        assert!(text.contains("version=\"v\\n1\" build=\"rev\\r2\""));
        assert_eq!(header("a\nb").lines().count(), 1);
    }

    #[test]
    fn records_and_reports_have_bounded_fields() {
        let huge = "界".repeat(100_000);
        assert!(entry(Level::Trace, &huge, &huge, &huge, &huge).len() < 60_000);
        assert!(header(&huge).len() < 50_000);
    }
}
