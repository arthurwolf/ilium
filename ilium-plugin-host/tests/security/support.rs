//! Shared helpers of the security lane: a harness rig with a scratch state directory and a fake
//! network, import-call shortcuts, JSONL readers and manual-page readers. Items outside the
//! skeleton are named in API_GAPS.md.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ilium_plugin_api::generated::host::http::HttpResponse;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::generated::types::ids::RequestId;
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::{ErrorCode, PluginError, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::testing::{FakeNet, HarnessHandle, HostHarness, ImportResult, NativeGuest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Plugin id the harness gives its plugin (DEV-420: `test.plugin`).
pub fn plugin_id() -> PluginId {
    PluginId("test.plugin".to_owned())
}

/// One event that makes the harness call `on-events` once.
pub fn one_event() -> Vec<Event> {
    vec![Event::ThemeChanged]
}

/// A guest that accepts every batch and returns no submission.
pub struct QuietGuest;

impl NativeGuest for QuietGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        Ok(Vec::new())
    }
}

/// What a test configures before the host starts. Every `with_*` call maps to a harness gap in
/// API_GAPS.md.
#[derive(Clone)]
pub struct Setup {
    pub tier: Tier,
    pub side: Side,
    pub grants: Vec<String>,
    pub system_policy: Option<(String, bool)>,
    pub user_policy: Option<String>,
    pub secrets: Vec<(String, String)>,
    pub offline: bool,
    pub effective_uid: Option<u32>,
    pub settings: Vec<(String, Value)>,
    pub options: Vec<(String, Value)>,
}

/// A T3 server plugin holding `granted`.
pub fn setup(granted: &[&str]) -> Setup {
    Setup {
        tier: Tier::T3,
        side: Side::Server,
        grants: granted.iter().map(|grant| (*grant).to_owned()).collect(),
        system_policy: None,
        user_policy: None,
        secrets: Vec::new(),
        offline: false,
        effective_uid: None,
        settings: Vec::new(),
        options: Vec::new(),
    }
}

impl Setup {
    pub fn tier(mut self, tier: Tier) -> Setup {
        self.tier = tier;
        self
    }

    pub fn side(mut self, side: Side) -> Setup {
        self.side = side;
        self
    }

    /// System policy text; `secure` tells whether the file is root-owned and not group/other
    /// writable (SEC-182).
    pub fn system_policy(mut self, text: &str, secure: bool) -> Setup {
        self.system_policy = Some((text.to_owned(), secure));
        self
    }

    pub fn user_policy(mut self, text: &str) -> Setup {
        self.user_policy = Some(text.to_owned());
        self
    }

    pub fn secret(mut self, key: &str, value: &str) -> Setup {
        self.secrets.push((key.to_owned(), value.to_owned()));
        self
    }

    pub fn offline(mut self) -> Setup {
        self.offline = true;
        self
    }

    pub fn effective_uid(mut self, uid: u32) -> Setup {
        self.effective_uid = Some(uid);
        self
    }

    pub fn setting(mut self, key: &str, value: Value) -> Setup {
        self.settings.push((key.to_owned(), value));
        self
    }

    /// A catch-all harness option (see API_GAPS.md `HostHarness::option`).
    pub fn option(mut self, key: &str, value: Value) -> Setup {
        self.options.push((key.to_owned(), value));
        self
    }

    pub fn declined(self, grants: &[&str]) -> Setup {
        self.option("declined", json!(grants))
    }

    pub fn safe_mode(self) -> Setup {
        self.option("safe-mode", json!(true))
    }

    pub fn kernel_secret(self, key: &str, value: &str) -> Setup {
        self.option(&format!("kernel-secret:{key}"), json!(value))
    }

    pub fn no_keyring(self) -> Setup {
        self.option("keyring", json!(false))
    }

    pub fn keyring(self) -> Setup {
        self.option("keyring", json!(true))
    }

    pub fn system_policy_unreadable(self) -> Setup {
        self.option("system-policy-unreadable", json!(true))
    }

    pub fn provider(self, name: &str, is_local: bool) -> Setup {
        self.option("provider", json!({ "name": name, "local": is_local }))
    }

    pub fn trapping_guest(self) -> Setup {
        self.option("trapping-guest", json!(true))
    }

    pub fn replacement_inference_plugin(self) -> Setup {
        self.option("replacement-inference-plugin", json!(true))
    }

    pub fn accepts_messages_from(self, ids: &[&str]) -> Setup {
        self.option("accepts-messages-from", json!(ids))
    }

    pub fn tampered_t1_bundle(self) -> Setup {
        self.option("tampered-t1-bundle", json!(true))
    }

