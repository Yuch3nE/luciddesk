//! Local, current-user/session-only pipes with bounded overlapped I/O.
use crate::{MAX_FRAME, Request, Response};
use std::{
    io, mem, ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::Authorization::*,
    Security::*,
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*, RemoteDesktop::*, Threading::*},
};

struct Handle(HANDLE);
// Owned kernel handles may be transferred to the worker; only that worker uses the pipe.
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn owned(h: HANDLE) -> io::Result<Handle> {
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(Handle(h))
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn identity(pid: u32) -> io::Result<(String, u32)> {
    unsafe {
        let process = owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))?;
        let mut token = ptr::null_mut();
        if OpenProcessToken(process.0, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = owned(token)?;
        let mut length = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut length);
        let mut buffer = vec![0usize; (length as usize).div_ceil(mem::size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
        LocalFree(sid.cast());
        let mut session = 0;
        if ProcessIdToSessionId(pid, &mut session) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((value, session))
    }
}
pub fn endpoint() -> io::Result<String> {
    let (sid, session) = identity(std::process::id())?;
    Ok(format!(r"\\.\pipe\LucidDesk.Control.v1.{sid}.{session}"))
}
fn verify_peer(pipe: HANDLE, server: bool) -> io::Result<()> {
    let mut pid = 0;
    let ok = unsafe {
        if server {
            GetNamedPipeServerProcessId(pipe, &mut pid)
        } else {
            GetNamedPipeClientProcessId(pipe, &mut pid)
        }
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if identity(pid)? != identity(std::process::id())? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pipe peer is not the current user/session",
        ));
    }
    Ok(())
}

