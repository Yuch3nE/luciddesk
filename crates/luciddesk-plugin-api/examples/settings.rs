//! Independent settings-page fixture: the host needs no plugin-specific UI adapter.
use luciddesk_plugin_api::{
    ABI_VERSION, Item, Manifest, PROTOCOL_VERSION, Page, PluginError, PluginV1, Reply,
    SETTINGS_PAGE_CAPABILITY, serve_json,
};
use serde_json::json;
use std::{cell::Cell, ffi::c_void};

thread_local! { static ENABLED: Cell<bool> = const { Cell::new(false) }; }

unsafe extern "C" fn invoke(
    context: *mut c_void,
    data: *const u8,
    len: usize,
    reply: Reply,
    user: *mut c_void,
) -> i32 {
    let _ = context;
    unsafe {
        serve_json(data, len, reply, user, |request| {
            match request["op"].as_str() {
                Some("manifest") => Ok(json!(Manifest {
                    id: "example.settings".into(),
                    name: "Example settings".into(),
                    version: "0.1.0".into(),
                    protocol_version: PROTOCOL_VERSION,
                    capabilities: vec![SETTINGS_PAGE_CAPABILITY.into()]
                })),
                Some("settings") => Ok(json!(Page {
                    description: "A settings page contributed by an independent plugin.".into(),
                    items: vec![Item::Toggle {
                        title: "Example option".into(),
                        description: "The plugin handles this action.".into(),
                        selected: ENABLED.get(),
                        action: json!({"op":"set", "enabled":!ENABLED.get()})
                    }]
                })),
                Some("settings_action") => {
                    if request["action"]["op"] != "set" {
                        return Err(PluginError::new(
                            "BAD_REQUEST",
                            "Unsupported settings action",
                        ));
                    }
                    let enabled = request["action"]["enabled"]
                        .as_bool()
                        .ok_or_else(|| PluginError::new("BAD_REQUEST", "Expected a boolean"))?;
                    ENABLED.set(enabled);
                    Ok(serde_json::Value::Null)
                }
                Some("get") => Ok(json!(ENABLED.get())),
                Some("shutdown") => Ok(serde_json::Value::Null),
                _ => Err(PluginError::new(
                    "UNSUPPORTED_OPERATION",
                    "Unknown example operation",
                )),
            }
        })
    }
}
static API: PluginV1 = PluginV1 {
    abi_version: ABI_VERSION,
    struct_size: size_of::<PluginV1>() as u32,
    invoke: Some(invoke),
};
#[unsafe(no_mangle)]
pub extern "C" fn luciddesk_plugin_v1(version: u32) -> *const PluginV1 {
    if version == ABI_VERSION {
        &API
    } else {
        std::ptr::null()
    }
}
