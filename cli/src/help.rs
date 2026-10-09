//! Offline command discovery backed by the same protocol as execution.
use serde_json::{Value, json};
use std::ffi::OsString;
mod topics;

fn commands() -> Vec<&'static str> {
    luciddesk_api::COMMANDS
        .iter()
        .chain(luciddesk_api::OPERATIONS)
        .copied()
        .chain(["schema", "skill.show"])
        .collect()
}

fn constraint(schema: &Value) -> String {
    if let Some(values) = schema["enum"].as_array() {
        return values.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect::<Vec<_>>().join("|");
    }
    if let Some(types) = schema["type"].as_array() {
        return types.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("|");
    }
    let kind = schema["type"].as_str().unwrap_or(if schema["$ref"].is_string() { "JSON (see --json schema)" } else { "JSON" });
    let mut result = if kind == "boolean" { "true|false".into() } else { kind.to_owned() };
    for (key, label) in [("minimum", "min"), ("maximum", "max"), ("minLength", "min length"), ("maxLength", "max length"), ("minItems", "min items"), ("maxItems", "max items"), ("default", "default")] {
        if let Some(value) = schema.get(key) { result.push_str(&format!("; {label}={value}")); }
    }
    if let Some(pattern) = schema["pattern"].as_str() { result.push_str(&format!("; pattern={pattern}")); }
    result
}

