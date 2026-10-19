//! Shared helpers of the install-deps lane: package builder, CLI fixture, fake fetcher, JSONL
//! readers and manual-page readers. Uses only the skeleton surface plus the items listed in
//! API_GAPS.md. Later parts of the lane extend this file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use ilium_plugin_api::Platform;
use ilium_plugin_host::cli::{run, CliEnv, CliOutput};
use ilium_plugin_host::package::write_package;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use serde_json::Value;
use tempfile::TempDir;

/// The 34-byte component of the minimal package vector (reference-package-format.md).
pub const COMPONENT_HEX: &str =
    "0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000";
/// SHA-256 of [`COMPONENT_HEX`] (reference-package-format.md, minimal package vector).
pub const COMPONENT_SHA256: &str =
    "028fb8ab3e154570597f3cc09ad59782ab44d9a6873f1834490da625ef3c6cd8";
/// 2026-10-12T09:14:04Z as Unix seconds.
pub const NOW_SECS: u64 = 1_791_796_444;
/// The `installed_at` text for [`NOW_SECS`].
pub const NOW_TEXT: &str = "2026-10-12T09:14:04Z";
/// The running Ilium release the fixtures claim (PKG-346 uses 0.1.0).
pub const HOST_RELEASE: &str = "0.1.0";
/// A registry base URL used by the fake registry.
pub const REGISTRY_URL: &str = "https://registry.example.test/ilium/v1/";

/// Decodes lowercase hex.
pub fn hex_decode(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap_or(0))
        .collect()
}

/// The component bytes every fixture package carries.
pub fn component_bytes() -> Vec<u8> {
    hex_decode(COMPONENT_HEX)
}

/// One `requires.plugins` entry.
#[derive(Clone)]
pub struct Need {
    pub id: String,
    pub version: String,
    pub optional: bool,
}

/// Builds a package in the accepted ZIP subset with a packed manifest (reference-package-format.md
/// minimal vector plus optional keys).
#[derive(Clone)]
pub struct Spec {
    pub id: String,
    pub version: String,
    pub name: String,
    pub needs: Vec<Need>,
    pub host_range: Option<String>,
    pub services: Vec<String>,
    pub wait_ms: Option<u64>,
    pub server_grants: Vec<String>,
    pub data_version: Option<u64>,
    pub extra_files: Vec<(String, Vec<u8>)>,
    /// Extra top-level manifest keys as (key, raw JSON value).
    pub extra_top_level: Vec<(String, String)>,
}

impl Spec {
    pub fn new(id: &str, version: &str) -> Spec {
        Spec {
            id: id.to_owned(),
            version: version.to_owned(),
            name: "Minimal".to_owned(),
            needs: Vec::new(),
            host_range: None,
            services: Vec::new(),
            wait_ms: None,
            server_grants: Vec::new(),
            data_version: None,
            extra_files: Vec::new(),
            extra_top_level: Vec::new(),
        }
    }

    pub fn named(mut self, name: &str) -> Spec {
        self.name = name.to_owned();
        self
    }

    pub fn needs(mut self, id: &str, range: &str, optional: bool) -> Spec {
        self.needs.push(Need {
            id: id.to_owned(),
            version: range.to_owned(),
            optional,
        });
        self
    }

    pub fn host(mut self, range: &str) -> Spec {
        self.host_range = Some(range.to_owned());
        self
    }

    pub fn service(mut self, text: &str) -> Spec {
        self.services.push(text.to_owned());
        self
    }

    pub fn wait_ms(mut self, value: u64) -> Spec {
        self.wait_ms = Some(value);
        self
    }

    pub fn grant(mut self, grant: &str) -> Spec {
        self.server_grants.push(grant.to_owned());
        self
    }

    pub fn data_version(mut self, value: u64) -> Spec {
        self.data_version = Some(value);
        self
    }

