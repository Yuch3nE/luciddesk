use super::*;
use crate::pane::assets::RECYCLE_BIN_PARSING_NAME;
use luciddesk_core::{DesktopItem, GridPosition, PanelId};

#[test]
fn refresh_retries_only_failed_icons_and_preserves_new_notifications() {
    let identities: Vec<_> = ["test:a", "test:b"]
        .into_iter()
        .map(|name| ShellIdentity::Namespace {
            parsing_name: name.into(),
        })
        .collect();
    let first = refresh_batch(identities.clone(), |id| {
        if id == &identities[1] {
            Err("temporary loader failure".into())
        } else {
            Ok(pixels())
        }
    });
    assert_eq!(first.images.len(), 1);
    assert_eq!(
        first.retry.affected(identities.clone()),
        vec![identities[1].clone()]
    );
    let mut pending = icon_changes::Pending::default();
    pending.add([icon_changes::Change::Name("test:a".into())]);
    pending.merge(first.retry);
    assert_eq!(pending.affected(identities.clone()), identities);
    let retry = refresh_batch(identities, |_| Ok(pixels()));
    assert_eq!(retry.images.len(), 2);
    assert!(retry.retry.is_empty());
}

#[test]
fn refresh_worker_disconnect_recovers_notifications_and_backoff_is_capped() {
    let (sender, receiver) = mpsc::channel();
    let mut pending = icon_changes::Pending::default();
    pending.add([icon_changes::Change::All]);
    let job = RefreshJob { receiver, pending };
    assert!(job.poll().is_none());
    drop(sender); // Covers early STA failure / worker exit before a result.
    let result = job.poll().unwrap();
    assert!(result.images.is_empty());
    assert!(!result.retry.is_empty());
    let mut failures = 0;
    for millis in [250, 500, 1000, 2000, 4000, 8000, 16000, 32000, 32000] {
        assert_eq!(
            refresh_retry_delay(&mut failures),
            Duration::from_millis(millis)
        );
    }
}

#[test]
fn bounded_batches_skip_cached_pending_failed_and_released_items() {
    let mut workspace = Workspace::new();
    workspace.reconcile_desktop_items((0..100).map(|index| {
        let mut item = DesktopItem::new(
            ShellIdentity::Namespace {
                parsing_name: format!("test:{index}"),
            },
            index.to_string(),
        );
        item.set_placement(DesktopPlacement::Pane {
            pane_id: PanelId::new(1),
            position: GridPosition::new(index, 0),
        });
        item
    }));
    let key = |index: usize| workspace.desktop_items()[index].identity().persistent_key();
    let images = HashMap::from([(
        key(0),
        Arc::new(assets::Pixels {
            width: 1,
            height: 1,
            data: vec![0; 4],
        }),
    )]);
    let mut pending = std::collections::HashSet::from([key(1)]);
    let failures = HashMap::from([
        (key(2), (5, Instant::now())),
        (key(3), (1, Instant::now() + Duration::from_secs(60))),
    ]);
    let first = collect_icon_requests(&workspace, &images, &mut pending, &failures);
    assert_eq!(first.len(), ICON_BATCH_SIZE);
    assert_eq!(first[0], workspace.desktop_items()[4].identity().clone());
    workspace.desktop_items_mut()[40].set_placement(DesktopPlacement::default());
    let second = collect_icon_requests(&workspace, &images, &mut pending, &failures);
    assert_eq!(second.len(), ICON_BATCH_SIZE);
    assert!(second.iter().all(|identity| !first.contains(identity)));
    assert!(!second.contains(workspace.desktop_items()[40].identity()));
    let third = collect_icon_requests(&workspace, &images, &mut pending, &failures);
    assert_eq!(third.len(), 31);
    assert!(collect_icon_requests(&workspace, &images, &mut pending, &failures).is_empty());
}

#[test]
fn initial_batch_loads_all_requested_namespace_icons_together() {
    let identities = [
        RECYCLE_BIN_PARSING_NAME,
        "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
    ]
    .map(|name| ShellIdentity::Namespace {
        parsing_name: name.into(),
    });
    let expected: std::collections::HashSet<_> = identities
        .iter()
        .map(ShellIdentity::persistent_key)
        .collect();
    let images = load_icon_batch(identities.to_vec(), 128);
    assert_eq!(
        images
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<std::collections::HashSet<_>>(),
        expected
    );
    assert!(images.iter().all(|(_, image)| !image.data.is_empty()));
}

