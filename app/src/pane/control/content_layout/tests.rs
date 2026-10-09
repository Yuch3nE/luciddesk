use super::measurement::{size_with_folders};
use super::*;
fn monitor() -> MonitorDescriptor {
    let rect = luciddesk_window::PixelRect {
        x: -3840,
        y: 0,
        width: 3840,
        height: 2088,
    };
    MonitorDescriptor {
        id: luciddesk_core::MonitorId::new("test"),
        bounds: rect,
        work_area: rect,
        dpi: 144,
        primary: true,
    }
}
#[test]
fn folder_sizing_uses_ready_snapshot_and_list_or_icon_metrics() {
    let mut s = super::super::super::tests::test_state();
    let id = PanelId::new(1);
    let root = std::env::temp_dir();
    s.workspace
        .panel_mut(id)
        .unwrap()
        .set_folder(Some(root.clone()));
    let mut folders = HashMap::from([(
        id,
        FolderSnapshot {
            root,
            labels: vec!["Item".into(); 17],
            ready: false,
        },
    )]);
    assert!(size_with_folders(&s.workspace, id, 4, &folders, None).is_err());
    folders.get_mut(&id).unwrap().ready = true;
    s.workspace.panel_mut(id).unwrap().set_list_view(false);
    let (width, height) = size_with_folders(&s.workspace, id, 4, &folders, None).unwrap();
    assert_eq!(width, 376.0);
    assert_eq!(height, layout::pane_content_height(5, 96.0, &vec![layout::icon_row_height(metrics(&s.workspace), ["Item"].into_iter()); 5]));
    assert_eq!(
        size_with_folders(&s.workspace, id, 4, &folders, Some(2))
            .unwrap()
            .1,
        layout::pane_content_height(2, 96.0, &[layout::icon_row_height(metrics(&s.workspace), ["Item"].into_iter()); 2])
    );
    s.workspace.panel_mut(id).unwrap().set_list_view(true);
    assert_eq!(
        size_with_folders(&s.workspace, id, 4, &folders, Some(5)).unwrap(),
        (s.workspace.panel(id).unwrap().rect().width, 240.0)
    );
    assert_eq!(
        folders[&id].labels.len(), 17,
        "viewport cap does not truncate inventory"
    );
    s.workspace
        .panel_mut(id)
        .unwrap()
        .set_folder(Some(std::env::temp_dir().join("changed")));
    assert!(size_with_folders(&s.workspace, id, 4, &folders, None).is_err());
}
#[test]
fn mixed_folder_arrangement_and_folder_fit_validate_before_saving() {
    let mut s = super::super::super::tests::test_state();
    let id = PanelId::new(1);
    let root = std::env::temp_dir();
    s.workspace
        .panel_mut(id)
        .unwrap()
        .set_folder(Some(root.clone()));
    s.workspace.panel_mut(id).unwrap().set_list_view(true);
    let folders = HashMap::from([(
        id,
        FolderSnapshot {
            root,
            labels: vec!["Item".into(); 8],
            ready: true,
        },
    )]);
    let m = monitor();
    let before = s.store.change_count();
    let op = Operation::Arrange {
        monitor_id: "test".into(),
        columns: vec![vec!["1".into(), "2".into()]],
        icon_columns: 6,
    };
    assert_eq!(
        expand_with_folders(
            &s.workspace,
            &s.store,
            &op,
            &[m.clone()],
            &HashMap::new(),
            &folders
        )
        .unwrap()
        .len(),
        2
    );
    for op in [
        Operation::FolderFit {
            pane_id: "1".into(),
            icon_columns: Some(4),
            max_rows: None,
        },
        Operation::FolderFit {
            pane_id: "1".into(),
            icon_columns: None,
            max_rows: Some(0),
        },
    ] {
        assert!(
            expand_with_folders(
                &s.workspace,
                &s.store,
                &op,
                &[m.clone()],
                &HashMap::new(),
                &folders
            )
            .is_err()
        );
    }
    assert_eq!(s.store.change_count(), before);
}
#[test]
fn relative_snap_has_fixed_gap_in_all_directions_and_alignments() {
    let s = super::super::super::tests::test_state();
    let m = monitor();
    let anchor = RectDip {
        x: -2200.0,
        y: 800.0,
        width: 900.0,
        height: 600.0,
    };
    let positions = HashMap::from([(PanelId::new(2), anchor)]);
    for side in [
        SnapSide::Left,
        SnapSide::Right,
        SnapSide::Top,
        SnapSide::Bottom,
    ] {
        for align in [SnapAlign::Start, SnapAlign::Center, SnapAlign::End] {
            let op = Operation::Snap {
                pane_id: "1".into(),
                target_pane_id: "2".into(),
                side,
                align,
                icon_columns: Some(6),
            };
            let ops = expand(&s.workspace, &s.store, &op, &[m.clone()], &positions).unwrap();
            let Operation::Geometry {
                x,
                y,
                width,
                height,
                ..
            } = ops[0]
            else {
                panic!()
            };
            let (_, r) = geometry::convert(
                &m,
                RectDip {
                    x,
                    y,
                    width,
                    height,
                },
            )
            .unwrap();
            let (gap, delta, available, extent) = match side {
                SnapSide::Left => (
                    anchor.x - r.x - r.width,
                    r.y - anchor.y,
                    anchor.height,
                    r.height,
                ),
                SnapSide::Right => (
                    r.x - anchor.x - anchor.width,
                    r.y - anchor.y,
                    anchor.height,
                    r.height,
                ),
                SnapSide::Top => (
                    anchor.y - r.y - r.height,
                    r.x - anchor.x,
                    anchor.width,
                    r.width,
                ),
                SnapSide::Bottom => (
                    r.y - anchor.y - anchor.height,
                    r.x - anchor.x,
                    anchor.width,
                    r.width,
                ),
            };
            assert_eq!(gap, snap::GAP_PX as f32);
            let expected = match align {
                SnapAlign::Start => 0.0,
                SnapAlign::Center => ((available - extent) / 2.0).round(),
                SnapAlign::End => available - extent,
            };
            assert_eq!(delta, expected);
        }
    }
}
#[test]
fn relative_snap_rejects_self_and_offscreen_and_uses_pending_target() {
    let s = super::super::super::tests::test_state();
    let m = monitor();
    let op = |target: &str, side| Operation::Snap {
        pane_id: "1".into(),
        target_pane_id: target.into(),
        side,
        align: SnapAlign::Start,
        icon_columns: None,
    };
    assert!(
        expand(
            &s.workspace,
            &s.store,
            &op("1", SnapSide::Left),
            &[m.clone()],
            &HashMap::new()
        )
        .is_err()
    );
    let positions = HashMap::from([(
        PanelId::new(2),
        RectDip {
            x: -3840.0,
            y: 100.0,
            width: 720.0,
            height: 540.0,
        },
    )]);
    assert!(
        expand(
            &s.workspace,
            &s.store,
            &op("2", SnapSide::Left),
            &[m.clone()],
            &positions
        )
        .is_err()
    );
    let ops = expand(
        &s.workspace,
        &s.store,
        &op("2", SnapSide::Right),
        &[m.clone()],
        &positions,
    )
    .unwrap();
    let Operation::Geometry { x, .. } = ops[0] else {
        panic!()
    };
    assert_eq!((x * 1.5).round(), 725.0);
}
#[test]
fn arrange_matches_grid_and_native_snap_gap_without_writes() {
    let s = super::super::super::tests::test_state();
    let m = monitor();
    let before = s.store.change_count();
    let ops = expand(
        &s.workspace,
        &s.store,
        &Operation::Arrange {
            monitor_id: "test".into(),
            columns: vec![vec!["1".into(), "2".into()]],
            icon_columns: 6,
        },
        &[m.clone()],
        &HashMap::new(),
    )
    .unwrap();
    let rects: Vec<_> = ops
        .iter()
        .map(|op| match op {
            Operation::Geometry {
                pane_id,
                x,
                y,
                width,
                height,
                ..
            } => {
                let (_, px) = geometry::convert(
                    &m,
                    RectDip {
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                    },
                )
                .unwrap();
                let grid = layout::desktop_grid(
                    *width,
                    *height,
                    layout::DESKTOP_ICON_SIZE,
                    s.workspace.pane_options().grid_scale,
                );
                assert!(grid.columns >= 6);
                assert!(
                    *height >= super::measurement::content_height(&s.workspace, parse(pane_id).unwrap(), grid.columns, &HashMap::new(), None).unwrap()
                );
                px
            }
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        rects[1].y - rects[0].y - rects[0].height,
        snap::GAP_PX as f32
    );
    assert_eq!(
        m.work_area.x as f32 + m.work_area.width as f32 - rects[0].x - rects[0].width,
        snap::GAP_PX as f32
    );
    assert_eq!(s.store.change_count(), before);
}
#[test]
fn layout_rejects_overflow_duplicates_and_unsupported_content() {
    let mut s = super::super::super::tests::test_state();
    let m = monitor();
    let op = |columns, icon_columns| Operation::Arrange {
        monitor_id: "test".into(),
        columns,
        icon_columns,
    };
    for invalid in [
        op(vec![], 6),
        op(vec![vec!["1".into(), "1".into()]], 6),
        op(vec![vec!["1".into()]], 0),
        op(vec![vec!["1".into()]], 64),
    ] {
        assert!(
            expand(
                &s.workspace,
                &s.store,
                &invalid,
                &[m.clone()],
                &HashMap::new()
            )
            .is_err()
        );
    }
    s.workspace
        .panel_mut(PanelId::new(1))
        .unwrap()
        .set_locked(true);
    assert!(size(&s.workspace, PanelId::new(1), 6).is_err());
    s.workspace
        .panel_mut(PanelId::new(1))
        .unwrap()
        .set_locked(false);
    s.workspace
        .panel_mut(PanelId::new(1))
        .unwrap()
        .set_folder(Some(std::env::temp_dir()));
    assert!(size(&s.workspace, PanelId::new(1), 6).is_err());
}
#[test]
fn arranging_subset_never_covers_unselected_panel() {
    let s = super::super::super::tests::test_state();
    let m = monitor();
    let positions = HashMap::from([(
        PanelId::new(2),
        RectDip {
            x: -1000.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        },
    )]);
    let op = Operation::Arrange {
        monitor_id: "test".into(),
        columns: vec![vec!["1".into()]],
        icon_columns: 6,
    };
    assert!(
        expand(&s.workspace, &s.store, &op, &[m], &positions)
            .unwrap_err()
            .contains("unselected panel")
    );
}
#[test]
fn fit_keeps_position_and_rounds_outward_at_fractional_dpi() {
    let s = super::super::super::tests::test_state();
    let mut m = monitor();
    m.dpi = 120;
    let positions = HashMap::from([(
        PanelId::new(1),
        RectDip {
            x: -3500.0,
            y: 100.0,
            width: 600.0,
            height: 450.0,
        },
    )]);
    let ops = expand(
        &s.workspace,
        &s.store,
        &Operation::Fit {
            pane_id: "1".into(),
            icon_columns: 5,
        },
        &[m.clone()],
        &positions,
    )
    .unwrap();
    let Operation::Geometry {
        x,
        y,
        width,
        height,
        ..
    } = ops[0]
    else {
        panic!()
    };
    let (_, px) = geometry::convert(
        &m,
        RectDip {
            x,
            y,
            width,
            height,
        },
    )
    .unwrap();
    assert_eq!(px.x, -3500.0);
    assert_eq!(px.y, 100.0);
    let grid = layout::desktop_grid(
        width,
        height,
        layout::DESKTOP_ICON_SIZE,
        s.workspace.pane_options().grid_scale,
    );
    assert!(grid.columns >= 5);
    assert!(height >= super::measurement::content_height(&s.workspace, PanelId::new(1), grid.columns, &HashMap::new(), None).unwrap());
}

#[test]
fn fit_matches_manual_content_snap_for_wrapped_labels_and_scaled_grids() {
    use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::WMSZ_BOTTOMRIGHT};
    let id = PanelId::new(1);
    let mut state = super::super::super::tests::test_state();
    for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
        item.set_display_name(["Short", "A very long document name that wraps over two lines", "End"][index]);
        item.set_placement(DesktopPlacement::Pane { pane_id: id, position: luciddesk_core::GridPosition::new(0, (2-index) as u32) });
    }
    for percent in [75.0, 100.0, 125.0, 150.0] {
        let mut options = state.workspace.pane_options();
        options.grid_scale = percent;
        state.workspace.set_pane_options(options);
        let model = super::super::super::create_model(&state, id).unwrap();
        for columns in [1, 2, 3] {
            let (width, height) = size(&state.workspace, id, columns).unwrap();
            let grid = model.grid(width, height);
            assert_eq!(grid.columns, columns as usize);
            let rows = model.row_contents(grid);
            assert_eq!(grid.visible_rows, rows.len());
            assert_eq!(grid.scroll_limit, Some(0));
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let expected = ((width * scale).ceil() as i32, (height * scale).ceil() as i32);
                let mut rect = RECT { left: -100, top: 20, right: -100 + expected.0 + 2, bottom: 20 + expected.1 + 2 };
                layout::resize_pane(&mut rect, WMSZ_BOTTOMRIGHT, model.resize_cell(), scale, false, &rows);
                assert_eq!((rect.right-rect.left, rect.bottom-rect.top), expected);
                let px = pixel_size(&state.workspace, id, columns, scale, &HashMap::new(), None).unwrap();
                assert_eq!(px, (expected.0 as f32, expected.1 as f32));
            }
        }
    }
}

