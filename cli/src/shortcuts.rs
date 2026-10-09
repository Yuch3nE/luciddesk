//! Single operations reuse the server's preview, concurrency and receipt protocol.
use super::*;
use luciddesk_api::{Context, Operation, Plan};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub(super) fn supported(command: &str) -> bool {
    luciddesk_api::OPERATIONS.contains(&command)
}
pub(super) fn flag(flag: &str) -> Option<&'static str> {
    Some(match flag {
        "--target" => "target_pane_id",
        "--descending" => "descending",
        "--sort-column" => "sort_column",
        "--auto-compact" => "auto_compact",
        "--align-icons-to-grid" => "align_icons_to_grid",
        "--item-id" => "item_id",
        "--column" => "column",
        "--row" => "row",
        "--side" => "side",
        "--align" => "align",
        "--max-rows" => "max_rows",
        "--icon-columns" => "icon_columns",
        "--title" => "title",
        "--path" => "path",
        "--query" => "query",
        "--into" => "into_pane_id",
        "--monitor" => "monitor_id",
        "--x" => "x",
        "--y" => "y",
        "--width" => "width",
        "--height" => "height",
        "--locked" => "locked",
        "--auto-hide" => "auto_hide",
        "--collapsed" => "collapsed",
        "--always-on-top" => "always_on_top",
        "--list-view" => "list_view",
        "--enabled" => "enabled",
        "--expected-status" => "expected_status",
        "--ids" => "item_ids",
        _ => return None,
    })
}
pub(super) fn value(flag_name: &str, raw: &str) -> Result<(String, Value), String> {
    let key = flag(flag_name).ok_or("unknown shortcut option")?;
    let value = match key {
        "column" | "row" => json!(raw.parse::<u32>().map_err(|_| format!("{flag_name} expects a nonnegative integer"))?),
        "max_rows" => json!(
            raw.parse::<u32>()
                .ok()
                .filter(|n| (1..=10000).contains(n))
                .ok_or("--max-rows expects 1..10000")?
        ),
        "icon_columns" => json!(
            raw.parse::<u32>()
                .ok()
                .filter(|n| (1..=64).contains(n))
                .ok_or("--icon-columns expects 1..64")?
        ),
        "auto_compact" | "align_icons_to_grid" | "descending" | "locked" | "auto_hide" | "collapsed" | "always_on_top" | "list_view" | "enabled" => json!(
            raw.parse::<bool>()
                .map_err(|_| format!("{flag_name} expects true or false"))?
        ),
        "x" | "y" | "width" | "height" => {
            let n = raw
                .parse::<f64>()
                .map_err(|_| format!("{flag_name} expects a number"))?;
            if !n.is_finite() {
                return Err("non-finite geometry".into());
            }
            json!(n)
        }
        "item_ids" => {
            let ids: Vec<_> = raw.split(',').collect();
            if ids.iter().any(|id| id.is_empty()) {
                return Err("--ids expects comma-separated nonempty IDs".into());
            }
            json!(ids)
        }
        _ => json!(raw),
    };
    Ok((key.into(), value))
}
pub(super) fn operation(
    command: &str,
    id: Option<String>,
    pane: Option<String>,
    fields: BTreeMap<String, Value>,
    input: Option<Vec<u8>>,
) -> Result<Operation, String> {
    let mut object = serde_json::Map::new();
    if let Some(input) = input {
        let value: Value = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
        if command == "settings.update" {
            object.insert("values".into(), value);
        } else if command == "item.reorder" && value.is_array() {
            object.insert("item_ids".into(), value);
        } else {
            object = value
                .as_object()
                .ok_or("shortcut input must be a JSON object")?
                .clone();
        }
    }
    let mut insert = |key: String, value: Value| -> Result<(), String> {
        if object.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate operation field: {key}"));
        }
        Ok(())
    };
    for (key, value) in fields {
        insert(key, value)?;
    }
    if let Some(id) = id {
        insert("pane_id".into(), json!(id))?;
    }
    if let Some(pane) = pane {
        insert("pane_id".into(), json!(pane))?;
    }
    insert("op".into(), json!(command))?;
    if matches!(command, "pane.create" | "folder.create") && !object.contains_key("ref") {
        object.insert("ref".into(), json!("created"));
    }
    serde_json::from_value(Value::Object(object)).map_err(|e| e.to_string())
}
pub(super) fn run(
    options: &Options,
    call: impl FnMut(&Request, Duration) -> Response,
) -> Response {
    let mut response = execute(options, call);
    crate::next_step::attach(&mut response, &options.request, options.timeout);
    response
}