// Each client call/server worker performs I/O sequentially. A mutable borrow
// prevents overlapping operations from sharing the event. Cancellation is drained
// before the operation returns, so neither OVERLAPPED nor buffers outlive a call.
struct PipeIo { event: Handle }
impl PipeIo {
    fn new() -> io::Result<Self> {
        Ok(Self { event: owned(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })? })
    }
    /// Owns an overlapped operation until completion, including cancellation.
    fn operation(
        &mut self,
        handle: HANDLE,
        deadline: Instant,
        stop: Option<HANDLE>,
        start: impl FnOnce(*mut OVERLAPPED) -> i32,
    ) -> io::Result<u32> {
        // Previous I/O has completed (including cancellation) before reuse.
        if unsafe { ResetEvent(self.event.0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
        overlapped.hEvent = self.event.0;
        let ok = start(&mut overlapped);
        if ok != 0 {
            // Synchronous completion already owns the result: no wait is needed.
            let mut transferred = 0;
            if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(transferred);
        }
        if unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err(io::Error::last_os_error());
        }
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u32::MAX as u128 - 1) as u32;
        let handles = [self.event.0, stop.unwrap_or(self.event.0)];
        let result = unsafe {
            WaitForMultipleObjects(
                if stop.is_some() { 2 } else { 1 },
                handles.as_ptr(),
                0,
                wait,
            )
        };
        let mut transferred = 0;
        if result != WAIT_OBJECT_0 {
            unsafe {
                CancelIoEx(handle, &overlapped);
                // Buffers and OVERLAPPED must remain alive until cancellation completes.
                GetOverlappedResult(handle, &overlapped, &mut transferred, 1);
            }
            return Err(io::Error::new(
                if result == WAIT_TIMEOUT {
                    io::ErrorKind::TimedOut
                } else {
                    io::ErrorKind::Interrupted
                },
                "pipe request interrupted or timed out",
            ));
        }
        if unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred)
    }
    fn read_exact(
        &mut self,
        h: HANDLE,
        mut bytes: &mut [u8],
        deadline: Instant,
        stop: Option<HANDLE>,
    ) -> io::Result<()> {
        while !bytes.is_empty() {
            let n = self.operation(h, deadline, stop, |ov| unsafe {
                ReadFile(
                    h,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    ptr::null_mut(),
                    ov,
                )
            })? as usize;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            bytes = &mut bytes[n..];
        }
        Ok(())
    }
    fn write_all(
        &mut self,
        h: HANDLE,
        mut bytes: &[u8],
        deadline: Instant,
        stop: Option<HANDLE>,
    ) -> io::Result<()> {
        while !bytes.is_empty() {
            let n = self.operation(h, deadline, stop, |ov| unsafe {
                WriteFile(h, bytes.as_ptr(), bytes.len() as u32, ptr::null_mut(), ov)
            })? as usize;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }
    fn read_frame(&mut self, h: HANDLE, deadline: Instant, stop: Option<HANDLE>) -> io::Result<Vec<u8>> {
        self.read_frame_with_deadlines(h, deadline, deadline, stop)
    }
    fn read_frame_with_deadlines(&mut self, h: HANDLE, header_deadline: Instant, deadline: Instant, stop: Option<HANDLE>) -> io::Result<Vec<u8>> {
        let mut header = [0; 4];
        self.read_exact(h, &mut header, header_deadline, stop)?;
        let len = u32::from_le_bytes(header) as usize;
        if len == 0 || len > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds limit",
            ));
        }
        let mut bytes = vec![0; len];
        self.read_exact(h, &mut bytes, deadline, stop)?;
        Ok(bytes)
    }
}
// Serialize directly after the header to send one contiguous frame without
// copying the JSON payload or issuing a separate four-byte pipe write.
fn encode_frame(value: &impl serde::Serialize) -> io::Result<Vec<u8>> {
    let mut frame = vec![0; 4];
    serde_json::to_writer(&mut frame, value)?;
    let len = frame.len() - 4;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds limit"));
    }
    frame[..4].copy_from_slice(&(len as u32).to_le_bytes());
    Ok(frame)
}
#[cfg(test)]
fn write_all(h: HANDLE, bytes: &[u8], deadline: Instant, stop: Option<HANDLE>) -> io::Result<()> {
    PipeIo::new()?.write_all(h, bytes, deadline, stop)
}
#[cfg(test)]
fn read_exact(h: HANDLE, bytes: &mut [u8], deadline: Instant, stop: Option<HANDLE>) -> io::Result<()> {
    PipeIo::new()?.read_exact(h, bytes, deadline, stop)
}
#[cfg(test)]
fn read_frame(h: HANDLE, deadline: Instant, stop: Option<HANDLE>) -> io::Result<Vec<u8>> {
    PipeIo::new()?.read_frame(h, deadline, stop)
}
#[cfg(test)]
fn write_frame(h: HANDLE, bytes: &[u8], deadline: Instant, stop: Option<HANDLE>) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds limit",
        ));
    }
    write_all(h, &(bytes.len() as u32).to_le_bytes(), deadline, stop)?;
    write_all(h, bytes, deadline, stop)
}
pub fn call(request: &Request, timeout: Duration) -> io::Result<Response> {
    call_at(&endpoint()?, request, timeout)
}
pub fn call_at(name: &str, request: &Request, timeout: Duration) -> io::Result<Response> {
    Client::at(name).exchange(request, timeout, false)
}
fn connect(name: &str, deadline: Instant) -> io::Result<Handle> {
    let name = wide(name);
    let pipe = loop {
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            break owned(h)?;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
            return Err(error);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        unsafe {
            WaitNamedPipeW(name.as_ptr(), remaining.as_millis().min(100) as u32);
        }
    };
    verify_peer(pipe.0, true)?;
    Ok(pipe)
}