    pub fn platform(self, name: &str) -> Setup {
        self.option("platform", json!(name))
    }

    pub fn sandbox_unavailable(self, prerequisite: &str) -> Setup {
        self.option("sandbox-unavailable", json!(prerequisite))
    }

    /// Starts the host with a fresh state directory and fake network.
    pub fn start(self) -> Rig {
        let state = tempfile::tempdir().expect("scratch state directory");
        let net = FakeNet::new();
        let host = Rig::build(&self, state.path(), &net).expect("the harness builds");
        Rig { host, net, state, setup: self }
    }
}

/// A host under test with its scratch state directory and fake network.
pub struct Rig {
    pub host: HarnessHandle,
    pub net: FakeNet,
    pub state: tempfile::TempDir,
    pub setup: Setup,
}

/// A rig for `side` at `tier` holding `granted`.
pub fn rig(tier: Tier, side: Side, granted: &[&str]) -> Rig {
    setup(granted).tier(tier).side(side).start()
}

/// A T3 server rig holding `granted`.
pub fn t3(granted: &[&str]) -> Rig {
    setup(granted).start()
}

impl Rig {
    fn build(setup: &Setup, state: &Path, net: &FakeNet) -> Result<HarnessHandle, ilium_plugin_host::HostError> {
        let grant_refs: Vec<&str> = setup.grants.iter().map(String::as_str).collect();
        let mut harness = HostHarness::new()
            .tier(setup.tier)
            .side(setup.side)
            .grants(&grant_refs)
            .state_dir(state.to_path_buf())
            .net(net.clone())
            .offline(setup.offline)
            .with_native_guest(Box::new(QuietGuest));
        if let Some((text, secure)) = &setup.system_policy {
            harness = harness.system_policy(text, *secure);
        }
        if let Some(text) = &setup.user_policy {
            harness = harness.user_policy(text);
        }
        if let Some(uid) = setup.effective_uid {
            harness = harness.effective_uid(uid);
        }
        for (key, value) in &setup.secrets {
            harness = harness.secret(key, value);
        }
        for (key, value) in &setup.settings {
            harness = harness.setting(key, value.clone());
        }
        for (key, value) in &setup.options {
            harness = harness.option(key, value.clone());
        }
        harness.build()
    }

    /// Stops the server and starts it again on the same state directory and network (policy and
    /// stores are re-read).
    pub fn restart(self) -> Rig {
        let Rig { host, net, state, setup } = self;
        drop(host);
        let host = Rig::build(&setup, state.path(), &net).expect("the harness builds again");
        Rig { host, net, state, setup }
    }

    /// Calls an `ilium:host` import by its WIT name.
    pub fn call(&self, name: &str, args: Value) -> ImportResult {
        self.host.call_import(name, args)
    }

    /// Calls an import and returns its value.
    pub fn value(&self, name: &str, args: Value) -> Result<Value, PluginError> {
        self.host.call_import_value(name, args)
    }

    /// Log lines of the plugin as plain messages.
    pub fn log_messages(&self) -> Vec<String> {
        self.host.logs().into_iter().map(|line| line.message).collect()
    }

    /// `ilium:host/http.fetch` of a GET with the given headers, driven to completion.
    pub fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, PluginError> {
        self.fetch(request_json("GET", url, headers, None))
    }

    /// Same with a POST body.
    pub fn post(&self, url: &str, body: Vec<u8>) -> Result<HttpResponse, PluginError> {
        self.fetch(request_json("POST", url, &[], Some(body)))
    }

    /// Calls `fetch` with a prepared request value and drives it to completion: the refusal at
    /// the call, the failed completion, or the response.
    pub fn fetch(&self, request: Value) -> Result<HttpResponse, PluginError> {
        let id = self.value("ilium:host/http.fetch", json!({ "request": request }))?;
        let request_id = id.as_u64().expect("fetch returns a request id");
        self.host.pump(16);
        let finished = self
            .host
            .outcomes(&plugin_id())
            .into_iter()
            .rev()
            .find(|result| result.request == RequestRef::Call(RequestId(request_id)));
        if let Some(result) = finished {
            if let Outcome::Failed(error) = result.outcome {
                return Err(error);
            }
        }
        let taken = self.value("ilium:host/http.take-response", json!({ "request": request_id }))?;
        let response: Option<HttpResponse> = serde_json::from_value(taken).expect("take-response value");
        Ok(response.expect("a completed request has a response"))
    }

