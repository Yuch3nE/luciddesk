use crate::{Metric, Preferences};
use luciddesk_plugin_api::{Item, Page};
use serde_json::{Value, json};

pub(crate) fn draw(options: &Preferences) -> Page {
    let mut current = options.clone();
    current.gpu = current.gpu.as_deref().map(crate::gpu::resolve);
    let options = &current;
    let mut form = Form(Page {
        description: crate::i18n::text("monitor-settings-description").into(),
        items: Vec::new(),
    });
    {
        let action = |options| json!({"op":"save", "options":options});
        form.toggle(
            crate::i18n::text("monitor-enable"),
            crate::i18n::text("monitor-enable-description"),
            options.enabled,
            action(Preferences {
                enabled: !options.enabled,
                ..options.clone()
            }),
        );
        form.toggle_group(
            crate::i18n::text("monitor-metrics"),
            [
                Metric::Cpu,
                Metric::Memory,
                Metric::Gpu,
                Metric::Network,
                Metric::Disk,
            ]
            .into_iter()
            .map(|metric| {
                let selected = options.metrics.contains(&metric);
                let mut next = options.clone();
                if selected {
                    next.metrics.retain(|m| *m != metric);
                } else {
                    next.metrics.push(metric);
                }
                (metric.label(), action(next), selected)
            })
            .collect(),
        );
        form.section(crate::i18n::text("monitor-display-settings"));
        form.dropdown(
            crate::i18n::text("monitor-layout"),
            [
                (false, "monitor-layout-standard"),
                (true, "monitor-compact"),
            ]
            .into_iter()
            .map(|(compact, key)| {
                (
                    crate::i18n::text(key),
                    action(Preferences {
                        compact,
                        ..options.clone()
                    }),
                    options.compact == compact,
                )
            })
            .collect(),
        );
        form.toggle(
            crate::i18n::text("monitor-pause"),
            "",
            options.paused,
            action(Preferences {
                paused: !options.paused,
                ..options.clone()
            }),
        );
        form.dropdown(
            crate::i18n::text("monitor-interval"),
            [
                (1, "monitor-one-second"),
                (2, "monitor-two-seconds"),
                (5, "monitor-five-seconds"),
            ]
            .into_iter()
            .map(|(interval, key)| {
                (
                    crate::i18n::text(key),
                    action(Preferences {
                        interval,
                        ..options.clone()
                    }),
                    options.interval == interval,
                )
            })
            .collect(),
        );
        if options.metrics.len() > 1 {
            form.0.items.push(Item::Reorder {
                title: crate::i18n::text("monitor-order").into(),
                values: options
                    .metrics
                    .iter()
                    .enumerate()
                    .map(|(index, metric)| {
                        (
                            metric.label().into(),
                            (index > 0)
                                .then(|| json!({"op":"move", "metric":metric, "direction":-1})),
                            (index + 1 < options.metrics.len())
                                .then(|| json!({"op":"move", "metric":metric, "direction":1})),
                        )
                    })
                    .collect(),
            });
        }
        if [Metric::Network, Metric::Disk, Metric::Gpu]
            .iter()
            .any(|metric| options.metrics.contains(metric))
        {
            form.section(crate::i18n::text("monitor-device-settings"));
        }
        for (metric, key, selected) in [
            (Metric::Network, "monitor-select-network", &options.network),
            (Metric::Disk, "monitor-select-disk", &options.disk),
            (Metric::Gpu, "monitor-select-gpu", &options.gpu),
        ] {
            if !options.metrics.contains(&metric) {
                continue;
            }
            let mut devices = vec![(None, crate::i18n::text("monitor-auto").to_owned())];
            devices.extend(
                crate::devices(metric)
                    .into_iter()
                    .map(|(id, name)| (Some(id), name)),
            );
            if let Some(id) = selected
                .as_ref()
                .filter(|id| !devices.iter().any(|(value, _)| value.as_ref() == Some(id)))
            {
                devices.push((
                    Some(id.clone()),
                    crate::i18n::text("monitor-unavailable").into(),
                ));
            }
            form.dropdown(
                crate::i18n::text(key),
                devices
                    .iter()
                    .map(|(device, name)| {
                        let mut next = options.clone();
                        match metric {
                            Metric::Network => next.network = device.clone(),
                            Metric::Disk => next.disk = device.clone(),
                            _ => next.gpu = device.clone(),
                        }
                        (name.as_str(), action(next), selected == device)
                    })
                    .collect(),
            );
        }
    }
    form.0
}

