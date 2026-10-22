pub const DEFAULT_GAP: u32 = 5;
pub const MAX_GAP: u32 = 64;
static GAP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(DEFAULT_GAP);

pub fn gap() -> u32 { GAP.load(std::sync::atomic::Ordering::Relaxed) }
pub fn read_gap(store: &luciddesk_storage::WorkspaceStore) -> Result<u32, String> {
    Ok(store.preference("pane_snap_gap").map_err(|e| e.to_string())?
        .and_then(|value| value.parse::<u32>().ok()).filter(|value| *value <= MAX_GAP)
        .unwrap_or(DEFAULT_GAP))
}
pub fn load_gap(store: &luciddesk_storage::WorkspaceStore) -> Result<(), String> {
    GAP.store(read_gap(store)?, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
pub fn save_gap(store: &luciddesk_storage::WorkspaceStore, value: u32) -> Result<(), String> {
    if value > MAX_GAP { return Err(format!("snap gap must be between 0 and {MAX_GAP}")); }
    store.save_preference("pane_snap_gap", &value.to_string()).map_err(|e| e.to_string())?;
    GAP.store(value, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
pub fn gap_px() -> i32 { gap() as i32 }
pub fn threshold(scale: f32) -> i32 { (14.0 * scale).round() as i32 }

use windows_sys::Win32::Foundation::RECT;

/// Integer-pixel origins shared by proximity snapping and explicit CLI placement.
pub struct AxisTargets {
    pub before: i32,
    pub after: i32,
    pub start: i32,
    pub center: i32,
    pub end: i32,
}
impl AxisTargets {
    pub fn new(start: i32, end: i32, extent: i32, gap: i32) -> Self {
        Self {
            before: start - extent - gap,
            after: end + gap,
            start,
            center: start + ((end - start - extent) as f32 / 2.0).round() as i32,
            end: end - extent,
        }
    }
}

/// Derive each proposal from the original pointer offset, never from a snapped frame.
pub struct DragOrigin {
    bounds: RECT,
    pointer: windows_sys::Win32::Foundation::POINT,
}
impl DragOrigin {
    pub fn new(bounds: RECT, pointer: windows_sys::Win32::Foundation::POINT) -> Self {
        Self { bounds, pointer }
    }
    pub fn proposal(&self, pointer: windows_sys::Win32::Foundation::POINT) -> RECT {
        let dx = pointer.x - self.pointer.x;
        let dy = pointer.y - self.pointer.y;
        RECT {
            left: self.bounds.left + dx,
            right: self.bounds.right + dx,
            top: self.bounds.top + dy,
            bottom: self.bounds.bottom + dy,
        }
    }
}

// Choose the nearest edge independently on each axis, using the actual visible pane bounds.
pub fn snap(rect: &mut RECT, peers: &[RECT], work: Option<&RECT>, gap: i32, threshold: i32) {
    let nearest = |values: Vec<i32>| {
        values
            .into_iter()
            .filter(|delta| delta.abs() <= threshold)
            .min_by_key(|delta| delta.abs())
            .unwrap_or(0)
    };
    let mut horizontal = Vec::new();
    if let Some(work) = work {
        horizontal.extend([work.left + gap - rect.left, work.right - gap - rect.right]);
    }
    for peer in peers {
        if rect.top < peer.bottom + gap + threshold && rect.bottom > peer.top - gap - threshold {
            let targets = AxisTargets::new(peer.left, peer.right, rect.right - rect.left, gap);
            horizontal.extend([targets.after - rect.left, targets.before - rect.left]);
            if (rect.top - peer.bottom).abs() <= gap + threshold
                || (rect.bottom - peer.top).abs() <= gap + threshold
            {
                horizontal.extend([targets.start - rect.left, targets.end - rect.left]);
            }
        }
    }
    let delta = nearest(horizontal);
    rect.left += delta;
    rect.right += delta;
    let mut vertical = Vec::new();
    if let Some(work) = work {
        vertical.extend([work.top + gap - rect.top, work.bottom - gap - rect.bottom]);
    }
    for peer in peers {
        if rect.left < peer.right + gap + threshold && rect.right > peer.left - gap - threshold {
            let targets = AxisTargets::new(peer.top, peer.bottom, rect.bottom - rect.top, gap);
            vertical.extend([targets.after - rect.top, targets.before - rect.top]);
            if (rect.left - peer.right).abs() <= gap + threshold
                || (rect.right - peer.left).abs() <= gap + threshold
            {
                vertical.extend([targets.start - rect.top, targets.end - rect.top]);
            }
        }
    }
    let delta = nearest(vertical);
    rect.top += delta;
    rect.bottom += delta;
}

/// Resize only the dragged edges. Peer alignment overrides grid snapping, but
/// capture distance always comes from the original Windows sizing proposal.
pub fn resize(
    rect: &mut RECT,
    proposal: &RECT,
    edge: u32,
    peers: &[RECT],
    gap: i32,
    threshold: i32,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let left = matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT);
    let right = matches!(edge, WMSZ_RIGHT | WMSZ_TOPRIGHT | WMSZ_BOTTOMRIGHT);
    let top = matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT);
    let bottom = matches!(edge, WMSZ_BOTTOM | WMSZ_BOTTOMLEFT | WMSZ_BOTTOMRIGHT);
    let nearest = |value: i32, candidates: Vec<i32>| {
        candidates
            .into_iter()
            .filter(|target| (target - value).abs() <= threshold)
            .min_by_key(|target| (target - value).abs())
    };
    let beside = |peer: &&RECT| {
        ((proposal.left - peer.right).abs() <= gap + threshold
            || (proposal.right - peer.left).abs() <= gap + threshold)
            && proposal.top < peer.bottom + threshold
            && proposal.bottom > peer.top - threshold
    };
    let stacked = |peer: &&RECT| {
        ((proposal.top - peer.bottom).abs() <= gap + threshold
            || (proposal.bottom - peer.top).abs() <= gap + threshold)
            && proposal.left < peer.right + threshold
            && proposal.right > peer.left - threshold
    };
    if left || right {
        let value = if left { proposal.left } else { proposal.right };
        let candidates = peers
            .iter()
            .filter(stacked)
            .map(|peer| if left { peer.left } else { peer.right })
            .filter(|target| {
                if left {
                    *target < proposal.right
                } else {
                    *target > proposal.left
                }
            })
            .collect();
        if let Some(target) = nearest(value, candidates) {
            if left {
                rect.left = target;
            } else {
                rect.right = target;
            }
        }
    }
    if top || bottom {
        let value = if top { proposal.top } else { proposal.bottom };
        let candidates = peers
            .iter()
            .filter(beside)
            .map(|peer| if top { peer.top } else { peer.bottom })
            .filter(|target| {
                if top {
                    *target < proposal.bottom
                } else {
                    *target > proposal.top
                }
            })
            .collect();
        if let Some(target) = nearest(value, candidates) {
            if top {
                rect.top = target;
            } else {
                rect.bottom = target;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_search_pane_escapes_peer_corner_during_slow_drag() {
        use windows_sys::Win32::Foundation::POINT;
        for scale in [1, 2] {
            let peer = RECT {
                left: 0,
                top: 0,
                right: 400 * scale,
                bottom: 400 * scale,
            };
            let bounds = RECT {
                left: peer.right + 5,
                top: peer.bottom - 56 * scale,
                right: peer.right + 5 + 360 * scale,
                bottom: peer.bottom,
            };
            let origin = DragOrigin::new(bounds, POINT::default());
            for distance in 1..=30 * scale {
                let mut proposal = origin.proposal(POINT {
                    x: distance,
                    y: distance,
                });
                snap(&mut proposal, &[peer], None, 5, 14 * scale);
                if distance <= 14 * scale {
                    assert_eq!((proposal.left, proposal.top), (bounds.left, bounds.top));
                } else {
                    assert_eq!(
                        (proposal.left, proposal.top),
                        (bounds.left + distance, bounds.top + distance)
                    );
                }
            }
        }
    }

    #[test]
    fn resizing_adjacent_panes_aligns_only_dragged_edges() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        for scale in [1, 2] {
            let peer = RECT {
                left: -400 * scale,
                top: 100 * scale,
                right: 0,
                bottom: 400 * scale,
            };
            for edge in [WMSZ_TOP, WMSZ_BOTTOM, WMSZ_TOPRIGHT, WMSZ_BOTTOMRIGHT] {
                let proposal = RECT {
                    left: 5,
                    top: 106 * scale,
                    right: 305 * scale,
                    bottom: 406 * scale,
                };
                let mut rect = proposal;
                resize(&mut rect, &proposal, edge, &[peer], 5, 14 * scale);
                assert_eq!((rect.left, rect.right), (proposal.left, proposal.right));
                if matches!(edge, WMSZ_TOP | WMSZ_TOPRIGHT) {
                    assert_eq!((rect.top, rect.bottom), (peer.top, proposal.bottom));
                } else {
                    assert_eq!((rect.top, rect.bottom), (proposal.top, peer.bottom));
                }
            }
            for edge in [WMSZ_LEFT, WMSZ_RIGHT, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT] {
                let proposal = RECT {
                    left: -394 * scale,
                    top: peer.bottom + 5,
                    right: 6 * scale,
                    bottom: 700 * scale,
                };
                let mut rect = proposal;
                resize(&mut rect, &proposal, edge, &[peer], 5, 14 * scale);
                assert_eq!((rect.top, rect.bottom), (proposal.top, proposal.bottom));
                if matches!(edge, WMSZ_LEFT | WMSZ_BOTTOMLEFT) {
                    assert_eq!((rect.left, rect.right), (peer.left, proposal.right));
                } else {
                    assert_eq!((rect.left, rect.right), (proposal.left, peer.right));
                }
            }
        }
    }

    #[test]
    fn resize_uses_raw_proposal_for_capture_and_releases_beyond_threshold() {
        use windows_sys::Win32::UI::WindowsAndMessaging::WMSZ_BOTTOM;
        let peer = RECT {
            left: 0,
            top: 0,
            right: 300,
            bottom: 300,
        };
        for (bottom, expected) in [(309, 300), (315, 310)] {
            let proposal = RECT {
                left: 305,
                top: 0,
                right: 605,
                bottom,
            };
            let mut grid_snapped = RECT {
                bottom: 310,
                ..proposal
            };
            resize(&mut grid_snapped, &proposal, WMSZ_BOTTOM, &[peer], 5, 14);
            assert_eq!(grid_snapped.bottom, expected);
        }
        let proposal = RECT {
            left: 900,
            top: 0,
            right: 1200,
            bottom: 309,
        };
        let mut rect = proposal;
        resize(&mut rect, &proposal, WMSZ_BOTTOM, &[peer], 5, 14);
        assert_eq!(rect.bottom, 309, "distant panels must not attract resizing");
        let close = RECT {
            bottom: 305,
            ..peer
        };
        let proposal = RECT {
            left: 305,
            top: 0,
            right: 605,
            bottom: 307,
        };
        let mut rect = proposal;
        resize(&mut rect, &proposal, WMSZ_BOTTOM, &[peer, close], 5, 14);
        assert_eq!(rect.bottom, 305, "nearest edge wins");
    }

    #[test]
    fn slow_drag_can_escape_the_top_edge_without_accumulating_snap_feedback() {
        use windows_sys::Win32::Foundation::POINT;
        let origin = DragOrigin::new(
            RECT {
                left: 100,
                top: 12,
                right: 400,
                bottom: 312,
            },
            POINT { x: 150, y: 24 },
        );
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let mut result = RECT::default();
        for dy in 1..=100 {
            result = origin.proposal(POINT { x: 150, y: 24 + dy });
            snap(&mut result, &[], Some(&work), 12, 14);
        }
        assert_eq!((result.left, result.top, result.bottom), (100, 112, 412));
    }
    #[test]
    fn adjacent_panes_keep_gap_align_and_preserve_size() {
        let peer = RECT {
            left: -400,
            top: 100,
            right: 0,
            bottom: 400,
        };
        let mut rect = RECT {
            left: 18,
            top: 106,
            right: 318,
            bottom: 406,
        };
        snap(&mut rect, &[peer], None, 12, 14);
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (12, 100, 312, 400)
        );
        let mut far = RECT {
            left: 500,
            top: 600,
            right: 800,
            bottom: 900,
        };
        snap(&mut far, &[peer], None, 12, 14);
        assert_eq!((far.left, far.top), (500, 600));
    }

    #[test]
    fn desktop_corners_respect_work_area_and_negative_monitor_coordinates() {
        let work = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        for (left, top, expected) in [(-1915, 5, (-1908, 12)), (-310, 735, (-312, 728))] {
            let mut rect = RECT {
                left,
                top,
                right: left + 300,
                bottom: top + 300,
            };
            snap(&mut rect, &[], Some(&work), 12, 14);
            assert_eq!((rect.left, rect.top), expected);
            assert_eq!((rect.right - rect.left, rect.bottom - rect.top), (300, 300));
        }
        let mut far = RECT {
            left: -1000,
            top: 400,
            right: -700,
            bottom: 700,
        };
        snap(&mut far, &[], Some(&work), 12, 14);
        assert_eq!((far.left, far.top), (-1000, 400));
    }

    #[test]
    fn nearest_pane_edge_wins_over_desktop_edge() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let peer = RECT {
            left: 20,
            top: 0,
            right: 320,
            bottom: 100,
        };
        let mut rect = RECT {
            left: 22,
            top: 114,
            right: 322,
            bottom: 414,
        };
        snap(&mut rect, &[peer], Some(&work), 12, 14);
        assert_eq!((rect.left, rect.top), (20, 112));
    }
}

#[cfg(test)]
mod distance_tests {
    use super::*;
    #[test]
    fn gap_persists_without_changing_capture_threshold() {
        let _sta = crate::pane::test_support::apartment();
        struct Restore(u32);
        impl Drop for Restore {
            fn drop(&mut self) { GAP.store(self.0, std::sync::atomic::Ordering::Relaxed); }
        }
        let _restore = Restore(gap());
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        load_gap(&store).unwrap();
        assert_eq!(gap(), DEFAULT_GAP);
        for value in [0, 4, 14, MAX_GAP] {
            save_gap(&store, value).unwrap();
            GAP.store(99, std::sync::atomic::Ordering::Relaxed);
            load_gap(&store).unwrap();
            assert_eq!(gap(), value);
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let limit = threshold(scale);
                assert_eq!(limit, (14.0 * scale).round() as i32);
                let work = RECT { left: -1000, top: 0, right: 1000, bottom: 1000 };
                for delta in [limit, limit + 1] {
                    let mut rect = RECT { left: 200, right: 400, top: gap_px() + delta, bottom: gap_px() + delta + 100 };
                    snap(&mut rect, &[], Some(&work), gap_px(), limit);
                    assert_eq!(rect.top, if delta <= limit { gap_px() } else { gap_px() + delta });
                    let peer = RECT { left: 0, top: 0, right: 100, bottom: 100 };
                    let mut adjacent = RECT { left: peer.right + gap_px() + delta, top: 0,
                        right: peer.right + gap_px() + delta + 100, bottom: 100 };
                    snap(&mut adjacent, &[peer], None, gap_px(), limit);
                    assert_eq!(adjacent.left - peer.right, gap_px() + if delta <= limit { 0 } else { delta });
                    let proposal = RECT { left: 100 + gap_px(), top: delta, right: 200 + gap_px(), bottom: 100 };
                    let mut resized = proposal;
                    resize(&mut resized, &proposal, windows_sys::Win32::UI::WindowsAndMessaging::WMSZ_TOP, &[peer], gap_px(), limit);
                    assert_eq!(resized.top, if delta <= limit { 0 } else { delta });
                }
            }
        }
        assert!(save_gap(&store, MAX_GAP + 1).is_err());
        assert_eq!(read_gap(&store).unwrap(), MAX_GAP);
        for invalid in ["bad", "-1", "65"] {
            store.save_preference("pane_snap_gap", invalid).unwrap();
            load_gap(&store).unwrap();
            assert_eq!(gap(), DEFAULT_GAP);
        }
    }
}
