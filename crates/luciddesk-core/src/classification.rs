//! Ordered, opt-in desktop classification rules. Matching never touches files.
use crate::{DesktopItem, ShellIdentity};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::fmt;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_regex: Option<String>,
}

const fn enabled() -> bool {
    true
}

#[derive(Debug)]
pub enum ClassificationError {
    InvalidRule(&'static str),
    InvalidRegex(regex::Error),
}

impl fmt::Display for ClassificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRule(message) => f.write_str(message),
            Self::InvalidRegex(error) => write!(f, "invalid keyword regular expression: {error}"),
        }
    }
}

impl std::error::Error for ClassificationError {}

impl Classification {
    pub fn validate(&self) -> Result<(), String> {
        self.compile().map(drop).map_err(|error| error.to_string())
    }

    /// Validate and prepare the ordered rules once for a classification pass.
    pub fn compile(&self) -> Result<Vec<CompiledClassificationRule<'_>>, ClassificationError> {
        if self.rules.len() > 256 {
            return Err(ClassificationError::InvalidRule(
                "at most 256 classification rules are supported",
            ));
        }
        self.rules.iter().map(ClassificationRule::compile).collect()
    }
}

enum Keyword {
    Literal(String),
    Regex(Regex),
}

pub struct CompiledClassificationRule<'a> {
    rule: &'a ClassificationRule,
    keyword: Option<Keyword>,
}

impl ClassificationRule {
    fn compile(&self) -> Result<CompiledClassificationRule<'_>, ClassificationError> {
        let literal = self.name_contains.as_deref();
        let pattern = self.name_regex.as_deref();
        if self.target_panel == 0
            || (self.extensions.is_empty() && literal.is_none() && pattern.is_none())
        {
            return Err(ClassificationError::InvalidRule(
                "classification rules require a target panel and a condition",
            ));
        }
        if literal.is_some() && pattern.is_some() {
            return Err(ClassificationError::InvalidRule(
                "classification rules accept one keyword condition per rule",
            ));
        }
        if literal
            .or(pattern)
            .is_some_and(|value| value.trim().is_empty())
            || self.extensions.iter().any(|s| {
                s.trim_start_matches('.').is_empty() || s.contains(['/', '\\', '*', '?', ' '])
            })
        {
            return Err(ClassificationError::InvalidRule(
                "classification conditions must not be blank or contain wildcard extensions",
            ));
        }
        let keyword = if let Some(pattern) = pattern {
            let regex = RegexBuilder::new(pattern)
                .case_insensitive(true)
                .size_limit(1024 * 1024)
                .build()
                .map_err(ClassificationError::InvalidRegex)?;
            Some(Keyword::Regex(regex))
        } else {
            literal.map(|needle| Keyword::Literal(needle.to_lowercase()))
        };
        Ok(CompiledClassificationRule {
            rule: self,
            keyword,
        })
    }

    pub fn matches(&self, item: &DesktopItem) -> bool {
        self.compile().is_ok_and(|rule| rule.matches(item))
    }
}

impl CompiledClassificationRule<'_> {
    pub fn target_panel(&self) -> u64 {
        self.rule.target_panel
    }

    pub fn matches(&self, item: &DesktopItem) -> bool {
        let ShellIdentity::FileSystem { path, .. } = item.identity() else {
            return false;
        };
        self.rule.enabled
            && (self.rule.extensions.is_empty()
                || path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| {
                        self.rule.extensions.iter().any(|candidate| {
                            ext.eq_ignore_ascii_case(candidate.trim_start_matches('.'))
                        })
                    }))
            && self.keyword.as_ref().is_none_or(|keyword| match keyword {
                Keyword::Literal(needle) => item.display_name().to_lowercase().contains(needle),
                Keyword::Regex(regex) => regex.is_match(item.display_name()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> DesktopItem {
        DesktopItem::new(
            ShellIdentity::FileSystem {
                path: "Report.PDF".into(),
                volume_id: None,
                file_id: None,
            },
            "Report",
        )
    }

    #[test]
    fn matches_real_extension_and_combines_conditions() {
        let item = report();
        let mut rule = ClassificationRule {
            target_panel: 1,
            enabled: true,
            extensions: vec![".pdf".into()],
            name_contains: Some("REPORT".into()),
            name_regex: None,
        };
        assert!(rule.matches(&item));
        rule.name_contains = Some("invoice".into());
        assert!(!rule.matches(&item));
        rule.name_contains = None;
        rule.enabled = false;
        assert!(!rule.matches(&item));
    }

    #[test]
    fn regex_matches_display_names_and_requires_the_extension_to_match() {
        let item = report();
        let mut rule = ClassificationRule {
            target_panel: 1,
            enabled: true,
            extensions: vec!["pdf".into()],
            name_contains: None,
            name_regex: Some("^REPORT$".into()),
        };
        assert!(rule.matches(&item));
        rule.extensions[0] = "docx".into();
        assert!(!rule.matches(&item));
        rule.extensions.clear();
        rule.name_regex = Some("^Report\\.PDF$".into());
        assert!(
            !rule.matches(&item),
            "use the displayed name without the hidden extension"
        );
        rule.name_regex = Some("[".into());
        assert!(matches!(
            rule.compile(),
            Err(ClassificationError::InvalidRegex(_))
        ));
        assert!(!rule.matches(&item));
        rule.name_regex = Some("Report".into());
        rule.name_contains = Some("Report".into());
        assert!(rule.compile().is_err());
    }

    #[test]
    fn rejects_accidental_catch_all() {
        assert!(
            Classification {
                enabled: true,
                rules: vec![ClassificationRule {
                    target_panel: 1,
                    enabled: true,
                    extensions: vec![],
                    name_contains: None,
                    name_regex: None,
                }],
            }
            .validate()
            .is_err()
        );
    }
}
