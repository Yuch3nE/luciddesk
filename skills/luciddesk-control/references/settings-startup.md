# Settings, defaults and startup

## Choose scope before changing values

Query `settings get --json`. `values` contains loaded TOML settings; `workspace_values` contains database preferences. Use only returned keys and their types. One `settings.update` must be the sole operation in its plan and cannot mix these two persistence domains. Omitted fields are preserved; disk edits not yet reloaded are not reflected in the query.

| User intent | Scope |
| --- | --- |
| Change this pane's icon/list view, icon-only mode or icon size | `pane.update`; check `capabilities.pane_display` and [Change one pane](desktop-layout.md#change-one-pane). |
| Choose icon display for future panes | `panel_defaults.icon_only` / `panel_defaults.icon_size`; existing panes are unchanged. |
| Choose the default folder view/columns | `folder_defaults.*`; existing folder panes are unchanged. |
| Set the gap between snapped panes | `interface.snap_gap`, in physical pixels; it is not the drag detection threshold. |
| Enable snapping during dragging | `panel_defaults.snap`; use `pane snap` to position a pane now. |

Do not infer every `panel_defaults.*` key is new-pane-only: grid scale and drag snapping also govern current behavior. Follow the particular setting's semantics. Appearance/font changes are application settings, not pane display overrides.

## Preview a settings map

Write a typed UTF-8 JSON map without BOM, for example:

```json
{"panel_defaults.snap":true}
```

Run `settings update --input settings.json --dry-run --json`, inspect the diff, apply the returned `next_step.args`, then query saved values and runtime activation separately. For failed activation, inspect the error and dependency; preview a fresh same-value update only when activation should be retried.

`cli.enabled:false` cuts off subsequent online verification and receipt queries. Use only when disabling control was requested; inspect the apply response and explain that re-enabling requires the Settings UI.

Backup settings configure policy. For backup files and restoration use the workflow below. Change `diagnostics.level` only for requested diagnostics and restore its previous value when that diagnostic session ends.

## Back up, export or restore configuration

Check capabilities first: older apps do not expose `backup.list`, `backup.create`, `backup.export`, or `backup.restore`. These save settings and panel layouts, not user files or plugin-owned settings. Never copy/edit a live database as a substitute.

- `backup list --json`: obtain exact backup paths and busy state.
- `backup create --dry-run --json`: preview a new manual backup in the app backup directory.
- `backup export --path C:\Backups\export.db --dry-run --json`: export current configuration. Add `--source ABSOLUTE_BACKUP_PATH` to export an existing backup instead.
- `backup restore --path ABSOLUTE_BACKUP_PATH --dry-run --json`: preview replacing current settings and layout. Use only for an explicitly requested restore, not as a cleanup or troubleshooting shortcut.

Each action requires its own plan. Review the restore's panel count and replacement scope, then submit the exact returned token within existing authorization. Preview validates and freezes a private copy of the selected file; changes to the original file afterward are not included. Sources are standalone backup files, limited to 64 MiB for preview. Current configuration and live database paths are not restore sources. Export destinations must have an existing parent directory and must not exist; there is no overwrite flag.

Retain the apply request ID and poll `request get` until `data.result.data.operation_status` is terminal. An accepted `pending` result is not completion. Successful exports/creates return `path`; restore returns `rollback_path` for the saved pre-restore state. Restoration rejects concurrent configuration changes before replacement. A failed runtime reload triggers rollback; `commit_status:rolled_back` means restoration failed and prior data was recovered. `unknown` or `rollback_error` requires inspection; do not automatically restore again. Keep the rollback file.

After restoration, re-query settings/workspace and discard previously prepared plans and cached IDs. If the restored configuration disables CLI control, online queries will be denied: verify in the app UI and do not bypass the setting. Never automatically retry a restore shortcut after timeout; query the original receipt. The app's GUI and CLI share the backup busy gate.

## Fonts

Query `font list --json`; boundedly wait for `busy:false` and no error. Choose an exact returned family, submit `{"font.family":"EXACT_RETURNED_FAMILY"}` through the settings workflow, then verify `runtime.font_family`. An empty string selects the language default.

Discovery is cached for 60 seconds and invalidated by language sample changes. Keep its generation when comparing results. Font/workspace preferences must not be mixed with TOML settings in one update.

## Login startup

Query `startup get --json`; wait within a deadline until not busy and without error. Distinguish `status`, `registered` and `effective_enabled`. Require `editable:true` before changing login behavior.

Preview `startup set --enabled true --expected-status OBSERVED_STATUS --dry-run --json` (or `false` to disable) using the freshly observed status. Submit it alone and retain the original receipt. The app checks OS state before writing; do not bypass Windows policy or switch installations.

Poll the receipt until `operation_status` is terminal, then inspect `commit_status`, `startup_status` and error and re-query `startup get`. Acceptance is not completion. Unknown outcomes require checking OS state before another action; see [Plans and recovery](plans-and-recovery.md).
