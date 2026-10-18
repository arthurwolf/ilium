//! Shared helpers: wire codec (length-prefixed bincode layout), helper process driver,
//! fixture loader and manual reader.
//!
//! Assumptions that the manual leaves open are listed in API_GAPS.md (frame length is a
//! little-endian u32, enum tags follow the message table order, integers are fixed width).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

pub const HELPER_BIN: &str = env!("CARGO_BIN_EXE_ilium-plugin-helper");
pub const MIB: usize = 1024 * 1024;
/// Largest message of the dispatcher channel (HST-155).
pub const MAX_MESSAGE: usize = 4 * MIB;
/// Largest `terminal-view` frame (SEC-051).
pub const MAX_TERMINAL_VIEW_MESSAGE: usize = 8 * MIB;
/// Preamble of an empty component (magic, version 0x0d, layer 1).
pub const EMPTY_COMPONENT: &[u8] = b"\0asm\x0d\x00\x01\x00";

pub mod tag {
    pub const HELLO: u32 = 0;
    pub const INSTANTIATE: u32 = 1;
    pub const INSTANTIATE_FAILED: u32 = 2;
    pub const READY: u32 = 3;
    pub const CALL_EXPORT: u32 = 4;
    pub const CALL_RESULT: u32 = 5;
    pub const HOST_CALL: u32 = 6;
    pub const HOST_REPLY: u32 = 7;
    pub const CANCEL: u32 = 8;
    pub const SHUTDOWN: u32 = 9;
    pub const HEARTBEAT: u32 = 10;
    /// A tag that no message kind uses.
    pub const UNKNOWN: u32 = 0xFFFF_FFF0;
}

// ---------------------------------------------------------------- codec

#[derive(Default)]
pub struct Enc(pub Vec<u8>);

impl Enc {
    pub fn new(tag: u32) -> Enc {
        let mut enc = Enc(Vec::new());
        enc.u32(tag);
        enc
    }
    pub fn u8(mut self, value: u8) -> Enc {
        self.0.push(value);
        self
    }
    pub fn u32(&mut self, value: u32) -> &mut Enc {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    pub fn u64(mut self, value: u64) -> Enc {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
    pub fn str(mut self, value: &str) -> Enc {
        self.0
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.0.extend_from_slice(value.as_bytes());
        self
    }
    pub fn bytes(mut self, value: &[u8]) -> Enc {
        self.0
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.0.extend_from_slice(value);
        self
    }
    pub fn strs(mut self, values: &[&str]) -> Enc {
        self.0
            .extend_from_slice(&(values.len() as u64).to_le_bytes());
        for value in values {
            self = self.str(value);
        }
        self
    }
    pub fn opt_str(self, value: Option<&str>) -> Enc {
        match value {
            None => self.u8(0),
            Some(text) => self.u8(1).str(text),
        }
    }
    pub fn raw(mut self, value: u32) -> Enc {
        self.u32(value);
        self
    }
    pub fn done(self) -> Vec<u8> {
        self.0
    }
}

pub struct Dec<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Dec<'a> {
    pub fn new(buf: &'a [u8]) -> Dec<'a> {
        Dec { buf, pos: 0 }
    }
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(count)?;
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(b);
            u64::from_le_bytes(raw)
        })
    }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let len = self.u64()? as usize;
        self.take(len).map(|b| b.to_vec())
    }
    pub fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    pub fn opt_string(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.string()?)),
            _ => None,
        }
    }
}

pub fn frame_tag(frame: &[u8]) -> Option<u32> {
    Dec::new(frame).u32()
}

#[derive(Debug, Clone)]
pub struct Hello {
    pub protocol_version: u32,
    pub fingerprint: String,
}

pub fn parse_hello(frame: &[u8]) -> Option<Hello> {
    let mut dec = Dec::new(frame);
    if dec.u32()? != tag::HELLO {
        return None;
    }
    Some(Hello {
        protocol_version: dec.u32()?,
        fingerprint: dec.string()?,
    })
}

#[derive(Debug, Clone)]
pub struct CallResult {
    pub call_id: u64,
    pub result: Vec<u8>,
    pub trap: Option<String>,
}

pub fn parse_call_result(frame: &[u8]) -> Option<CallResult> {
    let mut dec = Dec::new(frame);
    if dec.u32()? != tag::CALL_RESULT {
        return None;
    }
    Some(CallResult {
        call_id: dec.u64()?,
        result: dec.bytes()?,
        trap: dec.opt_string()?,
    })
}