fn pixels() -> assets::Pixels {
    assets::Pixels {
        width: 128,
        height: 128,
        data: vec![255; 128 * 128 * 4],
    }
}

#[test]
fn desktop_icons_share_pixels_with_other_sources_and_refresh_independently() {
    let mut s = crate::pane::tests::test_state();
    let shared = crate::pane::image_pool::intern(Arc::new(pixels()));
    let keys: Vec<_> = s.workspace.desktop_items().iter()
        .map(|item| item.identity().persistent_key()).collect();
    assert!(apply_pane_images(&s.workspace, &mut s.images,
        keys.iter().map(|key| (key.clone(), pixels())).collect()));
    assert!(s.images.values().all(|image| Arc::ptr_eq(image, &shared)));
    let mut changed = pixels();
    changed.data[0] = 0;
    assert!(apply_pane_images(&s.workspace, &mut s.images,
        vec![(keys[0].clone(), changed)]));
    assert!(!Arc::ptr_eq(&s.images[&keys[0]], &shared));
    assert!(Arc::ptr_eq(&s.images[&keys[1]], &shared));
    assert_eq!(shared.data[0], 255);
}

#[test]
fn released_images_are_freed_and_late_results_cannot_restore_them() {
    let mut s = crate::pane::tests::test_state();
    let mut retention = image_retention::ImageRetention::default();
    let now = Instant::now();
    let keys: Vec<_> = s
        .workspace
        .desktop_items()
        .iter()
        .map(|item| item.identity().persistent_key())
        .collect();
    assert!(apply_pane_images(
        &s.workspace,
        &mut s.images,
        keys.iter().enumerate().map(|(index, key)| {
            let mut image = pixels();
            // Distinct content isolates eviction from live-image sharing.
            image.data[0] = index as u8;
            image.data[1] = 73; // Isolate eviction from other parallel pixel-pool tests.
            (key.clone(), image)
        }).collect()
    ));
    let released = Arc::downgrade(&s.images[&keys[0]]);
    let kept = s.images[&keys[1]].clone();
    s.workspace.desktop_items_mut()[0].set_placement(DesktopPlacement::default());
    retain_pane_images(&s.workspace, &mut s.images, &mut retention, now);
    assert!(
        released.upgrade().is_some(),
        "keep a bounded window for quick reuse"
    );
    retention.expire(&mut s.images, now + image_retention::TTL);
    assert!(
        released.upgrade().is_none(),
        "the released bitmap must not stay cached"
    );
    assert_eq!(s.images.len(), 2);
    assert!(!apply_pane_images(
        &s.workspace,
        &mut s.images,
        vec![(keys[0].clone(), pixels()), (keys[1].clone(), (*kept).clone())]
    ));
    assert!(!s.images.contains_key(&keys[0]));
    assert!(
        Arc::ptr_eq(&kept, &s.images[&keys[1]]),
        "unchanged pixels must retain their GPU cache identity"
    );

    // Moving an item into another pane retains its image; removing the
    // final pane membership releases all remaining cache-owned pixels.
    s.workspace.desktop_items_mut()[1].set_placement(DesktopPlacement::Pane {
        pane_id: PanelId::new(2),
        position: GridPosition::new(0, 0),
    });
    retain_pane_images(&s.workspace, &mut s.images, &mut retention, now);
    assert!(Arc::ptr_eq(&kept, &s.images[&keys[1]]));
    for item in s.workspace.desktop_items_mut() {
        item.set_placement(DesktopPlacement::default());
    }
    retain_pane_images(&s.workspace, &mut s.images, &mut retention, now);
    retention.expire(&mut s.images, now + image_retention::TTL);
    assert!(s.images.is_empty());
    assert!(!apply_pane_images(
        &s.workspace,
        &mut s.images,
        keys.into_iter().map(|key| (key, pixels())).collect()
    ));
    assert!(s.images.is_empty());
}

