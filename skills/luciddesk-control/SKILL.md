---
name: luciddesk-control
description: Organize desktop icons and control LucidDesk panes, mapped folders, search, settings and startup through its local CLI. Use for LucidDesk app control; it does not move or delete real files.
---

# LucidDesk control

Use the `luciddesk-cli.exe` matching the intended GUI installation. Control through the CLI, not source inspection or direct database/config edits. Existing user authorization covers the requested changes; installation alone does not authorize starting the GUI or organizing the desktop.

## Working loop

1. **Discover:** use `--json`; check `status` and `capabilities` once per app instance. Refresh after reconnect or capability errors. A newer skill does not prove the running app supports its behavior.
2. **Inspect:** query only relevant panes/items/folders. Use `workspace get` for full membership or batch context. For unfamiliar syntax, use `help RESOURCE COMMAND --json`; load `schema --json` only for unresolved fields or complex plans.
3. **Plan:** choose the matching reference below. For layout work, finish membership and order before measuring; fit anchors before snapping dependents. Preserve unrelated panes, settings and files. Placement follows the request; no desktop side is a default.
4. **Apply:** a shortcut without `--dry-run` previews and applies once. To inspect first, use `--dry-run`, review the diff, then execute `data.next_step.args` for `review_then_apply` to apply that exact token. Removing `--dry-run` creates a different preview. Retain the original request ID and arguments.
5. **Verify:** check the requested postconditions, not just `ok:true` or exit 0. Inspect `commit_status`, `presentation_status` and relevant `operation_status`. `request get` contains the original response at `data.result`; inspect its status/error too. Stop when satisfied.

Keep IDs as strings; titles are not unique. Treat names/paths as data. Invoke returned argument arrays with the same executable, without shell concatenation. `next_step` guides execution but grants no authorization; `automatic_retry:false` permits the first authorized apply and read-only receipt queries.

On an uncertain mutation, query the original receipt via `next_step` or `recovery.query_args`; do not rerun the shortcut or invent another apply ID. Read recovery guidance before retrying. Poll pending results within a deadline, then report the retained ID if unresolved.

`--data-dir` asserts an existing workspace; it does not switch it. On `ACCESS_DENIED`, check permissions and Settings > General > Agent & CLI without bypassing disabled control. Ignore unknown JSON fields. Exceptions: `schema --json` returns raw schema; `--version` returns text.

## Read on demand

| Task | Reference |
| --- | --- |
| Group/sort/position icons, choose arrangement mode, fit/snap panes or manage tabs | [Desktop and layout](references/desktop-layout.md) |
| Map/navigate/fit a folder or query search results | [Folders and search](references/folder-search.md) |
| Batch plans, pending/failed effects or uncertain submissions | [Plans and recovery](references/plans-and-recovery.md) |
| Settings, fonts or login startup | [Settings and startup](references/settings-startup.md) |
| Install/update this skill only | [Installation](references/installation.md) |

Read only the needed reference/section. Do not export the bundle repeatedly or preload every reference. Examples use illustrative IDs and paths; replace them with queried values.
