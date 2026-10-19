use crate::native::Module;
use luciddesk_plugin_api::{Invoke, Manifest, call_json};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginState {
    Ready,
    Disabled,
    Faulted,
}

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub manifest: Manifest,
    pub path: PathBuf,
    pub state: PluginState,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub error: String,
}

struct Plugin {
    info: PluginInfo,
    invoke: Invoke,
    // Set before invoking resource-creating operations, including config_open.
    active: bool,
    configured: bool,
}

impl Plugin {
    unsafe fn invoke(
        &self,
        mut request: Value,
        language: &str,
        context: *mut c_void,
    ) -> Result<Value, String> {
        let object = request
            .as_object_mut()
            .ok_or("Plugin request must be a JSON object")?;
        if object
            .get("op")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err("Plugin request must contain a non-empty operation".into());
        }
        object.insert("language".into(), json!(language));
        // invoke belongs to a pinned module; caller supplies the context lifetime.
        unsafe { call_json(self.invoke, context, &request) }
            .map_err(|e| format!("{}: {e}", self.info.manifest.id))
    }

    fn stop(&mut self, language: &str) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        // Leave active set on failure so explicit shutdown can retry cleanup.
        if let Err(error) =
            unsafe { self.invoke(json!({"op":"shutdown"}), language, std::ptr::null_mut()) }
        {
            if self.info.state != PluginState::Disabled {
                self.info.state = PluginState::Faulted;
            }
            self.info.error = Some(error.clone());
            return Err(error);
        }
        self.active = false;
        self.configured = false;
        Ok(())
    }

    fn configure(&mut self, root: Option<&Path>, language: &str) -> Result<(), String> {
        if self.configured || !self.info.manifest.supports("config.v1") {
            return Ok(());
        }
        let Some(root) = root else { return Ok(()) }; // Child processes consume snapshots only.
        self.active = true;
        let result = unsafe {
            self.invoke(
                json!({"op":"config_open", "directory":root.join(&self.info.manifest.id)}),
                language,
                std::ptr::null_mut(),
            )
        };
        match result {
            Ok(_) => {
                self.configured = true;
                Ok(())
            }
            Err(mut error) => {
                if let Err(cleanup) = self.stop(language) {
                    error.push_str("; cleanup failed: ");
                    error.push_str(&cleanup);
                }
                self.info.state = PluginState::Faulted;
                self.info.error = Some(error.clone());
                Err(error)
            }
        }
    }
}

/// One runtime belongs to one UI thread; no function table or plugin state is shared.
///
/// ```compile_fail
/// use luciddesk_plugin_host::Runtime;
/// fn requires_send<T: Send>() {}
/// requires_send::<Runtime>();
/// ```
///
/// ```compile_fail
/// use luciddesk_plugin_host::Runtime;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<Runtime>();
/// ```
#[derive(Default)]
pub struct Runtime {
    plugins: BTreeMap<String, Plugin>,
    config_root: Option<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    thread: PhantomData<Rc<()>>,
}

