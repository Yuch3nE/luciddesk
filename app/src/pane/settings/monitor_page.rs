//! Render a plugin-owned settings description using the host's standard controls.
use super::*;
use luciddesk_plugin_api::{Item, Page};
use monitor::{PLUGIN_ID, Preferences};
use serde_json::{Value, json};

fn action(value: &Value) -> Result<Action, String> {
    match value["op"].as_str() {
        Some("new") => Ok(Action::Change(Event::NewMonitor)),
        Some("save") => {
            let id = value["id"].as_u64().ok_or("Invalid plugin panel ID")?;
            let prefs = <Preferences as serde::Deserialize>::deserialize(&value["options"])
                .map_err(|e| format!("Invalid plugin preferences: {e}"))?;
            Ok(Action::MonitorOptions(PanelId::new(id), prefs))
        }
        Some("task_manager") => Ok(Action::ProjectLink("taskmgr.exe")),
        _ => Err("Unsupported plugin action".into()),
    }
}
pub(super) fn draw(
    scene: &mut Scene,
    width: f32,
    panels: &Result<Vec<(PanelId, String, Preferences)>, String>,
) {
    let result = panels.as_ref().map_err(Clone::clone).and_then(|panels| {
        let panels: Vec<_> = panels
            .iter()
            .map(|(id, title, p)| (id.get(), title, p))
            .collect();
        crate::plugins::request(PLUGIN_ID, json!({"op":"settings","panels":panels}))
            .and_then(|v| serde_json::from_value::<Page>(v).map_err(|e| e.to_string()))
    });
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
    use monitor::Metric;

    #[test]
    fn monitor_settings_controls_persist_metrics_and_disable_without_deleting_panel() {
        if !monitor::available() {
            assert!(!layout::pages().iter().any(|(id, _, _)| *id == 14));
            return;
        }
        let _sta = crate::pane::test_support::apartment();
        let state = Rc::new(RefCell::new(crate::pane::tests::test_state()));
        let page = |panels| {
            let mut scene = layout::scene(
                900.0,
                600.0,
                14,
                false,
                (PanelTheme::Light, Backdrop::Acrylic),
                Default::default(),
            );
            draw(&mut scene, 900.0, &Ok(panels));
            scene
        };
        let empty = page(Vec::new());
        let create = empty
            .controls
            .iter()
            .find_map(|c| match &c.action {
                Action::Change(Event::NewMonitor) => Some(Event::NewMonitor),
                _ => None,
            })
            .unwrap();
        handle(&state, PanelId::new(0), create).unwrap();
        let id = state.borrow().workspace.panels().last().unwrap().id();
        let before = state.borrow().workspace.panel(id).unwrap().clone();
        for metric in [
            Metric::Cpu,
            Metric::Memory,
            Metric::Gpu,
            Metric::Network,
            Metric::Disk,
        ] {
            let options = Preferences::load(&state.borrow().store, id).unwrap();
            let scene = page(vec![(id, before.title().into(), options)]);
            assert!(scene.text.iter().any(|(_, text, _)| text == metric.label()));
            let control = scene
                .controls
                .iter()
                .find(|c| {
                    matches!(&c.action,
                Action::MonitorOptions(_, next) if !next.metrics.contains(&metric))
                        && matches!(c.kind, ControlKind::Toggle)
                })
                .unwrap();
            assert!(control.enabled);
            let Action::MonitorOptions(target, next) = &control.action else {
                panic!("metric action");
            };
            assert_eq!(*target, id);
            handle(&state, *target, Event::MonitorOptions(next.clone())).unwrap();
            assert!(
                !Preferences::load(&state.borrow().store, id)
                    .unwrap()
                    .metrics
                    .contains(&metric)
            );
        }
        let options = Preferences::load(&state.borrow().store, id).unwrap();
        assert!(options.metrics.is_empty());
        let scene = page(vec![(id, before.title().into(), options.clone())]);
        let off = scene
            .controls
            .iter()
            .find_map(|c| match &c.action {
                Action::MonitorOptions(_, next) if !next.enabled => Some(next.clone()),
                _ => None,
            })
            .unwrap();
        handle(&state, id, Event::MonitorOptions(off)).unwrap();
        assert!(!state.borrow().views.iter().any(|v| v.id == id));
        assert_eq!(state.borrow().workspace.panel(id), Some(&before));
        crate::pane::create_view(&state, id).unwrap(); // Reload/reconnect must respect the saved switch.
        assert!(!state.borrow().views.iter().any(|v| v.id == id));
        handle(&state, id, Event::MonitorOptions(options)).unwrap();
        assert!(state.borrow().views.iter().any(|v| v.id == id));
        assert!(
            Preferences::load(&state.borrow().store, id)
                .unwrap()
                .metrics
                .is_empty()
        );
    }
}
