//! Versioned in-process C ABI. Buffers are borrowed only for the duration of a call.
//! Calls, including the response callback, run synchronously on the host UI thread.
use serde::{Deserialize, Serialize};
use std::ffi::c_void;

pub const ABI_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;
pub const STATUS_OK: i32 = 0;
pub const STATUS_ERROR: i32 = -1;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
pub const ENTRY_NAME: &std::ffi::CStr = c"luciddesk_plugin_v1";
pub type Reply = unsafe extern "C" fn(*mut c_void, *const u8, usize);
/// `context` is a borrowed ID2D1DeviceContext for paint, null for other operations.
/// The plugin must never retain the context or input/response buffer pointers.
pub type Invoke = unsafe extern "C" fn(*mut c_void, *const u8, usize, Reply, *mut c_void) -> i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginHeader {
    pub abi_version: u32,
    pub struct_size: u32,
}
#[repr(C)]
pub struct PluginV1 {
    pub abi_version: u32,
    pub struct_size: u32,
    pub invoke: Option<Invoke>,
}
pub type Entry = unsafe extern "C" fn(u32) -> *const PluginV1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol_version: u32,
    pub capabilities: Vec<String>,
}
impl Manifest {
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(PluginError::new(
                "INCOMPATIBLE_PROTOCOL",
                "Unsupported JSON protocol version",
            ));
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self.id.as_bytes()[0].is_ascii_lowercase()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
            || self.name.trim().is_empty()
            || self.version.trim().is_empty()
            || self.capabilities.iter().any(|c| c.is_empty())
        {
            return Err(PluginError::new(
                "INVALID_MANIFEST",
                "Invalid identity, display name, version or capability",
            ));
        }
        Ok(())
    }
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginError {
    pub code: String,
    pub message: String,
}
impl PluginError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for PluginError {}

/// Validate only the fixed header before reading the complete function table.
/// # Safety
/// A non-null pointer must reference at least a readable PluginHeader; if its size
/// promises PluginV1, that complete table must be readable and live until process exit.
pub unsafe fn read_entry(api: *const PluginV1) -> Result<Invoke, PluginError> {
    if api.is_null() {
        return Err(PluginError::new(
            "INCOMPATIBLE_ABI",
            "Entry rejected the host ABI",
        ));
    }
    let header = unsafe { std::ptr::read(api.cast::<PluginHeader>()) };
    if header.abi_version != ABI_VERSION || header.struct_size < size_of::<PluginV1>() as u32 {
        return Err(PluginError::new(
            "INCOMPATIBLE_ABI",
            "Unsupported or truncated function table",
        ));
    }
    unsafe { (*api).invoke }
        .ok_or_else(|| PluginError::new("INVALID_ABI", "Missing invoke function"))
}

/// Implement a plugin's synchronous JSON entry without unwinding through C.
/// # Safety
/// Input must be readable for len bytes, and reply/user must be a valid host
/// callback pair. The handler must not retain borrowed host resources.
pub unsafe fn serve_json(
    data: *const u8,
    len: usize,
    reply: Reply,
    user: *mut c_void,
    handle: impl FnOnce(serde_json::Value) -> Result<serde_json::Value, PluginError>,
) -> i32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if data.is_null() || len > MAX_MESSAGE_BYTES {
            return Err(PluginError::new("BAD_REQUEST", "Null or oversized input"));
        }
        let request: serde_json::Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(data, len) })
                .map_err(|e| PluginError::new("BAD_REQUEST", e.to_string()))?;
        if !request
            .get("op")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|op| !op.is_empty())
        {
            return Err(PluginError::new(
                "BAD_REQUEST",
                "Request requires a nonempty string op",
            ));
        }
        handle(request)
    }))
    .unwrap_or_else(|_| Err(PluginError::new("PANIC", "Plugin panicked")));
    let (mut status, mut bytes) = match result {
        Ok(value) => (STATUS_OK, serde_json::to_vec(&value).unwrap()),
        Err(error) => (STATUS_ERROR, serde_json::to_vec(&error).unwrap()),
    };
    if bytes.len() > MAX_MESSAGE_BYTES {
        status = STATUS_ERROR;
        bytes = serde_json::to_vec(&PluginError::new(
            "RESPONSE_TOO_LARGE",
            "Response exceeds protocol limit",
        ))
        .unwrap();
    }
    unsafe {
        reply(user, bytes.as_ptr(), bytes.len());
    }
    status
}

