# Folder and search workflows

Preview mutations with `--dry-run --json`, apply the reviewed token, then verify the resource. Desktop icon IDs and folder/search entries are different resources. For snapping/arrangement, also read [Desktop and layout](desktop-layout.md); ordinary queries do not need it.

## Map a folder, choose its view, then fit

1. Reuse an appropriate mapping or preview `folder create --title Projects --path "C:\Projects" --dry-run --json` for an existing absolute directory. Apply and resolve the pane ID from `refs`.
2. Query `folder get --id 1 --json`; check current path, view, loading/error and `content_layout.ready`. New mappings inherit `folder_defaults.list_view` and default visible columns; query the result instead of assuming a fixed view. Wait for a ready snapshot before fitting.
3. Keep the requested view. For icons, preview/apply `folder update --id 1 --list-view false --dry-run --json`, then re-query. If sorting is requested, apply it before fitting because label order affects the last row.
4. Preview/apply the matching fit:
   - Icons: `folder fit --id 1 --icon-columns 4 --max-rows 5 --dry-run --json`.
   - List: `folder fit --id 1 --max-rows 5 --dry-run --json`; omit icon columns. Width is retained subject to the view minimum; height includes the column header.
5. Verify actual bounds, correct path/view and retained entries. Use `--max-rows` only for a requested bounded viewport; omit it to fit all entries. It adds scrolling, not truncation. Newly created/remapped content must load before a later fit plan.

For per-pane icon-only mode and icon size, use `pane.update` when `capabilities.pane_display` supports them; see [Change one pane](desktop-layout.md#change-one-pane). `folder.update` changes the saved mapping/view/sort preferences. Query returned `runtime` preferences; column widths use name/modified/type/size order and sum to 1, while sort indices use name=0, type=1, modified=2, size=3. Name stays visible. Pane options, geometry and removal apply; removing a mapping does not delete its directory.

## Refresh or navigate

- Refresh: preview/apply `folder refresh --id 1 --dry-run --json` once, then poll `folder get` within a deadline until ready and error-free at the intended path.
- Open a directory: `folder navigate --id 1 --path "C:\Projects\Demo" --dry-run --json`. Use `folder back` / `folder home` to return to history or the mapped directory.

Submit each refresh/navigation action separately. They do not persist navigation history or change the saved root. Loaded entries, loading/error and navigation affect plan conflicts. New builds expose `content_revision`: a process-local snapshot marker, not a file count or durable file version. A rename can invalidate a preview without changing the count. Even a ready snapshot can change between preview and apply when another scan completes. After `CONFLICT`, query again and preview from fresh context within a bounded deadline; an exact receipt retry must not trigger another scan. The app snapshot may lag external filesystem changes.

## Search, wait for its generation, then page

1. Query `pane list --json` for an existing search pane and `search get --id 1 --json` for its state. A desktop/folder pane is not a substitute. Enable search settings only when authorized by the task.
2. Preview/apply `search query --id 1 --query "*.pdf" --dry-run --json`. Retain `runtime_result.generation`.
3. Poll `search get` within a deadline for that generation, with no busy/replacing/failed state. Old entries can remain visible while a replacement loads; do not report them as the new result.
4. Check `has_more` before previewing/applying `search more --id 1 --dry-run --json`. Verify the resulting state before another page. `search refresh` repeats the current query; an empty query clears it.

Each search action is a separate plan and returns `commit_status:not_persisted`. Do not launch a backend beyond the user's authorization. Report results/remaining pages, or the unresolved generation and error; avoid unbounded polling.