#[test]
fn tab_fit_keeps_whole_columns_and_measures_each_members_labels() {
    let mut state = super::super::super::tests::test_state();
    for (index, item) in state.workspace.desktop_items_mut().iter_mut().enumerate() {
        item.set_display_name(if index == 2 { "A long filename that needs wrapping on two lines" } else { "Short" });
        item.set_placement(DesktopPlacement::Pane { pane_id: PanelId::new(if index == 2 { 2 } else { 1 }), position: luciddesk_core::GridPosition::new(index as u32, 0) });
    }
    state.workspace.set_tab_groups(vec![luciddesk_core::PaneTabs { members: vec![PanelId::new(1), PanelId::new(2)], active: PanelId::new(1) }]).unwrap();
    let (width, height) = size(&state.workspace, PanelId::new(1), 1).unwrap();
    assert_eq!(width, 288.0, "tab minimum rounds up to a whole number of columns");
    let heights: Vec<_> = [1, 2].into_iter().map(|id| {
        let model = super::super::super::create_model(&state, PanelId::new(id)).unwrap();
        let grid = model.grid(width, height);
        let rows = model.row_contents(grid);
        layout::pane_content_height(rows.len(), grid.cell_height, &rows)
    }).collect();
    assert!(heights[1] > heights[0]);
    assert_eq!(height, heights[1]);
}

