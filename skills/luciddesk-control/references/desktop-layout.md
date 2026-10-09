# Desktop organization and layout

## Group icons, then size and place panes

1. Query `workspace get --json` when organizing several panes; for a small change use `pane list`, `item list --unassigned` or `item list --pane ID`, with `--json`. Folder entries are not desktop item IDs.
2. Classify only the requested items by names/paths. Reuse suitable panes, preserve unrelated membership, and leave ambiguous items unchanged or clarify their destination.
3. Batch membership changes using [Plans and recovery](plans-and-recovery.md). For example, this is an `operations` array, not a complete plan:

```json
[
  {"op":"pane.create","ref":"work","title":"Work"},
  {"op":"item.assign","pane_ref":"work","item_ids":["WORK_ITEM_ID"]},
  {"op":"pane.create","ref":"games","title":"Games"},
  {"op":"item.assign","pane_ref":"games","item_ids":["GAME_ITEM_ID"]}
]
```

4. Apply once and verify membership/effect completion. Resolve new IDs from `refs` before planning their layout; fitting/snapping requires actual pane IDs.
5. If requested, sort/reorder first, fit next, then place. For known IDs these operations can share a plan. Fit each anchor before snapping its dependents; later operations use earlier planned geometry.
6. Verify actual bounds, membership and order. Report unresolved/unchanged ambiguous items; do not keep resizing after the requested layout is satisfied.

## Choose the operation

Mutation examples preview only. Apply the reviewed token using `next_step.args` before a dependent query/action.

| Intent | Preview command | Verify |
| --- | --- | --- |
| Six icons per row, tight content height | `pane fit --id 1 --icon-columns 6 --dry-run --json` | `pane get`: `content_layout`, `geometry`, `window_bounds_px` |
| Fit and place below pane 2, left-aligned | `pane snap --id 1 --target 2 --side bottom --align start --icon-columns 6 --dry-run --json` | Source size, shared gap, unchanged anchor |
| Keep size and place beside a pane | Same `pane snap`, omit `--icon-columns` | Source DIP size and relative position |
| Natural name order | `pane sort --id 1 --dry-run --json` | Items sorted by placement row/column; use `--descending true` to reverse |
| Newest modified files first | `pane sort --id 1 --sort-column modified --descending true --dry-run --json` | Same one-time order; arrangement mode retained |
| Preserve gaps and existing positions | `pane update --id 1 --auto-compact false --dry-run --json` | `auto_compact=false`, grid alignment retained |
| Place one grid icon | `item position --pane 1 --item-id ITEM_A --column 2 --row 3 --dry-run --json` | Exact `placement.column/row`; empty target cell required |
| Enable free positions | `pane update --id 1 --align-icons-to-grid false --dry-run --json` | `align_icons_to_grid=false`, `auto_compact=false` |
| Place one free icon | `item position --pane 1 --item-id ITEM_A --x 180.5 --y 90 --dry-run --json` | `placement.position_dip`; coordinates are content-relative DIP |
| Rename | `pane update --id 1 --title Work --dry-run --json` | Title, unchanged membership |
| Assign selected icons | `item assign --pane 1 --ids ITEM_A,ITEM_B --dry-run --json` | Destination membership and presentation completion |
| Return icons to desktop | `item release --ids ITEM_A,ITEM_B --dry-run --json` | Desktop placement; real files unchanged |
| Combine panes as tabs | `tab merge --id 1 --into 2 --dry-run --json` | Group members and retained target active tab |

For an explicitly requested top-right column layout, query `monitor list --json`, then use `pane arrange --input FILE_OR_DASH --dry-run --json` with:

```json
{"monitor_id":"ID_FROM_QUERY","columns":[["LEFT_TOP_ID","LEFT_NEXT_ID"],["RIGHT_TOP_ID"]],"icon_columns":6}
```

Columns run left-to-right; panes within them run top-to-bottom. **Arrange places against the top/right work-area edges.** For another location, preserve/place an anchor and snap dependents, or use explicit geometry. Do not choose right-side placement for an unspecified organization request.

## Fit and snap semantics

