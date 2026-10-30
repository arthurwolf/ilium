//! Offline file inspection. No guest is instantiated and no session is started.
use super::{CliEnv, CliOutput};
use crate::{install::Installer, package::read_package, registry::PackageStore};
use ilium_plugin_api::{grants::is_high_risk, manifest::CheckMode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) fn finish(mut records: Vec<Value>, exit: i32) -> CliOutput {
    let warnings = records
        .iter()
        .filter(|r| r["type"] == "warning" || (r["type"] == "check" && r["severity"] == "warn"))
        .count();
    let errors = records
        .iter()
        .filter(|r| r["type"] == "error" || (r["type"] == "check" && r["severity"] == "error"))
        .count();
    records.push(json!({"type":"summary","duration_ms":0,"warnings":warnings,"errors":errors}));
    records.push(json!({"type":"result","ok":exit == 0,"exit_code":exit}));
    render(records, exit)
}

/// The field order of every record type (reference-plugin-cli.md, the record catalogue); fields
/// a type does not list follow in alphabetical order.
const FIELD_ORDER: &[(&str, &[&str])] = &[
    ("progress", &["step", "message", "current", "total"]),
    ("warning", &["code", "message", "file", "line", "column"]),
    ("error", &["code", "message", "file", "line", "column", "key_path", "hint"]),
    ("artifact", &["kind", "path", "sha256", "bytes"]),
    ("check", &["rule", "severity", "message", "file", "line", "column", "key_path"]),
    ("lint", &["rule", "severity", "message", "file", "line", "column", "key_path", "fix", "suppressed"]),
    ("test-case", &["name", "fixture", "side", "status", "duration_ms", "diff", "reason"]),
    ("diff", &["subject", "expected", "actual", "first_difference"]),
    ("submission", &["index", "correlation", "intent", "dedup_key"]),
    ("outcome", &["correlation", "outcome", "code"]),
    ("reload", &["plugin", "side", "version", "restored_bytes", "restore_dropped", "build_ms"]),
    ("log", &["at", "plugin", "side", "level", "message"]),
    ("timing", &["plugin", "side", "export", "calls", "p50_ms", "p99_ms", "max_ms", "deadline_ms"]),
    ("trace", &["at", "plugin", "side", "kind", "detail"]),
    ("plugin", &["id", "version", "tier", "state", "dev", "sides", "grants_granted", "grants_requested"]),
    ("grant", &["plugin", "grant", "risk", "state"]),
    ("inspect", &["section", "data"]),
    ("doctor-check", &["item", "status", "message", "fix"]),
    ("summary", &["duration_ms", "warnings", "errors"]),
    ("result", &["ok", "exit_code"]),
];

pub(super) fn render(records: Vec<Value>, exit: i32) -> CliOutput {
    let mut raw = String::new();
    for record in &records {
        raw.push_str("{\"type\":");
        raw.push_str(&record["type"].to_string());
        if let Some(fields) = record.as_object() {
            let order: &[&str] = FIELD_ORDER
                .iter()
                .find(|(name, _)| record["type"] == *name)
                .map_or(&[], |(_, order)| order);
            let mut keys: Vec<&String> = fields.keys().filter(|key| key.as_str() != "type").collect();
            keys.sort_by_key(|key| {
                let rank = order
                    .iter()
                    .position(|name| *name == key.as_str())
                    .unwrap_or(order.len());
                (rank, key.as_str() == "details", (*key).clone())
            });
            for key in keys {
                raw.push(',');
                raw.push_str(&json!(key).to_string());
                raw.push(':');
                raw.push_str(&fields[key].to_string());
            }
        }
        raw.push_str("}\n");
    }
    CliOutput {
        exit_code: exit,
        stdout_jsonl: records,
        stdout_raw: raw.clone(),
        stdout_text: raw,
        stderr: String::new(),
    }
}
pub(super) fn failure(code: &str, message: impl Into<String>, exit: i32) -> CliOutput {
    finish(
        vec![json!({"type":"error","code":code,"message":message.into()})],
        exit,
    )
}