    /// The packed manifest text: top-level keys in table order, `files` last.
    pub fn manifest_text(&self) -> String {
        let mut text = String::from("{\n");
        text.push_str("  \"api\": \"ilium:plugin@0.1.0\",\n");
        text.push_str(&format!("  \"id\": {},\n", json_string(&self.id)));
        text.push_str(&format!("  \"name\": {},\n", json_string(&self.name)));
        text.push_str(&format!("  \"version\": {},\n", json_string(&self.version)));
        text.push_str("  \"publisher\": \"Example Labs\",\n");
        text.push_str("  \"server\": {\n    \"entry\": \"server.wasm\",\n    \"world\": \"ilium:server/base@0.1.0\",\n    \"exports\": []\n  },\n");
        if !self.server_grants.is_empty() {
            let grants: Vec<String> = self
                .server_grants
                .iter()
                .map(|grant| json_string(grant))
                .collect();
            text.push_str(&format!(
                "  \"capabilities\": {{\n    \"server\": [{}]\n  }},\n",
                grants.join(", ")
            ));
        }
        if self.host_range.is_some()
            || !self.needs.is_empty()
            || !self.services.is_empty()
            || self.wait_ms.is_some()
        {
            let mut parts: Vec<String> = Vec::new();
            if let Some(range) = &self.host_range {
                parts.push(format!("\"host\": {}", json_string(range)));
            }
            if !self.services.is_empty() {
                let services: Vec<String> = self
                    .services
                    .iter()
                    .map(|service| json_string(service))
                    .collect();
                parts.push(format!("\"services\": [{}]", services.join(", ")));
            }
            if !self.needs.is_empty() {
                let plugins: Vec<String> = self
                    .needs
                    .iter()
                    .map(|need| {
                        format!(
                            "{{\"id\": {}, \"version\": {}, \"optional\": {}}}",
                            json_string(&need.id),
                            json_string(&need.version),
                            need.optional
                        )
                    })
                    .collect();
                parts.push(format!("\"plugins\": [{}]", plugins.join(", ")));
            }
            if let Some(wait) = self.wait_ms {
                parts.push(format!("\"wait_ms\": {wait}"));
            }
            text.push_str(&format!("  \"requires\": {{ {} }},\n", parts.join(", ")));
        }
        for (key, raw) in &self.extra_top_level {
            if key != "requires"
                || (self.host_range.is_none()
                    && self.needs.is_empty()
                    && self.services.is_empty()
                    && self.wait_ms.is_none())
            {
                text.push_str(&format!("  {}: {},\n", json_string(key), raw));
            }
        }
        if let Some(version) = self.data_version {
            text.push_str(&format!("  \"data_version\": {version},\n"));
        }
        let mut entries: Vec<(String, Vec<u8>)> =
            vec![("server.wasm".to_owned(), component_bytes())];
        entries.extend(self.extra_files.iter().cloned());
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        text.push_str("  \"files\": [\n");
        let lines: Vec<String> = entries
            .iter()
            .map(|(path, bytes)| {
                let sha = if path == "server.wasm" { COMPONENT_SHA256.to_owned() } else { sha256_hex(bytes) };
                format!(
                    "    {{\n      \"path\": {},\n      \"length\": {},\n      \"sha256\": \"{}\"\n    }}",
                    json_string(path),
                    bytes.len(),
                    sha
                )
            })
            .collect();
        text.push_str(&lines.join(",\n"));
        text.push_str("\n  ]\n}\n");
        text
    }

    /// The package file bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>)> = vec![
            (
                "manifest.json".to_owned(),
                self.manifest_text().into_bytes(),
            ),
            ("server.wasm".to_owned(), component_bytes()),
        ];
        files.extend(self.extra_files.iter().cloned());
        files.sort_by(|left, right| left.0.cmp(&right.0));
        write_package(&files)
            .unwrap_or_else(|diagnostic| panic!("fixture package failed: {diagnostic:?}"))
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// A small, dependency-free SHA-256 for fixture inventories.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut v = state;
        for index in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(v[index]);
        }
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

/// One scripted answer of the fake fetcher.
#[derive(Clone)]
pub enum Served {
    Body {
        bytes: Vec<u8>,
        content_type: Option<String>,
        declared_length: Option<u64>,
    },
    Redirect {
        location: String,
    },
}

/// Serves URLs from memory and records every request (reference-registry.md transport, PKG-260).
#[derive(Default)]
pub struct FakeFetcher {
    routes: Mutex<BTreeMap<String, Served>>,
    calls: Mutex<Vec<String>>,
}

impl FakeFetcher {
    pub fn new() -> Arc<FakeFetcher> {
        Arc::new(FakeFetcher::default())
    }

    pub fn serve(&self, url: &str, bytes: Vec<u8>) {
        self.serve_as(url, bytes, None, None);
    }