#[derive(Debug, Clone)]
pub struct HostCall {
    pub call_id: u64,
    pub import: String,
    pub args: Vec<u8>,
}

pub fn parse_host_call(frame: &[u8]) -> Option<HostCall> {
    let mut dec = Dec::new(frame);
    if dec.u32()? != tag::HOST_CALL {
        return None;
    }
    Some(HostCall {
        call_id: dec.u64()?,
        import: dec.string()?,
        args: dec.bytes()?,
    })
}

pub fn parse_heartbeat(frame: &[u8]) -> Option<u64> {
    let mut dec = Dec::new(frame);
    if dec.u32()? != tag::HEARTBEAT {
        return None;
    }
    dec.u64()
}

pub fn msg_instantiate(
    plugin: &str,
    fingerprint: &str,
    component: &[u8],
    restored: &[u8],
) -> Vec<u8> {
    Enc::new(tag::INSTANTIATE)
        .str(plugin)
        .str(fingerprint)
        .bytes(component)
        .strs(&[])
        .raw_u32(64)
        .bytes(restored)
        .done()
}

pub fn msg_call_export(call_id: u64, export: &str, args: &[u8], deadline_ms: u32) -> Vec<u8> {
    Enc::new(tag::CALL_EXPORT)
        .u64(call_id)
        .str(export)
        .bytes(args)
        .raw_u32(deadline_ms)
        .done()
}

pub fn msg_heartbeat(counter: u64) -> Vec<u8> {
    Enc::new(tag::HEARTBEAT).u64(counter).done()
}

pub fn msg_shutdown(reason: &str) -> Vec<u8> {
    Enc::new(tag::SHUTDOWN).str(reason).done()
}

pub fn msg_cancel(call_id: u64) -> Vec<u8> {
    Enc::new(tag::CANCEL).u64(call_id).done()
}

pub fn msg_host_reply(call_id: u64, result: &[u8], error: Option<&str>) -> Vec<u8> {
    Enc::new(tag::HOST_REPLY)
        .u64(call_id)
        .bytes(result)
        .opt_str(error)
        .done()
}

impl Enc {
    pub fn raw_u32(self, value: u32) -> Enc {
        self.raw(value)
    }
}

/// An `Instantiate` body of exactly `total` bytes (padding goes into `restored`).
pub fn instantiate_of_size(total: usize, fingerprint: &str) -> Vec<u8> {
    let base = msg_instantiate("pad", fingerprint, EMPTY_COMPONENT, &[]);
    assert!(
        total >= base.len(),
        "requested size below the fixed message prefix"
    );
    msg_instantiate(
        "pad",
        fingerprint,
        EMPTY_COMPONENT,
        &vec![0u8; total - base.len()],
    )
}

