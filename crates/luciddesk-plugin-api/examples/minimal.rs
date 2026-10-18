//! Minimal loadable plugin; does not register a UI or start any background work.
use luciddesk_plugin_api::*;
use serde_json::json;
use std::ffi::c_void;

unsafe extern "C" fn invoke(
    _context: *mut c_void,
    data: *const u8,
    len: usize,
    reply: Reply,
    user: *mut c_void,
) -> i32 {
    unsafe {
        serve_json(data, len, reply, user, |request| {
            match request["op"].as_str() {
                Some("manifest") => Ok(json!(Manifest {
                    id: "example.echo".into(),
                    name: "Echo example".into(),
                    version: "0.1.0".into(),
                    protocol_version: PROTOCOL_VERSION,
                    capabilities: vec!["example.echo.v1".into()],
                })),
                Some("echo") => Ok(request["value"].clone()),
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