#[test]
fn mixed_tab_views_fit_with_a_usable_list_width() {
    let mut state = super::super::super::tests::test_state();
    state.workspace.panel_mut(PanelId::new(2)).unwrap().set_list_view(true);
    state.workspace.set_tab_groups(vec![luciddesk_core::PaneTabs {
        members: vec![PanelId::new(1), PanelId::new(2)], active: PanelId::new(1),
    }]).unwrap();
    let (width, _) = size(&state.workspace, PanelId::new(1), 1).unwrap();
    assert_eq!(width, 464.0);
    assert!(width >= layout::LIST_CELL_WIDTH + layout::PADDING * 2.0);
}

#[test]
fn explicit_snap_matches_manual_edges_using_pending_native_bounds() {
    let state = super::super::super::tests::test_state();
    for dpi in [96, 120, 144, 192] {
        let mut m = monitor();
        m.dpi = dpi;
        let anchor = RectDip::from_bounds(-2400.0, 600.0, 603.0, 405.0);
        let moving = RectDip::from_bounds(-3200.0, 50.0, 333.0, 201.0);
        let positions = HashMap::from([(PanelId::new(1), moving), (PanelId::new(2), anchor)]);
        for side in [SnapSide::Left, SnapSide::Right, SnapSide::Top, SnapSide::Bottom] {
            for align in [SnapAlign::Start, SnapAlign::End] {
                let op = Operation::Snap { pane_id: "1".into(), target_pane_id: "2".into(), side, align, icon_columns: None };
                let operations = expand(&state.workspace, &state.store, &op, &[m.clone()], &positions).unwrap();
                let Operation::Geometry { x, y, width, height, .. } = operations[0] else { panic!() };
                let (_, px) = geometry::convert(&m, RectDip { x, y, width, height }).unwrap();
                assert_eq!((px.width, px.height), (moving.width, moving.height), "use pending/native size, not stale workspace size");
                let mut manual = RECT { left: px.x as i32 + 3, top: px.y as i32 + 3, right: (px.x+px.width) as i32 + 3, bottom: (px.y+px.height) as i32 + 3 };
                let peer = RECT { left: anchor.x as i32, top: anchor.y as i32, right: (anchor.x+anchor.width) as i32, bottom: (anchor.y+anchor.height) as i32 };
                snap::snap(&mut manual, &[peer], None, snap::GAP_PX, (14.0*dpi as f32/96.0).round() as i32);
                assert_eq!((manual.left, manual.top, manual.right, manual.bottom), (px.x as i32, px.y as i32, (px.x+px.width) as i32, (px.y+px.height) as i32));
            }
        }
    }
}

