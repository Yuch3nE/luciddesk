//! Application-wide CPU thumbnail budget and bounded viewport loading.
use super::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

pub(in crate::pane) type SharedCache = Arc<Mutex<Cache>>;
const IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const WORKERS: usize = 3;

struct Entry {
    identity: ShellIdentity,
    modified_time: Option<std::time::SystemTime>,
    folder: bool,
    size: Option<u64>,
    variants: Vec<Variant>,
}

struct Variant {
    image: Arc<assets::Pixels>,
    resolution: i32,
    used: u64,
    bytes: usize,
}

impl Entry {
    fn matches(&self, item: &Item) -> bool {
        // Unknown timestamps cannot prove that a file still has the same content.
        item.details.modified_time.is_some()
            && item.details.modified_time == self.modified_time
            && item.details.folder == self.folder
            && item.details.size == self.size
            && item.identity == self.identity
    }
}

pub(in crate::pane) struct Cache {
    entries: HashMap<String, Entry>,
    order: BTreeMap<u64, (String, i32)>,
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
    pub(super) fn touch(&mut self, keys: &[String], resolution: i32) {
        for key in keys {
            if let Some(entry) = self.entries.get_mut(key)
                .and_then(|entry| entry.variants.iter_mut().find(|variant| variant.resolution == resolution))
            {
                self.order.remove(&entry.used);
                self.clock += 1;
                entry.used = self.clock;
                self.order.insert(entry.used, (key.clone(), resolution));
            }
        }
    }

    fn remove(&mut self, key: &str, resolution: i32) {
        let Some(entry) = self.entries.get_mut(key) else { return; };
        if let Some(index) = entry.variants.iter().position(|variant| variant.resolution == resolution) {
            let variant = entry.variants.swap_remove(index);
            self.bytes -= variant.bytes;
            self.order.remove(&variant.used);
            if entry.variants.is_empty() {
                self.entries.remove(key);
            }
        }
    }

    fn remove_path(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            for variant in entry.variants {
                self.bytes -= variant.bytes;
                self.order.remove(&variant.used);
            }
        }
    }

    pub(super) fn restore(&mut self, item: &mut Item, resolution: i32) -> bool {
        let key = item.identity.persistent_key();
        let Some(entry) = self.entries.get_mut(&key) else { return false; };
        if !entry.matches(item) {
            self.remove_path(&key);
            return false;
        }
        let Some(variant) = entry.variants.iter_mut().find(|variant| variant.resolution == resolution) else {
            return false;
        };
        self.order.remove(&variant.used);
        self.clock += 1;
        variant.used = self.clock;
        self.order.insert(variant.used, (key, resolution));
        item.image = Some(Arc::clone(&variant.image));
        true
    }

    pub(super) fn insert(&mut self, item: &Item, resolution: i32) {
        if item.details.modified_time.is_none() {
            return;
        }
        let Some(image) = &item.image else { return; };
        let key = item.identity.persistent_key();
        if self.entries.get(&key).is_some_and(|entry| !entry.matches(item)) {
            self.remove_path(&key);
        } else {
            self.remove(&key, resolution);
        }
        let bytes = image.data.capacity();
        if bytes > self.limit {
            return;
        }
        while self.bytes + bytes > self.limit || self.order.len() >= MAX_ENTRIES {
            let Some((_, (key, size))) = self.order.first_key_value() else {
                break;
            };
            self.remove(&key.clone(), *size);
        }
        self.clock += 1;
        self.bytes += bytes;
        self.order.insert(self.clock, (key.clone(), resolution));
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            identity: item.identity.clone(),
            modified_time: item.details.modified_time,
            folder: item.details.folder,
            size: item.details.size,
            variants: Vec::with_capacity(1),
        });
        entry.variants.push(Variant {
            image: Arc::clone(image),
            resolution,
            used: self.clock,
            bytes,
        });
    }

    pub(super) fn retain_folder(&mut self, root: &Path, live: &HashMap<String, &Item>) {
        let removed: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                entry
                    .identity
                    .file_system_path()
                    .and_then(Path::parent)
                    == Some(root)
                    && !live.get(*key).is_some_and(|item| entry.matches(item))
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in removed {
            self.remove_path(&key);
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
        cache.touch(&[item("a").identity.persistent_key()], 128);
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
        cache.retain_folder(Path::new(r"C:\test"), &HashMap::new());
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn thumbnails_without_timestamps_are_published_without_using_the_cache_budget() {
        let commands = Commands::new().unwrap();
        let cache = Arc::new(Mutex::new(Cache { limit: 8, ..Default::default() }));
        let mut cached = item("cached");
        load(&mut cached);
        cache.lock().unwrap().insert(&cached, 128);

        let mut unknown = item("unknown");
        unknown.details.modified_time = None;
        let mut output = [unknown.clone()];
        run(vec![unknown], &commands, &cache, load, |_| {}, &mut output);
        assert!(output[0].image.is_some());

        let mut cache = cache.lock().unwrap();
        assert!(!cache.entries.contains_key(&output[0].identity.persistent_key()));
        assert!(cache.restore(&mut item("cached"), 128));
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn resolutions_share_the_budget_and_invalidate_together() {
        let mut cache = Cache { limit: 16, ..Default::default() };
        let mut small = item("same");
        let mut large = small.clone();
        load(&mut small);
        load(&mut large);
        Arc::make_mut(large.image.as_mut().unwrap()).data[0] = 1;
        let mut restored = item("same");
        cache.insert(&small, 64);
        cache.insert(&large, 128);
        for (resolution, value) in [(64, 0), (128, 1), (64, 0)] {
            assert!(cache.restore(&mut restored, resolution));
            assert_eq!(restored.image.as_ref().unwrap().data[0], value);
        }
        let mut other = item("other");
        load(&mut other);
        cache.insert(&other, 64);
        assert_eq!(cache.bytes, 16);
        assert!(!cache.restore(&mut restored, 128));
        assert!(cache.restore(&mut restored, 64));
        cache.insert(&large, 128);

        restored.details.size = Some(1);
        // A new resolution can detect a revision change even before its first insert.
        assert!(!cache.restore(&mut restored, 192));
        assert!(cache.entries.is_empty());
        assert!(cache.order.is_empty());
        assert_eq!(cache.bytes, 0);

        cache.insert(&small, 64);
        cache.insert(&large, 128);
        cache.retain_folder(Path::new(r"C:\test"), &HashMap::from([
            (restored.identity.persistent_key(), &restored),
        ]));
        assert_eq!(cache.bytes, 0);
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
