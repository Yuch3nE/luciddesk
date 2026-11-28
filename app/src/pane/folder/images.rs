//! Application-wide CPU thumbnail budget and bounded viewport loading.
use super::*;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

pub(in crate::pane) type SharedCache = Arc<Mutex<Cache>>;
const IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const WORKERS: usize = 3;

struct Entry {
    identity: ShellIdentity,
    image: Arc<assets::Pixels>,
    resolution: i32,
    modified_time: Option<std::time::SystemTime>,
    folder: bool,
    size: Option<u64>,
    used: u64,
    bytes: usize,
}

pub(in crate::pane) struct Cache {
    entries: HashMap<String, Entry>,
    order: BTreeMap<u64, String>,
    clock: u64,
    bytes: usize,
    limit: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: BTreeMap::new(),
            clock: 0,
            bytes: 0,
            limit: IMAGE_BYTES,
        }
    }
}

impl Cache {
    pub(super) fn touch(&mut self, keys: &[String]) {
        for key in keys {
            if let Some(entry) = self.entries.get_mut(key) {
                self.order.remove(&entry.used);
                self.clock += 1;
                entry.used = self.clock;
                self.order.insert(entry.used, key.clone());
            }
        }
    }
    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes -= entry.bytes;
            self.order.remove(&entry.used);
        }
    }

    pub(super) fn restore(&mut self, item: &mut Item, resolution: i32) -> bool {
        let key = item.identity.persistent_key();
        let Some(entry) = self.entries.get_mut(&key) else {
            return false;
        };
        // Unknown timestamps cannot prove that a file still has the same content.
        if item.details.modified_time.is_none()
            || item.details.modified_time != entry.modified_time
            || item.details.folder != entry.folder
            || item.details.size != entry.size
            || item.identity != entry.identity
        {
            self.remove(&key);
            return false;
        }
        if entry.resolution != resolution { return false; }
        self.order.remove(&entry.used);
        self.clock += 1;
        entry.used = self.clock;
        self.order.insert(entry.used, key);
        item.image = Some(Arc::clone(&entry.image));
        true
    }

    pub(super) fn insert(&mut self, item: &Item, resolution: i32) {
        let key = item.identity.persistent_key();
        // Failed loads are retried, so an empty image has no reusable cache value.
        let Some(image) = &item.image else { return; };
        self.remove(&key);
        let bytes = image.data.capacity();
        if bytes > self.limit {
            return;
        }
        while self.bytes + bytes > self.limit || self.entries.len() >= MAX_ENTRIES {
            let Some((_, key)) = self.order.first_key_value() else {
                break;
            };
            self.remove(&key.clone());
        }
        self.clock += 1;
        self.bytes += bytes;
        self.order.insert(self.clock, key.clone());
        self.entries.insert(
            key,
            Entry {
                identity: item.identity.clone(),
                image: Arc::clone(image),
                resolution,
                modified_time: item.details.modified_time,
                folder: item.details.folder,
                size: item.details.size,
                used: self.clock,
                bytes,
            },
        );
    }

    pub(super) fn retain_folder(&mut self, root: &Path, live: &HashSet<String>) {
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                entry
                    .identity
                    .file_system_path()
                    .and_then(Path::parent)
                    == Some(root)
                    && !live.contains(*key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in removed {
            self.remove(&key);
        }
    }
}

/// Shell objects stay inside their worker's STA. Only pixel Arcs cross threads.
pub(super) fn enrich(
    request: ImageRequest,
    commands: &Commands,
    cache: &SharedCache,
    images: &mpsc::SyncSender<Patch>,
    wake: &wake::Wake,
) {
    use std::sync::atomic::Ordering;
    let ImageRequest { generation, size, items } = request;
    let current = || !commands.stop.load(Ordering::Acquire)
        && commands.active.load(Ordering::Acquire)
        && commands.image_generation.load(Ordering::Acquire) == generation;
    load_jobs(
        items, size, cache, current,
        |item| item.image = assets::load(&item.identity, size).ok().map(Arc::new),
        |batch| {
            if current() && images.send(Patch::Images(generation, batch)).is_ok() {
                wake.notify();
            }
        },
    );
}

