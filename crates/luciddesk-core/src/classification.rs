//! Ordered, opt-in desktop classification rules. Matching never touches files.
use crate::{DesktopItem, ShellIdentity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Classification {
    pub enabled: bool,
    pub rules: Vec<ClassificationRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassificationRule {
    pub target_panel: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub name_contains: Option<String>,
}
const fn enabled() -> bool { true }

impl Classification {
    pub fn validate(&self) -> Result<(), String> {
        if self.rules.len() > 256 { return Err("at most 256 classification rules are supported".into()); }
        for rule in &self.rules {
            if rule.target_panel == 0 || (rule.extensions.is_empty() && rule.name_contains.is_none()) {
                return Err("classification rules require a target panel and a condition".into());
            }
            if rule.name_contains.as_ref().is_some_and(|s| s.trim().is_empty())
                || rule.extensions.iter().any(|s| s.trim_start_matches('.').is_empty() || s.contains(['/', '\\', '*', '?', ' '])) {
                return Err("classification conditions must not be blank or contain wildcard extensions".into());
            }
        }
        Ok(())
    }
}

impl ClassificationRule {
    pub fn matches(&self, item: &DesktopItem) -> bool {
        let ShellIdentity::FileSystem { path, .. } = item.identity() else { return false; };
        self.enabled
            && (self.extensions.is_empty() || path.extension().and_then(|s| s.to_str()).is_some_and(|ext|
                self.extensions.iter().any(|candidate| ext.eq_ignore_ascii_case(candidate.trim_start_matches('.')))))
            && self.name_contains.as_ref().is_none_or(|needle| item.display_name().to_lowercase().contains(&needle.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_real_extension_and_combines_conditions() {
        let item = DesktopItem::new(ShellIdentity::FileSystem { path: "Report.PDF".into(), volume_id: None, file_id: None }, "Report");
        let mut rule = ClassificationRule { target_panel: 1, enabled: true, extensions: vec![".pdf".into()], name_contains: Some("REPORT".into()) };
        assert!(rule.matches(&item));
        rule.name_contains = Some("invoice".into());
        assert!(!rule.matches(&item));
        rule.name_contains = None;
        rule.enabled = false;
        assert!(!rule.matches(&item));
    }
    #[test]
    fn rejects_accidental_catch_all() {
        assert!(Classification { enabled: true, rules: vec![ClassificationRule { target_panel: 1, enabled: true, extensions: vec![], name_contains: None }] }.validate().is_err());
    }
}
