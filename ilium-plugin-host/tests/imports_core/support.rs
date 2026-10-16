//! Shared helpers of the imports-core lane: fixture shortcuts, manifest builders, manual-derived
//! constants and small byte builders. Everything uses only the public surface named in
//! API_GAPS.md or the skeleton. Part 2 of the lane extends this file.

#![allow(dead_code)]

use std::path::Path;
use std::time::Duration;

use ilium_plugin_api::generated::host::{
    HostClock, HostCodec, HostFs, HostHttp, HostJobs, HostJournal, HostLog, HostPanes,
    HostProcess, HostSettings,
};
use ilium_plugin_api::generated::types::{ErrorCode, FsPath, Outcome, PluginError, RequestId};
use ilium_plugin_api::manifest::{CheckMode, Diagnostic, ValidationContext};
use ilium_plugin_api::{Platform, Tier};
use ilium_plugin_host::testing::imports_fixture::{ImportsFixture, ImportsFixtureBuilder};
use serde_json::{json, Value};

/// Plugin id the fixture gives its plugin.
pub const PLUGIN: &str = "test.imports";

/// The wait budget for any asynchronous completion (every wait in this lane is bounded).
pub const WAIT: Duration = Duration::from_secs(5);

/// The test clock starts at 2030-01-01 00:00:00 UTC (api-clock-and-timers.md).
pub const TEST_CLOCK_START_MILLIS: u64 = 1_893_456_000_000;

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// A fixture for the given tier, side and grants, with no project folder.
pub fn fixture(tier: Tier, grants: &[&str]) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(tier)
        .grants(grants)
        .build()
}

/// A T1 fixture with the given grants.
pub fn fixture_t1(grants: &[&str]) -> ImportsFixture {
    fixture(Tier::T1, grants)
}

/// A T2 fixture with the given grants.
pub fn fixture_t2(grants: &[&str]) -> ImportsFixture {
    fixture(Tier::T2, grants)
}

/// A builder bound to a project folder (scope `project`).
pub fn builder_in(root: &Path, tier: Tier, grants: &[&str]) -> ImportsFixtureBuilder {
    ImportsFixture::builder()
        .tier(tier)
        .grants(grants)
        .project_root(root)
}

/// A fixture whose scope `project` is the given folder.
pub fn fixture_in(root: &Path, tier: Tier, grants: &[&str]) -> ImportsFixture {
    builder_in(root, tier, grants).build()
}

/// A path in the `project` scope.
pub fn project_path(relative: &str) -> FsPath {
    FsPath {
        scope: "project".to_owned(),
        path: relative.to_owned(),
    }
}

/// A path in the given scope.
pub fn scoped_path(scope: &str, relative: &str) -> FsPath {
    FsPath {
        scope: scope.to_owned(),
        path: relative.to_owned(),
    }
}

/// Unwraps an `Err`, failing the test with the debug text when the call succeeded.
pub fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>) -> ErrorCode {
    match result {
        Ok(value) => panic!("expected an error code, the call succeeded with {value:?}"),
        Err(error) => error.code,
    }
}

/// Unwraps an `Err` into the whole error.
pub fn expect_error<T: std::fmt::Debug>(result: Result<T, PluginError>) -> PluginError {
    match result {
        Ok(value) => panic!("expected an error, the call succeeded with {value:?}"),
        Err(error) => error,
    }
}

/// Unwraps an `Ok`, failing the test with the error code when the call failed.
pub fn expect_ok<T>(result: Result<T, PluginError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("expected success, got {} ({})", error.code.as_str(), error.message),
    }
}

/// The failure code of a completion `failed(code)` or `refused(code)`, if it is one.
pub fn outcome_code(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Failed(error) | Outcome::Refused(error) => Some(error.code),
        _ => None,
    }
}

/// Waits (bounded) for a request to complete and returns its outcome. Panics with the request id
/// when the budget runs out, which fails the test in a readable way.
pub fn complete(fixture: &mut ImportsFixture, request: RequestId) -> Outcome {
    match fixture.wait_outcome(request, WAIT) {
        Some(outcome) => outcome,
        None => panic!("request {request:?} did not complete within {WAIT:?}"),
    }
}

/// True when the outcome is a plain success.
pub fn is_ok(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_))
}

/// A validation context for `ilium plugin check` on Linux with nothing installed.
pub fn context(tier: Tier) -> ValidationContext {
    ValidationContext {
        tier,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    }
}

/// The codes of a diagnostic list, in order.
pub fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}

/// The smallest valid manifest (reference-manifest.md).
pub fn minimal_manifest() -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": "test.imports",
        "name": "Imports test",
        "version": "0.1.0",
        "publisher": "Example",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
    })
}

/// A manifest with one own settings tab holding the given sections (each a JSON object with
/// `id`, `title` and `rows`).
pub fn manifest_with_tab(sections: Vec<Value>) -> Value {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({
        "settings": [{
            "kind": "tab",
            "id": "main",
            "title": "Imports",
            "sections": sections
        }]
    });
    manifest
}

/// A manifest with one tab and one section holding the given rows.
pub fn manifest_with_rows(rows: Vec<Value>) -> Value {
    manifest_with_tab(vec![json!({ "id": "general", "title": "General", "rows": rows })])
}

