//! Desktop icon loading, refresh, and bounded pixel retention.
use super::{icon_changes, image_retention, refresh_views};
use crate::pane::{Loaded, PaneApp, assets};
use luciddesk_core::{DesktopPlacement, ShellIdentity, Workspace};
use luciddesk_shell::ShellApartment;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

fn pane_identities(workspace: &Workspace) -> impl Iterator<Item = &ShellIdentity> {
    workspace
        .desktop_items()
        .iter()
        .filter(|item| matches!(item.placement(), DesktopPlacement::Pane { .. }))
        .map(|item| item.identity())
}

fn pane_image_keys(workspace: &Workspace) -> HashSet<String> {
    pane_identities(workspace)
        .map(ShellIdentity::persistent_key)
        .collect()
}

pub(super) fn retain_pane_images(
    workspace: &Workspace,
    images: &mut HashMap<String, Arc<assets::Pixels>>,
    retention: &mut image_retention::ImageRetention,
    now: Instant,
) -> HashSet<String> {
    let live = pane_image_keys(workspace);
    retention.retain(&live, images, now);
    live
}

fn apply_pane_images(
    workspace: &Workspace,
    images: &mut HashMap<String, Arc<assets::Pixels>>,
    loaded: Vec<(String, assets::Pixels)>,
) -> bool {
    let live = pane_image_keys(workspace);
    let trace = luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace);
    let mut changed = false;
    for (key, image) in loaded {
        // A worker can complete after the item was released to Explorer.
        if !live.contains(&key) {
            continue;
        }
        let differs = images.get(&key).is_none_or(|old| {
            old.width != image.width || old.height != image.height || old.data != image.data
        });
        if trace {
            luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "pane.icons",
                "icon-result key={key} differs={differs} hash={:x}",
                image
                    .data
                    .iter()
                    .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(*b)))
            );
        }
        if differs {
            images.insert(key, crate::pane::image_pool::intern(Arc::new(image)));
            changed = true;
        }
    }
    changed
}

pub(super) fn tick(s: &mut PaneApp, invalidated: bool) -> bool {
    let h = s.session.as_mut().unwrap();
    if invalidated {
        h.image_retention.invalidate(&mut s.images);
    } else {
        h.image_retention.expire(&mut s.images, Instant::now());
    }
    let mut changed = false;
    let mut batch_completed = false;
    queue_pane_icons(s, false);
    while let Ok(loaded) = s.receiver.try_recv() {
        batch_completed = true;
        let live = pane_image_keys(&s.workspace);
        let h = s.session.as_mut().unwrap();
        h.initial_batches = h.initial_batches.saturating_sub(1);
        let successful: HashSet<_> = loaded.images.iter().map(|(key, _)| key.as_str()).collect();
        for key in &loaded.requested {
            h.requested.remove(key);
            if successful.contains(key.as_str()) || !live.contains(key) {
                h.icon_failures.remove(key);
            } else {
                let attempts = h
                    .icon_failures
                    .get(key)
                    .map_or(1, |(attempts, _)| attempts.saturating_add(1));
                h.icon_failures.insert(
                    key.clone(),
                    (
                        attempts,
                        Instant::now() + icon_retry_delay(attempts),
                    ),
                );
            }
        }
        changed |= apply_pane_images(&s.workspace, &mut s.images, loaded.images);
    }
    if batch_completed {
        // Continue large collections without waiting for the next heartbeat.
        queue_pane_icons(s, true);
    }
    if changed {
        refresh_views(s);
    }
    let refreshed = refresh_changed_icons(s);
    changed || refreshed
}
// Shell notifications are separate from layout revisions: Recycle Bin can change
// artwork without changing its identity, label, position or item count.
pub(super) struct RefreshJob {
    receiver: mpsc::Receiver<RefreshResult>,
    pending: icon_changes::Pending,
}

struct RefreshResult {
    images: Vec<(String, assets::Pixels)>,
    retry: icon_changes::Pending,
}

