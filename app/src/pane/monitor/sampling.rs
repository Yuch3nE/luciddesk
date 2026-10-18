//! Host lifetime adapter; worker and sampling state live entirely inside the DLL.
use super::{PLUGIN_ID, Preferences};
use serde_json::json;
pub(in crate::pane) struct Client(u64);
impl Client {
    pub(in crate::pane) fn new() -> Result<Self, String> {
        crate::plugins::request(PLUGIN_ID, json!({"op":"client_new"}))?
            .as_u64()
            .map(Self)
            .ok_or("Invalid plugin client ID".into())
    }
    pub(in crate::pane) fn tick(&mut self, visible: bool, options: &Preferences) -> bool {
        let changed = crate::plugins::request(
            PLUGIN_ID,
            json!({"op":"tick","id":self.0,"visible":visible,"options":options}),
        )
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
        super::process::publish_devices();
        changed
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = crate::plugins::request(PLUGIN_ID, json!({"op":"client_drop","id":self.0}));
    }
}