impl Runtime {
    /// Discover once, deterministically, without opening configuration or creating resources.
    /// Only entry/manifest rejections unload a module. A missing directory is normal.
    #[must_use]
    pub fn discover(directory: &Path, language: &str) -> Self {
        let mut runtime = Self::default();
        let directory = match std::path::absolute(directory) {
            Ok(path) => path,
            Err(error) => {
                runtime.report(directory, error.to_string());
                return runtime;
            }
        };
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return runtime,
            Err(error) => {
                runtime.report(&directory, error.to_string());
                return runtime;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
                        && path.is_file()
                    {
                        paths.push(path);
                    }
                }
                Err(error) => runtime.report(&directory, error.to_string()),
            }
        }
        paths.sort();
        for path in paths {
            if let Err(error) = runtime.load(&path, language) {
                runtime.report(&path, error);
            }
        }
        runtime
    }

    fn load(&mut self, path: &Path, language: &str) -> Result<(), String> {
        let module = Module::open(path)?;
        let invoke = module.entry()?;
        // Manifest is a resource-free operation; a rejection can safely release this load.
        let manifest: Manifest = serde_json::from_value(
            unsafe {
                call_json(
                    invoke,
                    std::ptr::null_mut(),
                    &json!({"op":"manifest", "language":language}),
                )
            }
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        manifest.validate().map_err(|e| e.to_string())?;
        if self.plugins.contains_key(&manifest.id) {
            return Err(format!(
                "duplicate plugin ID {}; first sorted file retained",
                manifest.id
            ));
        }
        module.pin();
        self.plugins.insert(
            manifest.id.clone(),
            Plugin {
                info: PluginInfo {
                    manifest,
                    path: path.to_owned(),
                    state: PluginState::Ready,
                    error: None,
                },
                invoke,
                active: false,
                configured: false,
            },
        );
        Ok(())
    }

    fn report(&mut self, path: &Path, error: String) {
        self.diagnostics.push(Diagnostic {
            path: path.to_owned(),
            error,
        });
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    #[must_use]
    pub fn installed(&self) -> Vec<PluginInfo> {
        self.plugins.values().map(|p| p.info.clone()).collect()
    }

    /// Apply host policy before configuration. Failure affects only that plugin;
    /// failed instances stay visible and can be retried by re-enabling/reloading.
    pub fn initialize(
        &mut self,
        root: Option<PathBuf>,
        disabled: &BTreeSet<String>,
        language: &str,
    ) {
        self.config_root = root;
        for (id, plugin) in &mut self.plugins {
            let result = if disabled.contains(id) {
                plugin.info.state = PluginState::Disabled;
                plugin.stop(language)
            } else {
                // A previous cleanup failure must be resolved before reopening resources.
                let cleanup =
                    if plugin.info.state != PluginState::Ready || plugin.info.error.is_some() {
                        plugin.stop(language)
                    } else {
                        Ok(())
                    };
                cleanup.and_then(|()| {
                    plugin.info.state = PluginState::Ready;
                    plugin.configured = false;
                    plugin.configure(self.config_root.as_deref(), language)
                })
            };
            plugin.info.error = result.err();
            if let Some(error) = &plugin.info.error {
                if !disabled.contains(id) {
                    plugin.info.state = PluginState::Faulted;
                }
                self.diagnostics.push(Diagnostic {
                    path: plugin.info.path.clone(),
                    error: error.clone(),
                });
            }
        }
    }

    #[must_use]
    pub fn supports(&self, id: &str, capability: &str) -> bool {
        self.plugins.get(id).is_some_and(|p| {
            p.info.state == PluginState::Ready && p.info.manifest.supports(capability)
        })
    }

    /// Apply activation policy without changing the installed inventory.
    /// # Errors
    /// Returns an error for an unknown ID or failed cleanup/configuration.
    pub fn set_enabled(&mut self, id: &str, enabled: bool, language: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin {id} is not installed"))?;
        let result = if enabled {
            if plugin.info.state == PluginState::Ready {
                return Ok(());
            }
            plugin.stop(language).and_then(|()| {
                plugin.info.state = PluginState::Ready;
                plugin.configure(self.config_root.as_deref(), language)
            })
        } else {
            plugin.info.state = PluginState::Disabled;
            plugin.stop(language)
        };
        plugin.info.error = result.as_ref().err().cloned();
        if result.is_err() && enabled {
            plugin.info.state = PluginState::Faulted;
        }
        result
    }

    /// Call an operation without a host drawing interface.
    /// # Errors
    /// Returns routing, lifecycle, protocol or plugin business errors.
    pub fn request(&mut self, id: &str, request: Value, language: &str) -> Result<Value, String> {
        // Ordinary operations never carry a borrowed host interface.
        unsafe { self.call(id, request, language, std::ptr::null_mut()) }
    }

    /// Invoke on this runtime's UI thread.
    /// # Errors
    /// Returns routing, lifecycle, protocol or plugin business errors.
    /// # Safety
    /// context must be null or a valid borrowed interface expected by the operation;
    /// the trusted plugin must obey the SDK pointer/callback lifetime contract.
    pub unsafe fn call(
        &mut self,
        id: &str,
        request: Value,
        language: &str,
        context: *mut c_void,
    ) -> Result<Value, String> {
        let plugin = self
            .plugins
            .get_mut(id)
            .ok_or_else(|| format!("Plugin {id} is not installed"))?;
        match plugin.info.state {
            PluginState::Disabled => return Err(format!("Plugin {id} is disabled")),
            PluginState::Faulted => {
                return Err(format!(
                    "Plugin {id} is faulted: {}",
                    plugin.info.error.as_deref().unwrap_or_default()
                ));
            }
            PluginState::Ready => {}
        }
        let operation = request
            .as_object()
            .ok_or("Plugin request must be a JSON object")?
            .get("op")
            .and_then(Value::as_str)
            .filter(|op| !op.is_empty())
            .ok_or("Plugin request must contain a non-empty operation")?;
        // Validate before opening configuration or marking the instance active.
        // Lifecycle/configuration cannot bypass runtime policy through the generic route.
        match operation {
            "config_open" | "manifest" => {
                return Err("Lifecycle operation is owned by the plugin runtime".into());
            }
            "shutdown" => {
                plugin.stop(language)?;
                return Ok(Value::Null);
            }
            _ => {}
        }
        plugin.configure(self.config_root.as_deref(), language)?;
        plugin.active = true;
        unsafe { plugin.invoke(request, language, context) }
    }

    /// Cleanup without discovering new DLLs. Repeated successful shutdown is a no-op.
    /// A later request can restart a ready plugin and reopen its configuration.
    pub fn shutdown(&mut self, language: &str) {
        for plugin in self.plugins.values_mut() {
            if let Err(error) = plugin.stop(language) {
                plugin.info.state = if plugin.info.state == PluginState::Disabled {
                    PluginState::Disabled
                } else {
                    PluginState::Faulted
                };
                plugin.info.error = Some(error.clone());
                self.diagnostics.push(Diagnostic {
                    path: plugin.info.path.clone(),
                    error,
                });
            }
        }
    }
}
