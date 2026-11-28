//! On-disk status for the temporary updater, which can outlive the main process.
use serde::{Deserialize, Serialize};

pub(super) const STATUS: &str = "status.json";
pub(super) const NEXT_STATUS: &str = "status.next";
// Before confirmation only the UI creates this file. Once the updater creates
// it, a late cancel request can no longer win the race with installation.
pub(super) const DECISION: &str = "decision";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Stage {
    #[default]
    Preparing,
    Downloading(u8),
    Verifying,
    Extracting,
    Confirming,
    Applying,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Appearance {
    pub confirm: String,
    pub cancel: String,
    pub copy: String,
    pub dark: bool,
    pub failed: String,
}