    pub fn serve_as(
        &self,
        url: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
        declared_length: Option<u64>,
    ) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(
                url.to_owned(),
                Served::Body {
                    bytes,
                    content_type: content_type.map(str::to_owned),
                    declared_length,
                },
            );
        }
    }

    pub fn redirect(&self, from: &str, to: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.insert(
                from.to_owned(),
                Served::Redirect {
                    location: to.to_owned(),
                },
            );
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    fn lookup(&self, url: &str) -> Option<Served> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(url.to_owned());
        }
        self.routes
            .lock()
            .ok()
            .and_then(|routes| routes.get(url).cloned())
    }
}

impl Fetcher for FakeFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match self.lookup(url) {
            Some(Served::Body { bytes, .. }) => Ok(bytes),
            Some(Served::Redirect { .. }) | None => Err(FetchError::Unreachable),
        }
    }

    // API gap: `Fetcher::get_response` and `FetchResponse` (see API_GAPS.md).
    fn get_response(
        &self,
        url: &str,
    ) -> Result<ilium_plugin_host::registry_client::FetchResponse, FetchError> {
        match self.lookup(url) {
            Some(Served::Body {
                bytes,
                content_type,
                declared_length,
            }) => Ok(ilium_plugin_host::registry_client::FetchResponse {
                status: 200,
                location: None,
                content_type,
                content_length: declared_length.or(Some(bytes.len() as u64)),
                body: bytes,
            }),
            Some(Served::Redirect { location }) => {
                Ok(ilium_plugin_host::registry_client::FetchResponse {
                    status: 302,
                    location: Some(location),
                    content_type: None,
                    content_length: Some(0),
                    body: Vec::new(),
                })
            }
            None => Err(FetchError::Unreachable),
        }
    }
}

/// A temporary plugin root with a CLI environment over it.
pub struct Fx {
    pub dir: TempDir,
    pub fetcher: Arc<FakeFetcher>,
    pub platform: Platform,
    pub cwd: PathBuf,
}

impl Fx {
    pub fn new() -> Fx {
        Fx::on(Platform::Linux)
    }

    pub fn on(platform: Platform) -> Fx {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        Fx {
            dir,
            fetcher: FakeFetcher::new(),
            platform,
            cwd,
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    pub fn config_dir(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    /// `<data dir>/plugins/`, the plugin root (reference-install-update-uninstall.md).
    pub fn root(&self) -> PathBuf {
        self.data_dir().join("plugins")
    }

    pub fn store_dir(&self, id: &str, version: &str) -> PathBuf {
        self.root().join("store").join(id).join(version)
    }

    pub fn registry_path(&self) -> PathBuf {
        self.root().join("installed.json")
    }

    /// The CLI environment; the fields after `is_ci` are listed in API_GAPS.md.
    pub fn env(&self) -> CliEnv {
        self.env_in(&self.cwd, BTreeMap::new())
    }

    pub fn env_in(&self, cwd: &Path, env: BTreeMap<String, String>) -> CliEnv {
        CliEnv {
            data_dir: self.data_dir(),
            state_dir: self.state_dir(),
            config_dir: self.config_dir(),
            cwd: cwd.to_path_buf(),
            now: UNIX_EPOCH + Duration::from_secs(NOW_SECS),
            is_ci: true,
            fetcher: self.fetcher.clone(),
            cache_dir: self.cache_dir(),
            platform: self.platform,
            host_release: HOST_RELEASE.to_owned(),
            env,
            system_dir: None,
            policy_file: None,
            probe: None,
            builder: None,
            runner: None,
            session: None,
            interrupt: None,
            dev_script: Vec::new(),
            build_duration_ms: 0,
        }
    }

    pub fn run(&self, args: &[&str]) -> CliOutput {
        let owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
        run(&owned, &self.env())
    }

    pub fn run_in(&self, cwd: &Path, args: &[&str]) -> CliOutput {
        let owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
        run(&owned, &self.env_in(cwd, BTreeMap::new()))
    }

    /// Writes package bytes under a name in the scratch folder and returns the path text.
    pub fn write_file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.dir.path().join("files").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        }
        std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("write failed: {error}"));
        path.to_string_lossy().into_owned()
    }

    /// Installs an unsigned file package with `--allow-unsigned --yes` plus extra flags.
    pub fn install_with(&self, spec: &Spec, extra: &[&str]) -> CliOutput {
        let path = self.write_file(
            &format!("{}-{}.iliumpkg", spec.id, spec.version),
            &spec.build(),
        );
        let mut args: Vec<&str> = vec!["install", &path, "--allow-unsigned", "--yes"];
        args.extend_from_slice(extra);
        self.run(&args)
    }