fn load_jobs(
    jobs: Vec<Item>,
    resolution: i32,
    cache: &SharedCache,
    current: impl Fn() -> bool + Sync,
    load: impl Fn(&mut Item) + Sync,
    mut publish: impl FnMut(Vec<Item>),
) {
    if jobs.is_empty() { return; }
    let queue = Mutex::new(VecDeque::from(jobs));
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(WORKERS * 2);
        for _ in 0..WORKERS {
            let sender = sender.clone();
            let queue = &queue;
            let load = &load;
            let current = &current;
            scope.spawn(move || {
                let Ok(_apartment) = ShellApartment::initialize_sta() else { return; };
                while current() {
                    let Some(mut item) = queue.lock().unwrap().pop_front() else { break; };
                    let restored = cache.lock().unwrap().restore(&mut item, resolution);
                    if !restored {
                        load(&mut item);
                        if let Some(image) = item.image.take() {
                            item.image = Some(super::super::image_pool::intern(image));
                        }
                        if current() {
                            cache.lock().unwrap().insert(&item, resolution);
                        }
                    }
                    if !current() || sender.send(item).is_err() { break; }
                }
            });
        }
        drop(sender);
        let mut batch = Vec::new();
        let mut published = Instant::now();
        loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(item) => batch.push(item),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !batch.is_empty() && (batch.len() >= 32 || published.elapsed() >= Duration::from_millis(100)) {
                publish(std::mem::take(&mut batch));
                published = Instant::now();
            }
        }
        if !batch.is_empty() {
            publish(batch);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn run(jobs: Vec<Item>, commands: &Commands, cache: &SharedCache,
        load: impl Fn(&mut Item) + Sync, mut publish: impl FnMut(Vec<Item>), items: &mut [Item],
    ) {
        load_jobs(jobs, 128, cache,
            || !commands.stop.load(Ordering::Acquire) && commands.active.load(Ordering::Acquire),
            load,
            |batch| {
                for update in &batch {
                    if let Some(item) = items.iter_mut().find(|item| item.identity == update.identity) {
                        *item = update.clone();
                    }
                }
                publish(batch);
            },
        );
    }

    fn item(name: &str) -> Item {
        Item {
            identity: ShellIdentity::FileSystem {
                path: PathBuf::from(format!(r"C:\test\{name}")),
                volume_id: None,
                file_id: None,
            },
            label: name.into(),
            image: None,
            details: ItemDetails {
                modified_time: Some(std::time::UNIX_EPOCH),
                ..Default::default()
            },
        }
    }
    fn load(item: &mut Item) {
        item.details.kind = "test".into();
        item.image = Some(Arc::new(assets::Pixels {
            width: 2,
            height: 1,
            data: vec![0; 8],
        }));
    }

    #[test]
    fn cache_budget_counts_reserved_pixel_storage() {
        let mut cache = Cache { limit: 16, ..Default::default() };
        let mut value = item("reserved");
        let mut data = Vec::with_capacity(32);
        data.resize(8, 0);
        value.image = Some(Arc::new(assets::Pixels { width: 2, height: 1, data }));
        cache.insert(&value, 128);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn independent_folder_caches_share_loaded_pixels() {
        let commands = Commands::new().unwrap();
        let first_cache: SharedCache = Arc::default();
        let second_cache: SharedCache = Arc::default();
        let mut first = vec![item("first")];
        let mut second = vec![item("second")];
        run(first.clone(), &commands, &first_cache, load, |_| {}, &mut first);
        run(second.clone(), &commands, &second_cache, load, |_| {}, &mut second);
        assert!(Arc::ptr_eq(
            first[0].image.as_ref().unwrap(),
            second[0].image.as_ref().unwrap(),
        ));
        let weak = Arc::downgrade(first[0].image.as_ref().unwrap());
        drop((first, second, first_cache, second_cache));
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn parallel_loading_reuses_pixels_after_navigation_and_reloads_changed_files() {
        let commands = Commands::new().unwrap();
        let cache: SharedCache = Arc::default();
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let calls = AtomicUsize::new(0);
        let mut items: Vec<_> = (0..12).map(|i| item(&format!("file-{i}"))).collect();
        let mut delivered = 0;
        let start = Instant::now();
        run(
            items.clone(),
            &commands,
            &cache,
            |item| {
                let concurrent = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(concurrent, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                load(item);
                calls.fetch_add(1, Ordering::SeqCst);
                active.fetch_sub(1, Ordering::SeqCst);
            },
            |batch| delivered += batch.len(),
            &mut items,
        );
        eprintln!(
            "12 image jobs: {:?}, peak workers={}, patches={delivered}",
            start.elapsed(),
            peak.load(Ordering::SeqCst)
        );
        assert!((2..=WORKERS).contains(&peak.load(Ordering::SeqCst)));
        assert_eq!(delivered, 12);
        assert_eq!(calls.load(Ordering::SeqCst), 12);
        let old_images: Vec<_> = items.iter().map(|i| i.image.clone().unwrap()).collect();
        assert!(old_images.iter().all(|image| Arc::ptr_eq(image, &old_images[0])));

        // Another directory uses the same bounded cache between the two visits.
        let mut child = item(r"child\nested");
        load(&mut child);
        cache.lock().unwrap().insert(&child, 128);
        for item in &mut items {
            item.image = None;
            item.details.kind.clear();
        }
        run(
            items.clone(),
            &commands,
            &cache,
            |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            },
            |_| {},
            &mut items,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            12,
            "back navigation must not invoke the image loader"
        );
        assert!(
            items
                .iter()
                .zip(&old_images)
                .all(|(item, image)| Arc::ptr_eq(item.image.as_ref().unwrap(), image))
        );

        items[0].details.modified_time = Some(std::time::UNIX_EPOCH + Duration::from_secs(1));
        items[0].image = None;
        run(
            items.clone(),
            &commands,
            &cache,
            |item| {
                calls.fetch_add(1, Ordering::SeqCst);
                load(item);
                Arc::make_mut(item.image.as_mut().unwrap()).data[0] = 1;
            },
            |_| {},
            &mut items,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 13);
        assert!(!Arc::ptr_eq(
            items[0].image.as_ref().unwrap(),
            &old_images[0]
        ));
    }

    #[test]
    fn slow_image_consumer_bounds_work_and_cancellation_releases_workers() {
        let commands = Commands::new().unwrap();
        let cache: SharedCache = Arc::default();
        let calls = AtomicUsize::new(0);
        let mut items: Vec<_> = (0..1000).map(|index| item(&index.to_string())).collect();
        let mut first_batch = true;
        run(items.clone(), &commands, &cache, |item| {
            load(item);
            calls.fetch_add(1, Ordering::SeqCst);
        }, |batch| {
            assert!(batch.len() <= 32);
            if first_batch {
                first_batch = false;
                std::thread::sleep(Duration::from_millis(50));
                // One delivered batch, a bounded worker queue, and one result per worker.
                assert!(calls.load(Ordering::SeqCst) <= 32 + WORKERS * 3);
                commands.stop.store(true, Ordering::Release);
            }
        }, &mut items);
        assert!(!first_batch);
        assert!(calls.load(Ordering::SeqCst) < 1000);
    }

    #[test]
    fn cache_evicts_by_bytes_but_keeps_recent_visible_items_and_retries_failures() {
        let mut cache = Cache {
            limit: 16,
            ..Default::default()
        };
        for name in ["a", "b"] {
            let mut value = item(name);
            load(&mut value);
            cache.insert(&value, 128);
        }
        cache.touch(&[item("a").identity.persistent_key()]);
        let mut value = item("c");
        load(&mut value);
        cache.insert(&value, 128);
        assert_eq!(cache.bytes, 16);
        assert!(cache.restore(&mut item("a"), 128));
        assert!(!cache.restore(&mut item("b"), 128));
        assert!(cache.restore(&mut item("c"), 128));
        let failed = item("failure");
        cache.insert(&failed, 128);
        // A refresh must retry extraction immediately after a transient failure.
        assert!(!cache.entries.contains_key(&item("failure").identity.persistent_key()));
        assert!(!cache.restore(&mut item("failure"), 128));
        cache.retain_folder(Path::new(r"C:\test"), &HashSet::new());
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn navigation_cancellation_stops_queued_work() {
        let commands = Commands::new().unwrap();
        let cache: SharedCache = Arc::default();
        let calls = AtomicUsize::new(0);
        let mut items: Vec<_> = (0..1000).map(|i| item(&i.to_string())).collect();
        run(
            items.clone(),
            &commands,
            &cache,
            |item| {
                calls.fetch_add(1, Ordering::SeqCst);
                commands.stop.store(true, Ordering::Release);
                load(item);
            },
            |_| {},
            &mut items,
        );
        assert!((1..=WORKERS).contains(&calls.load(Ordering::SeqCst)));
    }
}