impl RefreshJob {
    fn poll(&self) -> Option<RefreshResult> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(RefreshResult {
                images: Vec::new(),
                retry: self.pending.clone(),
            }),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

fn refresh_batch(
    identities: Vec<ShellIdentity>,
    mut load: impl FnMut(&ShellIdentity) -> Result<assets::Pixels, String>,
) -> RefreshResult {
    let mut result = RefreshResult {
        images: Vec::new(),
        retry: Default::default(),
    };
    for identity in identities {
        match load(&identity) {
            Ok(image) => result.images.push((identity.persistent_key(), image)),
            Err(_) => result.retry.add([icon_changes::Change::Name(
                identity.activation_name().to_string_lossy().into_owned(),
            )]),
        }
    }
    result
}

fn retry_refresh(h: &mut super::Session, pending: icon_changes::Pending) {
    let delay = refresh_retry_delay(&mut h.icon_refresh_failures);
    h.icons_dirty.borrow_mut().merge(pending);
    h.icon_due = Some(Instant::now() + delay);
}

const INITIAL_MAX_ATTEMPTS: u32 = 5;

// Every scheduled batch must complete its bookkeeping, even if a Shell loader
// unwinds. Otherwise initial_batches/requested would block all later icon work.
struct InitialCompletion {
    sender: mpsc::Sender<Loaded>,
    wake: crate::pane::wake::Wake,
    requested: Vec<String>,
    images: Vec<(String, assets::Pixels)>,
}
impl Drop for InitialCompletion {
    fn drop(&mut self) {
        let _ = self.sender.send(Loaded {
            requested: std::mem::take(&mut self.requested),
            images: std::mem::take(&mut self.images),
        });
        self.wake.notify();
    }
}

// Retry transient Shell failures promptly without spinning on broken providers.
// Initial loading stops after five attempts; refresh work keeps a 32-second cap.
fn icon_retry_delay(failures: u32) -> Duration {
    Duration::from_millis(250 << failures.saturating_sub(1).min(7))
}

fn refresh_retry_delay(failures: &mut u32) -> Duration {
    *failures = failures.saturating_add(1).min(8);
    icon_retry_delay(*failures)
}

fn apply_refreshed_images(
    workspace: &Workspace,
    images: &mut HashMap<String, Arc<assets::Pixels>>,
    failures: &mut HashMap<String, (u32, Instant)>,
    loaded: Vec<(String, assets::Pixels)>,
) -> bool {
    let changed = apply_pane_images(workspace, images, loaded);
    // A refresh can recover a failed initial load, including unchanged pixels.
    failures.retain(|key, _| !images.contains_key(key));
    changed
}

pub(super) fn retry_deadline(
    busy: bool,
    last_scan: Instant,
    images: &HashMap<String, Arc<assets::Pixels>>,
    requested: &HashSet<String>,
    failures: &HashMap<String, (u32, Instant)>,
) -> Option<Instant> {
    // Completion wakes the supervisor and resumes work; timers cannot make
    // progress while another batch or refresh owns the loading slot.
    if busy {
        return None;
    }
    failures
        .iter()
        .filter(|(key, (attempts, _))| {
            *attempts < INITIAL_MAX_ATTEMPTS && !images.contains_key(*key) && !requested.contains(*key)
        })
        .map(|(_, (_, due))| (*due).max(last_scan + Duration::from_millis(250)))
        .min()
}

fn refresh_changed_icons(s: &mut PaneApp) -> bool {
    let h = s.session.as_mut().unwrap();
    if !h.icons_dirty.borrow().is_empty() {
        h.image_retention.invalidate(&mut s.images);
        if h.icon_due.is_none() {
            h.icon_due = Some(Instant::now() + Duration::from_millis(200));
        }
    }
    let completed = h.icon_reload.as_ref().and_then(RefreshJob::poll);
    let finished = completed.is_some();
    let mut changed = false;
    if let Some(result) = completed {
        h.icon_reload = None;
        changed = apply_refreshed_images(
            &s.workspace, &mut s.images, &mut h.icon_failures, result.images,
        );
        if result.retry.is_empty() {
            h.icon_refresh_failures = 0;
        } else {
            retry_refresh(h, result.retry);
        }
    }
    if h.initial_batches == 0
        && h.icon_reload.is_none()
        && h.icon_due.is_some_and(|due| Instant::now() >= due)
    {
        h.icon_due = None;
        let pending = std::mem::take(&mut *h.icons_dirty.borrow_mut());
        let identities: Vec<_> = pane_identities(&s.workspace).cloned().collect();
        if !identities.is_empty() {
            let size = h.snapshot.icon_size.max(128);
            let (sender, receiver) = mpsc::channel();
            let wake = h.wake.clone();
            let recovery = pending.clone();
            match std::thread::Builder::new()
                .name("pane-icon-refresh".into())
                .spawn(move || {
                    // As in the audit worker, close the channel before the exit
                    // notification. Early return and unwinding share this path.
                    let _exit = wake.on_drop();
                    let sender = sender;
                    let Ok(_sta) = ShellApartment::initialize_sta() else {
                        return;
                    };
                    let affected = pending.affected(identities);
                    if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace) {
                        luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "pane.icons",
                            "icon-targets {:?}",
                            affected
                                .iter()
                                .map(ShellIdentity::persistent_key)
                                .collect::<Vec<_>>()
                        );
                    }
                    let result = refresh_batch(affected, |identity| {
                        assets::load(identity, size).map_err(|e| e.to_string())
                    });
                    let _ = sender.send(result);
                }) {
                Ok(_) => {
                    h.icon_reload = Some(RefreshJob {
                        receiver,
                        pending: recovery,
                    });
                }
                Err(error) => {
                    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.hybrid.icons", &format!("Icon refresh worker failed: {error}"));
                    retry_refresh(h, recovery);
                }
            }
        }
    }
    if changed {
        refresh_views(s);
    }
    // Collection may have queued missing icons while this worker held the slot.
    // Even identical pixels (or worker failure) must trigger sync so it resumes
    // those loads immediately, bypassing the normal icon-scan throttle.
    changed || finished
}

