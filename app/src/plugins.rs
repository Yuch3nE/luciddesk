//! Application-local, versioned DLL registry. Accepted modules remain pinned until exit.
use luciddesk_plugin_api::{
    ABI_VERSION, ENTRY_NAME, Entry, Invoke, Manifest, call_json, read_entry,
};
use serde_json::{Value, json};
use std::os::windows::ffi::OsStrExt;
use std::{collections::BTreeMap, ffi::c_void, path::Path, sync::OnceLock};
use windows_sys::Win32::System::LibraryLoader::*;

thread_local! {
    static CONFIG_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
    static DISABLED: std::cell::RefCell<std::collections::BTreeSet<String>> = Default::default();
}
pub fn initialize(store: &luciddesk_storage::WorkspaceStore) -> Result<(), String> {
    CONFIG_ROOT.with(|root| {
        *root.borrow_mut() = store
            .config_path()
            .and_then(|p| p.parent().map(|p| p.join("plugins")))
    });
    if let Some(plugins) = REGISTRY.get() {
        for plugin in plugins.values() {
            configure(plugin)?;
        }
    }
    let saved = store
        .preference("plugins_disabled")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let disabled = saved.split_whitespace().map(str::to_owned).collect();
    if let Some(plugins) = REGISTRY.get() {
        for id in plugins.keys() {
            if enabled(id) && std::collections::BTreeSet::contains(&disabled, id) {
                set_enabled(id, false);
            }
        }
    }
    DISABLED.with(|v| *v.borrow_mut() = disabled);
    Ok(())
}
pub fn enabled(id: &str) -> bool {
    DISABLED.with(|v| !v.borrow().contains(id))
}
pub fn installed() -> Vec<Manifest> {
    registry().values().map(|p| p.manifest.clone()).collect()
}
pub fn save_enabled(
    store: &luciddesk_storage::WorkspaceStore,
    id: &str,
    enabled: bool,
) -> Result<(), String> {
    if !registry().contains_key(id) {
        return Err(format!("Plugin {id} is not installed"));
    }
    let mut disabled = DISABLED.with(|v| v.borrow().clone());
    if enabled {
        disabled.remove(id);
    } else {
        disabled.insert(id.to_owned());
    }
    store
        .save_preference(
            "plugins_disabled",
            &disabled.into_iter().collect::<Vec<_>>().join(" "),
        )
        .map_err(|e| e.to_string())
}
pub fn set_enabled(id: &str, enabled: bool) {
    if !enabled {
        if let Err(error) = request(id, json!({"op":"shutdown"})) {
            log(&error);
        }
    }
    DISABLED.with(|v| {
        if enabled {
            v.borrow_mut().remove(id);
        } else {
            v.borrow_mut().insert(id.to_owned());
        }
    });
}

