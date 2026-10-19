use super::*;
use luciddesk_plugin_api::{PROTOCOL_VERSION, PluginError, Reply, serve_json};
use std::cell::{Cell, RefCell};
use std::os::windows::ffi::OsStrExt;

thread_local! {
    static CALLS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static FAIL_CONFIG: Cell<bool> = const { Cell::new(false) };
    static FAIL_STOP: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "C" fn fixture(
    context: *mut c_void,
    bytes: *const u8,
    len: usize,
    reply: Reply,
    user: *mut c_void,
) -> i32 {
    unsafe {
        serve_json(bytes, len, reply, user, |request| {
            CALLS.with(|v| v.borrow_mut().push(request.clone()));
            match request["op"].as_str().unwrap() {
                "config_open" if FAIL_CONFIG.get() => {
                    Err(PluginError::new("CONFIG_ERROR", "Invalid config"))
                }
                "shutdown" if FAIL_STOP.get() => {
                    Err(PluginError::new("STOP_ERROR", "Worker still running"))
                }
                "echo" => Ok(request),
                "context" => Ok(json!(context as usize)),
                "business_error" => Err(PluginError::new("NOT_FOUND", "Missing item")),
                _ => Ok(Value::Null),
            }
        })
    }
}

fn runtime() -> Runtime {
    CALLS.with(|v| v.borrow_mut().clear());
    FAIL_CONFIG.set(false);
    FAIL_STOP.set(false);
    let mut runtime = Runtime::default();
    for (id, capabilities) in [
        (
            "configurable",
            vec!["config.v1".into(), "example.v1".into()],
        ),
        ("independent", vec!["example.v1".into()]),
    ] {
        runtime.plugins.insert(
            id.into(),
            Plugin {
                info: PluginInfo {
                    manifest: Manifest {
                        id: id.into(),
                        name: id.into(),
                        version: "1.0.0".into(),
                        protocol_version: PROTOCOL_VERSION,
                        capabilities,
                    },
                    path: PathBuf::from(format!("{id}.dll")),
                    state: PluginState::Ready,
                    error: None,
                },
                invoke: fixture,
                active: false,
                configured: false,
            },
        );
    }
    runtime
}

fn calls() -> Vec<Value> {
    CALLS.with(|v| std::mem::take(&mut *v.borrow_mut()))
}
fn ops() -> Vec<String> {
    calls()
        .into_iter()
        .map(|v| v["op"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn disabled_policy_precedes_configuration_and_enable_opens_assigned_directory() {
    let mut runtime = runtime();
    let root = PathBuf::from("data/plugins");
    let disabled = BTreeSet::from(["configurable".into(), "uninstalled".into()]);
    runtime.initialize(Some(root.clone()), &disabled, "zh-CN");
    assert_eq!(calls(), Vec::<Value>::new());
    assert_eq!(runtime.installed().len(), 2);
    assert!(!runtime.supports("configurable", "example.v1"));
    assert!(
        runtime
            .request("configurable", json!({"op":"echo"}), "en-US")
            .unwrap_err()
            .contains("disabled")
    );
    runtime.set_enabled("configurable", true, "en-US").unwrap();
    let requests = calls();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["directory"], json!(root.join("configurable")));
    assert_eq!(requests[0]["language"], "en-US");
    runtime.set_enabled("configurable", true, "en-US").unwrap();
    assert_eq!(calls(), Vec::<Value>::new());
    assert!(runtime.supports("configurable", "example.v1"));
}

#[test]
fn failed_configuration_is_cleaned_up_retained_and_recoverable() {
    let mut runtime = runtime();
    FAIL_CONFIG.set(true);
    runtime.initialize(Some("data/plugins".into()), &BTreeSet::new(), "zh-CN");
    assert_eq!(ops(), ["config_open", "shutdown"]);
    let info = &runtime.installed()[0];
    assert_eq!(info.state, PluginState::Faulted);
    assert!(info.error.as_ref().unwrap().contains("CONFIG_ERROR"));
    assert_eq!(runtime.take_diagnostics().len(), 1);
    assert!(!runtime.supports("configurable", "example.v1"));
    assert!(
        runtime
            .request("configurable", json!({"op":"echo"}), "en-US")
            .unwrap_err()
            .contains("faulted")
    );
    runtime
        .request("independent", json!({"op":"echo"}), "en-US")
        .unwrap();
    assert_eq!(ops(), ["echo"]);
    FAIL_CONFIG.set(false);
    runtime.set_enabled("configurable", true, "en-US").unwrap();
    assert_eq!(ops(), ["config_open"]);
    assert!(runtime.installed()[0].error.is_none());
}

#[test]
fn failed_disable_blocks_calls_and_must_finish_cleanup_before_reenable() {
    let mut runtime = runtime();
    runtime.initialize(Some("data/plugins".into()), &BTreeSet::new(), "zh-CN");
    calls();
    FAIL_STOP.set(true);
    assert!(runtime.set_enabled("configurable", false, "zh-CN").is_err());
    assert_eq!(runtime.installed()[0].state, PluginState::Disabled);
    assert!(
        runtime
            .request("configurable", json!({"op":"echo"}), "zh-CN")
            .is_err()
    );
    // Reloading host preferences cannot reopen configuration over a failed cleanup.
    runtime.initialize(Some("other/plugins".into()), &BTreeSet::new(), "en-US");
    assert_eq!(ops(), ["shutdown", "shutdown"]);
    assert_eq!(runtime.installed()[0].state, PluginState::Faulted);
    FAIL_STOP.set(false);
    runtime.set_enabled("configurable", true, "en-US").unwrap();
    assert_eq!(ops(), ["shutdown", "config_open"]);
}

#[test]
fn shutdown_is_idempotent_and_next_request_reopens_configuration() {
    let mut runtime = runtime();
    runtime.initialize(Some("data/plugins".into()), &BTreeSet::new(), "zh-CN");
    runtime
        .request("independent", json!({"op":"echo"}), "zh-CN")
        .unwrap();
    calls();
    runtime.shutdown("zh-CN");
    assert_eq!(ops(), ["shutdown", "shutdown"]);
    runtime.shutdown("zh-CN");
    assert_eq!(calls(), Vec::<Value>::new());
    runtime
        .request("configurable", json!({"op":"echo"}), "en-US")
        .unwrap();
    assert_eq!(ops(), ["config_open", "echo"]);
}

#[test]
fn child_runtime_never_opens_disk_configuration_and_uses_current_language() {
    let mut runtime = runtime();
    runtime.initialize(None, &BTreeSet::new(), "zh-CN");
    let response = runtime
        .request(
            "configurable",
            json!({"op":"echo", "language":"untrusted"}),
            "en-US",
        )
        .unwrap();
    assert_eq!(response["language"], "en-US");
    assert_eq!(ops(), ["echo"]);
    runtime.shutdown("en-US");
    assert_eq!(calls()[0]["language"], "en-US");
}

#[test]
fn ordinary_requests_cannot_replace_runtime_configuration_and_business_errors_are_local() {
    let mut runtime = runtime();
    for request in [
        json!(null),
        json!({"op":""}),
        json!({}),
        json!({"op":"config_open", "directory":"elsewhere"}),
        json!({"op":"manifest"}),
    ] {
        assert!(runtime.request("independent", request, "en-US").is_err());
    }
    assert_eq!(calls(), Vec::<Value>::new());
    assert!(
        runtime
            .request("missing", json!({"op":"echo"}), "en-US")
            .unwrap_err()
            .contains("not installed")
    );
    assert!(
        runtime
            .request("independent", json!({"op":"business_error"}), "en-US")
            .unwrap_err()
            .contains("NOT_FOUND")
    );
    assert!(runtime.supports("independent", "example.v1"));
    runtime
        .request("independent", json!({"op":"echo"}), "en-US")
        .unwrap();
}

#[test]
fn borrowed_context_is_forwarded_only_for_the_synchronous_call() {
    let mut runtime = runtime();
    let mut context = 7_u32;
    let pointer = (&raw mut context).cast();
    // The fixture reads only the pointer's address and does not retain/dereference it.
    let result =
        unsafe { runtime.call("independent", json!({"op":"context"}), "en-US", pointer) }.unwrap();
    assert_eq!(result, json!(pointer as usize));
    assert_eq!(
        runtime
            .request("independent", json!({"op":"context"}), "en-US")
            .unwrap(),
        json!(0)
    );
}

#[test]
fn discovery_tolerates_missing_directory_and_reports_invalid_dll_without_stopping() {
    let directory = tempfile::tempdir().unwrap();
    let mut missing = Runtime::discover(&directory.path().join("missing"), "en-US");
    assert!(missing.installed().is_empty());
    assert!(missing.take_diagnostics().is_empty());
    let invalid = directory.path().join("broken.DLL");
    std::fs::write(&invalid, b"not a PE image").unwrap();
    std::fs::create_dir(directory.path().join("directory.dll")).unwrap();
    std::fs::write(directory.path().join("ignored.txt"), b"ignored").unwrap();
    let mut runtime = Runtime::discover(directory.path(), "en-US");
    let diagnostics = runtime.take_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].path, invalid);
    assert!(runtime.installed().is_empty());
}

#[test]
fn real_dll_routes_and_duplicate_rejection_releases_only_the_rejected_reference() {
    if std::env::var_os("LUCIDDESK_TEST_EXAMPLE_PLUGIN").is_none() {
        return;
    }
    let plugins = std::env::current_exe().unwrap().with_file_name("plugins");
    let mut runtime = Runtime::discover(&plugins, "en-US");
    assert!(runtime.supports("example.echo", "example.echo.v1"));
    assert_eq!(
        runtime
            .request("example.echo", json!({"op":"echo", "value":42}), "en-US")
            .unwrap(),
        json!(42)
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("duplicate-echo.dll");
    std::fs::copy(plugins.join("echo-example.dll"), &path).unwrap();
    let error = runtime.load(&path, "en-US").unwrap_err();
    assert!(error.contains("duplicate plugin ID example.echo"));
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    assert!(
        unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) }
            .is_null()
    );
    std::fs::remove_file(path).unwrap();
    // Discovery does not activate resources; only the echo instance needs shutdown.
    runtime.shutdown("en-US");
}