const ICON_BATCH_SIZE: usize = 32;

pub(super) fn queue_pane_icons(s: &mut PaneApp, force: bool) {
    let h = s.session.as_mut().unwrap();
    // A batch already uses up to four Shell STAs. Serialise batches and refresh
    // work so repeated collection events cannot multiply threads and buffers.
    if h.initial_batches != 0 || h.icon_reload.is_some() {
        return;
    }
    if !force && h.last_icon_scan.elapsed() < Duration::from_millis(250) {
        return;
    }
    h.last_icon_scan = Instant::now();
    let requests =
        collect_icon_requests(&s.workspace, &s.images, &mut h.requested, &h.icon_failures);
    if requests.is_empty() {
        return;
    }
    let sender = h.sender.clone();
    let wake = h.wake.clone();
    let size = h.snapshot.icon_size.max(128);
    h.initial_batches += 1;
    std::thread::spawn(move || {
        let count = requests.len();
        let started = Instant::now();
        let mut completion = InitialCompletion {
            sender,
            wake,
            requested: requests.iter().map(ShellIdentity::persistent_key).collect(),
            images: Vec::new(),
        };
        completion.images = load_icon_batch(requests, size);
        if luciddesk_diagnostics::enabled(luciddesk_diagnostics::Level::Trace) {
            luciddesk_diagnostics::emit!(luciddesk_diagnostics::Level::Trace, "pane.icons",
                "startup-icon-batch requested={count} loaded={} elapsed_ms={}",
                completion.images.len(),
                started.elapsed().as_millis()
            );
        }
    });
}

fn collect_icon_requests(
    workspace: &Workspace,
    images: &HashMap<String, Arc<assets::Pixels>>,
    requested: &mut HashSet<String>,
    failures: &HashMap<String, (u32, Instant)>,
) -> Vec<ShellIdentity> {
    let now = Instant::now();
    pane_identities(workspace)
        .filter_map(|identity| {
            let key = identity.persistent_key();
            (!images.contains_key(&key)
                && failures
                    .get(&key)
                    .is_none_or(|(attempts, next)| *attempts < INITIAL_MAX_ATTEMPTS && now >= *next)
                && requested.insert(key))
            .then(|| identity.clone())
        })
        .take(ICON_BATCH_SIZE)
        .collect()
}

fn load_icon_batch(requests: Vec<ShellIdentity>, size: i32) -> Vec<(String, assets::Pixels)> {
    let mut batches = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for (index, identity) in requests.into_iter().enumerate() {
        batches[index % 4].push(identity);
    }
    std::thread::scope(|scope| {
        let workers: Vec<_> = batches
            .into_iter()
            .filter(|batch| !batch.is_empty())
            .map(|batch| {
                scope.spawn(move || {
                    let Ok(_sta) = ShellApartment::initialize_sta() else {
                        return Vec::new();
                    };
                    batch
                        .into_iter()
                        .filter_map(|identity| match assets::load(&identity, size) {
                            Ok(image) => Some((identity.persistent_key(), image)),
                            Err(error) => {
                                luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Error, "pane.hybrid.icons", &format!("Pane icon load failed: {error}"));
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    })
}

pub(in crate::pane) fn refresh_icons(state: &mut PaneApp) {
    if let Some(session) = &state.session {
        session
            .icons_dirty
            .borrow_mut()
            .add([icon_changes::Change::All]);
    }
}