/// Call a trusted in-process plugin and parse its single synchronous response.
/// # Safety
/// invoke must remain loaded. context must be null or a valid borrowed paint
/// interface, and the plugin must obey the callback/pointer lifetime contract.
pub unsafe fn call_json(
    invoke: Invoke,
    context: *mut c_void,
    request: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let bytes =
        serde_json::to_vec(request).map_err(|e| PluginError::new("BAD_REQUEST", e.to_string()))?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(PluginError::new(
            "BAD_REQUEST",
            "Request exceeds protocol limit",
        ));
    }
    #[derive(Default)]
    struct Response {
        value: Option<Result<serde_json::Value, serde_json::Error>>,
        calls: usize,
        invalid: bool,
    }
    unsafe extern "C" fn receive(user: *mut c_void, data: *const u8, len: usize) {
        let result = unsafe { &mut *user.cast::<Response>() };
        result.calls += 1;
        if result.calls != 1 || data.is_null() || len > MAX_MESSAGE_BYTES {
            result.invalid = true;
            return;
        }
        // Parse while the plugin buffer is borrowed; retain only owned JSON values.
        result.value = Some(serde_json::from_slice(unsafe {
            std::slice::from_raw_parts(data, len)
        }));
    }
    let mut result = Response::default();
    let status = unsafe {
        invoke(
            context,
            bytes.as_ptr(),
            bytes.len(),
            receive,
            (&raw mut result).cast(),
        )
    };
    if result.invalid || result.calls != 1 {
        return Err(PluginError::new(
            "INVALID_RESPONSE",
            "Expected one bounded synchronous reply",
        ));
    }
    let value = result
        .value
        .unwrap()
        .map_err(|e| PluginError::new("INVALID_RESPONSE", e.to_string()))?;
    match status {
        STATUS_OK => Ok(value),
        STATUS_ERROR => Err(serde_json::from_value::<PluginError>(value)
            .map_err(|e| PluginError::new("INVALID_RESPONSE", e.to_string()))?),
        _ => Err(PluginError::new(
            "INVALID_RESPONSE",
            "Unknown return status",
        )),
    }
}

pub const SETTINGS_PAGE_CAPABILITY: &str = "settings-page.v1";