#[test]
fn snap_converts_measured_size_between_monitor_dpis_once() {
    let state = super::super::super::tests::test_state();
    let mut target = monitor();
    target.dpi = 144;
    let mut source = monitor();
    source.id = luciddesk_core::MonitorId::new("source");
    source.dpi = 96;
    source.primary = false;
    source.bounds.x = 0;
    source.work_area.x = 0;
    let positions = HashMap::from([
        (PanelId::new(1), RectDip::from_bounds(200.0, 100.0, 333.0, 201.0)),
        (PanelId::new(2), RectDip::from_bounds(-2400.0, 600.0, 603.0, 405.0)),
    ]);
    let op = Operation::Snap { pane_id: "1".into(), target_pane_id: "2".into(), side: SnapSide::Right, align: SnapAlign::Center, icon_columns: None };
    let ops = expand(&state.workspace, &state.store, &op, &[target.clone(), source], &positions).unwrap();
    let Operation::Geometry { x,y,width,height,.. } = ops[0] else { panic!() };
    let (_,px) = geometry::convert(&target, RectDip {x,y,width,height}).unwrap();
    assert_eq!((px.width,px.height), (500.0,302.0));
    assert_eq!(px.x, -2400.0 + 603.0 + snap::GAP_PX as f32);
    assert_eq!(px.y, 652.0);
}

