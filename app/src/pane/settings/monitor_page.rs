//! Render a plugin-owned settings description using the host's standard controls.
use super::*;
use luciddesk_plugin_api::{Item, Page};
use monitor::{PLUGIN_ID, Preferences};
use serde_json::{Value, json};

fn action(value: &Value) -> Result<Action, String> {
    match value["op"].as_str() {
        Some("save") => {
            let prefs = <Preferences as serde::Deserialize>::deserialize(&value["options"])
                .map_err(|e| format!("Invalid plugin preferences: {e}"))?;
            Ok(Action::MonitorOptions(prefs))
        }
        Some("task_manager") => Ok(Action::ProjectLink("taskmgr.exe")),
        _ => Err("Unsupported plugin action".into()),
    }
}
pub(super) fn draw(scene: &mut Scene, width: f32) {
    let result = crate::plugins::request(PLUGIN_ID, json!({"op":"settings"}))
        .and_then(|v| serde_json::from_value::<Page>(v).map_err(|e| e.to_string()));
    let page = match result {
        Ok(page) => page,
        Err(error) => {
            SettingsForm::new(scene, width, "").section(&error);
            return;
        }
    };
    let mut form = SettingsForm::new(scene, width, &page.description);
    let result = (|| -> Result<(), String> {
        for item in page.items {
            match item {
                Item::Section(title) => form.section(&title),
                Item::Toggle {
                    title,
                    description,
                    selected,
                    action: a,
                } => form.toggle(&title, &description, selected, action(&a)?),
                Item::Toggles { title, values } => form.toggle_group(
                    &title,
                    values
                        .iter()
                        .map(|(s, a, b)| Ok((s.as_str(), action(a)?, *b)))
                        .collect::<Result<_, String>>()?,
                ),
                Item::Choices { title, values } => form.choices(
                    &title,
                    "",
                    values
                        .iter()
                        .map(|(s, a, b)| Ok((s.as_str(), action(a)?, *b)))
                        .collect::<Result<_, String>>()?,
                ),
                Item::Button {
                    title,
                    label,
                    action: a,
                } => form.button(&title, "", &label, action(&a)?),
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        form.section(&error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_switch_uses_plugin_configuration_without_panel_ids() {
        if !monitor::available() {
            return;
        }
        let mut options = Preferences::load().unwrap();
        options.enabled = false;
        options.save().unwrap();
        let mut scene = layout::scene(
            900.0,
            600.0,
            14,
            false,
            (PanelTheme::Light, Backdrop::Acrylic),
            Default::default(),
        );
        draw(&mut scene, 900.0);
        let next = scene
            .controls
            .iter()
            .find_map(|c| match &c.action {
                Action::MonitorOptions(next) if next.enabled => Some(next.clone()),
                _ => None,
            })
            .unwrap();
        next.save().unwrap();
        assert!(Preferences::load().unwrap().enabled);
    }
}
