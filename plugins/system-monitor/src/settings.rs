use crate::{Metric, Preferences};
use luciddesk_plugin_api::{Item, Page};
use serde_json::{Value, json};

pub(crate) fn draw(panels: &[(u64, String, Preferences)]) -> Page {
    let mut form = Form(Page {
        description: crate::i18n::text("monitor-settings-description").into(),
        items: Vec::new(),
    });
    if panels.is_empty() {
        form.toggle(
            crate::i18n::text("monitor-enable"),
            "",
            false,
            json!({"op":"new"}),
        );
        return form.0;
    }
    for (id, title, options) in panels {
        let action = |options| json!({"op":"save", "id":id, "options":options});
        form.section(title);
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
        form.toggle(
            crate::i18n::text("monitor-compact"),
            "",
            options.compact,
            action(Preferences {
                compact: !options.compact,
                ..options.clone()
            }),
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
        form.choices(
            crate::i18n::text("monitor-interval"),
            "",
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
        if !options.metrics.is_empty() {
            form.choices(
                crate::i18n::text("monitor-first"),
                "",
                options
                    .metrics
                    .iter()
                    .map(|metric| {
                        let mut next = options.clone();
                        next.metrics.retain(|m| m != metric);
                        next.metrics.insert(0, *metric);
                        (
                            metric.label(),
                            action(next),
                            options.metrics.first() == Some(metric),
                        )
                    })
                    .collect(),
            );
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
                devices.push((Some(id.clone()), id.clone()));
            }
            form.choices(
                crate::i18n::text(key),
                "",
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
    form.button(
        crate::i18n::text("monitor-new"),
        "",
        crate::i18n::text("monitor-new"),
        json!({"op":"new"}),
    );
    form.button(
        crate::i18n::text("monitor-task-manager"),
        "",
        crate::i18n::text("ui-open"),
        json!({"op":"task_manager"}),
    );
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
    fn choices(&mut self, title: &str, _: &str, values: Vec<(&str, Value, bool)>) {
        self.0.items.push(Item::Choices {
            title: title.into(),
            values: values
                .into_iter()
                .map(|(s, a, b)| (s.into(), a, b))
                .collect(),
        });
    }
    fn button(&mut self, title: &str, _: &str, label: &str, action: Value) {
        self.0.items.push(Item::Button {
            title: title.into(),
            label: label.into(),
            action,
        });
    }
}