/// A manifest with the given `contributes.settings` entries and nothing else.
pub fn manifest_with_settings_entries(entries: Vec<Value>) -> Value {
    let mut manifest = minimal_manifest();
    manifest["contributes"] = json!({ "settings": entries });
    manifest
}

/// The diagnostic codes `validate_json` gives a manifest for the tier.
pub fn install_codes(manifest: &Value, tier: Tier) -> Vec<String> {
    let text = manifest.to_string();
    codes(&ilium_plugin_api::manifest::validate_json(&text, &context(tier)))
}

/// A toggle row.
pub fn toggle(key: &str, default: bool) -> Value {
    json!({ "type": "toggle", "key": key, "title": key, "default": default })
}

/// A number row.
pub fn number(key: &str, minimum: i64, maximum: i64, default: i64) -> Value {
    json!({
        "type": "number", "key": key, "title": key,
        "minimum": minimum, "maximum": maximum, "default": default
    })
}

/// A text row with a maximum length.
pub fn text(key: &str, max_length: u32, default: &str) -> Value {
    json!({ "type": "text", "key": key, "title": key, "max_length": max_length, "default": default })
}

/// A choice row with the given option values (titles equal values).
pub fn choice(key: &str, options: &[&str]) -> Value {
    let list: Vec<Value> = options
        .iter()
        .map(|value| json!({ "value": value, "title": value }))
        .collect();
    json!({ "type": "choice", "key": key, "title": key, "default": options[0], "options": list })
}

/// A choice row with `dynamic: true` (its options are replaced with `set-choices`).
pub fn dynamic_choice(key: &str, options: &[&str]) -> Value {
    let mut row = choice(key, options);
    row["dynamic"] = json!(true);
    row
}

/// A text-area row with a maximum length.
pub fn text_area(key: &str, max_length: u32) -> Value {
    json!({ "type": "text-area", "key": key, "title": key, "max_length": max_length, "default": "" })
}

/// A live info row with an id.
pub fn info(id: &str) -> Value {
    json!({ "type": "info", "id": id, "title": id, "text": "x", "live": true })
}

/// A meter row with an id.
pub fn meter(id: &str) -> Value {
    json!({ "type": "meter", "id": id, "title": id })
}

/// The manifest every runtime settings test uses: a toggle, a number (0 to 10), a text (40), a
/// choice, an info row and a meter row.
pub fn runtime_settings_manifest() -> Value {
    manifest_with_rows(vec![
        toggle("enabled", true),
        number("count", 0, 10, 5),
        text("label", 40, "work"),
        choice("mode", &["fast", "slow"]),
        dynamic_choice("dyn", &["a", "b"]),
        info("status"),
        meter("usage"),
    ])
}

/// A fixture with the runtime settings manifest.
pub fn settings_fixture(tier: Tier) -> ImportsFixture {
    ImportsFixture::builder()
        .tier(tier)
        .manifest_json(&runtime_settings_manifest().to_string())
        .build()
}

/// `n` distinct rows `row0`, `row1`, ... of toggles.
pub fn many_toggles(count: usize, prefix: &str) -> Vec<Value> {
    (0..count)
        .map(|index| toggle(&format!("{prefix}{index}"), false))
        .collect()
}

/// A string of exactly `count` ASCII characters.
pub fn ascii(count: usize) -> String {
    "x".repeat(count)
}

/// A 24-bit BMP of the given size, every pixel the same blue-green-red triple. Used as a known
/// image for the codec tests (api-codec.md lists BMP among the formats).
pub fn bmp(width: u32, height: u32, blue: u8, green: u8, red: u8) -> Vec<u8> {
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let pixel_bytes = row_bytes * height;
    let total = 54 + pixel_bytes;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&pixel_bytes.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for _row in 0..height {
        for _column in 0..width {
            out.extend_from_slice(&[blue, green, red]);
        }
        for _pad in 0..(row_bytes - width * 3) {
            out.push(0);
        }
    }
    out
}

/// Lower-case hex of bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A one-shot loopback HTTP server: accepts up to `connections` connections on 127.0.0.1, answers
/// each with the canned response, and records the raw request heads. Returns the port and a
/// receiver of request heads. The thread ends after the connections are served; every accept
/// is bounded by a 10 s read timeout so a test cannot hang.
pub fn loopback_server(
    response: Vec<u8>,
    connections: usize,
) -> (u16, std::sync::mpsc::Receiver<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for _ in 0..connections {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let mut buffer = vec![0u8; 16 * 1024];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let _ = sender.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });
    (port, receiver)
}

/// A canned `HTTP/1.1` response with the given status line and body.
pub fn http_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// The `Host*` trait objects are used through the fixture; these aliases keep test signatures short.
pub type Settings<'a> = &'a mut dyn HostSettings;
pub type Fs<'a> = &'a mut dyn HostFs;
pub type Http<'a> = &'a mut dyn HostHttp;
pub type Jobs<'a> = &'a mut dyn HostJobs;
pub type Process<'a> = &'a mut dyn HostProcess;
pub type Panes<'a> = &'a mut dyn HostPanes;
pub type Journal<'a> = &'a mut dyn HostJournal;
pub type Log<'a> = &'a mut dyn HostLog;
pub type Clock<'a> = &'a mut dyn HostClock;
pub type Codec<'a> = &'a mut dyn HostCodec;
