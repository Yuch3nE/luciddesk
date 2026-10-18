//! Stable panel, monitor, and Shell identifiers.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PanelId(u64);

impl PanelId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ShellIdentity {
    FileSystem {
        path: PathBuf,
        volume_id: Option<u64>,
        file_id: Option<u128>,
    },
    Namespace {
        parsing_name: String,
    },
}

impl ShellIdentity {
    #[must_use]
    pub fn persistent_key(&self) -> String {
        match self {
            Self::FileSystem {
                path,
                volume_id,
                file_id,
            } => match (volume_id, file_id) {
                (Some(volume_id), Some(file_id)) => {
                    format!("fsid:{volume_id:016x}:{file_id:032x}")
                }
                _ => format!("fs:{}", path.as_os_str().to_string_lossy().to_lowercase()),
            },
            Self::Namespace { parsing_name } => {
                format!("shell:{}", parsing_name.to_lowercase())
            }
        }
    }

    /// Compares two Shell identities using file IDs when available and paths as a migration
    /// fallback for records created before stable file identity was captured.
    #[must_use]
    pub fn equivalent_to(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::FileSystem {
                    path: left_path,
                    volume_id: left_volume,
                    file_id: left_file,
                },
                Self::FileSystem {
                    path: right_path,
                    volume_id: right_volume,
                    file_id: right_file,
                },
            ) => {
                let stable_match = match (left_volume, left_file, right_volume, right_file) {
                    (Some(left_volume), Some(left_file), Some(right_volume), Some(right_file)) => {
                        left_volume == right_volume && left_file == right_file
                    }
                    _ => false,
                };
                stable_match || paths_equal(left_path, right_path)
            }
            (
                Self::Namespace { parsing_name: left },
                Self::Namespace {
                    parsing_name: right,
                },
            ) => left.eq_ignore_ascii_case(right),
            _ => false,
        }
    }

    #[must_use]
    pub fn file_system_path(&self) -> Option<&Path> {
        match self {
            Self::FileSystem { path, .. } => Some(path),
            Self::Namespace { .. } => None,
        }
    }

    #[must_use]
    pub fn activation_name(&self) -> &OsStr {
        match self {
            Self::FileSystem { path, .. } => path.as_os_str(),
            Self::Namespace { parsing_name } => OsStr::new(parsing_name),
        }
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    if left.as_os_str() == right.as_os_str() { return true; }
    let left = left.as_os_str().to_string_lossy();
    let right = right.as_os_str().to_string_lossy();
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(&right)
    } else {
        left.to_lowercase() == right.to_lowercase()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MonitorId(String);

impl MonitorId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<u64> for PanelId {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl From<PanelId> for u64 {
    fn from(value: PanelId) -> Self {
        value.get()
    }
}

impl std::fmt::Display for PanelId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<String> for MonitorId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MonitorId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for MonitorId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for MonitorId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Default for MonitorId {
    fn default() -> Self {
        Self::new("primary")
    }
}

#[cfg(test)]
mod path_comparison_tests {
    use super::*;

    #[test]
    fn fast_paths_preserve_unicode_and_ascii_comparison() {
        let paths = ["C:/Folder/File.txt", "c:/folder/file.TXT", "C:/OTHER.txt",
            "C:/\u{6587}\u{4ef6}.txt", "C:/\u{c4}.txt", "C:/\u{e4}.txt", "C:/\u{39f}\u{3a3}.txt",
            "C:/\u{3bf}\u{3c2}.txt", "C:/\u{130}.txt", "C:/i\u{307}.txt", "C:/a/../b", "C:/b"];
        for left in paths {
            for right in paths {
                assert_eq!(paths_equal(Path::new(left), Path::new(right)),
                    left.to_lowercase() == right.to_lowercase(), "{left} / {right}");
            }
        }
    }
}