fn document(topic: &str) -> Result<Value, String> {
    let commands = commands();
    let children: Vec<_> = commands
        .iter()
        .filter(|name| topic.is_empty() || name.starts_with(&format!("{topic}.")))
        .map(|name| name.replace('.', " "))
        .collect();
    if !topic.is_empty() && !commands.contains(&topic) && children.is_empty() {
        let parent = topic.split('.').next().unwrap_or("");
        let hint = if commands.iter().any(|name| name.starts_with(&format!("{parent}."))) {
            format!("help {parent}")
        } else { "help".into() };
        return Err(format!("Unknown help topic '{topic}'. Run 'luciddesk-cli {hint}' to list valid commands."));
    }
    let mutation = luciddesk_api::OPERATIONS.contains(&topic);
    let offline = matches!(topic, "schema" | "skill" | "skill.show");
    let group = !children.is_empty();
    let command = topic.replace('.', " ");
    let arguments = match topic {
        "plan.preview" => " --input FILE|-",
        "plan.apply" => " --token TOKEN [--request-id ID]",
        "pane.get" | "folder.get" | "search.get" | "request.get" => " --id ID",
        "item.list" => " [--pane ID | --unassigned]",
        _ if mutation => " [--input FILE|-] [FIELD_FLAGS] [--dry-run]",
        _ if !children.is_empty() => " <command>",
        _ => "",
    };
    let mut doc = json!({
        "topic": if topic.is_empty() {"overview"} else {topic},
        "usage": if topic.is_empty() { "luciddesk-cli <command> [options]".into() } else { format!("luciddesk-cli {command}{arguments} [--json]") },
        "cli_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": luciddesk_api::VERSION,
        "help_version": 1,
        "output_contract": {
            "recommended_args": ["--json"],
            "format": "single_json_value",
            "stdout": "response", "stderr": "not_for_parsing",
            "schema_command_returns": "raw_json_schema",
            "ok_means": "request_succeeded_not_necessarily_effect_applied",
            "receipt_status_fields": ["commit_status", "presentation_status", "operation_status"],
            "receipt_lookup_result_path": "data.result",
            "next_step_path": "data.next_step",
            "next_step_args": "argument_array_without_executable; absent_when_manual_action_is_required",
            "retry_rule": "Never repeat an uncertain mutation with a new request ID; inspect the receipt and current state first.",
            "unknown_fields": "ignore_for_forward_compatibility"
        },
        "schema_args": ["schema", "--json"],
        "command_args": if group { Vec::new() } else { topic.split('.').collect::<Vec<_>>() },
        "requires_app": if group { Value::Null } else { json!(!offline) },
        "mutates_workspace": if group { Value::Null } else { json!(mutation || topic == "plan.apply") },
        "supports_dry_run": mutation,
        "default_effect": if group { "group" } else if mutation { "preview_then_apply" } else if topic == "plan.apply" { "apply" } else if topic == "plan.preview" { "preview" } else { "read" },
        "summary": topics::summary(topic),
        "availability": if topic.is_empty() { "mixed; see command help" } else if offline { "offline" } else { "online" },
        "behavior": if mutation { "Preview then apply; add --dry-run to preview only." } else if topic == "plan.apply" { "Applies changes using a previously previewed token." } else if topic == "plan.preview" { "Preview only; does not save changes." } else if group { "Use help RESOURCE COMMAND for parameters and behavior." } else { "Read-only; does not change the workspace." },
        "commands": children,
        "options": ["--json: machine-readable output", "--data-dir PATH: assert the running app's directory", "--timeout-ms 1..60000: default 10000", "--protocol-version N: protocol compatibility", "--request-id ID: retain for recovery", "help [RESOURCE [COMMAND]], -h, --help: offline help", "--version: CLI and protocol version"],
        "notes": []
    });
    doc["command_details"] = json!(children.iter().map(|name| json!({
        "command": name, "summary": topics::summary(&name.replace(' ', ".")),
    })).collect::<Vec<_>>());
    doc["examples"] = json!(if group && !topic.is_empty() {
        vec![format!("luciddesk-cli help {}", children[0])]
    } else { topics::examples(topic, mutation) });
    // These are controlled examples without quoted/space-containing values,
    // not a shell parser. Agents can pass the arrays as arguments directly.
    doc["example_args"] = json!(doc["examples"].as_array().unwrap().iter().map(|example|
        example.as_str().unwrap().split_whitespace().skip(1).collect::<Vec<_>>()
    ).collect::<Vec<_>>());
    if topic.is_empty() {
        let groups: std::collections::BTreeSet<_> = commands.iter().map(|name| name.split('.').next().unwrap()).collect();
        doc["groups"] = json!(groups.into_iter().map(|name| json!({"command": name, "summary": topics::summary(name)})).collect::<Vec<_>>());
    }
    doc["notes"] = json!(if offline {
        vec!["This command does not connect to the app and works when CLI control is disabled."]
    } else {
        vec!["Online commands require the same-version GUI with CLI control enabled in Settings > General > Agent & CLI.",
            "Get IDs from live queries; example IDs and paths are placeholders. The CLI never opens the database.",
            "After a timeout, query request get with the original request ID before retrying; do not repeat an uncertain change with a new ID."]
    });
    if offline {
        doc["options"] = json!(["--json: machine-readable output", "help, -h, --help: offline help"]);
    }
    let detail = match topic {
        "pane.sort" => Some("One-time sort; defaults to name ascending. Specify --descending true for newest-first modified dates. Keeps arrangement mode; repacks using visible columns. Metadata reads run off the UI thread; previews wait up to --timeout-ms and apply the measured order without rereading files."),
        "pane.update" => Some("Desktop panes: auto_compact=true enables aligned compact packing; align_icons_to_grid=false disables compact packing and preserves free positions. Conflicting true/false is rejected. These two fields affect only the named pane, including in a tab group."),
        "item.position" => Some("Requires an existing member of an unlocked desktop pane with auto_compact=false. Aligned mode: column/row (0..10000), occupied cells rejected. Free mode: x/y (0..1000000), relative to pane content in DIP, overlap allowed. Do not mix coordinate types. Use item.assign first for another pane; other icons stay in place."),
        _ => None,
    };
    if let Some(detail) = detail { doc["notes"].as_array_mut().unwrap().push(json!(detail)); }
    if topic == "plan.preview" {
        doc["input"] = json!("--input FILE|- contains a complete plan with protocol_version, base and operations; UTF-8 without BOM. FILE=- reads stdin.");
    }
    if !mutation {
        let required = match topic {
            "pane.get" | "folder.get" | "search.get" | "request.get" => vec!["id"],
            "plan.preview" => vec!["input"],
            "plan.apply" => vec!["token"],
            _ => Vec::new(),
        };
        let names = match topic {
            "item.list" => vec!["pane", "unassigned"],
            _ => required.clone(),
        };
        doc["fields"] = json!(names.iter().map(|name| json!({
            "name": name, "flags": [format!("--{name}")],
            "schema": {"type": if *name == "unassigned" { "boolean" } else { "string" }},
            "required": required.contains(name), "input_only": false,
            "flag_takes_value": *name != "unassigned",
        })).collect::<Vec<_>>());
        doc["required_fields"] = json!(required);
        if topic == "item.list" { doc["exclusive_fields"] = json!([["pane", "unassigned"]]); }
    }
    if mutation {
        let schema: Value = serde_json::from_str(include_str!(
            "../../crates/luciddesk-api/protocol.schema.json"
        ))
        .unwrap();
        fn find<'a>(v: &'a Value, topic: &str) -> Option<&'a Value> {
            if v.pointer("/properties/op/const").and_then(Value::as_str) == Some(topic) {
                return Some(v);
            }
            match v {
                Value::Object(map) => map.values().find_map(|v| find(v, topic)),
                Value::Array(values) => values.iter().find_map(|v| find(v, topic)),
                _ => None,
            }
        }
        doc["operation_schema"] = find(&schema, topic)
            .ok_or_else(|| format!("Missing schema for {topic}"))?
            .clone();
        doc["input"] = json!(match topic {
            "settings.update" => "--input contains the plain settings map (not a values wrapper).",
            "item.reorder" =>
                "--input accepts a complete item-ID array or an operation-fields object without op.",
            _ =>
                "--input contains an operation-fields object without op; use UTF-8 without BOM. FILE=- reads stdin. Do not duplicate fields in input and flags. Creation ref defaults to created.",
        });
        let flags = [
            "--target",
            "--descending",
            "--sort-column",
            "--auto-compact",
            "--align-icons-to-grid",
            "--item-id",
            "--column",
            "--row",
            "--side",
            "--align",
            "--max-rows",
            "--icon-columns",
            "--title",
            "--path",
            "--query",
            "--into",
            "--monitor",
            "--x",
            "--y",
            "--width",
            "--height",
            "--locked",
            "--auto-hide",
            "--collapsed",
            "--always-on-top",
            "--list-view",
            "--enabled",
            "--expected-status",
            "--ids",
        ];
        let properties = doc["operation_schema"]["properties"].as_object().unwrap();
        let mut supported: Vec<_> = flags
            .iter()
            .filter_map(|flag| {
                let field = crate::shortcuts::flag(flag)?;
                properties
                    .contains_key(field)
                    .then(|| format!("{flag} {} -> {field}{}", constraint(&properties[field]),
                        if doc["operation_schema"]["required"].as_array().is_some_and(|v| v.contains(&json!(field))) { " (required via flag or input)" } else { "" }))
            })
            .collect();
        if properties.contains_key("pane_id") {
            let required = doc["operation_schema"]["required"].as_array().unwrap().contains(&json!("pane_id"));
            supported.push(format!("--id ID (or --pane ID) -> pane_id{}",
                if required { " (required via flag or input)" } else { "" }));
        }
        if properties.contains_key("release_items") {
            supported.push("--release-items -> release_items:true".into());
        }
        let fields: Vec<_> = properties.iter().filter(|(name, _)| name.as_str() != "op").map(|(name, schema)| {
            let mut aliases: Vec<_> = flags.iter().filter(|flag| crate::shortcuts::flag(flag) == Some(name.as_str())).copied().collect();
            if name == "pane_id" { aliases.extend(["--id", "--pane"]); }
            if name == "release_items" { aliases.push("--release-items"); }
            json!({"name": name, "flags": aliases, "schema": schema,
                "required": name != "ref" && doc["operation_schema"]["required"].as_array().unwrap().contains(&json!(name)),
                "default": if name == "ref" { json!("created") } else { schema.get("default").cloned().unwrap_or(Value::Null) },
                "input_only": aliases.is_empty(), "flag_takes_value": name != "release_items"})
        }).collect();
        doc["fields"] = json!(fields);
        doc["field_flags"] = json!(supported);
        doc["required_fields"] = json!(doc["operation_schema"]["required"].as_array().unwrap().iter()
            .filter(|field| field.as_str() != Some("op") && field.as_str() != Some("ref")).cloned().collect::<Vec<_>>());
        doc["options"].as_array_mut().unwrap().push(json!("--dry-run: preview only; without it the shortcut applies immediately after preview"));
        doc["notes"].as_array_mut().unwrap().push(json!("Fields can be provided using flags, --input, or both without duplicates. Schema required fields apply to the combined payload. Settings, startup and transient actions require separate plans."));
    }
    Ok(doc)
}

