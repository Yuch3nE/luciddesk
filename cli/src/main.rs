mod diagnostics;
mod preview_summary;
mod help;
mod next_step;
mod skill;
mod shortcuts;
use luciddesk_api::{Request, Response, VERSION};
use std::{ffi::OsString, time::Duration};
struct Options {
    request: Request,
    json: bool,
    timeout: Duration,
    operation: Option<luciddesk_api::Operation>,
    dry_run: bool,
}
// Reject option-shaped values before they can consume safety/output flags.
// Use JSON input for literal strings beginning with -- or equal to -h.
fn option_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, String> {
    let value = args.next().ok_or_else(|| format!("{flag} requires a value"))?
        .into_string().map_err(|_| "invalid Unicode option")?;
    if value.starts_with("--") || value == "-h" {
        return Err(format!("{flag} requires a value before {value}; use --input JSON for literal option-like strings"));
    }
    Ok(value)
}
fn parse(args: Vec<OsString>) -> Result<Options, String> {
    let mut request = Request {
        protocol_version: VERSION,
        request_id: luciddesk_api::request_id(),
        command: String::new(),
        id: None,
        pane: None,
        unassigned: false,
        data_dir: None,
        plan: None,
        token: None,
    };
    let mut json = false;
    let mut timeout = 10000;
    let mut words = Vec::new();
    let mut input = None;
    let mut fields = std::collections::BTreeMap::new();
    let mut dry_run = false;
    let mut args = args.into_iter();
    let mut seen = std::collections::HashSet::new();
    while let Some(raw) = args.next() {
        let arg = raw
            .into_string()
            .map_err(|_| "arguments must be valid Unicode")?;
        if arg.starts_with('-') && !seen.insert(arg.clone()) {
            return Err(format!("duplicate option {arg}"));
        }
        match arg.as_str() {
            "--json" => json = true,
            "--dry-run" => dry_run = true,
            "--release-items" => {
                fields.insert("release_items".into(), serde_json::json!(true));
            }
            flag if shortcuts::flag(flag).is_some() => {
                let value = option_value(&mut args, &flag)?;
                let (key, value) = shortcuts::value(flag, &value)?;
                fields.insert(key, value);
            }
            "--unassigned" => request.unassigned = true,
            "--id" | "--pane" | "--data-dir" | "--timeout-ms" | "--protocol-version"
            | "--input" | "--token" | "--request-id" => {
                let value = option_value(&mut args, &arg)?;
                match arg.as_str() {
                    "--input" => input = Some(value),
                    "--token" => request.token = Some(value),
                    "--request-id" => request.request_id = value,
                    "--id" => request.id = Some(value),
                    "--pane" => request.pane = Some(value),
                    "--data-dir" => {
                        let path = std::fs::canonicalize(value)
                            .map_err(|e| format!("invalid data directory: {e}"))?;
                        request.data_dir = Some(path.to_string_lossy().into_owned());
                    }
                    "--timeout-ms" => {
                        timeout = value
                            .parse::<u64>()
                            .ok()
                            .filter(|n| (1..=60000).contains(n))
                            .ok_or("timeout must be 1..60000 ms")?
                    }
                    _ => {
                        request.protocol_version =
                            value.parse().map_err(|_| "invalid protocol version")?
                    }
                }
            }
            value if value.starts_with('-') => return Err(format!("unknown option {value}")),
            _ => words.push(arg),
        }
    }
    request.command = words.join(".");
    let operation = if shortcuts::supported(&request.command) {
        if request.token.is_some() || request.unassigned {
            return Err("shortcut commands reject --token and --unassigned".into());
        }
        let payload = input.map(|path| read_input(&path)).transpose()?;
        let operation = shortcuts::operation(
            &request.command,
            request.id.take(),
            request.pane.take(),
            fields,
            payload,
        )?;
        // Status carries the same concurrency context without the full inventory.
        request.command = "status".into();
        Some(operation)
    } else {
        if dry_run || !fields.is_empty() {
            return Err("mutation options require a shortcut command".into());
        }
        if let Some(input) = input {
            if request.command != "plan.preview" {
                return Err("--input is only valid for plan preview or shortcut commands".into());
            }
            request.plan =
                Some(luciddesk_api::decode_json(&read_input(&input)?).map_err(|e| e.to_string())?);
        }
        None
    };
    request.validate().map_err(str::to_owned)?;
    Ok(Options {
        request,
        json,
        timeout: Duration::from_millis(timeout),
        operation,
        dry_run,
    })
}
fn read_input(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut reader: Box<dyn Read> = if path == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((luciddesk_api::MAX_FRAME + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > luciddesk_api::MAX_FRAME {
        return Err("input exceeds 4 MiB".into());
    }
    Ok(bytes)
}
fn call(client: &mut Option<luciddesk_api::transport::Client>, request: &Request, timeout: Duration) -> Response {
    let started = std::time::Instant::now();
    let result = (|| {
        if client.is_none() { *client = Some(luciddesk_api::transport::Client::new()?); }
        client.as_mut().unwrap().call(request, timeout)
    })();
    let response = result.unwrap_or_else(|e: std::io::Error| {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "APP_NOT_RUNNING",
            std::io::ErrorKind::PermissionDenied => "ACCESS_DENIED",
            std::io::ErrorKind::TimedOut => "TIMEOUT",
            _ => "TRANSPORT_ERROR",
        };
        Response::failure(&request.request_id, code, e.to_string())
    });
    diagnostics::trace(request, &response, started.elapsed());
    response
}
fn offline_output(args: &[OsString]) -> Option<String> {
    let json_count = args.iter().filter(|arg| *arg == "--json").count();
    if json_count > 1 {
        return None; // Let the regular parser report duplicate options.
    }
    let words: Vec<_> = args.iter().filter(|arg| *arg != "--json").collect();
    if words == [&OsString::from("schema")] {
        return Some(include_str!("../../crates/luciddesk-api/protocol.schema.json").into());
    }
    if words != [&OsString::from("skill"), &OsString::from("show")] {
        return None;
    }
    Some(if json_count == 1 {
        let response = Response::success(
            "offline",
            serde_json::Value::Null,
            skill::bundle(),
        );
        serde_json::to_string(&response).expect("skill serialization")
    } else {
        skill::ENTRYPOINT.into()
    })
}
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let error_help = help::error_args(&args);
    if let Some(output) = help::output(&args) {
        match output {
            Ok(text) => println!("{text}"),
            Err(message) => {
                let mut response = next_step::invalid_input(message);
                response.data.as_mut().unwrap()["next_step"]["args"] = error_help;
                if args.iter().any(|a| a == "--json") {
                    println!("{}", serde_json::to_string(&response).unwrap());
                } else {
                    eprint!("{}", diagnostics::error_text(&response));
                }
                std::process::exit(response.exit_code());
            }
        }
        return;
    }
    if args == [OsString::from("--version")] {
        println!(
            "luciddesk-cli {} protocol {VERSION}",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    if let Some(output) = offline_output(&args) {
        println!("{output}");
        return;
    }
    let wants_json = args.iter().any(|a| a == "--json");
    let (mut response, json) = match parse(args) {
        Err(error) => (next_step::invalid_input(format!("{error}. Run 'luciddesk-cli help' or 'luciddesk-cli help RESOURCE COMMAND' for usage.")), wants_json),
        Ok(options) => {
            let mut client = None;
            let response = shortcuts::run(&options, |request, timeout| call(&mut client, request, timeout));
            (response, options.json)
        }
    };
    if let Some(data) = response.data.as_mut() {
        if data.pointer("/next_step/action").and_then(serde_json::Value::as_str) == Some("read_help") {
            data["next_step"]["args"] = error_help;
        }
    }
    let code = response.exit_code();
    if json {
        println!(
            "{}",
            serde_json::to_string(&response).expect("response serialization")
        );
    } else if response.error.is_some() {
        eprint!("{}", diagnostics::error_text(&response));
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&response.data).expect("response serialization")
        );
    }
    std::process::exit(code);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &str) -> Vec<OsString> {
        s.split_whitespace().map(Into::into).collect()
    }
    #[test]
    fn offline_commands_accept_global_json_without_ignoring_other_options() {
        let schema = offline_output(&args("schema --json")).unwrap();
        assert_eq!(offline_output(&args("--json schema")).unwrap(), schema);
        let skill = offline_output(&args("skill show --json")).unwrap();
        for command in ["--json skill show", "skill --json show"] {
            assert_eq!(offline_output(&args(command)).unwrap(), skill);
        }
        let response: Response = serde_json::from_str(&skill).unwrap();
        assert!(response.ok);
        assert_eq!(
            response.data.unwrap()["content"],
            offline_output(&args("skill show")).unwrap()
        );
        for command in [
            "schema --json --json",
            "skill --json show --json",
            "schema --id 1",
            "skill show --timeout-ms 5",
        ] {
            assert!(offline_output(&args(command)).is_none());
            assert!(parse(args(command)).is_err());
        }
    }
    #[test]
    fn missing_values_cannot_consume_safety_or_output_options() {
        for command in [
            "pane create --title --dry-run --json",
            "pane create --title test --request-id --dry-run",
            "pane update --id --dry-run --title test",
            "search query --id 1 --query --json",
            "plan apply --token --request-id fixed",
            "pane fit --id 1 --icon-columns --dry-run",
        ] {
            assert!(parse(args(command)).is_err(), "{command}");
        }
    }
    #[test]
    fn value_guard_preserves_negative_numbers_stdin_and_json_literals() {
        let options = parse(args("pane geometry --id 1 --monitor m --x -10 --y -20 --width 300 --height 200 --dry-run")).unwrap();
        assert!(options.dry_run);
        assert!(matches!(options.operation, Some(luciddesk_api::Operation::Geometry { x, y, .. }) if x == -10.0 && y == -20.0));
        assert_eq!(option_value(&mut args("-").into_iter(), "--input").unwrap(), "-");
        let operation = shortcuts::operation("pane.create", None, None, Default::default(), Some(br#"{"title":"--dry-run"}"#.to_vec())).unwrap();
        assert!(matches!(operation, luciddesk_api::Operation::Create {title,..} if title == "--dry-run"));
    }
    #[test]
    fn options_validate_commands_and_filters() {
        assert!(parse(args("--json pane get --id 9007199254740993")).is_ok());
        assert!(parse(args("plan apply --token token --request-id retry-1 --json")).is_ok());
        assert!(parse(args("request get --id retry-1 --json")).is_ok());
        assert!(parse(args("settings get --json")).is_ok());
        assert!(parse(args("font list --json")).is_ok());
        assert!(parse(args("folder get --id 1 --json")).is_ok());
        assert!(parse(args("search get --id 1 --json")).is_ok());
        for s in [
            "pane get",
            "status --id 1",
            "item list --pane 1 --unassigned",
            "pane remove",
            "plan preview",
            "plan apply",
            "status --token token",
            "request get",
            "status --timeout-ms 0",
            "status --json --json",
        ] {
            assert!(parse(args(s)).is_err(), "{s}");
        }
    }
}
