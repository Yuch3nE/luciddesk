//! CLI-only continuation hints. Original server results and exit codes remain authoritative.
use luciddesk_api::{Request, Response};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) fn invalid_input(message: impl Into<String>) -> Response {
    let mut response = Response::failure("", "INVALID_REQUEST", message);
    response.data = Some(json!({"next_step":{
        "action":"read_help", "args":["help","--json"], "automatic_retry":false
    }}));
    response
}

fn args(request: &Request, timeout: Duration, words: &[&str]) -> Value {
    let mut args: Vec<String> = words.iter().map(|word| (*word).into()).collect();
    args.extend([
        "--json".into(),
        "--protocol-version".into(),
        request.protocol_version.to_string(),
        "--timeout-ms".into(),
        timeout.as_millis().to_string(),
    ]);
    if let Some(directory) = &request.data_dir {
        args.extend(["--data-dir".into(), directory.clone()]);
    }
    json!(args)
}

pub(super) fn attach(response: &mut Response, request: &Request, timeout: Duration) {
    // request.get wraps the original response; outer ok only means the lookup succeeded.
    let result = response.data.as_ref().and_then(|data| data.get("result"));
    let data = result
        .and_then(|result| result.get("data"))
        .or(response.data.as_ref());
    let error = response
        .error
        .as_ref()
        .map(|error| error.code.as_str())
        .or_else(|| {
            result
                .and_then(|result| result.pointer("/error/code"))
                .and_then(Value::as_str)
        });
    let receipt_id = result
        .and_then(|result| result["request_id"].as_str())
        .unwrap_or(&response.request_id);
    let query = || args(request, timeout, &["request", "get", "--id", receipt_id]);
    let inspect = || args(request, timeout, &["workspace", "get"]);
    let step = if let Some(error) = error {
        match error {
            "SORT_METADATA_PENDING" => json!({"action":"wait_and_preview", "automatic_retry":false,
                "args":args(request, timeout, &["plan","preview","--input","-","--request-id",&response.request_id]),
                "stdin_json":data.and_then(|d|d.pointer("/pending_preview/plan")),
                "note":"Preview only; no commit occurred. Retry within a deadline using the same plan and request ID."}),
            "TIMEOUT" | "TRANSPORT_ERROR"
                if request.command == "plan.apply"
                    || data.is_some_and(|data| data.get("recovery").is_some()) =>
            {
                json!({"action":"query_receipt", "args":query(), "automatic_retry":false})
            }
            "CONFLICT" | "PLAN_EXPIRED" => {
                json!({"action":"refresh_and_preview", "args":inspect(), "automatic_retry":false})
            }
            "RESULT_UNKNOWN" => {
                json!({"action":"inspect_state", "args":inspect(), "automatic_retry":false})
            }
            "APP_NOT_RUNNING" => json!({"action":"start_matching_app", "automatic_retry":false}),
            "ACCESS_DENIED" => {
                json!({"action":"check_access_and_cli_setting", "automatic_retry":false})
            }
            "INVALID_REQUEST" => {
                json!({"action":"read_help", "args":["help","--json"], "automatic_retry":false})
            }
            _ => json!({"action":"inspect_error", "automatic_retry":false}),
        }
    } else if let Some(token) = data.and_then(|data| data["plan_token"].as_str()) {
        let id = luciddesk_api::request_id();
        json!({"action":"review_then_apply", "automatic_retry":false,
            "args":args(request, timeout, &["plan","apply","--token",token,"--request-id",&id])})
    } else {
        match data.and_then(|data| data["presentation_status"].as_str()) {
            Some("pending") => {
                json!({"action":"query_receipt", "args":query(), "automatic_retry":false})
            }
            Some("failed" | "superseded") => {
                json!({"action":"inspect_state", "args":inspect(), "automatic_retry":false})
            }
            _ => return,
        }
    };
    response.data.get_or_insert_with(|| json!({}))["next_step"] = step;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn options(command: &str) -> crate::Options {
        crate::parse(command.split_whitespace().map(OsString::from).collect()).unwrap()
    }

    #[test]
    fn pending_sort_continuation_preserves_preview_id_and_plan() {
        let options = options("pane sort --id 1 --sort-column size --dry-run");
        let plan = json!({"protocol_version":1,"base":{"instance_id":"app","state_version":"1","inventory_version":"1","topology_token":"1"},
            "operations":[{"op":"pane.sort","pane_id":"1","sort_column":"size"}]});
        let mut response = Response::failure("metadata-preview", "SORT_METADATA_PENDING", "pending");
        response.data = Some(json!({"pending_preview":{"request_id":"metadata-preview","plan":plan}}));
        attach(&mut response, &options.request, options.timeout);
        let step = &response.data.as_ref().unwrap()["next_step"];
        assert_eq!(step["action"], "wait_and_preview");
        assert_eq!(step["stdin_json"], plan);
        let args: Vec<_> = step["args"].as_array().unwrap().iter().map(|v|v.as_str().unwrap()).collect();
        assert!(args.windows(2).any(|p|p == ["--request-id", "metadata-preview"]));
        assert_eq!(&args[..2], ["plan", "preview"]);
        assert_eq!(step["automatic_retry"], false);
    }

    #[test]
    fn preview_continuation_is_executable_without_shell_parsing() {
        let mut options = options("pane create --title test --dry-run");
        options.request.data_dir = Some(std::env::current_dir().unwrap().to_string_lossy().into());
        let mut response = Response::success(
            "preview",
            json!({}),
            json!({"plan_token":"token with spaces"}),
        );
        attach(&mut response, &options.request, options.timeout);
        let step = &response.data.as_ref().unwrap()["next_step"];
        assert_eq!(step["action"], "review_then_apply");
        let parsed = crate::parse(
            step["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| OsString::from(value.as_str().unwrap()))
                .collect(),
        )
        .unwrap();
        assert_eq!(parsed.request.command, "plan.apply");
        assert_eq!(parsed.request.token.as_deref(), Some("token with spaces"));
        assert!(parsed.json);
        assert_ne!(parsed.request.request_id, "preview");
        assert_eq!(parsed.timeout, options.timeout);
        assert!(parsed.request.data_dir.is_some());
    }

    #[test]
    fn receipt_lookup_uses_original_id_and_preserves_status() {
        let options = options("request get --id original");
        for (status, action) in [
            ("pending", Some("query_receipt")),
            ("failed", Some("inspect_state")),
            ("superseded", Some("inspect_state")),
            ("applied", None),
        ] {
            let original = json!({"request_id":"original","ok":true,"data":{
                "commit_status":"committed","presentation_status":status}});
            let mut response = Response::success("lookup", json!({}), json!({"result":original}));
            attach(&mut response, &options.request, options.timeout);
            let data = response.data.unwrap();
            assert_eq!(data["result"], original);
            assert_eq!(data["next_step"]["action"].as_str(), action);
            if status == "pending" {
                assert_eq!(data["next_step"]["args"][3], "original");
            }
        }
    }

    #[test]
    fn uncertain_apply_is_queried_not_repeated() {
        let options = options("plan apply --token token --request-id fixed");
        for code in [
            "TIMEOUT",
            "TRANSPORT_ERROR",
            "RESULT_UNKNOWN",
            "CONFLICT",
            "ACCESS_DENIED",
        ] {
            let mut response = Response::failure("fixed", code, "test");
            let exit = response.exit_code();
            attach(&mut response, &options.request, options.timeout);
            assert_eq!(response.exit_code(), exit);
            assert_eq!(response.error.as_ref().unwrap().code, code);
            let step = &response.data.unwrap()["next_step"];
            assert_eq!(step["automatic_retry"], false);
            if matches!(code, "TIMEOUT" | "TRANSPORT_ERROR") {
                assert_eq!(step["action"], "query_receipt");
                assert_eq!(step["args"][3], "fixed");
            }
        }
    }
}
