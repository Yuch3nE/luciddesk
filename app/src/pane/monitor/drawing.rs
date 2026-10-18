//! Host paint adapter. Fonts, layouts, charts and presentation belong to the plugin.
use super::{PLUGIN_ID, Preferences};
use crate::pane::canvas::DrawPass;
use serde_json::json;
use windows_canvas::Rect;
pub(in crate::pane) struct Drawing(u64);
fn error(message: String) -> windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32), message)
}
impl Drawing {
    pub(in crate::pane) fn new() -> windows::core::Result<Self> {
        let value = crate::plugins::request(
            PLUGIN_ID,
            json!({"op":"drawing_new","font":crate::pane::fonts::family()}),
        )
        .map_err(error)?;
        value
            .as_u64()
            .map(Self)
            .ok_or_else(|| error("Invalid plugin drawing ID".into()))
    }
    pub(in crate::pane) fn paint(
        &mut self,
        target: &DrawPass<'_>,
        options: &Preferences,
        bounds: Rect,
        dark: bool,
        ink: f32,
    ) -> windows::core::Result<()> {
        let request = json!({
            "op": "paint",
            "id": self.0,
            "options": options,
            "dark": dark,
            "ink": ink,
            "font": crate::pane::fonts::family(),
            "bounds": [bounds.left, bounds.top, bounds.right - bounds.left, bounds.bottom - bounds.top],
        });
        crate::plugins::call(PLUGIN_ID, request, target.plugin_context()).map_err(error)?;
        Ok(())
    }
}
impl Drop for Drawing {
    fn drop(&mut self) {
        let _ = crate::plugins::request(PLUGIN_ID, json!({"op":"drawing_drop","id":self.0}));
    }
}