- Query `content_layout` instead of calculating sizes from source or guessed grid formulas. Fit uses the same row measurement as manual resizing: icon scale, wrapped labels, padding and the last row's actual content. A tab window fits the tallest member, not merely the member with most items. Single panes may be narrower than the initial default; tabs/list views can require more width. Check returned columns rather than claiming an impossible exact count.
- Fit preserves position where possible and moves inward only to remain on-screen. Insufficient space is an error: adjust columns/grouping within the request and preview again. Do not shrink icon scale, hide items or cap visible rows without user intent.
- Snap uses the GUI's fixed gap (currently **5 physical pixels**), not DIP or icon-grid spacing. There is no per-action gap parameter. `side` is left/right/top/bottom; `align` is start/center/end (default start), meaning top/center/bottom beside a pane, or left/center/right above/below it.
- Snap prefers visible window bounds; earlier geometry in the same plan takes precedence. Across monitors it preserves DIP size unless fitting was requested. The anchor stays fixed and may be locked; the moving pane must be unlocked. Both must actually be expanded, including effective auto-hide state. Do not disable auto-hide or change locks just to work around a failure without task authorization.
- Preview/apply returns `CONFLICT` if observed window bounds or collapse state changes. Query again and preview from current state; an old token is not a fresh measurement. For uncertain submissions use receipt recovery, not another layout command.
- Snap/arrange reject out-of-bounds placement and overlap with other panes. List each tab window once; it cannot snap to itself. Loaded folders are supported; search panes have dynamic height and cannot be fit or used as snap targets/sources.
- Snap/arrange place windows once; they do not bind future movement. `panel_defaults.snap` controls proximity snapping during manual dragging. Explicit CLI snapping does not require enabling that setting.

Use `window_bounds_px` for visual verification and physical gaps; `geometry` is saved expanded layout. Do not compare raw DIP values to pixels. Auto-hide can make visible height differ from saved height. For explicit `pane.geometry`, use the queried monitor ID and work-area-relative DIP; the full rectangle must fit. Minimums follow manual content sizing (`minimum_size_dip` for desktop panes); search retains 260 × 160 DIP. Re-query after topology changes.

## Membership and tabs

IDs are authoritative. `item.reorder` needs the complete current item-ID array; verify order by `placement.row`, then `placement.column`, not response array order. Name sorting is a one-time natural order change, only for the named pane, with no auto-sort rule. Folder sorting uses `folder.update`. An already satisfied sort/fit is a no-op.

## Sorting and precise icon layout

Check `capabilities.pane_sort_columns`, `pane_arrangement` and `item_position` before using these extensions on an older app. `pane.sort` accepts `sort_column=name|modified|type|size`; every field defaults to ascending, so request `descending:true` explicitly for newest-first dates. Repeating the same command does not toggle direction. Metadata sorting uses each shortcut file's own properties, not its target. It reads in a worker during preview; apply uses the prepared order, not a fresh filesystem scan.

`auto_compact` means continuous packing, not automatic sorting. Enabling it also aligns to the grid. Disabling alignment disables compact packing; re-enabling alignment snaps to nearby free cells without enabling compact packing. Reject the conflicting pair `auto_compact:true, align_icons_to_grid:false`. Omitted fields preserve state. Arrangement fields affect the named desktop pane, not every tab.

For “keep these icons in specific places”: query membership and mode, turn off compact packing, then use `item.position` for each selected icon. These operations can share a plan. Grid coordinates are nonnegative integers up to 10000; occupied cells are rejected rather than swapping/moving unrelated icons. Free coordinates are content-relative DIP in 0..1000000 and may overlap. Use exactly one complete pair (`column/row` or `x/y`) matching the mode. Assign cross-pane items first; positioning alone never transfers membership. For a grid swap, use an empty temporary cell in the same plan.

Sort/reorder explicitly compact the arrangement using the current visible column count; do not use them to refresh icons or to preserve holes. `fit` includes all occupied positions and may exceed the requested columns. An explicit width reduction preserves positions and allows horizontal scrolling; horizontal scroll is transient and is not an icon coordinate. Verify free placement using `position_dip`, not response order or fallback grid coordinates.

Assignment/release requires desktop integration; receipts can stay pending while images load or Explorer confirms visibility. Inspect `status.desktop_sync_status`; an open native menu can defer synchronization. Removing a nonempty desktop pane requires `release_items:true`; releasing/removing never moves or deletes real files.

Ordinary panes support `tab.merge`, `tab.select`, `tab.reorder` (complete `pane_ids`) and `tab.detach`. Merge appends the source group and retains target active tab/window options. Selection is allowed while locked; detach retains bounds. Folder/search panes cannot join tabs. Window changes affect the shared group, while titles and item order belong to individual panes. Omitted update fields stay unchanged; `null` does not reset them.