#[test]
fn real_configuration_failure_keeps_module_loaded_and_other_plugins_available() {
    if std::env::var("LUCIDDESK_TEST_MONITOR_PLUGIN").as_deref() != Ok("1") {
        return;
    }
    let plugins = std::env::current_exe().unwrap().with_file_name("plugins");
    let mut runtime = Runtime::discover(&plugins, "en-US");
    assert!(runtime.supports("system-monitor", "system-monitor.v1"));
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let config_directory = root.join("system-monitor");
    std::fs::create_dir_all(&config_directory).unwrap();
    let config = config_directory.join("config.toml");
    let invalid = "[options\ninvalid TOML";
    std::fs::write(&config, invalid).unwrap();
    runtime.initialize(Some(root), &BTreeSet::new(), "en-US");
    let info = runtime
        .installed()
        .into_iter()
        .find(|p| p.manifest.id == "system-monitor")
        .unwrap();
    assert_eq!(info.state, PluginState::Faulted);
    assert!(info.error.is_some());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), invalid);
    assert!(!runtime.supports("system-monitor", "system-monitor.v1"));
    let wide: Vec<u16> = info.path.as_os_str().encode_wide().chain(Some(0)).collect();
    assert!(
        !unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(wide.as_ptr()) }
            .is_null()
    );
    if std::env::var_os("LUCIDDESK_TEST_EXAMPLE_PLUGIN").is_some() {
        assert_eq!(
            runtime
                .request(
                    "example.echo",
                    json!({"op":"echo", "value":"still available"}),
                    "en-US"
                )
                .unwrap(),
            json!("still available")
        );
    }
    std::fs::write(config, "").unwrap();
    runtime
        .set_enabled("system-monitor", true, "en-US")
        .unwrap();
    assert!(runtime.supports("system-monitor", "system-monitor.v1"));
    let config = runtime
        .request("system-monitor", json!({"op":"config_get"}), "en-US")
        .unwrap();
    assert_eq!(config["options"]["enabled"], false);
    runtime.shutdown("en-US");
}

#[test]
fn failed_explicit_shutdown_blocks_calls_until_cleanup_and_activation_succeed() {
    let mut runtime = runtime();
    runtime
        .request("independent", json!({"op":"echo"}), "en-US")
        .unwrap();
    FAIL_STOP.set(true);
    assert!(
        runtime
            .request("independent", json!({"op":"shutdown"}), "en-US")
            .is_err()
    );
    assert!(!runtime.supports("independent", "example.v1"));
    assert!(
        runtime
            .request("independent", json!({"op":"echo"}), "en-US")
            .is_err()
    );
    FAIL_STOP.set(false);
    runtime.set_enabled("independent", true, "en-US").unwrap();
    assert!(runtime.supports("independent", "example.v1"));
}

#[test]
fn invalid_requests_do_not_restart_stopped_resources() {
    let mut runtime = runtime();
    runtime.initialize(Some("data/plugins".into()), &BTreeSet::new(), "en-US");
    runtime.shutdown("en-US");
    calls();
    for request in [json!(null), json!({"op":""}), json!({"op":1})] {
        assert!(runtime.request("configurable", request, "en-US").is_err());
    }
    runtime.shutdown("en-US");
    assert_eq!(calls(), Vec::<Value>::new());
}
