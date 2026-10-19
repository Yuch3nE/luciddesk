//! Installed plugins and their persistent activation state.
use super::*;

pub(super) fn draw(scene: &mut Scene, width: f32) {
    let mut form = SettingsForm::new(scene, width, crate::i18n::text("plugins-description"));
    let plugins = crate::plugins::snapshots();
    if plugins.is_empty() {
        form.section(crate::i18n::text("plugins-empty"));
    }
    for plugin in plugins {
        let enabled = crate::plugins::enabled(&plugin.manifest.id);
        let mut description = format!("{} · {}", plugin.manifest.version, plugin.manifest.id);
        if let Some(error) = plugin.error {
            description.push('\n');
            description.push_str(&error);
        }
        form.toggle(
            &plugin.manifest.name,
            &description,
            enabled,
            Action::PluginEnabled(plugin.manifest.id.clone(), !enabled),
        );
        if enabled
            && plugin.state == luciddesk_plugin_host::PluginState::Ready
            && plugin
                .manifest
                .supports(luciddesk_plugin_api::SETTINGS_PAGE_CAPABILITY)
        {
            form.button(
                crate::i18n::text("ui-settings"),
                "",
                crate::i18n::text("ui-open"),
                Action::PluginPage(plugin.manifest.id.clone()),
            );
        }
    }
}

pub(in crate::pane) fn set_enabled(
    state: &Rc<RefCell<PaneApp>>,
    id: &str,
    enabled: bool,
) -> Result<(), String> {
    if crate::plugins::enabled(id) == enabled {
        return Ok(());
    }
    crate::plugins::save_enabled(id, enabled)?;
    if id == monitor::PLUGIN_ID && !enabled {
        // Closing stdin lets children release their windows and sampling normally.
        // Drop outside the application borrow; child exit can deliver window messages.
        let children = std::mem::take(&mut state.borrow_mut().monitors);
        drop(children);
    }
    let result = crate::plugins::set_enabled(id, enabled).and_then(|()| {
        if id == monitor::PLUGIN_ID && enabled {
            monitor::process::configure(state)?;
        }
        Ok(())
    });
    // Configuration/cleanup failure is a visible state change too.
    state.borrow().wake.notify();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_management_and_settings_pages_share_the_plugin_group() {
        let mut scene = layout::scene(
            900.0,
            520.0,
            15,
            false,
            (PanelTheme::Light, Backdrop::Acrylic),
            Default::default(),
        );
        let nav: Vec<_> = scene
            .controls
            .iter()
            .filter(|c| matches!(c.action, Action::Page(_)))
            .collect();
        assert!(!nav.iter().any(|c| matches!(c.action, Action::Page(14))));
        assert_eq!(
            nav.iter().any(|c| matches!(c.action, Action::Page(15))),
            !crate::plugins::installed().is_empty()
        );
        for pair in nav.windows(2) {
            assert!(pair[0].bounds.bottom <= pair[1].bounds.top);
        }
        assert!(nav.last().unwrap().bounds.bottom <= 520.0);
        draw(&mut scene, 900.0);
        for plugin in crate::plugins::installed() {
            assert!(scene.controls.iter().any(|c| matches!(&c.action,
                Action::PluginEnabled(id, next) if id == &plugin.id && *next != crate::plugins::enabled(id))));
        }
    }
}