#[test]
fn refresh_recovery_clears_initial_failures_and_stops_retry_timer() {
    let mut s = crate::pane::tests::test_state();
    let keys: Vec<_> = s.workspace.desktop_items().iter()
        .map(|item| item.identity().persistent_key()).collect();
    let now = Instant::now();
    let mut failures = HashMap::from([
        (keys[0].clone(), (1, now)),
        (keys[1].clone(), (2, now)),
    ]);
    assert!(apply_refreshed_images(&s.workspace, &mut s.images, &mut failures,
        vec![(keys[0].clone(), pixels())]));
    assert!(!failures.contains_key(&keys[0]));
    assert!(failures.contains_key(&keys[1]), "keep unsuccessful retries");

    // Identical pixels must clear stale failure state as well.
    failures.insert(keys[0].clone(), (1, now));
    assert!(!apply_refreshed_images(&s.workspace, &mut s.images, &mut failures,
        vec![(keys[0].clone(), pixels())]));
    assert!(!failures.contains_key(&keys[0]));
    assert!(apply_refreshed_images(&s.workspace, &mut s.images, &mut failures,
        vec![(keys[1].clone(), pixels())]));
    assert!(failures.is_empty());
    assert_eq!(retry_deadline(false, now, &s.images, &HashSet::new(), &failures), None);
}

#[test]
fn busy_loader_defers_retry_timer_but_completion_resumes_missing_icons() {
    let s = crate::pane::tests::test_state();
    let identity = s.workspace.desktop_items()[0].identity();
    let now = Instant::now();
    let failures = HashMap::from([(identity.persistent_key(), (1, now))]);
    let last_scan = now - Duration::from_secs(1);
    let mut requested = HashSet::new();
    assert_eq!(retry_deadline(true, last_scan, &s.images, &requested, &failures), None);
    assert_eq!(retry_deadline(false, last_scan, &s.images, &requested, &failures), Some(now));
    let queued = collect_icon_requests(&s.workspace, &s.images, &mut requested, &failures);
    assert!(queued.contains(identity));
    assert_eq!(retry_deadline(false, last_scan, &s.images, &requested, &failures), None);
}

#[test]
fn retry_timer_ignores_cached_and_exhausted_items_and_respects_scan_throttle() {
    let now = Instant::now();
    let images = HashMap::from([("cached".into(), Arc::new(pixels()))]);
    let mut failures = HashMap::from([
        ("cached".into(), (1, now)),
        ("exhausted".into(), (5, now)),
    ]);
    let requested = HashSet::new();
    assert_eq!(retry_deadline(false, now, &images, &requested, &failures), None);
    failures.insert("missing".into(), (1, now));
    assert_eq!(retry_deadline(false, now, &images, &requested, &failures),
        Some(now + Duration::from_millis(250)));
    failures.insert("missing".into(), (1, now + Duration::from_secs(2)));
    assert_eq!(retry_deadline(false, now, &images, &requested, &failures),
        Some(now + Duration::from_secs(2)));
}

#[test]
fn initial_retry_budget_is_short_and_exhaustion_stops_scheduling() {
    let s = crate::pane::tests::test_state();
    let key = s.workspace.desktop_items()[0].identity().persistent_key();
    let now = Instant::now();
    let mut elapsed = Duration::ZERO;
    let images = HashMap::new();
    let requested = HashSet::new();
    for attempts in 1..INITIAL_MAX_ATTEMPTS {
        let due = now + icon_retry_delay(attempts);
        elapsed += icon_retry_delay(attempts);
        let failures = HashMap::from([(key.clone(), (attempts, due))]);
        assert_eq!(retry_deadline(false, now, &images, &requested, &failures), Some(due));
        assert_eq!(retry_deadline(true, now, &images, &requested, &failures), None);
        let mut pending = HashSet::new();
        assert!(!collect_icon_requests(&s.workspace, &images, &mut pending, &failures)
            .iter().any(|identity| identity.persistent_key() == key));
    }
    assert_eq!(elapsed, Duration::from_millis(3750));
    let exhausted = HashMap::from([(key.clone(), (INITIAL_MAX_ATTEMPTS, now))]);
    assert_eq!(retry_deadline(false, now, &images, &requested, &exhausted), None);
    assert!(!collect_icon_requests(&s.workspace, &images, &mut HashSet::new(), &exhausted)
        .iter().any(|identity| identity.persistent_key() == key));
    assert_eq!(icon_retry_delay(u32::MAX), Duration::from_secs(32));
    let mut failures = u32::MAX;
    assert_eq!(refresh_retry_delay(&mut failures), Duration::from_secs(32));
}