struct Plugin {
    invoke: Invoke,
    manifest: Manifest,
}
static REGISTRY: OnceLock<BTreeMap<String, Plugin>> = OnceLock::new();
fn configure(plugin: &Plugin) -> Result<(), String> {
    if plugin.manifest.supports("config.v1") {
        if let Some(directory) =
            CONFIG_ROOT.with(|root| root.borrow().as_ref().map(|p| p.join(&plugin.manifest.id)))
        {
            unsafe {
                call_json(
                    plugin.invoke,
                    std::ptr::null_mut(),
                    &json!({"op":"config_open", "directory":directory}),
                )
            }
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn load(path: &Path, plugins: &BTreeMap<String, Plugin>) -> Result<Plugin, String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let module = LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let result = (|| {
            let entry = GetProcAddress(module, ENTRY_NAME.as_ptr().cast())
                .ok_or("Missing plugin entry point")?;
            let entry: Entry = std::mem::transmute(entry);
            let invoke = read_entry(entry(ABI_VERSION)).map_err(|e| e.to_string())?;
            let value = call_json(
                invoke,
                std::ptr::null_mut(),
                &json!({"op":"manifest","language":crate::i18n::language()}),
            )
            .map_err(|e| e.to_string())?;
            let manifest: Manifest = serde_json::from_value(value).map_err(|e| e.to_string())?;
            manifest.validate().map_err(|e| e.to_string())?;
            if plugins.contains_key(&manifest.id) {
                return Err(format!(
                    "duplicate plugin ID {}; first sorted file retained",
                    manifest.id
                ));
            }
            let plugin = Plugin { invoke, manifest };
            configure(&plugin)?;
            Ok(plugin)
        })();
        if result.is_err() {
            windows_sys::Win32::Foundation::FreeLibrary(module);
        }
        result
    }
}
fn log(message: &str) {
    luciddesk_diagnostics::log(luciddesk_diagnostics::Level::Warn, "plugins", message);
}
fn registry() -> &'static BTreeMap<String, Plugin> {
    REGISTRY.get_or_init(|| {
        let mut plugins = BTreeMap::new();
        let directory = match std::env::current_exe() {
            Ok(path) => path.with_file_name("plugins"),
            Err(error) => {
                log(&error.to_string());
                return plugins;
            }
        };
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return plugins,
            Err(e) => {
                log(&format!("{}: {e}", directory.display()));
                return plugins;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry)
                    if entry
                        .path()
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("dll")) =>
                {
                    paths.push(entry.path())
                }
                Ok(_) => {}
                Err(e) => log(&e.to_string()),
            }
        }
        paths.sort();
        for path in paths {
            match load(&path, &plugins) {
                Ok(plugin) => {
                    plugins.insert(plugin.manifest.id.clone(), plugin);
                }
                Err(e) => log(&format!("{}: {e}", path.display())),
            }
        }
        plugins
    })
}
pub fn supports(id: &str, capability: &str) -> bool {
    enabled(id)
        && registry()
            .get(id)
            .is_some_and(|p| p.manifest.supports(capability))
}
pub fn call(id: &str, mut request: Value, context: *mut c_void) -> Result<Value, String> {
    if !enabled(id) {
        return Err(format!("Plugin {id} is disabled"));
    }
    let plugin = registry()
        .get(id)
        .ok_or_else(|| format!("Plugin {id} is not installed"))?;
    let object = request
        .as_object_mut()
        .ok_or("Plugin request must be a JSON object")?;
    object.insert("language".into(), json!(crate::i18n::language()));
    unsafe { call_json(plugin.invoke, context, &request) }.map_err(|e| format!("{id}: {e}"))
}
pub fn request(id: &str, request: Value) -> Result<Value, String> {
    call(id, request, std::ptr::null_mut())
}
pub fn shutdown() {
    // Do not discover or load plugins merely because the graphics lifetime ends.
    if let Some(plugins) = REGISTRY.get() {
        for (id, plugin) in plugins {
            if let Err(error) = unsafe {
                call_json(
                    plugin.invoke,
                    std::ptr::null_mut(),
                    &json!({"op":"shutdown"}),
                )
            } {
                log(&format!("{id}: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_plugin_is_unloaded_after_manifest_rejection() {
        if std::env::var_os("LUCIDDESK_TEST_EXAMPLE_PLUGIN").is_none() {
            return;
        }
        let plugins = registry();
        assert!(plugins.contains_key("example.echo"));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("duplicate-echo.dll");
        let source = std::env::current_exe()
            .unwrap()
            .with_file_name("plugins")
            .join("echo-example.dll");
        std::fs::copy(source, &path).unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert!(unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null());
        let error = load(&path, plugins)
            .err()
            .expect("duplicate must be rejected");
        assert!(error.contains("duplicate plugin ID example.echo"));
        assert!(unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null());
        std::fs::remove_file(path).unwrap();
    }

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
        let store = luciddesk_storage::WorkspaceStore::open_in_memory().unwrap();
        save_enabled(&store, "example.echo", false).unwrap();
        set_enabled("example.echo", false);
        assert!(installed().iter().any(|m| m.id == "example.echo"));
        assert!(!supports("example.echo", "example.echo.v1"));
        assert!(
            request("example.echo", json!({"op":"echo"}))
                .unwrap_err()
                .contains("disabled")
        );
        initialize(&store).unwrap();
        assert!(!enabled("example.echo"));
        save_enabled(&store, "example.echo", true).unwrap();
        set_enabled("example.echo", true);
        shutdown();
        assert!(request("example.echo", json!({"op":"echo","value":42})).is_ok());
    }
}