struct Connection { pipe: Handle, io: PipeIo, used: Instant }
/// Sequential, bounded reuse within one CLI invocation. Errors drop the connection
/// without replaying a request: the caller must resolve uncertain writes by receipt.
pub struct Client { name: String, connection: Option<Connection> }
impl Client {
    pub fn new() -> io::Result<Self> { Ok(Self::at(&endpoint()?)) }
    pub fn at(name: &str) -> Self { Self { name: name.into(), connection: None } }
    pub fn call(&mut self, request: &Request, timeout: Duration) -> io::Result<Response> {
        self.exchange(request, timeout, true)
    }
    fn exchange(&mut self, request: &Request, timeout: Duration, reuse: bool) -> io::Result<Response> {
        let deadline = Instant::now() + timeout;
        let mut connection = match self.connection.take().filter(|c| c.used.elapsed() < Duration::from_millis(500)) {
            Some(connection) => connection,
            None => Connection { pipe: connect(&self.name, deadline)?, io: PipeIo::new()?, used: Instant::now() },
        };
        let h = connection.pipe.0;
        connection.io.write_all(h, &encode_frame(request)?, deadline, None)?;
        let bytes = connection.io.read_frame(h, deadline, None)?;
        let mut value = crate::parse_json(&bytes)?;
        let supported = value.get("connection_reuse").and_then(serde_json::Value::as_bool) == Some(true);
        if let Some(object) = value.as_object_mut() { object.remove("connection_reuse"); }
        let response: Response = serde_json::from_value(value)?;
        if response.request_id != request.request_id || response.protocol_version != crate::VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid response identity/version"));
        }
        let retain = reuse && supported;
        connection.io.write_all(h, &[if retain { 2 } else { 1 }], deadline, None)?;
        if retain { connection.used = Instant::now(); self.connection = Some(connection); }
        Ok(response)
    }
}

#[derive(serde::Serialize)]
struct WireResponse<'a> {
    #[serde(flatten)] response: &'a Response,
    connection_reuse: bool,
}

