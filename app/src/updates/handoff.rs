//! On-disk status for the temporary updater, which can outlive the main process.
use serde::{Deserialize, Serialize};
use std::{fs::OpenOptions, io, path::Path};

pub(super) const STATUS: &str = "status.json";
pub(super) const NEXT_STATUS: &str = "status.next";
// Settings and the updater compete for this file at the end of preparation.
// Once the updater claims it, the confirmation dialog owns cancellation.
pub(super) const DECISION: &str = "decision";

pub(super) fn claim_decision(directory: &Path) -> io::Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(directory.join(DECISION)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

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
