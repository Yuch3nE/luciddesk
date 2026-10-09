//! Versioned local control protocol. No storage dependency.
use serde::{Deserialize, Serialize};
use serde_json::Value;
mod plan;
mod strict_json;
pub use strict_json::validate_json;
pub mod transport;
pub use plan::{SnapSide, SnapAlign, Context, Operation, Plan, SettingValue, FolderColumn};
pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
pub const COMMANDS: &[&str] = &[
    "status",
    "capabilities",
    "workspace.get",
    "settings.get",
    "font.list",
    "startup.get",
    "monitor.list",
    "folder.get",
    "search.get",
    "pane.list",
    "pane.get",
    "item.list",
    "plan.preview",
    "plan.apply",
    "request.get",
];

/// Plan operations supported by this protocol implementation.
pub const OPERATIONS: &[&str] = &[
    "pane.sort",
    "pane.create",
    "pane.update",
    "pane.remove",
    "pane.geometry",
    "pane.fit",
    "pane.snap",
    "pane.arrange",
    "item.assign",
    "item.release",
    "item.reorder",
    "item.position",
    "settings.update",
    "tab.merge",
    "tab.select",
    "tab.reorder",
    "tab.detach",
    "folder.create",
    "folder.update",
    "folder.navigate",
    "folder.fit",
    "folder.refresh",
    "folder.back",
    "folder.home",
    "search.query",
    "search.refresh",
    "search.more",
    "startup.set",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    pub command: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub unassigned: bool,
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub plan: Option<Plan>,
    #[serde(default)]
    pub token: Option<String>,
}
impl Request {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_empty() || self.request_id.len() > 128 {
            return Err("invalid request_id");
        }
        if !COMMANDS.contains(&self.command.as_str()) {
            return Err("unsupported command");
        }
        if matches!(self.command.as_str(), "pane.get" | "folder.get" | "search.get" | "request.get") != self.id.is_some() {
            return Err("--id is required for pane get, folder get, search get or request get");
        }
        if self.command != "item.list" && (self.pane.is_some() || self.unassigned) {
            return Err("item filters require item list");
        }
        if self.pane.is_some() && self.unassigned {
            return Err("conflicting item filters");
        }
        if (self.command == "plan.preview") != self.plan.is_some() {
            return Err("plan.preview requires a plan, other commands reject it");
        }
        if (self.command == "plan.apply") != self.token.is_some() {
            return Err("plan.apply requires --token, other commands reject it");
        }
        if self
            .token
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 128)
        {
            return Err("invalid plan token");
        }
        if self
            .id
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 128)
        {
            return Err("invalid query ID");
        }
        let panel_id = self.id.as_ref().filter(|_| matches!(self.command.as_str(), "pane.get" | "folder.get" | "search.get"));
        for id in [panel_id, self.pane.as_ref()].into_iter().flatten() {
            if id
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                .is_none()
            {
                return Err("invalid panel ID");
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub protocol_version: u32,
    pub request_id: String,
    pub ok: bool,
    pub context: Option<Value>,
    pub data: Option<Value>,
    pub error: Option<ApiError>,
}
impl Response {
    pub fn success(id: &str, context: Value, data: Value) -> Self {
        Self {
            protocol_version: VERSION,
            request_id: id.into(),
            ok: true,
            context: Some(context),
            data: Some(data),
            error: None,
        }
    }
    pub fn failure(id: &str, code: &str, message: impl Into<String>) -> Self {
        Self {
            protocol_version: VERSION,
            request_id: id.into(),
            ok: false,
            context: None,
            data: None,
            error: Some(ApiError {
                code: code.into(),
                message: message.into(),
                retryable: matches!(code, "BUSY" | "TIMEOUT"),
            }),
        }
    }
    pub fn exit_code(&self) -> i32 {
        match self.error.as_ref().map(|e| e.code.as_str()) {
            None => 0,
            Some("INVALID_REQUEST") => 2,
            Some("APP_NOT_RUNNING") => 3,
            Some("NOT_FOUND") => 4,
            Some(
                "DATA_DIR_MISMATCH"
                | "CONFLICT"
                | "PLAN_EXPIRED"
                | "PLAN_ALREADY_APPLIED"
                | "REQUEST_ID_REUSED"
                | "PANE_LOCKED",
            ) => 5,
            Some("BUSY" | "RESULT_TOO_LARGE" | "CAPABILITY_UNAVAILABLE") => 6,
            Some("PERSISTENCE_ERROR") => 7,
            Some("TIMEOUT" | "RESULT_UNKNOWN") => 8,
            Some("ACCESS_DENIED") => 9,
            Some("PROTOCOL_MISMATCH") => 10,
            _ => 1,
        }
    }
}
pub fn request_id() -> String {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advertised_schema_and_operation_fixtures_match_protocol() {
        use std::collections::BTreeSet;
        let schema:Value=serde_json::from_str(include_str!("../protocol.schema.json")).unwrap();
        let commands:BTreeSet<_>=schema["$defs"]["request"]["properties"]["command"]["enum"].as_array().unwrap().iter().map(|v|v.as_str().unwrap()).collect();
        assert_eq!(commands,COMMANDS.iter().copied().collect());
        let variants:BTreeSet<_>=schema["$defs"]["plan"]["properties"]["operations"]["items"]["oneOf"].as_array().unwrap().iter().map(|v|v["properties"]["op"]["const"].as_str().unwrap()).collect();
        assert_eq!(variants,OPERATIONS.iter().copied().collect());
        let fixtures:Vec<Value>=serde_json::from_str(include_str!("../tests/fixtures/operations.json")).unwrap();
        let covered:BTreeSet<_>=fixtures.iter().map(|v|v["op"].as_str().unwrap()).collect();
        assert_eq!(covered,variants);
        for fixture in fixtures {
            let operation:Operation=serde_json::from_value(fixture.clone()).unwrap();
            let normalized=serde_json::to_value(operation).unwrap();
            fn equivalent(left:&Value,right:&Value)->bool {
                match (left,right) {
                    (Value::Number(a),Value::Number(b))=> (a.as_f64().unwrap()-b.as_f64().unwrap()).abs()<0.000001,
                    (Value::Array(a),Value::Array(b))=> a.len()==b.len() && a.iter().zip(b).all(|(a,b)|equivalent(a,b)),
                    (Value::Object(a),Value::Object(b))=> a.len()==b.len() && a.iter().all(|(k,v)|b.get(k).is_some_and(|b|equivalent(v,b))),
                    _=>left==right,
                }
            }
            for (key,value) in fixture.as_object().unwrap() {assert!(equivalent(&normalized[key],value),"field {key}: {normalized}");}
        }
    }
    #[test]
    fn strict_request_and_unicode() {
        let raw = r#"{"protocol_version":1,"request_id":"中文","command":"status"}"#;
        let request: Request = serde_json::from_str(raw).unwrap();
        assert!(request.validate().is_ok());
        assert!(serde_json::from_str::<Request>(&raw.replace("command", "unknown")).is_err());
        assert!(
            serde_json::from_str::<Request>(
                &raw.replace(r#""command":"#, r#""command":"status","command":"#)
            )
            .is_err()
        );
    }
}