#[test]
fn fixed_grid_fit_keeps_holes_and_occupied_columns() {
    let mut s = super::super::super::tests::test_state();
    let id = PanelId::new(1);
    fixed_grid::toggle(&mut s.workspace, id).unwrap();
    let key = super::super::super::ordered_desktop_items(&s.workspace, id)[0].identity().persistent_key();
    fixed_grid::place(&mut s.workspace, id, &[key], luciddesk_core::GridPosition::new(2, 4));
    let before = s.workspace.clone();
    let (width, height) = size_with_folders(&s.workspace, id, 1, &HashMap::new(), None).unwrap();
    let model = super::super::super::create_model(&s, id).unwrap();
    let grid = model.grid(width, height);
    assert_eq!(grid.columns, 3);
    assert_eq!(model.content_rows(grid), 5);
    assert_eq!(grid.max_scroll(model.items.len()), 0);
    let rows = model.row_contents(grid);
    assert_eq!(height, layout::pane_content_height(5, grid.cell_height, &rows));
    assert_eq!(measurement::query(&s.workspace,id)["content_rows"], 5);
    assert!(measurement::minimum(&s.workspace,id,112.0,&HashMap::new()).0 >= width);
    assert_eq!(s.workspace, before);
}

#[test]
fn free_layout_fit_preserves_coordinates_and_covers_actual_bounds() {
    let mut s = super::super::super::tests::test_state(); let id=PanelId::new(1);
    fixed_grid::toggle(&mut s.workspace,id).unwrap();
    free_layout::toggle(&mut s.workspace,id).unwrap();
    let key=super::super::super::ordered_desktop_items(&s.workspace,id)[0].identity().persistent_key();
    free_layout::move_items(&mut s.workspace,id,id,&[key.clone()],&key,luciddesk_core::PointDip::new(251.25,337.5)).unwrap();
    let before=s.workspace.clone();
    let (width,height)=size_with_folders(&s.workspace,id,1,&HashMap::new(),None).unwrap();
    let model=super::super::super::create_model(&s,id).unwrap();let grid=model.grid(width,height);
    assert_eq!(grid.max_scroll(model.items.len()),0);
    for (i,_) in model.items.iter().enumerate() {
        let bounds=model.selection_bounds(grid,i,1.0);
        assert!(bounds.x+bounds.width <= width-layout::PADDING+0.01);
        assert!(bounds.y+bounds.height <= height-layout::PADDING+0.01);
    }
    assert_eq!(measurement::query(&s.workspace,id)["align_icons_to_grid"],false);
    assert_eq!(s.workspace,before);
}

#[test]
fn free_fit_uses_actual_right_edge_without_rounding_an_extra_column() {
    let mut s=super::super::super::tests::test_state();let id=PanelId::new(1);
    fixed_grid::toggle(&mut s.workspace,id).unwrap();free_layout::toggle(&mut s.workspace,id).unwrap();
    for item in s.workspace.desktop_items_mut() { item.set_pane_position(Some(luciddesk_core::PointDip::new(10.0,20.0))); }
    let g=metrics(&s.workspace);let expected=layout::PADDING*2.0+10.0+g.cell_width;
    let (width,_)=size_with_folders(&s.workspace,id,1,&HashMap::new(),None).unwrap();
    assert_eq!(width,expected);
    assert_eq!(measurement::minimum(&s.workspace,id,width,&HashMap::new()).0,expected);
    assert_eq!(super::super::super::create_model(&s,id).unwrap().fixed_width(),expected);
}