    pub fn install(&self, spec: &Spec) -> CliOutput {
        self.install_with(spec, &[])
    }

    /// `update <file>` for a package file.
    pub fn update_with(&self, spec: &Spec, extra: &[&str]) -> CliOutput {
        let path = self.write_file(
            &format!("{}-{}.iliumpkg", spec.id, spec.version),
            &spec.build(),
        );
        let mut args: Vec<&str> = vec!["update", &path, "--allow-unsigned", "--yes"];
        args.extend_from_slice(extra);
        self.run(&args)
    }

    /// The parsed `installed.json`, or `Null` when absent.
    pub fn installed(&self) -> Value {
        match std::fs::read(self.registry_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        }
    }

    pub fn installed_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.registry_path()).ok()
    }

    pub fn record(&self, id: &str) -> Option<Value> {
        self.installed()["plugins"]
            .as_array()
            .and_then(|plugins| plugins.iter().find(|entry| entry["id"] == id).cloned())
    }

    pub fn active_version(&self, id: &str) -> Option<String> {
        self.record(id)
            .and_then(|record| record["active_version"].as_str().map(str::to_owned))
    }

    /// Every file below a directory with its bytes, sorted; the install lock is left out because
    /// taking it creates the file (PKG-275).
    pub fn snapshot_tree(&self, base: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut found = BTreeMap::new();
        collect_files(base, base, &mut found);
        found.retain(|path, _| path != "installed.lock");
        found
    }
}

fn collect_files(base: &Path, current: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, found);
        } else if let Ok(bytes) = std::fs::read(&path) {
            let relative = path
                .strip_prefix(base)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            found.insert(relative, bytes);
        }
    }
}

/// Readers over the JSONL stdout of a command.
pub trait OutExt {
    fn lines_of(&self, kind: &str) -> Vec<Value>;
    fn error_codes(&self) -> Vec<String>;
    fn warning_codes(&self) -> Vec<String>;
    fn has_error(&self, code: &str) -> bool;
    fn has_warning(&self, code: &str) -> bool;
    fn steps(&self) -> Vec<String>;
    fn result_line(&self) -> Option<Value>;
    fn all_text(&self) -> String;
}

impl OutExt for CliOutput {
    fn lines_of(&self, kind: &str) -> Vec<Value> {
        self.stdout_jsonl
            .iter()
            .filter(|line| line["type"] == kind)
            .cloned()
            .collect()
    }

    fn error_codes(&self) -> Vec<String> {
        self.lines_of("error")
            .iter()
            .filter_map(|line| line["code"].as_str().map(str::to_owned))
            .collect()
    }

    fn warning_codes(&self) -> Vec<String> {
        self.lines_of("warning")
            .iter()
            .filter_map(|line| line["code"].as_str().map(str::to_owned))
            .collect()
    }

    fn has_error(&self, code: &str) -> bool {
        self.error_codes().iter().any(|found| found == code)
    }

    fn has_warning(&self, code: &str) -> bool {
        self.warning_codes().iter().any(|found| found == code)
    }

    fn steps(&self) -> Vec<String> {
        let mut steps: Vec<String> = Vec::new();
        for line in self.lines_of("progress") {
            if let Some(step) = line["step"].as_str() {
                if steps.last().map(String::as_str) != Some(step) {
                    steps.push(step.to_owned());
                }
            }
        }
        steps
    }

    fn result_line(&self) -> Option<Value> {
        self.lines_of("result").pop()
    }

    fn all_text(&self) -> String {
        let mut text = self.stderr.clone();
        for line in &self.stdout_jsonl {
            text.push('\n');
            text.push_str(&line.to_string());
        }
        text
    }
}

/// True when `text` is 64 lowercase hexadecimal characters.
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// True when `text` is `YYYY-MM-DDTHH:MM:SSZ`.
pub fn is_utc_stamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}

/// Absolute path of a manual page, honouring `ILIUM_PLUGIN_MANUAL_DIR`.
pub fn manual_page(name: &str) -> String {
    let base = std::env::var("ILIUM_PLUGIN_MANUAL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages")
        });
    std::fs::read_to_string(base.join(name)).unwrap_or_default()
}