    /// The lines of the audit file, parsed. Empty when no file exists yet.
    pub fn audit_lines(&self) -> Vec<Value> {
        read_jsonl(&self.host.audit_path())
    }

    /// The audit records of one event name.
    pub fn audit_events(&self, event: &str) -> Vec<Value> {
        self.audit_lines().into_iter().filter(|line| line["event"] == event).collect()
    }

    /// Persistent notices the server shows.
    pub fn notices(&self) -> Vec<String> {
        self.host.notices()
    }

    /// `ilium plugin policy show` output lines.
    pub fn policy_show(&self) -> Vec<Value> {
        self.host.policy_show()
    }
}

/// An `http-request` value in its JSON form.
pub fn request_json(method: &str, url: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Value {
    json!({
        "method": method,
        "url": url,
        "headers": headers.iter().map(|(name, value)| json!({ "name": name, "value": value })).collect::<Vec<_>>(),
        "body": body,
        "timeout-millis": 5000,
    })
}

/// True when the result is `not-permitted`.
pub fn is_not_permitted(result: &ImportResult) -> bool {
    matches!(result, ImportResult::Err(error) if error.code == ErrorCode::NotPermitted)
}

/// True when the result is a success.
pub fn is_ok(result: &ImportResult) -> bool {
    matches!(result, ImportResult::Ok)
}

/// The refusal reason word of an error: the text before the first `: ` of the message.
pub fn reason_of(error: &PluginError) -> String {
    error.message.split(": ").next().unwrap_or_default().to_owned()
}

/// Asserts `Err` with this code and reason word (`not-permitted`, `quota-exceeded`, `network`).
pub fn assert_refused<T: std::fmt::Debug>(result: &Result<T, PluginError>, code: ErrorCode, reason: &str) {
    match result {
        Err(error) => {
            assert_eq!(error.code, code, "{error:?}");
            assert_eq!(reason_of(error), reason, "{error:?}");
        }
        Ok(value) => panic!("expected {code:?} {reason}, got Ok({value:?})"),
    }
}

/// Asserts `Err` with this code only.
pub fn assert_code<T: std::fmt::Debug>(result: &Result<T, PluginError>, code: ErrorCode) {
    match result {
        Err(error) => assert_eq!(error.code, code, "{error:?}"),
        Ok(value) => panic!("expected {code:?}, got Ok({value:?})"),
    }
}

/// Parses a JSONL file; empty when the file does not exist.
pub fn read_jsonl(path: &Path) -> Vec<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Raw lines of a text file without the trailing newline.
pub fn raw_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 64 zeros: the `prev_hash` of the first audit record.
pub fn zero_hash() -> String {
    "0".repeat(64)
}

/// Command line arguments from string slices.
pub fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// A registry fetcher that has no network.
pub struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// A CLI environment below a scratch root. Every directory is below `root`.
pub fn cli_env(root: &Path) -> CliEnv {
    CliEnv {
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        config_dir: root.join("config"),
        cwd: root.join("cwd"),
        now: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
        is_ci: false,
        fetcher: Arc::new(NoNetwork),
    }
}

/// Root of the workspace.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The text of a manual page: `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_page(name: &str) -> String {
    let dir = std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("docs/plugins-manual/pages"));
    std::fs::read_to_string(dir.join(name)).unwrap_or_else(|error| panic!("manual page {name}: {error}"))
}

/// Asserts that a page contains every needle.
pub fn assert_page_has(page: &str, needles: &[&str]) {
    let text = manual_page(page);
    for needle in needles {
        assert!(text.contains(needle), "{page} lacks {needle:?}");
    }
}

/// A scratch directory.
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("scratch directory")
}

/// Number of occurrences of `needle` in `haystack`.
pub fn count_of(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// A valid server-side source manifest with extra top-level keys merged in.
pub fn server_manifest(extra: Value) -> Value {
    let mut manifest = json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.demo",
        "name": "Example demo",
        "version": "0.1.0",
        "publisher": "Example Publisher",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
    });
    if let (Some(base), Some(more)) = (manifest.as_object_mut(), extra.as_object()) {
        for (key, value) in more {
            base.insert(key.clone(), value.clone());
        }
    }
    manifest
}

/// Error codes of a manifest checked as `ilium plugin check` does for a T3 package.
pub fn manifest_error_codes(manifest: &Value) -> Vec<String> {
    use ilium_plugin_api::manifest::{validate_json, CheckMode, ValidationContext};
    use ilium_plugin_api::{Platform, Severity};
    let context = ValidationContext {
        tier: Tier::T3,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    };
    validate_json(&manifest.to_string(), &context)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, Severity::Error))
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}
