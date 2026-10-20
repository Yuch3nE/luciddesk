//! Background and user-initiated release checks. Downloads remain manual.
mod http;
use crate::i18n;
use semver::Version;
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

const API: &str = "https://api.github.com/repos/Yuch3nE/luciddesk/releases/latest";
pub const PAGE: &str = "https://github.com/Yuch3nE/luciddesk/releases/latest";
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub fn automatic_enabled(store: &luciddesk_storage::WorkspaceStore) -> bool {
    store
        .preference("auto_check_updates")
        .ok()
        .flatten()
        .as_deref()
        != Some("false")
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

fn parse_release(bytes: &[u8], current: &str) -> Result<Option<String>, String> {
    let release: Release = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|e| e.to_string())?;
    let current = Version::parse(current).map_err(|e| e.to_string())?;
    Ok((version.pre.is_empty() && version > current).then(|| version.to_string()))
}

type CheckResult = Result<Option<String>, String>;
struct Work {
    receiver: mpsc::Receiver<CheckResult>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Work {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub struct Controller {
    work: Option<Work>,
    available: Option<String>,
    checked: bool,
    error: Option<String>,
    last_attempt: Option<std::time::Instant>,
    automatic: bool,
    notified: Option<String>,
}
impl Controller {
    /// Poll from the runtime, independently of the settings window's lifetime.
    /// Only a new version from an automatic check produces a notification.
    pub fn maintain(&mut self, enabled: bool, now: std::time::Instant) -> Option<String> {
        self.set_automatic_enabled(enabled);
        let completed_automatic = self.automatic && self.busy();
        self.poll();
        let notice = if enabled && completed_automatic && !self.busy() {
            self.available
                .as_ref()
                .filter(|version| self.notified.as_ref() != Some(version))
                .cloned()
        } else {
            None
        };
        if let Some(version) = &notice {
            self.notified = Some(version.clone());
        }
        if self.automatic_due(enabled, now) {
            if let Err(error) = self.check() {
                self.error = Some(error);
            }
            self.last_attempt = Some(now);
            self.automatic = self.busy();
        }
        notice
    }

    fn automatic_due(&self, enabled: bool, now: std::time::Instant) -> bool {
        enabled
            && !self.busy()
            && self
                .last_attempt
                .is_none_or(|last| now.saturating_duration_since(last) >= CHECK_INTERVAL)
    }

    pub fn set_automatic_enabled(&mut self, enabled: bool) {
        if !enabled && self.automatic {
            self.work = None;
            self.automatic = false;
        }
    }

    pub fn busy(&self) -> bool {
        self.work.is_some()
    }
    pub fn status(&self) -> String {
        if self.busy() {
            return i18n::text("update-checking").into();
        }
        if let Some(error) = &self.error {
            return format!("{}: {error}", i18n::text("update-failed"));
        }
        if let Some(version) = &self.available {
            return i18n::format("update-available", &[("version", version.clone())]);
        }
        i18n::text(if self.checked {
            "update-current"
        } else {
            "update-manual"
        })
        .into()
    }
    pub fn check(&mut self) -> Result<(), String> {
        self.start(|cancel| {
            let bytes = http::get(&cancel)?;
            parse_release(&bytes, env!("CARGO_PKG_VERSION"))
        })
    }
    fn start(
        &mut self,
        job: impl FnOnce(Arc<AtomicBool>) -> CheckResult + Send + 'static,
    ) -> Result<(), String> {
        if self.busy() {
            return Ok(());
        }
        self.last_attempt = Some(std::time::Instant::now());
        self.automatic = false;
        self.available = None;
        self.checked = false;
        self.error = None;
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::Builder::new()
            .name("release-check".into())
            .spawn(move || {
                let _ = sender.send(job(worker_cancel));
            })
            .map_err(|e| {
                self.error = Some(e.to_string());
                e.to_string()
            })?;
        self.work = Some(Work { receiver, cancel });
        Ok(())
    }
    pub fn poll(&mut self) {
        let Some(work) = &self.work else {
            return;
        };
        match work.receiver.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Update check worker stopped".into());
            }
            Ok(Ok(available)) => {
                self.available = available;
                self.checked = true;
            }
            Ok(Err(error)) => self.error = Some(error),
        }
        self.work = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_checks_obey_switch_interval_and_busy_work() {
        let now = std::time::Instant::now();
        let mut controller = Controller::default();
        assert!(!controller.automatic_due(false, now));
        assert!(controller.maintain(false, now).is_none());
        assert!(!controller.busy());
        assert!(controller.automatic_due(true, now));
        controller.last_attempt = Some(now);
        assert!(!controller.automatic_due(
            true,
            now + CHECK_INTERVAL - std::time::Duration::from_secs(1)
        ));
        assert!(controller.automatic_due(true, now + CHECK_INTERVAL));
        let (_sender, receiver) = mpsc::channel();
        controller.work = Some(Work {
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        assert!(!controller.automatic_due(true, now + CHECK_INTERVAL));
        // Disabling automatic checks must leave a user-initiated check running.
        controller.set_automatic_enabled(false);
        assert!(controller.busy());
        controller.automatic = true;
        let cancel = controller.work.as_ref().unwrap().cancel.clone();
        controller.set_automatic_enabled(false);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!controller.busy());
        assert!(controller.maintain(false, now + CHECK_INTERVAL).is_none());
    }

    fn completed(
        controller: &mut Controller,
        result: CheckResult,
        automatic: bool,
        now: std::time::Instant,
    ) {
        let (sender, receiver) = mpsc::channel();
        sender.send(result).unwrap();
        controller.work = Some(Work {
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        controller.automatic = automatic;
        controller.last_attempt = Some(now);
    }

    #[test]
    fn automatic_results_notify_once_per_version_and_failures_stay_silent() {
        let now = std::time::Instant::now();
        let mut controller = Controller::default();
        completed(&mut controller, Ok(Some("1.0.0".into())), true, now);
        assert_eq!(controller.maintain(true, now).as_deref(), Some("1.0.0"));
        assert!(controller.maintain(true, now).is_none());
        completed(&mut controller, Ok(Some("1.0.0".into())), true, now);
        assert!(controller.maintain(true, now).is_none());
        completed(&mut controller, Err("offline".into()), true, now);
        assert!(controller.maintain(true, now).is_none());
        assert!(!controller.busy());
        assert!(!controller.automatic_due(true, now));
        completed(&mut controller, Ok(Some("1.1.0".into())), false, now);
        assert!(controller.maintain(true, now).is_none());
        assert_eq!(controller.available.as_deref(), Some("1.1.0"));
        completed(&mut controller, Ok(Some("1.1.0".into())), true, now);
        assert_eq!(controller.maintain(true, now).as_deref(), Some("1.1.0"));
        completed(&mut controller, Ok(Some("1.2.0".into())), true, now);
        assert!(controller.maintain(false, now).is_none());
        completed(&mut controller, Ok(None), true, now);
        assert!(controller.maintain(true, now).is_none());
        assert!(controller.available.is_none());
    }

    fn release(tag: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "tag_name": tag, "draft": false, "prerelease": false
        }))
        .unwrap()
    }
    #[test]
    fn numeric_versions_and_both_tag_styles_without_packages() {
        for tag in ["v0.10.0", "0.10.0"] {
            assert_eq!(
                parse_release(&release(tag), "0.9.9").unwrap(),
                Some("0.10.0".into())
            );
            assert_eq!(parse_release(&release(tag), "0.10.0").unwrap(), None);
            assert_eq!(parse_release(&release(tag), "0.11.0").unwrap(), None);
        }
    }
    #[test]
    fn rejects_preview_releases_and_invalid_metadata() {
        for flag in ["draft", "prerelease"] {
            let mut value: serde_json::Value = serde_json::from_slice(&release("v0.15.0")).unwrap();
            value[flag] = true.into();
            assert_eq!(
                parse_release(&serde_json::to_vec(&value).unwrap(), "0.14.0").unwrap(),
                None
            );
        }
        assert_eq!(
            parse_release(&release("0.15.0-beta.1"), "0.14.0").unwrap(),
            None
        );
        assert!(parse_release(&release("invalid"), "0.14.0").is_err());
        assert!(parse_release(b"invalid JSON", "0.14.0").is_err());
    }
    fn wait(controller: &mut Controller) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while controller.busy() {
            assert!(
                std::time::Instant::now() < deadline,
                "release check timed out"
            );
            controller.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn check_failure_can_be_retried_and_close_cancels_work() {
        let mut controller = Controller::default();
        controller.start(|_| Err("offline".into())).unwrap();
        wait(&mut controller);
        assert_eq!(controller.error.as_deref(), Some("offline"));
        assert!(!controller.checked);
        controller.start(|_| Ok(Some("0.15.0".into()))).unwrap();
        wait(&mut controller);
        assert_eq!(controller.available.as_deref(), Some("0.15.0"));
        assert!(controller.error.is_none());
        controller.start(|_| Ok(None)).unwrap();
        let cancel = controller.work.as_ref().unwrap().cancel.clone();
        drop(controller);
        assert!(cancel.load(Ordering::Relaxed));
    }
    #[test]
    #[ignore = "Checks GitHub metadata without downloading packages or opening a browser"]
    fn live_release_check() {
        let mut controller = Controller::default();
        controller.check().unwrap();
        wait(&mut controller);
        assert!(controller.error.is_none(), "{:?}", controller.error);
        assert!(controller.checked);
    }
}
