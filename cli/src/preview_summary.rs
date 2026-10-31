//! Additive CLI review aid. The original diff and apply token stay authoritative.
use luciddesk_api::Response;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const LIMIT: usize = 50;

fn compare(before: Option<&Value>, after: Option<&Value>, path: &str, count: &mut usize, changes: &mut Vec<Value>) {
    if before == after { return; }
    match (before, after) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                compare(a.get(key), b.get(key), &format!("{path}/{escaped}"), count, changes);
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            for i in 0..a.len().max(b.len()) {
                compare(a.get(i), b.get(i), &format!("{path}/{i}"), count, changes);
            }
        }
        _ => {
            *count += 1;
            if changes.len() < LIMIT {
                let mut change = json!({"path":path,"kind":match (before, after) {
                    (None, _) => "added", (_, None) => "removed", _ => "modified"
                }});
                if let Some(v) = before { change["before"] = v.clone(); }
                if let Some(v) = after { change["after"] = v.clone(); }
                changes.push(change);
            }
        }
    }
}

pub(super) fn attach(response: &mut Response) {
    if !response.ok { return; }
    let Some(data) = response.data.as_mut() else { return; };
    if !data["plan_token"].is_string() { return; }
    let Some(diff) = data["diff"].as_object() else { return; };
    let mut count = 0;
    let mut changes = Vec::new();
    compare(diff.get("before"), diff.get("after"), "", &mut count, &mut changes);
    // Geometry, runtime actions and future sections may not have before/after
    // snapshots. Point to them explicitly rather than claiming no changes.
    let additional: Vec<_> = diff.keys().filter(|k| !matches!(k.as_str(), "before" | "after")).cloned().collect();
    data["diff_summary"] = json!({"changes":changes,"change_count":count,
        "truncated":count > LIMIT,"additional_sections":additional,
        "full_diff_path":"data.diff"});
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preview(diff: Value) -> Response {
        Response::success("preview", Value::Null, json!({"plan_token":"unchanged-token","diff":diff}))
    }
    #[test]
    fn omits_unchanged_inventory_without_altering_reviewed_diff() {
        let placements = json!({"a":null,"b":{"pane_id":"1"}});
        let diff = json!({"before":{"panes":[{"id":"1"}],"placements":placements},
            "after":{"panes":[{"id":"1"},{"id":"2","title":"New"}],"placements":placements},"geometry":{"2":{}}});
        let mut response = preview(diff.clone());
        attach(&mut response);
        let data = response.data.unwrap();
        assert_eq!(data["diff"], diff);
        assert_eq!(data["plan_token"], "unchanged-token");
        assert_eq!(data["diff_summary"]["change_count"], 1);
        assert_eq!(data["diff_summary"]["changes"][0]["path"], "/panes/1");
        assert_eq!(data["diff_summary"]["additional_sections"], json!(["geometry"]));
    }
    #[test]
    fn distinguishes_missing_and_null_and_escapes_pointer_keys() {
        let mut response = preview(json!({"before":{"removed":null,"a/b~c":1},"after":{"added":null,"a/b~c":2}}));
        attach(&mut response);
        let summary = &response.data.as_ref().unwrap()["diff_summary"];
        assert_eq!(summary["change_count"], 3);
        assert_eq!(summary["changes"][0]["path"], "/a~1b~0c");
        assert_eq!(summary["changes"][1]["kind"], "added");
        assert_eq!(summary["changes"][2]["kind"], "removed");
    }
    #[test]
    fn bounds_summary_and_keeps_runtime_only_sections_visible() {
        let after: Vec<_> = (0..100).collect();
        let mut response = preview(json!({"before":[],"after":after}));
        attach(&mut response);
        let summary = &response.data.as_ref().unwrap()["diff_summary"];
        assert_eq!(summary["change_count"], 100);
        assert_eq!(summary["changes"].as_array().unwrap().len(), LIMIT);
        assert_eq!(summary["truncated"], true);
        let mut runtime = preview(json!({"runtime_action":{"op":"search.refresh"},"persistence":"none"}));
        attach(&mut runtime);
        assert_eq!(runtime.data.unwrap()["diff_summary"]["additional_sections"], json!(["persistence","runtime_action"]));
    }
}
