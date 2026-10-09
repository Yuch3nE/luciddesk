# Plans and recovery

## Batch plans

For related desktop changes, use a single plan so validation and durable workspace changes form one transaction. Settings, startup, folder navigation/refresh and search actions require separate single-operation plans.

1. Query `workspace get --json` for fresh `context`, exact IDs, pane kinds and lock state. Use already-discovered capabilities unless the instance changed.
2. Serialize UTF-8 JSON without BOM: `protocol_version:1`, `base` equal to the complete returned context, and ordered `operations`. Pass via `--input -` or a file. Use dotted operation names (`pane.update`) in JSON and words (`pane update`) at the CLI.
3. Run `plan preview --input FILE_OR_DASH --json`. Inspect `ok`, `data.diff`, and `data.provisional_refs` against the requested scope.
4. Apply via `data.next_step.args` for `review_then_apply`, or `plan apply --token TOKEN --request-id UNIQUE_ID --json`. Retain the exact token/ID/options. Check original result fields and query postconditions by returned `refs`; use fresh context for subsequent plans.

To create then assign, put `pane.create` with `ref`/`title` before `item.assign` with `pane_ref`/`item_ids`. Other operations require actual pane IDs as defined by the schema. Tokens expire after five minutes and may be evicted earlier.

Shortcut input differs from a plan: operation fields omit `op`; `settings update` accepts a plain settings map, and `item reorder` also accepts a complete ID array. Flags and input cannot duplicate fields. Use JSON for structured fields or literal values beginning with `--` or equal to `-h`; query command help for types, required fields and aliases.

## Next-step decisions

`data.next_step` supplements original state. For `request get`, the hint is on the outer `data`; the original result remains at `data.result`. Older builds may omit hints. Unknown actions require inspecting the original response and help, not guessing a mutation.

| Action | Decision |
| --- | --- |
| `review_then_apply` | Review diff, then apply within existing authorization. |
| `query_receipt` | Query original receipt; poll pending status within a deadline. |
| `refresh_and_preview` | Read fresh state, then build a new plan preserving concurrent changes. |
| `inspect_state` | Inspect current state; also query folder/search/settings/startup as relevant. Workspace alone may not establish their state. |
| `read_help` | Read help, then the specific command topic to fix input. |
| `start_matching_app` | Resolve intended GUI/session within authorization. |
| `check_access_and_cli_setting` | Resolve access without bypassing restrictions. |
| `inspect_error` | Handle the original error code; no generic retry is implied. |

Some manual actions omit `args`. In PowerShell, invoke a supplied array with `$nextArgs = [string[]]$result.data.next_step.args; & $cli @nextArgs`, after interpreting its action and scope.

## Recover without repeating effects

Direct and shortcut submissions provide `data.recovery`. Retain the entire object: `query_args` finds the original receipt; `retry_args` replays the exact apply. Both preserve token, request ID, directory assertion, protocol and timeout. If absent, use retained original arguments.

On timeout or transport failure after submission, query first. Exact replay is only for recovery after inspecting the result; rerunning a shortcut creates a new plan and can repeat effects. Same ID with a different request produces `REQUEST_ID_REUSED`. A timeout does not cancel submitted work.

- `pending`: completion unresolved. Stop polling at the deadline and report the original ID; do not resubmit with a new ID.
- Presentation `failed`: a committed save is not rolled back. Inspect error and current resource state before another plan.
- `superseded`: later changes replaced the desired desktop membership; preserve those changes.
- Restart or `RESULT_UNKNOWN`: receipts/item IDs are instance-local; inspect current state before deciding whether further action is needed.

| Error code | Recovery |
| --- | --- |
| `CONFLICT`, `PLAN_EXPIRED` | Refresh state and preview a new plan. Use a new ID only for the new request. |
| `PLAN_ALREADY_APPLIED`, `REQUEST_ID_REUSED` | Inspect original receipt and postconditions. |
| `PROTOCOL_MISMATCH`, `DATA_DIR_MISMATCH` | Select matching CLI/GUI or intended workspace; do not silently switch instances. |
| `INVALID_REQUEST` | Fix input using command help/schema. |
| `CAPABILITY_UNAVAILABLE` | Inspect relevant loading/backend state; folders/search may work without desktop integration. |
| `BUSY` | Bounded delay/deadline; retain mutation identity. |
| `SORT_METADATA_PENDING` | Pure preview is still reading file properties; nothing was saved. The CLI polls within its timeout. If still pending, use the returned `pending_preview` with the same preview request ID; do not query an apply receipt or claim a commit. |
| `NOT_FOUND`, `PANE_LOCKED` | Re-query targets or resolve lock within task scope. |
| `PERSISTENCE_ERROR` | Report failure and inspect state before another plan. |

Prefer `error.code` over parsing messages. Stop polling terminal results; queries and previews do not save.
