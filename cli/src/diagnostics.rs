//! Opt-in transport diagnostics stay on stderr and exclude request/response payloads.
use luciddesk_api::{Request, Response};
use serde_json::{Value, json};
use std::{io::Write, time::Duration};

fn event(request: &Request, response: &Response, elapsed: Duration) -> Value {
    json!({
        "event":"cli.ipc.completed",
        "command":request.command,
        "request_id":request.request_id,
        "elapsed_us":elapsed.as_micros(),
        "ok":response.ok,
        "exit_code":response.exit_code(),
        "error_code":response.error.as_ref().map(|error| &error.code)
    })
}

pub(super) fn trace(request: &Request, response: &Response, elapsed: Duration) {
    if std::env::var_os("LUCIDDESK_CLI_TRACE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    // A failed diagnostic sink must not change the result of an operation.
    let _ = writeln!(std::io::stderr().lock(), "{}", event(request, response, elapsed));
}

pub(super) fn error_text(response: &Response) -> String {
    let Some(error) = &response.error else { return String::new() };
    // Escape control characters in untrusted messages/IDs without changing JSON output.
    let single_line = |text: &str| text.chars().flat_map(|c| {
        if c.is_control() { c.escape_default().collect::<Vec<_>>() } else { vec![c] }
    }).collect::<String>();
    let mut text = format!("{}: {}\n", single_line(&error.code), single_line(&error.message));
    if !response.request_id.is_empty() {
        text.push_str(&format!("Request ID: {}\n", single_line(&response.request_id)));
    }
    if let Some(data) = &response.data {
        for (field, label) in [("busy", "Busy"), ("next_step", "Next step"), ("recovery", "Recovery")] {
            if let Some(value) = data.get(field) {
                text.push_str(&format!("{label}: {value}\n"));
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_contains_correlation_and_timing_but_no_payloads() {
        let request: Request = serde_json::from_value(json!({"protocol_version":1,
            "request_id":"trace-id","command":"plan.apply","token":"secret-token",
            "data_dir":"private-directory"})).unwrap();
        let mut response = Response::failure("trace-id", "TIMEOUT", "private error path");
        response.data = Some(json!({"private":"response-payload"}));
        let record = event(&request, &response, Duration::from_micros(123));
        assert_eq!(record["elapsed_us"], 123);
        assert_eq!(record["request_id"], "trace-id");
        assert_eq!(record["command"], "plan.apply");
        assert_eq!(record["error_code"], "TIMEOUT");
        assert_eq!(record["exit_code"], response.exit_code());
        let encoded = record.to_string();
        for secret in ["secret-token", "private-directory", "private error path", "response-payload"] {
            assert!(!encoded.contains(secret));
        }
    }

    #[test]
    fn text_uses_authoritative_next_step_and_escapes_control_characters() {
        let mut response = Response::failure("id\nforged", "TIMEOUT", "read timed out\u{1b}[31m");
        response.data = Some(json!({"next_step":{"action":"inspect_error","automatic_retry":false}}));
        let text = error_text(&response);
        assert!(text.contains("Next step: {"));
        assert!(text.contains("inspect_error"));
        assert!(!text.contains("ORIGINAL_ID"));
        assert!(!text.contains("query_receipt"));
        assert!(!text.contains('\u{1b}'));
        assert_eq!(text.lines().count(), 3);
    }
}