#[derive(Serialize, Deserialize)]
pub struct Page {
    pub description: String,
    pub items: Vec<Item>,
}
#[derive(Serialize, Deserialize)]
pub enum Item {
    Section(String),
    Toggle {
        title: String,
        description: String,
        selected: bool,
        action: serde_json::Value,
    },
    Toggles {
        title: String,
        values: Vec<(String, serde_json::Value, bool)>,
    },
    Choices {
        title: String,
        values: Vec<(String, serde_json::Value, bool)>,
    },
    Button {
        title: String,
        label: String,
        action: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    unsafe extern "C" fn echo(
        _: *mut c_void,
        d: *const u8,
        n: usize,
        r: Reply,
        u: *mut c_void,
    ) -> i32 {
        unsafe { serve_json(d, n, r, u, |v| Ok(v)) }
    }
    #[test]
    fn header_is_checked_before_reading_a_truncated_or_null_function_table() {
        let header = PluginHeader {
            abi_version: ABI_VERSION,
            struct_size: size_of::<PluginHeader>() as u32,
        };
        assert!(unsafe { read_entry((&raw const header).cast()) }.is_err());
        let table = PluginV1 {
            abi_version: ABI_VERSION,
            struct_size: size_of::<PluginV1>() as u32,
            invoke: None,
        };
        assert!(unsafe { read_entry(&table) }.is_err());
        assert!(unsafe { read_entry(std::ptr::null()) }.is_err());
    }
    #[test]
    fn manifest_identity_and_protocol_are_validated() {
        let mut m = Manifest {
            id: "example.echo".into(),
            name: "Echo".into(),
            version: "0.1.0".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: vec!["example.echo.v1".into()],
        };
        m.validate().unwrap();
        assert!(m.supports("example.echo.v1"));
        assert!(!m.supports("system-monitor.v1"));
        for id in ["", "../bad", "UPPER", "9name"] {
            m.id = id.into();
            assert!(m.validate().is_err());
        }
        m.id = "echo".into();
        m.protocol_version += 1;
        assert!(m.validate().is_err());
    }
    #[test]
    fn valid_json_roundtrips_through_the_real_callback_boundary() {
        let request = json!({"op":"echo","value":"中文"});
        assert_eq!(
            unsafe { call_json(echo, std::ptr::null_mut(), &request) }.unwrap(),
            request
        );
    }
    #[test]
    fn response_owns_strings_after_plugin_reuses_its_buffer() {
        unsafe extern "C" fn reuse(
            _: *mut c_void,
            _: *const u8,
            _: usize,
            reply: Reply,
            user: *mut c_void,
        ) -> i32 {
            let mut bytes = br#"{"text":"temporary buffer","items":[1,2,3]}"#.to_vec();
            unsafe {
                reply(user, bytes.as_ptr(), bytes.len());
            }
            bytes.fill(b' ');
            STATUS_OK
        }
        let response =
            unsafe { call_json(reuse, std::ptr::null_mut(), &json!({"op":"test"})) }.unwrap();
        assert_eq!(response, json!({"text":"temporary buffer","items":[1,2,3]}));
    }
    #[test]
    fn callback_count_and_status_are_not_silently_accepted() {
        unsafe extern "C" fn missing(
            _: *mut c_void,
            _: *const u8,
            _: usize,
            _: Reply,
            _: *mut c_void,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn double(
            _: *mut c_void,
            _: *const u8,
            _: usize,
            r: Reply,
            u: *mut c_void,
        ) -> i32 {
            unsafe {
                r(u, b"null".as_ptr(), 4);
                r(u, b"null".as_ptr(), 4);
            }
            0
        }
        unsafe extern "C" fn bad_status(
            _: *mut c_void,
            _: *const u8,
            _: usize,
            r: Reply,
            u: *mut c_void,
        ) -> i32 {
            unsafe {
                r(u, b"null".as_ptr(), 4);
            }
            7
        }
        for function in [missing as Invoke, double, bad_status] {
            assert_eq!(
                unsafe { call_json(function, std::ptr::null_mut(), &json!({"op":"test"})) }
                    .unwrap_err()
                    .code,
                "INVALID_RESPONSE"
            );
        }
    }
    #[test]
    fn errors_and_panics_return_structured_errors() {
        unsafe extern "C" fn fails(
            _: *mut c_void,
            d: *const u8,
            n: usize,
            r: Reply,
            u: *mut c_void,
        ) -> i32 {
            unsafe {
                serve_json(d, n, r, u, |_| {
                    Err(PluginError::new("NOT_FOUND", "Missing client"))
                })
            }
        }
        unsafe extern "C" fn panics(
            _: *mut c_void,
            d: *const u8,
            n: usize,
            r: Reply,
            u: *mut c_void,
        ) -> i32 {
            unsafe { serve_json(d, n, r, u, |_| panic!("test panic")) }
        }
        for (function, code) in [(fails as Invoke, "NOT_FOUND"), (panics as Invoke, "PANIC")] {
            assert_eq!(
                unsafe { call_json(function, std::ptr::null_mut(), &json!({"op":"test"})) }
                    .unwrap_err()
                    .code,
                code
            );
        }
    }
}
