//! Bounded, cancellable metadata reads for pure CLI previews, off the UI thread.
use super::*;
use luciddesk_api::{FolderColumn, Operation, Plan};
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;

const MAX_WORKERS: usize = 4;

struct WorkerPermit(Arc<AtomicUsize>);
impl Drop for WorkerPermit {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::AcqRel); }
}

struct Job {
    signature: String,
    expires: Instant,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<HashMap<String, ItemDetails>>,
    result: Option<HashMap<String, ItemDetails>>,
}
impl Drop for Job {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); }
}
#[derive(Default)]
pub(super) struct Cache {
    jobs: HashMap<String, Job>,
    active: Arc<AtomicUsize>,
}
impl Cache {
    pub(super) fn prepare(&mut self, workspace: &Workspace, plan: &Plan, request_id: &str,
        ids: &HashMap<String, String>) -> Result<Option<&HashMap<String, ItemDetails>>, (&'static str, String)> {
        let panes: Vec<_> = plan.operations.iter().filter_map(|op| match op {
            Operation::Sort { pane_id, sort_column, .. } if *sort_column != FolderColumn::Name => Some(pane_id.as_str()),
            _ => None,
        }).collect();
        if panes.is_empty() { return Ok(None); }
        let signature = serde_json::to_string(plan).unwrap();
        if let Some(permit) = self.reserve(request_id, &signature)? {
            // Include assigned items so sort can follow membership changes in the same plan.
            let assigned: HashSet<_> = plan.operations.iter().filter_map(|op| match op {
                Operation::Assign { item_ids, .. } => Some(item_ids.iter()), _ => None,
            }).flatten().collect();
            let inventory: Vec<_> = workspace.desktop_items().iter().filter(|item| {
                matches!(item.placement(), DesktopPlacement::Pane { pane_id, .. } if panes.contains(&pane_id.get().to_string().as_str()))
                    || ids.get(&item.identity().persistent_key()).is_some_and(|id| assigned.contains(id))
            }).map(|item| item.identity().clone()).collect();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = Arc::clone(&cancel);
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new().name("cli-sort-metadata".into()).spawn(move || {
                // Cancellation requests exit but cannot interrupt a blocked read.
                // Release after COM cleanup or unwinding; failed spawn drops it too.
                let _permit = permit;
                let _sta = luciddesk_shell::ShellApartment::initialize_sta().ok();
                let mut result = HashMap::new();
                for identity in inventory {
                    if worker_cancel.load(Ordering::Relaxed) { return; }
                    let details = super::super::sorting::file_details(&identity);
                    if worker_cancel.load(Ordering::Relaxed) { return; }
                    result.insert(identity.persistent_key(), details);
                }
                let _ = sender.send(result);
            }).map_err(|e| ("CAPABILITY_UNAVAILABLE", e.to_string()))?;
            self.jobs.insert(request_id.into(), Job { signature, expires: Instant::now() + Duration::from_secs(30), cancel, receiver, result: None });
        }
        let job = self.jobs.get_mut(request_id).unwrap();
        if job.result.is_none() {
            match job.receiver.try_recv() {
                Ok(result) => job.result = Some(result),
                Err(mpsc::TryRecvError::Empty) => return Err(("SORT_METADATA_PENDING", "Preview is reading metadata; retry the same preview request ID and plan. Nothing has been saved.".into())),
                Err(mpsc::TryRecvError::Disconnected) => return Err(("CAPABILITY_UNAVAILABLE", "sort metadata worker stopped".into())),
            }
        }
        Ok(job.result.as_ref())
    }
    fn reserve(&mut self, request_id: &str, signature: &str) -> Result<Option<WorkerPermit>, (&'static str, String)> {
        self.jobs.retain(|_, job| job.expires > Instant::now());
        if self.jobs.get(request_id).is_some_and(|job| job.signature != signature) {
            self.jobs.remove(request_id);
        }
        if self.jobs.contains_key(request_id) { return Ok(None); }
        if self.jobs.len() >= MAX_WORKERS || self.active.fetch_update(Ordering::AcqRel, Ordering::Acquire,
            |n| (n < MAX_WORKERS).then_some(n + 1)).is_err() {
            return Err(("BUSY", "sort metadata workers are busy; retry preview later".into()));
        }
        Ok(Some(WorkerPermit(Arc::clone(&self.active))))
    }
    pub(super) fn remove(&mut self, id: &str) { self.jobs.remove(id); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaced_removed_and_expired_jobs_keep_slots_until_blocked_workers_exit() {
        let mut cache = Cache::default();
        let mut workers = Vec::new();
        let mut releases = Vec::new();
        for i in 0..MAX_WORKERS {
            let signature = i.to_string();
            let permit = cache.reserve("same-request", &signature).unwrap().unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let (sender, receiver) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            workers.push(std::thread::spawn(move || {
                let _permit = permit;
                // Model an uninterruptible Shell/filesystem call.
                wait.recv().unwrap();
                let _ = sender.send(HashMap::new());
            }));
            releases.push(release);
            cache.jobs.insert("same-request".into(), Job { signature,
                expires: Instant::now() + Duration::from_secs(30), cancel: Arc::clone(&cancel), receiver, result: None });
            match i {
                0 => { cache.remove("same-request"); assert!(cancel.load(Ordering::Relaxed)); }
                1 => { cache.jobs.get_mut("same-request").unwrap().expires = Instant::now(); }
                _ => {},
            }
        }
        for i in 0..20 {
            assert_eq!(cache.reserve("same-request", &format!("replacement-{i}")).err().unwrap().0, "BUSY");
            assert_eq!(cache.active.load(Ordering::Acquire), MAX_WORKERS);
        }
        for release in releases { release.send(()).unwrap(); }
        for worker in workers { worker.join().unwrap(); }
        assert_eq!(cache.active.load(Ordering::Acquire), 0);
        let permit = cache.reserve("same-request", "recovered").unwrap().unwrap();
        drop(permit); // Also covers abandoning a reservation before spawning.
        assert_eq!(cache.active.load(Ordering::Acquire), 0);
    }
    #[test]
    fn removing_a_job_cancels_it_and_releases_the_reply_channel() {
        let mut cache = Cache::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        cache.jobs.insert("request".into(), Job { signature: String::new(), expires: Instant::now(),
            cancel: Arc::clone(&cancel), receiver, result: None });
        cache.remove("request");
        assert!(cancel.load(Ordering::Relaxed));
        assert!(sender.send(HashMap::new()).is_err());
    }
}