pub(super) fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    if let Some(flag) = args.iter().find(|arg| {
        arg.starts_with('-')
            && ![
                "--json",
                "--quiet",
                "--no-color",
                "--verify",
                "--memory",
                "--view",
            ]
            .contains(&arg.as_str())
    }) {
        return failure("usage.unknown-flag", format!("Unknown flag: {flag}"), 2);
    }
    if command == "inspect"
        && args
            .iter()
            .any(|arg| ["--view", "--memory"].contains(&arg.as_str()))
    {
        return super::devtools_command::inspect_live(args, env);
    }
    if command != "inspect"
        && args
            .iter()
            .any(|arg| ["--verify", "--memory", "--view"].contains(&arg.as_str()))
    {
        return failure(
            "usage.unknown-flag",
            "Inspection flags require the inspect command",
            2,
        );
    }
    if args.iter().filter(|arg| !arg.starts_with('-')).count() > 1 {
        return failure(
            "usage.argument-unexpected",
            "Only one package path is accepted",
            2,
        );
    }
    let Some(name) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return failure("usage.argument-missing", "A package path is required", 2);
    };
    let path = env.cwd.join(name);
    if command == "verify"
        && !name.contains(['/', '\\'])
        && !name.ends_with(".iliumpkg")
        && !path.is_file()
    {
        if let Some(context) = &env.audit_context {
            return super::verify_install::run(name, env, context.clone());
        }
    }
    if command == "check" && path.is_dir() {
        return check_directory(&path);
    }
    if !path.is_file() {
        let (code, exit) = if command == "inspect"
            && !name.contains('/')
            && !name.ends_with(".iliumpkg")
            && !name.ends_with(".wasm")
        {
            if env.session.is_none() {
                ("session.unreachable", 6)
            } else {
                ("inspect.id-unknown", 5)
            }
        } else if command == "verify" {
            ("plugin.not-found", 5)
        } else {
            ("inspect.file-unreadable", 1)
        };
        return failure(
            code,
            format!("No readable package file at {}", path.display()),
            exit,
        );
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => return failure("inspect.file-unreadable", error.to_string(), 1),
    };
    if command == "inspect" && bytes.starts_with(b"\0asm") {
        if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .is_err()
        {
            return failure("inspect.not-a-package", "Invalid WebAssembly binary", 1);
        }
        return finish(
            vec![
                json!({"type":"inspect","section":"component","data":{"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"name_section":super::component_has_name_section(&bytes)}}),
            ],
            0,
        );
    }
    let archive = match read_package(&bytes) { Ok(archive)=>archive,Err(errors)=>return finish(errors.into_iter().map(|error|json!({"type":"error","code":error.code.as_str(),"message":error.message,"key_path":error.pointer})).collect(),if command == "check" {3}else{1}) };
    let installer = Installer::new(PackageStore {
        root: env.data_dir.join("plugins"),
    });
    // Public keys of the trust store let stage 7 check a signature (the keys stay untrusted for
    // this read-only command, which grants no installation authority).
    let installer = installer.with_check_context(
        ilium_plugin_api::manifest::ValidationContext {
            tier: ilium_plugin_api::Tier::T3,
            mode: CheckMode::Check,
            platform: ilium_plugin_api::Platform::Linux,
            installed: Vec::new(),
            core_plugins: Vec::new(),
            unstable_features: ilium_plugin_api::abi::UNSTABLE_FEATURES
                .iter()
                .map(|feature| (*feature).into())
                .collect(),
        },
        trust_store_keys(env),
    );
    if command != "inspect" {
        let report = installer.check(&bytes, CheckMode::Check);
        let has_errors = report
            .diagnostics
            .iter()
            .any(|d| d.severity == ilium_plugin_api::Severity::Error);
        // Package files print `progress`, `warning` and `error` lines (reference-package-format.md
        // PKG-154, PKG-156); a key outside the trust store is a warning for `verify`.
        let mut records: Vec<Value> = Vec::new();
        if command == "verify" {
            const STAGES: [&str; 7] = ["archive", "manifest-parse", "schema", "semantic", "inventory", "components", "signature"];
            let reached = usize::from(report.last_stage).clamp(1, STAGES.len());
            for (index, name) in STAGES.iter().enumerate().take(reached) {
                records.push(json!({"type":"progress","stage":index + 1,"name":name}));
            }
        }
        let mut key_unknown = false;
        for d in &report.diagnostics {
            key_unknown |= d.code.as_str() == "trust.key-unknown";
            let kind = match d.severity {
                ilium_plugin_api::Severity::Error => "error",
                _ => "warning",
            };
            records.push(json!({"type":kind,"code":d.code.as_str(),"message":d.message,"key_path":d.pointer}));
        }
        let exit = if has_errors { if command == "check" { 3 } else { 1 } } else { 0 };
        if command == "verify" {
            let manifest_sha256 = archive
                .entries
                .iter()
                .find(|entry| entry.path == "manifest.json")
                .map(|entry| format!("{:x}", Sha256::digest(&entry.bytes)));
            let mut result = json!({
                "type":"result","ok":exit == 0,"exit_code":exit,
                "id":archive.manifest.id,"version":archive.manifest.version.to_string(),
                "sha256":format!("{:x}", Sha256::digest(&bytes)),"manifest_sha256":manifest_sha256,
                "tier":if key_unknown || has_errors { "untrusted" } else { "T2" },
                "signer_key_id":Value::Null,"publisher_id":Value::Null,
            });
            result["type"] = json!("result");
            let warnings = records.iter().filter(|r| r["type"] == "warning").count();
            let errors = records.iter().filter(|r| r["type"] == "error").count();
            records.push(json!({"type":"summary","duration_ms":0,"warnings":warnings,"errors":errors}));
            records.push(result);
            return render(records, exit);
        }
        return finish(records, exit);
    }
    let manifest = match archive
        .entries
        .iter()
        .find(|entry| entry.path == "manifest.json")
        .and_then(|entry| serde_json::from_slice::<Value>(&entry.bytes).ok())
    {
        Some(manifest) => manifest,
        None => return failure("manifest.json-invalid", "Manifest cannot be inspected", 1),
    };
    let mut records = vec![json!({"type":"inspect","section":"manifest","data":manifest})];
    if args.iter().any(|arg| arg == "--verify") {
        for expected in &archive.manifest.files {
            let actual = archive
                .entries
                .iter()
                .find(|entry| entry.path == expected.path)
                .map(|entry| {
                    (
                        entry.bytes.len() as u64,
                        format!("{:x}", Sha256::digest(&entry.bytes)),
                    )
                });
            if actual
                .as_ref()
                .is_none_or(|(length, hash)| *length != expected.length || *hash != expected.sha256)
            {
                records.push(json!({"type":"diff","subject":expected.path,"expected":{"bytes":expected.length,"sha256":expected.sha256},"actual":actual.map(|(length,hash)|json!({"bytes":length,"sha256":hash}))}));
            }
        }
        if records.iter().any(|record| record["type"] == "diff") {
            records.push(json!({"type":"error","code":"inspect.hash-mismatch","message":"Package bytes differ from the manifest inventory"}));
            return finish(records, 1);
        }
    }
    for (side, grants) in [
        ("server", &archive.manifest.capabilities.server),
        ("client", &archive.manifest.capabilities.client),
        ("scene", &archive.manifest.capabilities.scene),
    ] {
        for grant in grants {
            records.push(json!({"type":"grant","plugin":archive.manifest.id,"grant":grant.as_str(),"risk":if is_high_risk(grant.as_str()) {"high"}else{"normal"},"state":"requested","details":{"side":side}}));
        }
    }
    for entry in &archive.entries {
        records.push(json!({"type":"inspect","section":"file","data":{"path":entry.path,"bytes":entry.bytes.len(),"sha256":format!("{:x}",Sha256::digest(&entry.bytes))}}));
        if ["sbom.spdx.json", "provenance.json"].contains(&entry.path.as_str()) {
            records.push(json!({"type":"inspect","section":"provenance","data":{"path":entry.path,"content":String::from_utf8_lossy(&entry.bytes),"trusted":false}}));
        }
    }
    let trust_path = env
        .env
        .get("ILIUM_TRUST_STORE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| env.state_dir.join("trust.json"));
    let trust =
        crate::trust::TrustRepository::new(trust_path, crate::trust::TrustDocument::default())
            .and_then(|repository| repository.snapshot_at(env.now));
    let verification = trust.and_then(|snapshot| snapshot.verify_package(&archive));
    let (status, tier) = match verification {
        Ok(verified) => (json!({"verified":true,"key_id":verified.key_id}), "T2"),
        Err(error) => (
            json!({"verified":false,"code":error.code.as_str(),"key_id":archive.signature.as_ref().map(|signature|signature.key_id.as_str())}),
            "T3",
        ),
    };
    records.push(json!({"type":"inspect","section":"signature","data":status}));
    records.push(json!({"type":"inspect","section":"package","data":{"id":archive.manifest.id,"version":archive.manifest.version,"tier":tier}}));
    finish(records, 0)
}

pub(super) fn check_directory(directory: &Path) -> CliOutput {
    let manifest = match std::fs::read_to_string(directory.join("manifest.json")) {
        Ok(text) => text,
        Err(error) => return failure("manifest.file-unreadable", error.to_string(), 3),
    };
    let context = ilium_plugin_api::manifest::ValidationContext {
        tier: ilium_plugin_api::Tier::T3,
        mode: CheckMode::Check,
        platform: ilium_plugin_api::Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    };
    let diagnostics = ilium_plugin_api::manifest::validate_json(&manifest, &context);
    let failed = diagnostics
        .iter()
        .any(|d| d.severity == ilium_plugin_api::Severity::Error);
    let records = diagnostics.into_iter().map(|d|json!({"type":"check","rule":d.code.as_str(),"severity":match d.severity {ilium_plugin_api::Severity::Error=>"error",ilium_plugin_api::Severity::Warning=>"warn",_=>"info"},"message":d.message,"key_path":d.pointer})).collect();
    finish(records, if failed { 3 } else { 0 })
}

/// Adds the validation `stage` of the diagnostic to the first `error` record of a refused result.
pub(super) fn with_stage(output: CliOutput, stage: Option<u8>) -> CliOutput {
    let Some(stage) = stage else {
        return output;
    };
    let mut records = output.stdout_jsonl;
    if let Some(record) = records.iter_mut().find(|record| record["type"] == "error") {
        record["stage"] = json!(stage);
    }
    render(records, output.exit_code)
}

/// The public keys of every entry of the trust store, for read-only signature checking.
fn trust_store_keys(env: &CliEnv) -> Vec<crate::signing::TrustedKey> {
    let Some(document) = std::fs::read(super::keys_command::trust_store_path(env))
        .ok()
        .and_then(|raw| crate::trust::TrustDocument::parse(&raw).ok())
    else {
        return Vec::new();
    };
    document
        .keys
        .iter()
        .filter_map(|entry| {
            let public_key = base64::Engine::decode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                &entry.public_key,
            )
            .ok()?;
            Some(crate::signing::TrustedKey {
                key_id: entry.key_id.clone(),
                publisher_id: entry.publisher_id.clone(),
                public_key: crate::signing::PublicKey(public_key),
            })
        })
        .collect()
}