pub fn framed(body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

// ---------------------------------------------------------------- helper process

pub enum Recv {
    Frame(Vec<u8>),
    Timeout,
    Closed,
}

pub struct Helper {
    pub child: Child,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    pub hello: Option<Hello>,
}

impl Helper {
    fn start(
        child: Child,
        writer: Box<dyn Write + Send>,
        mut reader: Box<dyn Read + Send>,
    ) -> Helper {
        let (tx, rx) = channel();
        std::thread::spawn(move || loop {
            let mut len = [0u8; 4];
            if reader.read_exact(&mut len).is_err() {
                break;
            }
            let count = u32::from_le_bytes(len) as usize;
            if count > 64 * MIB {
                break;
            }
            let mut body = vec![0u8; count];
            if reader.read_exact(&mut body).is_err() {
                break;
            }
            if tx.send(body).is_err() {
                break;
            }
        });
        Helper {
            child,
            writer,
            rx,
            hello: None,
        }
    }

    /// Spawns the helper binary with stdin and stdout as the dispatcher channel (pipes).
    pub fn spawn() -> Helper {
        let mut child = Command::new(HELPER_BIN)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ilium-plugin-helper");
        let writer = child.stdin.take().expect("helper stdin");
        let reader = child.stdout.take().expect("helper stdout");
        Helper::start(child, Box::new(writer), Box::new(reader))
    }

    /// Spawns the helper with an inherited `socketpair` as the dispatcher channel (HST-153).
    pub fn spawn_socketpair() -> Helper {
        let (parent, child_end) = UnixStream::pair().expect("socketpair");
        let stdin_fd: std::os::fd::OwnedFd = child_end.try_clone().expect("clone").into();
        let stdout_fd: std::os::fd::OwnedFd = child_end.into();
        let mut command = Command::new(HELPER_BIN);
        command
            .env_clear()
            .stdin(Stdio::from(stdin_fd))
            .stdout(Stdio::from(stdout_fd))
            .stderr(Stdio::null());
        let child = command.spawn().expect("spawn ilium-plugin-helper");
        drop(command);
        let writer = parent.try_clone().expect("clone parent end");
        Helper::start(child, Box::new(writer), Box::new(parent))
    }

    /// Closes the host end of the channel (the helper sees end of input).
    pub fn close_input(&mut self) {
        self.writer = Box::new(std::io::sink());
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn send(&mut self, body: &[u8]) -> bool {
        self.send_raw(&framed(body))
    }

    pub fn send_raw(&mut self, bytes: &[u8]) -> bool {
        self.writer.write_all(bytes).is_ok() && self.writer.flush().is_ok()
    }

    pub fn recv(&self, millis: u64) -> Recv {
        match self.rx.recv_timeout(Duration::from_millis(millis)) {
            Ok(frame) => Recv::Frame(frame),
            Err(RecvTimeoutError::Timeout) => Recv::Timeout,
            Err(RecvTimeoutError::Disconnected) => Recv::Closed,
        }
    }

    /// The next frame with `wanted` as tag within `millis`, skipping other frames.
    pub fn recv_tag(&self, wanted: u32, millis: u64) -> Option<Vec<u8>> {
        let deadline = Instant::now() + Duration::from_millis(millis);
        loop {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            match self.recv(left.max(1)) {
                Recv::Frame(frame) if frame_tag(&frame) == Some(wanted) => return Some(frame),
                Recv::Frame(_) => continue,
                Recv::Timeout | Recv::Closed => return None,
            }
        }
    }

    /// Reads `Hello` (at most 5,000 ms) and remembers it.
    pub fn expect_hello(&mut self) -> Hello {
        let frame = self
            .recv_tag(tag::HELLO, 5_000)
            .expect("helper sent no Hello");
        let hello = parse_hello(&frame).expect("Hello frame does not decode");
        self.hello = Some(hello.clone());
        hello
    }

    /// Hello, Instantiate with the empty component, wait for Ready. `Err` carries the reason.
    pub fn ready_with(&mut self, plugin: &str, component: &[u8]) -> Result<(), String> {
        let hello = match self.hello.clone() {
            Some(hello) => hello,
            None => self.expect_hello(),
        };
        if !self.send(&msg_instantiate(plugin, &hello.fingerprint, component, &[])) {
            return Err("channel closed on Instantiate".into());
        }
        let deadline = Instant::now() + Duration::from_millis(5_000);
        loop {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64;
            match self.recv(left.max(1)) {
                Recv::Frame(frame) => match frame_tag(&frame) {
                    Some(tag::READY) => return Ok(()),
                    Some(tag::INSTANTIATE_FAILED) => {
                        return Err(Dec::new(&frame[4..])
                            .string()
                            .unwrap_or_else(|| "InstantiateFailed".into()))
                    }
                    _ => continue,
                },
                Recv::Timeout => return Err("Ready not received in 5000 ms".into()),
                Recv::Closed => return Err("channel closed before Ready".into()),
            }
        }
    }

    pub fn ready(&mut self, plugin: &str) -> Result<(), String> {
        self.ready_with(plugin, EMPTY_COMPONENT)
    }

    /// Waits until the process has exited; `None` when it is still running after `millis`.
    pub fn wait_exit(&mut self, millis: u64) -> Option<ExitStatus> {
        let deadline = Instant::now() + Duration::from_millis(millis);
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn signal(&self, name: &str) {
        let _ = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.pid().to_string())
            .status();
    }

    /// Heartbeat echo: true when the helper answers `counter` within `millis`.
    pub fn heartbeat_echo(&mut self, counter: u64, millis: u64) -> bool {
        if !self.send(&msg_heartbeat(counter)) {
            return false;
        }
        match self.recv_tag(tag::HEARTBEAT, millis) {
            Some(frame) => parse_heartbeat(&frame) == Some(counter),
            None => false,
        }
    }

    /// `/proc/<pid>/fd` link targets of the helper, as `(fd number, target)`.
    pub fn fd_links(&self) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(format!("/proc/{}/fd", self.pid())) {
            for entry in entries.flatten() {
                let number = entry
                    .file_name()
                    .to_string_lossy()
                    .parse::<u32>()
                    .unwrap_or(u32::MAX);
                let target = std::fs::read_link(entry.path())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                out.push((number, target));
            }
        }
        out.sort();
        out
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Drives a guest call: sends `CallExport`, answers each `HostCall` through `answer`
/// (returning `(result, error)`), and returns the `CallResult` plus the import names seen.
pub fn run_call(
    helper: &mut Helper,
    call_id: u64,
    export: &str,
    args: &[u8],
    deadline_ms: u32,
    wait_ms: u64,
    mut answer: impl FnMut(&HostCall) -> (Vec<u8>, Option<String>),
) -> (Option<CallResult>, Vec<String>) {
    let mut imports = Vec::new();
    if !helper.send(&msg_call_export(call_id, export, args, deadline_ms)) {
        return (None, imports);
    }
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    loop {
        let left = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if left == 0 {
            return (None, imports);
        }
        match helper.recv(left) {
            Recv::Frame(frame) => {
                if let Some(result) = parse_call_result(&frame) {
                    return (Some(result), imports);
                }
                if let Some(call) = parse_host_call(&frame) {
                    imports.push(call.import.clone());
                    let (result, error) = answer(&call);
                    helper.send(&msg_host_reply(call.call_id, &result, error.as_deref()));
                }
            }
            Recv::Timeout | Recv::Closed => return (None, imports),
        }
    }
}

// ---------------------------------------------------------------- fixtures and manual

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A built `.wasm` guest fixture; fails red with `fixture.missing` until the toolchain builds it.
pub fn require_fixture(name: &str) -> Vec<u8> {
    let dir = std::env::var("ILIUM_PLUGIN_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("ilium-plugin-conformance/fixtures/wasm"));
    let path = dir.join(format!("{name}.wasm"));
    match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => panic!("fixture.missing: {name} at {}", path.display()),
    }
}

pub fn manual_dir() -> PathBuf {
    std::env::var("ILIUM_PLUGIN_MANUAL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("docs/plugins-manual/pages"))
}

/// A manual page with runs of whitespace collapsed to single spaces.
pub fn page(name: &str) -> String {
    let path = manual_dir().join(format!("{name}.md"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("manual page {}: {e}", path.display()));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn assert_page_has(name: &str, needle: &str) {
    let text = page(name);
    assert!(text.contains(needle), "page {name} lacks: {needle}");
}

// ---------------------------------------------------------------- hostile fixture driver

pub struct Hostile {
    pub result: String,
    pub trap: Option<String>,
    /// Every forwarded import as `(name, argument bytes)`.
    pub calls: Vec<(String, Vec<u8>)>,
}

impl Hostile {
    /// True when some forwarded import has a name containing `part` and arguments containing `needle`.
    pub fn forwarded(&self, part: &str, needle: &[u8]) -> bool {
        self.calls.iter().any(|(name, args)| {
            name.contains(part) && args.windows(needle.len().max(1)).any(|w| w == needle)
        })
    }
}

/// Runs `export` of the `hostile-suite` fixture in a helper. The fixture performs the import
/// with `arg`, the host side (this function) answers every forwarded import with `host_error`,
/// and the guest returns the code it received as its result bytes.
pub fn hostile_call(export: &str, arg: &[u8], host_error: Option<&str>) -> Hostile {
    let wasm = require_fixture("hostile-suite");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("hostile", &wasm), Ok(()));
    let mut calls = Vec::new();
    let (result, _) = run_call(&mut helper, 1, export, arg, 5_000, 8_000, |call| {
        calls.push((call.import.clone(), call.args.clone()));
        (Vec::new(), host_error.map(str::to_string))
    });
    let result = result.unwrap_or_else(|| panic!("no CallResult for {export}"));
    Hostile {
        result: String::from_utf8_lossy(&result.result).into_owned(),
        trap: result.trap,
        calls,
    }
}

/// The text of the section of `page_name` that starts at `heading` (up to the next `## ` heading).
pub fn section(page_name: &str, heading: &str) -> String {
    let text = page(page_name);
    let start = text
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing in {page_name}"));
    let rest = &text[start + heading.len()..];
    let end = rest.find(" ## ").unwrap_or(rest.len());
    rest[..end].to_string()
}