fn execute(
    options: &Options,
    mut call: impl FnMut(&Request, Duration) -> Response,
) -> Response {
    let Some(operation) = &options.operation else {
        return if options.request.command == "plan.apply" {
            submit(&options.request, options.timeout, &mut call)
        } else {
            preview_call(&options.request, options.timeout, &mut call)
        };
    };
    let mut query = options.request.clone();
    query.request_id = luciddesk_api::request_id();
    let snapshot = call(&query, options.timeout);
    if !snapshot.ok {
        return snapshot;
    }
    let context: Context = match snapshot
        .context
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(context) => context,
        None => {
            return Response::failure(
                &options.request.request_id,
                "PROTOCOL_MISMATCH",
                "Workspace response omitted a valid context",
            );
        }
    };
    let mut preview = options.request.clone();
    preview.command = "plan.preview".into();
    preview.request_id = luciddesk_api::request_id();
    preview.plan = Some(Plan {
        protocol_version: options.request.protocol_version,
        base: context,
        operations: vec![operation.clone()],
    });
    let prepared = preview_call(&preview, options.timeout, &mut call);
    if !prepared.ok || options.dry_run {
        return prepared;
    }
    let Some(token) = prepared
        .data
        .as_ref()
        .and_then(|data| data["plan_token"].as_str())
        .map(str::to_owned)
    else {
        return Response::failure(
            &options.request.request_id,
            "PROTOCOL_MISMATCH",
            "Preview response omitted plan token",
        );
    };
    let mut apply = options.request.clone();
    apply.command = "plan.apply".into();
    apply.token = Some(token.clone());
    submit(&apply, options.timeout, &mut call)
}

