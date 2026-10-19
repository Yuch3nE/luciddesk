//! Application bridge: user plugin configuration and language stay outside the runtime.
use luciddesk_plugin_api::Manifest;
use luciddesk_plugin_host::{ActivationConfig, PluginInfo, Runtime};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeSet, ffi::c_void, path::PathBuf};

#[derive(Default)]
struct Host {
    runtime: Option<Runtime>,
    config_root: Option<PathBuf>,
    activation: Option<ActivationConfig>,
    disabled: BTreeSet<String>,
}

thread_local! {
    // Plugin TLS, configuration, lifecycle and calls all belong to the same UI thread.
    static HOST: RefCell<Host> = RefCell::new(Host::default());
    // Native window activation/destruction can repaint the application during an ABI call.
    // Read-only UI queries use this owned snapshot; nested invocations return an error.
    static VIEW: RefCell<Vec<PluginInfo>> = const { RefCell::new(Vec::new()) };
}

fn log(message: &str) {
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "plugins", message);
}

fn log_diagnostics(runtime: &mut Runtime) {
    for diagnostic in runtime.take_diagnostics() {
        log(&format!(
            "{}: {}",
            diagnostic.path.display(),
            diagnostic.error
        ));
    }
}

impl Host {
    fn runtime(&mut self) -> &mut Runtime {
        self.runtime.get_or_insert_with(|| {
            let mut runtime = match std::env::current_exe() {
                Ok(path) => {
                    Runtime::discover(&path.with_file_name("plugins"), crate::i18n::language())
                }
                Err(error) => {
                    log(&error.to_string());
                    Runtime::default()
                }
            };
            runtime.initialize(
                self.config_root.clone(),
                &self.disabled,
                crate::i18n::language(),
            );
            runtime
        })
    }
}

fn with_runtime<T>(f: impl FnOnce(&mut Runtime) -> Result<T, String>) -> Result<T, String> {
    HOST.with(|host| {
        let mut host = host
            .try_borrow_mut()
            .map_err(|_| "Plugin calls cannot reenter the runtime")?;
        let runtime = host.runtime();
        VIEW.with(|v| *v.borrow_mut() = runtime.installed());
        let result = f(runtime);
        VIEW.with(|v| *v.borrow_mut() = runtime.installed());
        log_diagnostics(runtime);
        result
    })
}

pub fn initialize(store: &luciddesk_storage::WorkspaceStore) -> Result<(), String> {
    let root = store
        .config_path()
        .and_then(|p| p.parent().map(|p| p.join("plugins")));
    let activation = root.as_deref().map(ActivationConfig::open).transpose()?;
    let disabled = activation
        .as_ref()
        .map(ActivationConfig::disabled)
        .transpose()?
        .unwrap_or_default();
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        if let Some(runtime) = &mut host.runtime {
            runtime.initialize(root.clone(), &disabled, crate::i18n::language());
            log_diagnostics(runtime);
        }
        host.config_root = root;
        host.activation = activation;
        host.disabled = disabled;
    });
    Ok(())
}

/// The user's activation preference; a requested plugin can still be faulted.
pub fn enabled(id: &str) -> bool {
    HOST.with(|host| match host.try_borrow() {
        Ok(host) => !host.disabled.contains(id),
        Err(_) => VIEW.with(|v| {
            !v.borrow().iter().any(|p| {
                p.manifest.id == id && p.state == luciddesk_plugin_host::PluginState::Disabled
            })
        }),
    })
}

pub fn installed() -> Vec<Manifest> {
    snapshots().into_iter().map(|p| p.manifest).collect()
}

pub fn snapshots() -> Vec<PluginInfo> {
    with_runtime(|runtime| Ok(runtime.installed()))
        .unwrap_or_else(|_| VIEW.with(|v| v.borrow().clone()))
}

pub fn save_enabled(id: &str, enabled: bool) -> Result<(), String> {
    if !installed().iter().any(|plugin| plugin.id == id) {
        return Err(format!("Plugin {id} is not installed"));
    }
    HOST.with(|host| {
        host.borrow_mut()
            .activation
            .as_mut()
            .ok_or("Plugin configuration directory is not initialized")?
            .save_enabled(id, enabled)
    })
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let result = with_runtime(|runtime| runtime.set_enabled(id, enabled, crate::i18n::language()));
    // Keep the requested policy even when cleanup/configuration fails; runtime exposes the fault.
    HOST.with(|host| {
        let mut host = host.borrow_mut();
        if enabled {
            host.disabled.remove(id);
        } else {
            host.disabled.insert(id.to_owned());
        }
    });
    result
}

pub fn supports(id: &str, capability: &str) -> bool {
    snapshots().iter().any(|p| {
        p.manifest.id == id
            && p.state == luciddesk_plugin_host::PluginState::Ready
            && p.manifest.supports(capability)
    })
}

pub fn call(id: &str, request: Value, context: *mut c_void) -> Result<Value, String> {
    // Only the monitor drawing adapter supplies a non-null, borrowed draw-pass interface.
    with_runtime(|runtime| unsafe { runtime.call(id, request, crate::i18n::language(), context) })
}

pub fn request(id: &str, request: Value) -> Result<Value, String> {
    with_runtime(|runtime| runtime.request(id, request, crate::i18n::language()))
}

pub fn shutdown() {
    HOST.with(|host| {
        if let Some(runtime) = &mut host.borrow_mut().runtime {
            runtime.shutdown(crate::i18n::language());
            log_diagnostics(runtime);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn independent_example_plugin_is_discovered_and_routed_by_id() {
        if std::env::var_os("LUCIDDESK_TEST_EXAMPLE_PLUGIN").is_none() {
            return;
        }
        assert!(supports("example.echo", "example.echo.v1"));
        assert!(!supports("example.echo", "system-monitor.v1"));
        let response = request("example.echo", json!({"op":"echo","value":"路由验证"})).unwrap();
        assert_eq!(response, json!("路由验证"));
        assert!(request("not-installed", json!({"op":"echo"})).is_err());
        assert!(
            request("example.echo", json!({"op":"invalid"}))
                .unwrap_err()
                .contains("UNSUPPORTED_OPERATION")
        );
        let directory = tempfile::tempdir().unwrap();
        let store = luciddesk_storage::WorkspaceStore::open(&directory.path().join("workspace.db"))
            .unwrap();
        initialize(&store).unwrap();
        let global_before = std::fs::read(directory.path().join("config.toml")).ok();
        save_enabled("example.echo", false).unwrap();
        set_enabled("example.echo", false).unwrap();
        assert!(installed().iter().any(|m| m.id == "example.echo"));
        assert!(!supports("example.echo", "example.echo.v1"));
        assert!(
            request("example.echo", json!({"op":"echo"}))
                .unwrap_err()
                .contains("disabled")
        );
        initialize(&store).unwrap();
        assert!(!enabled("example.echo"));
        save_enabled("example.echo", true).unwrap();
        set_enabled("example.echo", true).unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("config.toml")).ok(),
            global_before
        );
        assert!(directory.path().join("plugins/config.toml").exists());
        shutdown();
        assert!(request("example.echo", json!({"op":"echo","value":42})).is_ok());
    }
}
