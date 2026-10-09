//! CPU-only scaled pixels shared by renderers on the same UI thread.
//! GPU textures remain local to each rendering context and viewport.
use super::assets::Pixels;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Weak},
};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 128;
type Key = (usize, u32, u32);

struct Entry {
    source: Weak<Pixels>,
    pixels: Arc<Pixels>,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    clock: u64,
    bytes: usize,
}

impl Cache {
    fn get(&mut self, key: Key, source: &Arc<Pixels>) -> Option<Arc<Pixels>> {
        let entry = self.entries.get_mut(&key)?;
        if !entry
            .source
            .upgrade()
            .is_some_and(|old| Arc::ptr_eq(&old, source))
        {
            return None;
        }
        self.clock += 1;
        entry.used = self.clock;
        Some(Arc::clone(&entry.pixels))
    }

    fn insert(&mut self, key: Key, source: &Arc<Pixels>, pixels: Arc<Pixels>) {
        let bytes = pixels.data.capacity();
        // Do not keep source images alive or retain unbounded size variants.
        self.entries.retain(|old_key, entry| {
            if *old_key == key || entry.source.strong_count() == 0 {
                self.bytes -= entry.pixels.data.capacity();
                false
            } else {
                true
            }
        });
        if bytes > MAX_BYTES {
            return;
        }
        while self.bytes + bytes > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.bytes -= self.entries.remove(&key).unwrap().pixels.data.capacity();
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                source: Arc::downgrade(source),
                pixels,
                used: self.clock,
            },
        );
    }
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

pub(super) fn resample(
    source: &Arc<Pixels>,
    width: u32,
    height: u32,
) -> windows::core::Result<Arc<Pixels>> {
    if (source.width, source.height) == (width, height) {
        return Ok(Arc::clone(source));
    }
    let key = (Arc::as_ptr(source) as usize, width, height);
    if let Some(pixels) = CACHE.with(|cache| cache.borrow_mut().get(key, source)) {
        return Ok(pixels);
    }
    // WIC may enter COM; never hold a cache borrow across the call.
    let pixels = Arc::new(super::assets::resample(source, width, height)?.into_owned());
    CACHE.with(|cache| cache.borrow_mut().insert(key, source, Arc::clone(&pixels)));
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(size: u32) -> Arc<Pixels> {
        Arc::new(Pixels {
            width: size,
            height: size,
            data: vec![255; (size * size * 4) as usize],
        })
    }

    #[test]
    fn repeated_sizes_reuse_exact_pixels_and_changed_sources_do_not() {
        let _sta = crate::pane::test_support::apartment();
        let mut source = pixels(128);
        assert!(Arc::ptr_eq(&source, &resample(&source, 128, 128).unwrap()));
        let first = resample(&source, 48, 48).unwrap();
        let other_size = resample(&source, 64, 64).unwrap();
        assert_eq!(
            first.data,
            super::super::assets::resample(&source, 48, 48)
                .unwrap()
                .data
        );
        assert!(Arc::ptr_eq(&first, &resample(&source, 48, 48).unwrap()));
        assert!(!Arc::ptr_eq(&first, &other_size));
        Arc::make_mut(&mut source).data.fill(0);
        let changed = resample(&source, 48, 48).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
        assert!(changed.data.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn cache_bounds_variants_and_does_not_keep_sources_alive() {
        let mut cache = Cache::default();
        let source = pixels(128);
        for variant in 0..200 {
            cache.insert((1, variant, 1), &source, pixels(128));
            assert!(cache.bytes <= MAX_BYTES);
            assert!(cache.entries.len() <= MAX_ENTRIES);
        }
        let weak = Arc::downgrade(&source);
        drop(source);
        assert!(weak.upgrade().is_none());
        let next = pixels(16);
        cache.insert((2, 8, 8), &next, pixels(8));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 8 * 8 * 4);
        for variant in 0..200 {
            cache.insert((2, variant, 1), &next, pixels(1));
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(cache.get((2, 0, 1), &next).is_none());
        assert!(cache.get((2, 199, 1), &next).is_some());
    }

}