pub struct Server {
    stop: Handle,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub fn start(handler: impl Fn(Request) -> Response + Send + 'static) -> io::Result<Self> {
        Self::start_at(&endpoint()?, handler)
    }
    pub fn start_at(
        name: &str,
        handler: impl Fn(Request) -> Response + Send + 'static,
    ) -> io::Result<Self> {
        Self::start_mode(name, handler, true)
    }
    fn start_mode(
        name: &str,
        handler: impl Fn(Request) -> Response + Send + 'static,
        reuse: bool,
    ) -> io::Result<Self> {
        let (sid, _) = identity(std::process::id())?;
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(name);
        let raw = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                &attributes,
            )
        };
        let pipe = owned(raw);
        unsafe {
            LocalFree(descriptor);
        }
        let pipe = pipe?;
        let stop = owned(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
        let stop_raw = stop.0 as usize;
        let mut io = PipeIo::new()?;
        let thread = std::thread::Builder::new()
            .name("luciddesk-control".into())
            .spawn(move || {
                let pipe = pipe;
                let stop = stop_raw as HANDLE;
                while unsafe { WaitForSingleObject(stop, 0) } != WAIT_OBJECT_0 {
                    let connected = io.operation(
                        pipe.0,
                        Instant::now() + Duration::from_secs(86400),
                        Some(stop),
                        |ov| unsafe { ConnectNamedPipe(pipe.0, ov) },
                    );
                    if let Err(error) = connected {
                        if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                            if error.kind() == io::ErrorKind::Interrupted {
                                break;
                            }
                            continue;
                        }
                    }
                    let deadline = Instant::now() + Duration::from_secs(10);
                    let _ = (|| -> io::Result<()> {
                        verify_peer(pipe.0, false)?;
                        for index in 0..8 {
                            // Legacy ACK=1 closes immediately. ACK=2 is accepted only
                            // after advertising support; bound both idle time and turns.
                            let deadline = if index == 0 { deadline } else { Instant::now() + Duration::from_secs(10) };
                            let header_deadline = if index == 0 { deadline } else { Instant::now() + Duration::from_secs(1) };
                            let bytes = io.read_frame_with_deadlines(pipe.0, header_deadline, deadline, Some(stop))?;
                            let response = match crate::decode_json::<Request>(&bytes) {
                                Ok(request) => handler(request),
                                Err(error) => {
                                    Response::failure("", "INVALID_REQUEST", error.to_string())
                                }
                            };
                            let reusable = reuse && index < 7;
                            let encoded = if reuse {
                                encode_frame(&WireResponse { response: &response, connection_reuse: reusable })
                            } else { encode_frame(&response) };
                            let bytes = match encoded {
                                Ok(bytes) => bytes,
                                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                                    encode_frame(&Response::failure(
                                        &response.request_id,
                                        "RESULT_TOO_LARGE",
                                        "response exceeds 4 MiB",
                                    ))?
                                }
                                Err(error) => return Err(error),
                            };
                            io.write_all(pipe.0, &bytes, deadline, Some(stop))?;
                            let mut ack = [0];
                            io.read_exact(pipe.0, &mut ack, deadline, Some(stop))?;
                            if ack[0] != 2 || !reusable { break; }
                        }
                        Ok(())
                    })();
                    unsafe {
                        DisconnectNamedPipe(pipe.0);
                    }
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        unsafe {
            SetEvent(self.stop.0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "manual local IPC timing; no production endpoint"]
    fn ipc_timing_probe() {
        fn measure(label: &str, mut action: impl FnMut()) {
            for _ in 0..30 { action(); }
            let mut samples = Vec::new();
            for _ in 0..500 {
                let start = Instant::now(); action(); samples.push(start.elapsed().as_nanos());
            }
            samples.sort_unstable();
            println!("{label}: median={}ns p95={}ns", samples[250], samples[475]);
        }
        let request: Request = serde_json::from_value(serde_json::json!({
            "protocol_version": 1, "request_id": "timing", "command": "status"
        })).unwrap();
        measure("identity", || { std::hint::black_box(identity(std::process::id()).unwrap()); });
        measure("serialize", || { std::hint::black_box(encode_frame(&request).unwrap()); });
        let bytes = serde_json::to_vec(&request).unwrap();
        measure("validate_decode", || {
            crate::validate_json(&bytes).unwrap();
            std::hint::black_box(serde_json::from_slice::<Request>(&bytes).unwrap());
        });
        measure("single_parse_decode", || { std::hint::black_box(crate::decode_json::<Request>(&bytes).unwrap()); });
        let name = format!("{}-timing-{}", endpoint().unwrap(), crate::request_id());
        let _server = Server::start_at(&name, |r| Response::success(
            &r.request_id, serde_json::json!({}), serde_json::json!({"ready":true}),
        )).unwrap();
        measure("pipe_round_trip", || {
            assert!(call_at(&name, &request, Duration::from_secs(2)).unwrap().ok);
        });
        let mut client = Client::at(&name);
        measure("reused_round_trip", || { assert!(client.call(&request, Duration::from_secs(2)).unwrap().ok); });
    }
    #[test]
    fn reusable_client_negotiates_and_rotates_without_replaying() {
        for reuse in [false, true] {
            let name = format!("{}-session-{}", endpoint().unwrap(), crate::request_id());
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed = calls.clone();
            let _server = Server::start_mode(&name, move |r| {
                let n = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({"sequence":n}))
            }, reuse).unwrap();
            let mut client = Client::at(&name);
            for n in 0..18 {
                let request: Request = serde_json::from_value(serde_json::json!({
                    "protocol_version":1,"request_id":format!("request-{n}"),"command":"status"
                })).unwrap();
                let response = client.call(&request, Duration::from_secs(2)).unwrap();
                assert_eq!(response.data.unwrap()["sequence"], n);
                assert_eq!(client.connection.is_some(), reuse && (n + 1) % 8 != 0);
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 18);
        }
    }
    #[test]
    fn idle_session_releases_server_and_shutdown_cancels_idle_read() {
        let name = format!("{}-idle-{}", endpoint().unwrap(), crate::request_id());
        let server = Server::start_at(&name, |r| Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({}))).unwrap();
        let request: Request = serde_json::from_value(serde_json::json!({"protocol_version":1,"request_id":"idle","command":"status"})).unwrap();
        let mut client = Client::at(&name);
        client.call(&request, Duration::from_secs(2)).unwrap();
        // Another client eventually connects even if the first does not release its handle.
        assert!(call_at(&name, &request, Duration::from_secs(3)).unwrap().ok);
        // Expired cached connection reconnects before sending anything.
        assert!(client.call(&request, Duration::from_secs(2)).unwrap().ok);
        let start = Instant::now();
        drop(server);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(client.call(&request, Duration::from_millis(100)).is_err());
        assert!(client.connection.is_none());
    }
    #[test]
    fn encoded_frames_preserve_wire_format_and_limit() {
        for text in ["中文", "", &"x".repeat(MAX_FRAME - 2)] {
            let frame = encode_frame(&text).unwrap();
            let payload = serde_json::to_vec(text).unwrap();
            assert_eq!(&frame[..4], &(payload.len() as u32).to_le_bytes());
            assert_eq!(&frame[4..], payload);
        }
        assert_eq!(encode_frame(&"x".repeat(MAX_FRAME - 1)).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
    #[test]
    fn large_response_crosses_pipe_buffer_without_truncation() {
        let name = format!("{}-large-{}", endpoint().unwrap(), crate::request_id());
        let title = "中文".repeat(100_000);
        let expected = title.clone();
        let _server = Server::start_at(&name, move |r| Response::success(
            &r.request_id, serde_json::json!({}), serde_json::json!({"title": title}),
        )).unwrap();
        let request: Request = serde_json::from_value(serde_json::json!({
            "protocol_version": 1, "request_id": crate::request_id(), "command": "status"
        })).unwrap();
        let response = call_at(&name, &request, Duration::from_secs(5)).unwrap();
        assert_eq!(response.data.unwrap()["title"], expected);
    }
    #[test]
    fn pipe_round_trip_repeated_and_exclusive() {
        let name = format!("{}-test-{}", endpoint().unwrap(), crate::request_id());
        let server = Server::start_at(&name, |r| {
            Response::success(
                &r.request_id,
                serde_json::json!({}),
                serde_json::json!({"title":"中文"}),
            )
        })
        .unwrap();
        assert!(Server::start_at(&name, |_| unreachable!()).is_err());
        for _ in 0..3 {
            let request = Request {
                protocol_version: 1,
                request_id: crate::request_id(),
                command: "status".into(),
                id: None,
                pane: None,
                unassigned: false,
                data_dir: None,
                plan: None,
                token: None,
            };
            let response = call_at(&name, &request, Duration::from_secs(2)).unwrap();
            assert_eq!(response.data.unwrap()["title"], "中文");
        }
        drop(server);
        let server = Server::start_at(&name, |_| unreachable!()).unwrap();
        let start = Instant::now();
        drop(server);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[cfg(test)]
mod fault_tests {
    use super::*;
    fn request() -> Request {
        Request {
            protocol_version: 1,
            request_id: crate::request_id(),
            command: "status".into(),
            id: None,
            pane: None,
            unassigned: false,
            data_dir: None,
            plan: None,
            token: None,
        }
    }
    #[test]
    fn missing_server_does_not_create_anything() {
        let name = format!("{}-absent-{}", endpoint().unwrap(), crate::request_id());
        assert_eq!(
            call_at(&name, &request(), Duration::from_millis(100))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
    #[test]
    fn session_timeout_never_replays_dispatched_request() {
        let name = format!("{}-no-replay-{}", endpoint().unwrap(), crate::request_id());
        let (resume, wait) = std::sync::mpsc::channel();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let server = Server::start_at(&name, move |r| {
            if observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({}))
        }).unwrap();
        let mut client = Client::at(&name);
        client.call(&request(), Duration::from_secs(2)).unwrap();
        assert_eq!(client.call(&request(), Duration::from_millis(50)).unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(client.connection.is_none());
        resume.send(()).unwrap();
        client.call(&request(), Duration::from_secs(2)).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
        drop(client);
        drop(server);
    }
    #[test]
    fn event_is_reusable_after_cancelled_read() {
        let name = format!("{}-reuse-{}", endpoint().unwrap(), crate::request_id());
        let (resume, wait) = std::sync::mpsc::channel();
        let _server = Server::start_at(&name, move |r| {
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({"resumed":true}))
        }).unwrap();
        let pipe = owned(unsafe {
            CreateFileW(wide(&name).as_ptr(), GENERIC_READ | GENERIC_WRITE, 0,
                ptr::null(), OPEN_EXISTING, FILE_FLAG_OVERLAPPED, ptr::null_mut())
        }).unwrap();
        let mut io = PipeIo::new().unwrap();
        io.write_all(pipe.0, &encode_frame(&request()).unwrap(), Instant::now() + Duration::from_secs(2), None).unwrap();
        assert_eq!(io.read_frame(pipe.0, Instant::now() + Duration::from_millis(20), None).unwrap_err().kind(), io::ErrorKind::TimedOut);
        resume.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let response: Response = serde_json::from_slice(&io.read_frame(pipe.0, deadline, None).unwrap()).unwrap();
        assert_eq!(response.data.unwrap()["resumed"], true);
        io.write_all(pipe.0, &[1], deadline, None).unwrap();
    }
    #[test]
    fn read_timeout_cancels_pending_io() {
        let name = format!("{}-slow-{}", endpoint().unwrap(), crate::request_id());
        let server = Server::start_at(&name, |r| {
            std::thread::sleep(Duration::from_millis(150));
            Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({}))
        })
        .unwrap();
        let start = Instant::now();
        assert_eq!(
            call_at(&name, &request(), Duration::from_millis(25))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(server);
    }
    #[test]
    fn disconnect_after_dispatch_does_not_cancel_handler_or_break_next_connection() {
        let name = format!("{}-disconnect-{}", endpoint().unwrap(), crate::request_id());
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = completed.clone();
        let server = Server::start_at(&name, move |r| {
            if observed.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                started_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({}))
        }).unwrap();
        let pipe = owned(unsafe {
            CreateFileW(wide(&name).as_ptr(), GENERIC_READ | GENERIC_WRITE, 0,
                ptr::null(), OPEN_EXISTING, FILE_FLAG_OVERLAPPED, ptr::null_mut())
        }).unwrap();
        write_frame(pipe.0, &serde_json::to_vec(&request()).unwrap(),
            Instant::now() + Duration::from_secs(2), None).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(pipe);
        resume_tx.send(()).unwrap();
        assert!(call_at(&name, &request(), Duration::from_secs(2)).unwrap().ok);
        assert_eq!(completed.load(std::sync::atomic::Ordering::SeqCst), 2);
        drop(server);
    }
    #[test]
    fn duplicate_nested_keys_never_reach_handler_and_server_recovers() {
        let name=format!("{}-duplicates-{}",endpoint().unwrap(),crate::request_id());
        let calls=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed=calls.clone();
        let server=Server::start_at(&name,move |r| {
            observed.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
            Response::success(&r.request_id,serde_json::json!({}),serde_json::json!({}))
        }).unwrap();
        let pipe=owned(unsafe {CreateFileW(wide(&name).as_ptr(),GENERIC_READ|GENERIC_WRITE,0,std::ptr::null(),OPEN_EXISTING,FILE_FLAG_OVERLAPPED,std::ptr::null_mut())}).unwrap();
        let bytes=br#"{"protocol_version":1,"request_id":"duplicate-test","command":"plan.preview","plan":{"protocol_version":1,"base":{"instance_id":"i","state_version":"1","inventory_version":"1","topology_token":"1"},"operations":[{"op":"settings.update","values":{"search.enabled":true,"search.enabled":false}}]}}"#;
        let deadline=Instant::now()+Duration::from_secs(2);
        write_frame(pipe.0,bytes,deadline,None).unwrap();
        let response:Response=serde_json::from_slice(&read_frame(pipe.0,deadline,None).unwrap()).unwrap();
        assert_eq!(response.error.unwrap().code,"INVALID_REQUEST");
        write_all(pipe.0,&[1],deadline,None).unwrap();
        drop(pipe);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed),0);
        assert!(call_at(&name,&request(),Duration::from_secs(2)).unwrap().ok);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed),1);
        drop(server);
    }
    #[test]
    fn oversized_frame_is_rejected_before_allocation_and_server_recovers() {
        let name = format!("{}-frame-{}", endpoint().unwrap(), crate::request_id());
        let server = Server::start_at(&name, |r| {
            Response::success(&r.request_id, serde_json::json!({}), serde_json::json!({}))
        })
        .unwrap();
        let pipe = owned(unsafe {
            CreateFileW(
                wide(&name).as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        })
        .unwrap();
        write_all(
            pipe.0,
            &((MAX_FRAME + 1) as u32).to_le_bytes(),
            Instant::now() + Duration::from_secs(1),
            None,
        )
        .unwrap();
        let mut byte = [0];
        assert!(
            read_exact(
                pipe.0,
                &mut byte,
                Instant::now() + Duration::from_secs(1),
                None
            )
            .is_err()
        );
        drop(pipe);
        assert!(
            call_at(&name, &request(), Duration::from_secs(2))
                .unwrap()
                .ok
        );
        drop(server);
    }
}