// Direct and shortcut submissions share the same replay contract, even on timeout.
fn preview_call(request: &Request, timeout: Duration, call: &mut impl FnMut(&Request, Duration) -> Response) -> Response {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let response = call(request, remaining);
        if request.command != "plan.preview" || !response.error.as_ref().is_some_and(|e| e.code == "SORT_METADATA_PENDING") || remaining <= Duration::from_millis(50) {
            return response;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn submit(
    request: &Request,
    timeout: Duration,
    call: &mut impl FnMut(&Request, Duration) -> Response,
) -> Response {
    let mut response = call(request, timeout);
    let token = request.token.as_deref().expect("validated plan.apply token");
    let mut retry_args = vec!["plan".to_owned(), "apply".into(), "--token".into(), token.into(),
        "--request-id".into(), request.request_id.clone()];
    let mut query_args = vec!["request".to_owned(), "get".into(), "--id".into(), request.request_id.clone()];
    let mut common = vec!["--protocol-version".to_owned(), request.protocol_version.to_string(),
        "--timeout-ms".into(), timeout.as_millis().to_string(), "--json".into()];
    if let Some(directory) = &request.data_dir {
        common.extend(["--data-dir".into(), directory.clone()]);
    }
    retry_args.extend(common.clone());
    query_args.extend(common);
    let data = response.data.get_or_insert_with(|| json!({}));
    data["recovery"] = json!({
        "command":"plan.apply", "plan_token":token, "request_id":request.request_id,
        "protocol_version":request.protocol_version, "data_dir":request.data_dir,
        "retry_args":retry_args, "query_args":query_args
    });
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrangement_sort_and_position_flags_are_typed() {
        assert!(matches!(parse_words(&["pane","update","--id","1","--auto-compact","false","--align-icons-to-grid","false"]).operation,
            Some(Operation::Update { auto_compact: Some(false), align_icons_to_grid: Some(false), .. })));
        assert!(matches!(parse_words(&["pane","sort","--id","1","--sort-column","size"]).operation,
            Some(Operation::Sort { sort_column: luciddesk_api::FolderColumn::Size, descending: false, .. })));
        assert!(matches!(parse_words(&["item","position","--pane","1","--item-id","abc","--column","2","--row","3"]).operation,
            Some(Operation::Position { column: Some(2), row: Some(3), .. })));
        for words in ["pane sort --id 1 --sort-column unknown", "pane update --id 1 --auto-compact yes", "item position --pane 1 --item-id abc --column -1 --row 0"] {
            assert!(parse(words.split_whitespace().map(OsString::from).collect()).is_err());
        }
    }

    #[test]
    fn pending_metadata_retries_only_the_same_pure_preview() {
        let mut req = parse_words(&["workspace", "get"]).request;
        req.command = "plan.preview".into();
        let mut calls = 0;
        let result = preview_call(&req, Duration::from_secs(1), &mut |request, _| {
            assert_eq!(request.request_id, req.request_id);
            assert_eq!(request.command, "plan.preview");
            calls += 1;
            if calls == 1 { Response::failure(&request.request_id, "SORT_METADATA_PENDING", "pending") }
            else { Response::success(&request.request_id, json!({}), json!({"plan_token":"ready"})) }
        });
        assert!(result.ok);
        assert_eq!(calls, 2);
        let mut calls = 0;
        let result = preview_call(&req, Duration::ZERO, &mut |request, _| {
            calls += 1;
            Response::failure(&request.request_id, "SORT_METADATA_PENDING", "pending")
        });
        assert!(!result.ok);
        assert_eq!(calls, 1);
    }
    fn parse_words(words: &[&str]) -> Options {
        parse(words.iter().map(OsString::from).collect()).unwrap()
    }
    #[test]
    fn folder_fit_and_refresh_preserve_typed_optional_fields() {
        assert!(matches!(parse_words(&["folder","fit","--id","1","--max-rows","5"]).operation,Some(Operation::FolderFit{icon_columns:None,max_rows:Some(5),..})));
        assert!(matches!(parse_words(&["folder","refresh","--id","1"]).operation,Some(Operation::FolderRefresh{..})));
        assert!(parse("folder fit --id 1 --max-rows 0".split_whitespace().map(OsString::from).collect()).is_err());
    }
    #[test]
    fn snap_flags_are_typed_and_do_not_accept_per_operation_gap() {
        let parsed = parse_words(&[
            "pane", "snap", "--id", "1", "--target", "2", "--side", "bottom", "--align", "end",
        ]);
        assert!(matches!(
            parsed.operation,
            Some(Operation::Snap {
                side: luciddesk_api::SnapSide::Bottom,
                align: luciddesk_api::SnapAlign::End,
                ..
            })
        ));
        for command in [
            "pane snap --id 1 --target 2 --side sideways",
            "pane snap --id 1 --target 2 --side left --gap-px 8",
        ] {
            assert!(parse(command.split_whitespace().map(OsString::from).collect()).is_err());
        }
    }
    #[test]
    fn shortcuts_preserve_typed_values_and_reject_ambiguous_flags() {
        let options = parse_words(&[
            "pane",
            "create",
            "--title",
            "中文 --title content",
            "--dry-run",
        ]);
        assert!(options.dry_run);
        assert!(
            matches!(options.operation,Some(Operation::Create{title,..}) if title=="中文 --title content")
        );
        let options = parse_words(&["pane", "update", "--id", "1", "--locked", "false"]);
        assert!(matches!(
            options.operation,
            Some(Operation::Update {
                locked: Some(false),
                ..
            })
        ));
        for words in [
            vec!["status", "--title", "bad"],
            vec!["pane", "create", "--id", "1", "--title", "bad"],
            vec!["pane", "update", "--id", "1", "--locked", "yes"],
            vec!["pane", "remove", "--id", "1", "--pane", "2"],
            vec!["status", "--dry-run"],
            vec!["pane", "geometry", "--x", "NaN"],
        ] {
            assert!(
                parse(words.iter().map(OsString::from).collect()).is_err(),
                "{words:?}"
            );
        }
    }
    #[test]
    fn sort_shortcut_has_typed_direction_and_default() {
        let ascending = parse_words(&["pane", "sort", "--id", "1"]);
        assert!(matches!(ascending.operation, Some(Operation::Sort { descending: false, .. })));
        let descending = parse_words(&["pane", "sort", "--id", "1", "--descending", "true"]);
        assert!(matches!(descending.operation, Some(Operation::Sort { descending: true, .. })));
        assert!(value("--descending", "yes").is_err());
    }
    #[test]
    fn direct_and_shortcut_recovery_reconstruct_exact_request_with_directory() {
        let directory = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        for shortcut in [false, true] {
            for timed_out in [false, true] {
                let mut options = if shortcut {
                    parse_words(&["pane", "create", "--title", "test"])
                } else {
                    parse_words(&["plan", "apply", "--token", "token"])
                };
                options.request.data_dir = Some(directory.to_string_lossy().into_owned());
                options.timeout = Duration::from_millis(1234);
                let context = json!({"instance_id":"i","state_version":"1","inventory_version":"1","topology_token":"1"});
                let mut submitted = None;
                let response = run(&options, |request, _| {
                    if request.command == "plan.apply" {
                        submitted = Some(request.clone());
                        if timed_out { return Response::failure(&request.request_id, "TIMEOUT", "injected"); }
                        return Response::success(&request.request_id, context.clone(), json!({"commit_status":"committed"}));
                    }
                    Response::success(&request.request_id, context.clone(), json!({"plan_token":"token"}))
                });
                assert_eq!(response.exit_code(), if timed_out {8} else {0});
                let data = response.data.unwrap();
                if !timed_out { assert_eq!(data["commit_status"], "committed"); }
                let recovery = &data["recovery"];
                let parse_recovery = |field: &str| parse(recovery[field].as_array().unwrap().iter().map(|v| OsString::from(v.as_str().unwrap())).collect()).unwrap();
                let replay = parse_recovery("retry_args");
                assert_eq!(serde_json::to_value(&replay.request).unwrap(), serde_json::to_value(submitted.unwrap()).unwrap());
                assert_eq!(replay.timeout, options.timeout);
                assert!(replay.json);
                let query = parse_recovery("query_args");
                assert_eq!(query.request.command, "request.get");
                assert_eq!(query.request.id.as_deref(), Some(options.request.request_id.as_str()));
                assert_eq!(query.request.data_dir, options.request.data_dir);
                assert_eq!(recovery["request_id"], options.request.request_id);
            }
        }
    }
    #[test]
    fn shortcut_preview_and_apply_use_same_context_and_preserve_recovery_after_timeout() {
        for dry_run in [false, true] {
            let mut options =
                parse_words(&["pane", "remove", "--id", "1", "--request-id", "fixed-id"]);
            options.dry_run = dry_run;
            let context = json!({"instance_id":"instance","state_version":"1","inventory_version":"2","topology_token":"3"});
            let mut commands = Vec::new();
            let response = run(&options, |request, _| {
                commands.push(request.command.clone());
                match request.command.as_str() {
                    "workspace.get" => {
                        Response::success(&request.request_id, context.clone(), json!({}))
                    }
                    "plan.preview" => {
                        assert_eq!(json!(request.plan.as_ref().unwrap().base), context);
                        Response::success(
                            &request.request_id,
                            context.clone(),
                            json!({"plan_token":"token"}),
                        )
                    }
                    "plan.apply" => {
                        assert_eq!(request.token.as_deref(), Some("token"));
                        assert_eq!(request.request_id, "fixed-id");
                        Response::failure(&request.request_id, "TIMEOUT", "injected timeout")
                    }
                    _ => panic!("unexpected command"),
                }
            });
            if dry_run {
                assert_eq!(commands, vec!["workspace.get", "plan.preview"]);
                assert!(response.ok);
                assert_eq!(response.data.as_ref().unwrap()["next_step"]["action"], "review_then_apply");
            } else {
                assert_eq!(
                    commands,
                    vec!["workspace.get", "plan.preview", "plan.apply"]
                );
                assert_eq!(response.exit_code(), 8);
                assert_eq!(response.data.as_ref().unwrap()["next_step"]["action"], "query_receipt");
                assert_eq!(response.data.unwrap()["recovery"]["plan_token"], "token");
            }
        }
    }
}