pub(super) fn output(args: &[OsString]) -> Option<Result<String, String>> {
    let requested = args.is_empty()
        || args.first().is_some_and(|a| a == "help")
        || args.iter().any(|a| a == "--help" || a == "-h");
    if !requested {
        return None;
    }
    Some((|| {
        let mut words = Vec::new();
        let mut json = false;
        let mut help = false;
        for (index, arg) in args.iter().enumerate() {
            let arg = arg.to_str().ok_or("Help arguments must be valid Unicode")?;
            match arg {
                "--json" if !json => json = true,
                "help" if index == 0 && !help => help = true,
                "--help" | "-h" if !help => help = true,
                option if option.starts_with('-') => {
                    return Err(format!(
                        "Unexpected help option '{option}'. Use 'luciddesk-cli help RESOURCE COMMAND [--json]'."
                    ));
                }
                word => words.push(word),
            }
        }
        let doc = document(&words.join("."))?;
        if json {
            return Ok(serde_json::to_string(&luciddesk_api::Response::success(
                "offline",
                Value::Null,
                doc,
            ))
            .unwrap());
        }
        let mut text = format!(
            "LucidDesk CLI {} (protocol {})\n{}\n\nUsage: {}\nMode: {}\n{}\n",
            env!("CARGO_PKG_VERSION"), luciddesk_api::VERSION,
            doc["summary"].as_str().unwrap(), doc["usage"].as_str().unwrap(),
            doc["availability"].as_str().unwrap(), doc["behavior"].as_str().unwrap()
        );
        let entries = doc[if doc["groups"].is_array() { "groups" } else { "command_details" }].as_array().unwrap();
        if !entries.is_empty() {
            text.push_str("\nCommands (use help RESOURCE COMMAND for details):\n");
            for entry in entries {
                text.push_str(&format!("  {:<22} {}\n", entry["command"].as_str().unwrap(), entry["summary"].as_str().unwrap()));
            }
        }
        for (key, label) in [
            ("field_flags", "Field flags"),
            ("options", "Options"),
            ("notes", "Notes"),
            ("examples", "Examples"),
        ] {
            if let Some(values) = doc[key].as_array().filter(|v| !v.is_empty()) {
                text.push_str(&format!("\n{label}:\n"));
                for value in values {
                    text.push_str(&format!("  {}\n", value.as_str().unwrap()));
                }
            }
        }
        if let Some(input) = doc["input"].as_str() {
            text.push_str(&format!("\nInput: {input}\n"));
        }
        if let Some(fields) = doc["fields"].as_array() {
            let input_only: Vec<_> = fields.iter().filter(|field| field["input_only"] == true).collect();
            if !input_only.is_empty() {
                text.push_str("\nJSON input fields (no direct flag):\n");
                for field in input_only {
                    let name = field["name"].as_str().unwrap();
                    text.push_str(&format!("  {name}{}: {}\n",
                        if name == "ref" { " (default: created)" } else if field["required"] == true { " (required)" } else { " (optional)" },
                        constraint(&field["schema"])));
                }
            }
        }
        if doc["operation_schema"].is_object() {
            text.push_str("\nFull JSON constraints: add --json to this help command.\n");
        }
        Ok(text)
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &str) -> Vec<OsString> {
        s.split_whitespace().map(Into::into).collect()
    }
    #[test]
    fn every_protocol_command_has_help_and_every_mutation_has_schema() {
        for name in commands() {
            let doc = document(name).unwrap();
            assert!(!doc["summary"].as_str().unwrap().is_empty(), "{name}");
            assert_eq!(doc["cli_version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(doc["protocol_version"], luciddesk_api::VERSION);
            if luciddesk_api::OPERATIONS.contains(&name) {
                assert_eq!(doc["operation_schema"]["properties"]["op"]["const"], name);
            }
        }
    }
    #[test]
    fn examples_use_their_own_command_and_mutations_preview_by_default() {
        for name in commands() {
            let doc = document(name).unwrap();
            for example in doc["examples"].as_array().unwrap() {
                let example = example.as_str().unwrap().strip_prefix("luciddesk-cli ").unwrap();
                assert!(example.starts_with(&name.replace('.', " ")), "{name}: {example}");
                if luciddesk_api::OPERATIONS.contains(&name) {
                    assert!(example.contains("--dry-run"), "{example}");
                }
                // File examples require user-provided JSON; parse all flag-only
                // online examples without connecting or executing anything.
                if !example.contains("--input") && doc["availability"] == "online" {
                    assert!(crate::parse(args(example)).is_ok(), "{example}");
                }
            }
        }
    }
    #[test]
    fn help_scopes_options_examples_and_reports_required_fields() {
        let overview = document("").unwrap();
        assert_eq!(overview["usage"], "luciddesk-cli <command> [options]");
        for entry in overview["groups"].as_array().unwrap() {
            assert!(!entry["summary"].as_str().unwrap().is_empty());
        }
        for topic in ["schema", "skill", "skill.show"] {
            let doc = document(topic).unwrap();
            assert_eq!(doc["availability"], "offline");
            assert!(!doc["options"].to_string().contains("--data-dir"));
        }
        let snap = document("pane.snap").unwrap();
        assert!(snap["required_fields"].as_array().unwrap().contains(&json!("side")));
        assert!(snap["field_flags"].to_string().contains("left|right|top|bottom"));
        assert!(document("pane.snpa").unwrap_err().contains("help pane"));
        let group = document("folder").unwrap();
        let example = group["examples"][0].as_str().unwrap().strip_prefix("luciddesk-cli ").unwrap();
        assert!(output(&args(example)).unwrap().is_ok());
    }
    #[test]
    fn agent_metadata_distinguishes_discovery_preview_and_apply() {
        for name in commands() {
            let doc = document(name).unwrap();
            assert_eq!(doc["help_version"], 1);
            assert_eq!(doc["command_args"], json!(name.split('.').collect::<Vec<_>>()));
            assert_eq!(doc["requires_app"], !matches!(name, "schema" | "skill.show"));
            let shortcut = luciddesk_api::OPERATIONS.contains(&name);
            assert_eq!(doc["supports_dry_run"], shortcut);
            assert_eq!(doc["mutates_workspace"], shortcut || name == "plan.apply");
            for (example, args) in doc["examples"].as_array().unwrap().iter().zip(doc["example_args"].as_array().unwrap()) {
                let joined = args.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>().join(" ");
                assert_eq!(example, &json!(format!("luciddesk-cli {joined}")));
            }
        }
        let snap = document("pane.snap").unwrap();
        let id = snap["fields"].as_array().unwrap().iter().find(|field| field["name"] == "pane_id").unwrap();
        assert_eq!(id["flags"], json!(["--id", "--pane"]));
        assert_eq!(id["required"], true);
        assert_eq!(document("pane.create").unwrap()["required_fields"], json!(["title"]));
        assert_eq!(document("request.get").unwrap()["required_fields"], json!(["id"]));
        assert_eq!(document("plan.preview").unwrap()["default_effect"], "preview");
    }
    #[test]
    fn help_aliases_are_offline_and_json_is_an_envelope() {
        let expected = output(&args("help pane snap")).unwrap().unwrap();
        for command in ["pane snap --help", "pane snap -h", "--help pane snap"] {
            assert_eq!(output(&args(command)).unwrap().unwrap(), expected);
        }
        let json: Value =
            serde_json::from_str(&output(&args("help pane snap --json")).unwrap().unwrap())
                .unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["data"]["topic"], "pane.snap");
        assert!(output(&args("pane snap --id 1")).is_none());
    }
    #[test]
    fn invalid_help_never_falls_through_to_execution() {
        for command in [
            "help nonsense",
            "help pane nonsense",
            "help --json --json",
            "pane snap --help --input -",
            "help --help",
        ] {
            assert!(output(&args(command)).unwrap().is_err(), "{command}");
        }
        assert!(output(&[]).unwrap().is_ok());
        assert!(
            !document("pane").unwrap()["commands"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
