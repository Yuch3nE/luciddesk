//! Optional native system-monitor plugin. No worker is started by loading the DLL.
mod options;
mod config;
use options::{Metric, Preferences};
mod drawing;
mod gpu;
mod counters;
mod graphics;
mod i18n;
mod sampling;
mod settings;
use luciddesk_plugin_api::{
    ABI_VERSION, Manifest, PROTOCOL_VERSION, PluginError, PluginV1, Reply, serve_json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, ffi::c_void};

thread_local! {
    static INVENTORY: RefCell<Option<[Vec<(String, String)>; 3]>> = const { RefCell::new(None) };
    static CLIENTS: RefCell<HashMap<u64, sampling::Client>> = RefCell::new(HashMap::new());
    static DRAWINGS: RefCell<HashMap<u64, drawing::Drawing>> = RefCell::new(HashMap::new());
    static FONT: RefCell<String> = RefCell::new("Segoe UI".into());
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}
fn font() -> String {
    FONT.with(|f| f.borrow().clone())
}
fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}
fn devices(metric: Metric) -> Vec<(String, String)> {
    let index = match metric { Metric::Network => 0, Metric::Disk => 1, Metric::Gpu => 2, _ => return Vec::new() };
    if let Some(devices) = INVENTORY.with(|v| v.borrow().as_ref().map(|all| all[index].clone())) { return devices; }
    let s = sampling::snapshot();
    let values = match metric {
        Metric::Network => &s.networks,
        Metric::Disk => &s.disks,
        Metric::Gpu => &s.gpus,
        _ => return Vec::new(),
    };
    values
        .iter()
        .map(|d| (d.id.clone(), d.name.clone()))
        .collect()
}
#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    language: String,
    #[serde(default)]
    font: String,
    #[serde(flatten)]
    command: Command,
}