struct Form(Page);
impl Form {
    fn section(&mut self, title: &str) {
        self.0.items.push(Item::Section(title.into()));
    }
    fn toggle(&mut self, title: &str, description: &str, selected: bool, action: Value) {
        self.0.items.push(Item::Toggle {
            title: title.into(),
            description: description.into(),
            selected,
            action,
        });
    }
    fn toggle_group(&mut self, title: &str, values: Vec<(&str, Value, bool)>) {
        self.0.items.push(Item::Toggles {
            title: title.into(),
            values: values
                .into_iter()
                .map(|(s, a, b)| (s.into(), a, b))
                .collect(),
        });
    }
    fn dropdown(&mut self, title: &str, values: Vec<(&str, Value, bool)>) {
        self.0.items.push(Item::Dropdown {
            title: title.into(),
            values: values
                .into_iter()
                .map(|(s, a, b)| (s.into(), a, b))
                .collect(),
        });
    }
}

/// Actions belong to the plugin; the host only routes the opaque JSON payload.
pub(crate) fn apply(action: Value) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum Action {
        Save { options: Preferences },
        Move { metric: Metric, direction: i8 },
    }
    match serde_json::from_value::<Action>(action).map_err(|e| e.to_string())? {
        Action::Save { options } => crate::config::options(options),
        Action::Move { metric, direction } => {
            if ![-1, 1].contains(&direction) {
                return Err("Invalid metric direction".into());
            }
            let mut options = crate::config::get().options;
            let Some(index) = options.metrics.iter().position(|m| *m == metric) else {
                return Ok(());
            };
            let next = index as isize + direction as isize;
            if next >= 0 && (next as usize) < options.metrics.len() {
                options.metrics.swap(index, next as usize);
                crate::config::options(options)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_settings_use_dropdowns_and_reorder_all_enabled_metrics() {
        let options = Preferences::default();
        let page = draw(&options);
        assert!(
            page.items
                .iter()
                .all(|item| !matches!(item, Item::Choices { .. } | Item::Button { .. }))
        );
        assert_eq!(
            page.items
                .iter()
                .filter(|item| matches!(item, Item::Dropdown { .. }))
                .count(),
            5
        );
        let Item::Reorder { values, .. } = page
            .items
            .iter()
            .find(|item| matches!(item, Item::Reorder { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(values.len(), options.metrics.len());
        assert!(values[0].1.is_none());
        assert!(values.last().unwrap().2.is_none());
    }
    #[test]
    fn moving_a_metric_preserves_selection_and_obeys_list_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        crate::config::open(dir.path().into()).unwrap();
        let options = Preferences::default();
        crate::config::options(options.clone()).unwrap();
        apply(json!({"op":"move","metric":"Gpu","direction":-1})).unwrap();
        assert_eq!(
            crate::config::get().options.metrics,
            [
                Metric::Cpu,
                Metric::Gpu,
                Metric::Memory,
                Metric::Network,
                Metric::Disk
            ]
        );
        apply(json!({"op":"move","metric":"Gpu","direction":1})).unwrap();
        assert_eq!(crate::config::get().options, options);
        let bytes = std::fs::read(dir.path().join("config.toml")).unwrap();
        apply(json!({"op":"move","metric":"Cpu","direction":-1})).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("config.toml")).unwrap(),
            bytes
        );
        assert!(apply(json!({"op":"move","metric":"Cpu","direction":9})).is_err());
        assert!(apply(json!({"op":"task_manager"})).is_err());
    }
}
