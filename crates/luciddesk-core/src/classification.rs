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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassificationRule {
    pub target_panel: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_type: Option<ClassificationItemType>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub name_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_regex: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationItemType {
    File,
    Folder,
    Shortcut,
}

const fn enabled() -> bool {
    true
}

#[derive(Debug)]
pub enum ClassificationError {
    TooManyRules,
    MissingTarget,
    MissingCondition,
    ConflictingKeywords,
    BlankKeyword,
    InvalidExtension,
    InvalidRegex(regex::Error),
}

impl fmt::Display for ClassificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRules => f.write_str("at most 256 classification rules are supported"),
            Self::MissingTarget => f.write_str("classification rules require a target panel"),
            Self::MissingCondition => f.write_str("classification rules require a matching condition"),
            Self::ConflictingKeywords => f.write_str("classification rules accept one keyword condition per rule"),
            Self::BlankKeyword => f.write_str("classification keyword conditions must not be blank"),
            Self::InvalidExtension => f.write_str("classification extensions must be nonempty and contain no whitespace, control characters or invalid filename characters"),
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
            return Err(ClassificationError::TooManyRules);
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
    /// Validate extensions entered in a rule editor or supplied in a settings update.
    /// Inventory matching and config loading accept the older file format so users can repair it.
    pub fn validate_extensions(&self) -> Result<(), ClassificationError> {
        if self.extensions.iter().any(|extension| {
            extension.trim_start_matches('.').is_empty() || extension.chars().any(|ch| {
                ch.is_whitespace() || ch.is_control() || r#"/\*?:<>"|"#.contains(ch)
            })
        }) {
            return Err(ClassificationError::InvalidExtension);
        }
        Ok(())
    }

    fn compile(&self) -> Result<CompiledClassificationRule<'_>, ClassificationError> {
        let literal = self.name_contains.as_deref();
        let pattern = self.name_regex.as_deref();
        if self.target_panel == 0 {
            return Err(ClassificationError::MissingTarget);
        }
        if self.item_type.is_none() && self.extensions.is_empty() && literal.is_none() && pattern.is_none() {
            return Err(ClassificationError::MissingCondition);
        }
        if literal.is_some() && pattern.is_some() {
            return Err(ClassificationError::ConflictingKeywords);
        }
        if literal.or(pattern).is_some_and(|value| value.trim().is_empty()) {
            return Err(ClassificationError::BlankKeyword);
        }
        // This check also runs when opening old config files. Apply the stricter
        // extension rules only to new input so an old rule can still be edited.
        if self.extensions.iter().any(|s| {
            s.trim_start_matches('.').is_empty() || s.contains(['/', '\\', '*', '?', ' '])
        }) {
            return Err(ClassificationError::InvalidExtension);
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

    pub fn requires_item_type(&self) -> bool {
        self.rule.enabled && self.rule.item_type.is_some()
    }

    /// Preserves matching without type metadata for rules that omit the type condition.
    pub fn matches(&self, item: &DesktopItem) -> bool {
        self.matches_with_type(item, None)
    }

    pub fn matches_with_type(
        &self,
        item: &DesktopItem,
        item_type: Option<ClassificationItemType>,
    ) -> bool {
        let ShellIdentity::FileSystem { path, .. } = item.identity() else {
            return false;
        };
        self.rule.enabled
            && self
                .rule
                .item_type
                .is_none_or(|required| item_type == Some(required))
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
    fn newly_edited_extensions_reject_whitespace_and_invalid_filename_characters() {
        let mut rule = ClassificationRule {
            target_panel: 1,
            enabled: true,
            item_type: None,
            extensions: vec![".PDF".into()],
            name_contains: None,
            name_regex: None,
        };
        assert!(rule.validate_extensions().is_ok());
        for extension in ["", ".", "*.pdf", "a/b", "a\\b", "a?b", "a:b", "a<b", "a>b", "a\"b", "a|b", "pdf\tdocx", "pdf\u{3000}docx", "a\u{0007}b"] {
            rule.extensions = vec![extension.into()];
            assert!(matches!(rule.validate_extensions(), Err(ClassificationError::InvalidExtension)), "{extension:?}");
        }
        rule.extensions = vec!["pdf\tdocx".into()];
        assert!(rule.compile().is_ok(), "the existing rule must remain readable for repair");
    }

    #[test]
    fn matches_real_extension_and_combines_conditions() {
        let item = report();
        let mut rule = ClassificationRule {
            target_panel: 1,
            enabled: true,
            item_type: None,
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
            item_type: None,
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
                    item_type: None,
                    extensions: vec![],
                    name_contains: None,
                    name_regex: None,
                }],
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn item_type_is_optional_and_combines_with_the_other_conditions() {
        let item = report();
        let mut config = ClassificationRule {
            target_panel: 1,
            enabled: true,
            item_type: None,
            extensions: vec!["pdf".into()],
            name_contains: Some("report".into()),
            name_regex: None,
        };
        assert!(
            config
                .compile()
                .unwrap()
                .matches_with_type(&item, Some(ClassificationItemType::Folder))
        );

        config.item_type = Some(ClassificationItemType::File);
        let rule = config.compile().unwrap();
        assert!(rule.matches_with_type(&item, Some(ClassificationItemType::File)));
        assert!(!rule.matches_with_type(&item, Some(ClassificationItemType::Folder)));
        assert!(!rule.matches_with_type(&item, Some(ClassificationItemType::Shortcut)));
        assert!(!rule.matches(&item));
        let mut other_name = item;
        other_name.set_display_name("Invoice");
        assert!(!rule.matches_with_type(&other_name, Some(ClassificationItemType::File)));

        let virtual_item = DesktopItem::new(
            ShellIdentity::Namespace {
                parsing_name: "test:report".into(),
            },
            "Report",
        );
        let just_type = ClassificationRule {
            item_type: Some(ClassificationItemType::Folder),
            extensions: Vec::new(),
            name_contains: None,
            ..config
        };
        let compiled = just_type.compile().unwrap();
        assert!(compiled.matches_with_type(&other_name, Some(ClassificationItemType::Folder)));
        assert!(!compiled.matches_with_type(&virtual_item, Some(ClassificationItemType::Folder)));
    }
}