// Each operation owns only its payload; lifecycle calls do not allocate preferences.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Manifest,
    ConfigOpen { directory: std::path::PathBuf },
    ConfigGet,
    SaveOptions { options: Preferences },
    SaveWindow { window: config::Window },
    Settings,
    Devices,
    Inventory { devices: [Vec<(String, String)>; 3] },
    ClientNew,
    ClientDrop {
        #[serde(default)]
        id: u64,
    },
    Tick {
        #[serde(default)]
        id: u64,
        #[serde(default)]
        visible: bool,
        #[serde(default)]
        options: Preferences,
    },
    DrawingNew,
    DrawingDrop {
        #[serde(default)]
        id: u64,
    },
    Paint {
        #[serde(default)]
        id: u64,
        #[serde(default)]
        options: Preferences,
        #[serde(default)]
        dark: bool,
        #[serde(default)]
        ink: f32,
        #[serde(default)]
        bounds: [f32; 4],
    },
    Shutdown,
    #[cfg(feature = "test-diagnostics")]
    Diagnostics,
    #[serde(other)]
    Unknown,
}
fn dispatch(context: *mut c_void, r: Request) -> Result<Value, String> {
    if !r.language.is_empty() {
        i18n::set(&r.language);
    }
    if !r.font.is_empty() {
        FONT.with(|f| *f.borrow_mut() = r.font);
    }
    match r.command {
        Command::Manifest => serde_json::to_value(Manifest {
            id: "system-monitor".into(),
            name: i18n::text("monitor-title").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: vec!["system-monitor.v1".into(), "config.v1".into()],
        })
        .map_err(|e| e.to_string()),
        Command::ConfigOpen { directory } => { config::open(directory)?; Ok(Value::Null) },
        Command::ConfigGet => serde_json::to_value(config::get()).map_err(|e| e.to_string()),
        Command::SaveOptions { options } => { config::options(options)?; Ok(Value::Null) },
        Command::SaveWindow { window } => { config::window(window)?; Ok(Value::Null) },
        Command::Settings => serde_json::to_value(settings::draw(&config::get().options)).map_err(|e| e.to_string()),
        Command::Inventory { devices } => { INVENTORY.with(|v| *v.borrow_mut() = Some(devices)); Ok(Value::Null) },
        Command::Devices => Ok(json!([
            devices(Metric::Network),
            devices(Metric::Disk),
            devices(Metric::Gpu)
        ])),
        Command::ClientNew => {
            let id = next_id();
            let c = sampling::Client::new()?;
            CLIENTS.with(|m| m.borrow_mut().insert(id, c));
            Ok(json!(id))
        }
        Command::ClientDrop { id } => {
            CLIENTS.with(|m| m.borrow_mut().remove(&id));
            Ok(Value::Null)
        }
        Command::Tick {
            id,
            visible,
            options,
        } => CLIENTS.with(|m| {
            m.borrow_mut()
                .get_mut(&id)
                .map(|c| json!(c.tick(visible, &options)))
                .ok_or("Unknown client".into())
        }),
        Command::DrawingNew => {
            let id = next_id();
            let d = drawing::Drawing::new().map_err(|e| e.to_string())?;
            DRAWINGS.with(|m| m.borrow_mut().insert(id, d));
            Ok(json!(id))
        }
        Command::DrawingDrop { id } => {
            DRAWINGS.with(|m| m.borrow_mut().remove(&id));
            Ok(Value::Null)
        }
        Command::Paint {
            id,
            options,
            dark,
            ink,
            bounds,
        } => {
            use canvas_core::Interface as _;
            use windows::core::Interface;
            if context.is_null() {
                return Err("Missing paint context".into());
            }
            // Both bindings borrow the same COM interface. Neither takes host ownership.
            let native = unsafe {
                windows::Win32::Graphics::Direct2D::ID2D1DeviceContext::from_raw_borrowed(&context)
                    .unwrap()
            };
            let canvas =
                unsafe { windows_canvas::ID2D1DeviceContext::from_raw_borrowed(&context).unwrap() };
            let target = graphics::DrawPass::new(canvas, native);
            let text = target
                .create_solid_brush(windows_canvas::ColorF::new(ink, ink, ink, 1.0))
                .map_err(|e| e.to_string())?;
            let dim = target
                .create_solid_brush(windows_canvas::ColorF::new(ink, ink, ink, 0.85))
                .map_err(|e| e.to_string())?;
            DRAWINGS.with(|m| {
                m.borrow_mut()
                    .get_mut(&id)
                    .ok_or("Unknown drawing".to_string())?
                    .paint(
                        &target,
                        &options,
                        windows_canvas::Rect::from_xywh(bounds[0], bounds[1], bounds[2], bounds[3]),
                        &text,
                        &dim,
                        dark,
                        ink,
                    )
                    .map_err(|e| e.to_string())
            })?;
            Ok(Value::Null)
        }
        Command::Shutdown => {
            CLIENTS.with(|m| m.borrow_mut().clear());
            DRAWINGS.with(|m| m.borrow_mut().clear());
            Ok(Value::Null)
        }
        #[cfg(feature = "test-diagnostics")]
        Command::Diagnostics => Ok(json!({
            "clients": CLIENTS.with(|m| m.borrow().len()),
            "drawings": DRAWINGS.with(|m| m.borrow().len()),
            "service": sampling::service_alive(),
            "frames": sampling::snapshot().history.len(),
        })),
        Command::Unknown => Err("Unknown plugin operation".into()),
    }
}
unsafe extern "C" fn invoke(
    context: *mut c_void,
    data: *const u8,
    len: usize,
    reply: Reply,
    user: *mut c_void,
) -> i32 {
    unsafe {
        serve_json(data, len, reply, user, |value| {
            let request = serde_json::from_value::<Request>(value)
                .map_err(|e| PluginError::new("BAD_REQUEST", e.to_string()))?;
            dispatch(context, request).map_err(|e| PluginError::new("PLUGIN_ERROR", e))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_rejects_other_versions_and_returns_errors_without_unwinding() {
        assert!(luciddesk_plugin_v1(ABI_VERSION + 1).is_null());
        let api = unsafe { &*luciddesk_plugin_v1(ABI_VERSION) };
        assert_eq!(api.struct_size as usize, size_of::<PluginV1>());
        unsafe extern "C" fn receive(user: *mut c_void, bytes: *const u8, len: usize) {
            unsafe {
                (&mut *user.cast::<Vec<u8>>())
                    .extend_from_slice(std::slice::from_raw_parts(bytes, len));
            }
        }
        for input in [
            b"not JSON".as_slice(),
            br#"{"op":"unknown"}"#,
            br#"{"op":"paint"}"#,
        ] {
            let mut output = Vec::<u8>::new();
            assert_eq!(
                unsafe {
                    (api.invoke.unwrap())(
                        std::ptr::null_mut(),
                        input.as_ptr(),
                        input.len(),
                        receive,
                        (&raw mut output).cast(),
                    )
                },
                -1
            );
            assert!(
                serde_json::from_slice::<Value>(&output)
                    .unwrap()
                    .get("code")
                    .is_some()
            );
        }
    }
}
