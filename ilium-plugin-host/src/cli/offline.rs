//! Offline file inspection. No guest is instantiated and no session is started.
use std::path::Path;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use super::{CliEnv, CliOutput};
use crate::{install::Installer, package::read_package, registry::PackageStore};
use ilium_plugin_api::{manifest::CheckMode, grants::is_high_risk};

pub(super) fn finish(mut records: Vec<Value>, exit: i32) -> CliOutput {
    let warnings = records.iter().filter(|r| r["type"] == "warning" || (r["type"] == "check" && r["severity"] == "warn")).count();
    let errors = records.iter().filter(|r| r["type"] == "error" || (r["type"] == "check" && r["severity"] == "error")).count();
    records.push(json!({"type":"summary","duration_ms":0,"warnings":warnings,"errors":errors}));
    records.push(json!({"type":"result","ok":exit == 0,"exit_code":exit}));
    render(records, exit)
}

pub(super) fn render(records: Vec<Value>, exit: i32) -> CliOutput {
    let mut raw = String::new();
    for record in &records {
        // serde_json's default map sorts keys; the CLI contract puts type first.
        raw.push_str("{\"type\":");
        raw.push_str(&record["type"].to_string());
        if let Some(fields) = record.as_object() {
            for (key,value) in fields.iter().filter(|(key,_)| key.as_str() != "type") {
                raw.push(','); raw.push_str(&json!(key).to_string()); raw.push(':'); raw.push_str(&value.to_string());
            }
        }
        raw.push_str("}\n");
    }
    CliOutput { exit_code:exit, stdout_jsonl:records, stdout_raw:raw.clone(), stdout_text:raw, stderr:String::new() }
}
pub(super) fn failure(code: &str, message: impl Into<String>, exit: i32) -> CliOutput {
    finish(vec![json!({"type":"error","code":code,"message":message.into()})],exit)
}

pub(super) fn run(command: &str, args: &[String], env: &CliEnv) -> CliOutput {
    if let Some(flag) = args.iter().find(|arg| arg.starts_with('-') && !["--json","--quiet","--no-color","--verify","--memory","--view"].contains(&arg.as_str())) {
        return failure("usage.unknown-flag",format!("Unknown flag: {flag}"),2);
    }
    if args.iter().any(|arg| ["--view","--memory"].contains(&arg.as_str())) {
        return failure(if env.session.is_none() {"session.unreachable"}else{"command.context-required"},"Live inspection requires its session adapter",if env.session.is_none() {6}else{1});
    }
    if command != "inspect" && args.iter().any(|arg| ["--verify", "--memory", "--view"].contains(&arg.as_str())) {
        return failure("usage.unknown-flag", "Inspection flags require the inspect command", 2);
    }
    if args.iter().filter(|arg| !arg.starts_with('-')).count() > 1 {
        return failure("usage.argument-unexpected", "Only one package path is accepted", 2);
    }
    let Some(name) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return failure("usage.argument-missing","A package path is required",2);
    };
    let path = env.cwd.join(name);
    if command == "verify" && !name.contains(['/', '\\']) && !name.ends_with(".iliumpkg") && !path.is_file() {
        if let Some(context) = &env.audit_context {
            return super::verify_install::run(name, env, context.clone());
        }
    }
    if command == "check" && path.is_dir() {
        return check_directory(&path);
    }
    if !path.is_file() {
        let (code,exit) = if command == "inspect" && !name.contains('/') && !name.ends_with(".iliumpkg") && !name.ends_with(".wasm") {
            if env.session.is_none() { ("session.unreachable",6) } else { ("inspect.id-unknown",5) }
        } else if command == "verify" { ("plugin.not-found",5) } else { ("inspect.file-unreadable",1) };
        return failure(code,format!("No readable package file at {}",path.display()),exit);
    }
    let bytes = match std::fs::read(&path) { Ok(bytes)=>bytes,Err(error)=>return failure("inspect.file-unreadable",error.to_string(),1) };
    if command == "inspect" && bytes.starts_with(b"\0asm") {
        if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all()).validate_all(&bytes).is_err() {
            return failure("inspect.not-a-package","Invalid WebAssembly binary",1);
        }
        return finish(vec![json!({"type":"inspect","section":"component","data":{"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"name_section":super::component_has_name_section(&bytes)}})],0);
    }
    let archive = match read_package(&bytes) { Ok(archive)=>archive,Err(errors)=>return finish(errors.into_iter().map(|error|json!({"type":"error","code":error.code.as_str(),"message":error.message,"key_path":error.pointer})).collect(),if command == "check" {3}else{1}) };
    let installer = Installer::new(PackageStore {root:env.data_dir.join("plugins")});
    if command != "inspect" {
        let report = installer.check(&bytes,CheckMode::Check);
        let has_errors = report.diagnostics.iter().any(|d| d.severity == ilium_plugin_api::Severity::Error);
        let records = report.diagnostics.into_iter().map(|d| json!({"type":"check","rule":d.code.as_str(),"severity":match d.severity {ilium_plugin_api::Severity::Error=>"error",ilium_plugin_api::Severity::Warning=>"warn",_=>"info"},"message":d.message,"key_path":d.pointer})).collect();
        return finish(records,if has_errors {if command == "check" {3}else{1}}else{0});
    }
    let manifest = match archive.entries.iter().find(|entry|entry.path=="manifest.json").and_then(|entry|serde_json::from_slice::<Value>(&entry.bytes).ok()) {
        Some(manifest)=>manifest,None=>return failure("manifest.json-invalid","Manifest cannot be inspected",1),
    };
    let mut records = vec![json!({"type":"inspect","section":"manifest","data":manifest})];
    if args.iter().any(|arg|arg == "--verify") {
        for expected in &archive.manifest.files {
            let actual = archive.entries.iter().find(|entry|entry.path==expected.path)
                .map(|entry|(entry.bytes.len() as u64,format!("{:x}",Sha256::digest(&entry.bytes))));
            if actual.as_ref().is_none_or(|(length,hash)| *length != expected.length || *hash != expected.sha256) {
                records.push(json!({"type":"diff","subject":expected.path,"expected":{"bytes":expected.length,"sha256":expected.sha256},"actual":actual.map(|(length,hash)|json!({"bytes":length,"sha256":hash}))}));
            }
        }
        if records.iter().any(|record|record["type"]=="diff") {
            records.push(json!({"type":"error","code":"inspect.hash-mismatch","message":"Package bytes differ from the manifest inventory"}));
            return finish(records,1);
        }
    }
    for (side,grants) in [("server",&archive.manifest.capabilities.server),("client",&archive.manifest.capabilities.client),("scene",&archive.manifest.capabilities.scene)] {
        for grant in grants { records.push(json!({"type":"grant","plugin":archive.manifest.id,"grant":grant.as_str(),"risk":if is_high_risk(grant.as_str()) {"high"}else{"normal"},"state":"requested","details":{"side":side}})); }
    }
    for entry in &archive.entries {
        records.push(json!({"type":"inspect","section":"file","data":{"path":entry.path,"bytes":entry.bytes.len(),"sha256":format!("{:x}",Sha256::digest(&entry.bytes))}}));
        if ["sbom.spdx.json","provenance.json"].contains(&entry.path.as_str()) {
            records.push(json!({"type":"inspect","section":"provenance","data":{"path":entry.path,"content":String::from_utf8_lossy(&entry.bytes),"trusted":false}}));
        }
    }
    let trust_path = env.env.get("ILIUM_TRUST_STORE").map(std::path::PathBuf::from).unwrap_or_else(||env.state_dir.join("trust.json"));
    let trust = crate::trust::TrustRepository::new(trust_path,crate::trust::TrustDocument::default()).and_then(|repository|repository.snapshot_at(env.now));
    let verification = trust.and_then(|snapshot|snapshot.verify_package(&archive));
    let (status,tier) = match verification {
        Ok(verified)=>(json!({"verified":true,"key_id":verified.key_id}),"T2"),
        Err(error)=>(json!({"verified":false,"code":error.code.as_str(),"key_id":archive.signature.as_ref().map(|signature|signature.key_id.as_str())}),"T3"),
    };
    records.push(json!({"type":"inspect","section":"signature","data":status}));
    records.push(json!({"type":"inspect","section":"package","data":{"id":archive.manifest.id,"version":archive.manifest.version,"tier":tier}}));
    finish(records,0)
}

pub(super) fn check_directory(directory: &Path) -> CliOutput {
    let manifest = match std::fs::read_to_string(directory.join("manifest.json")) {Ok(text)=>text,Err(error)=>return failure("manifest.file-unreadable",error.to_string(),3)};
    let context = ilium_plugin_api::manifest::ValidationContext {tier:ilium_plugin_api::Tier::T3,mode:CheckMode::Check,platform:ilium_plugin_api::Platform::Linux,installed:Vec::new(),core_plugins:Vec::new(),unstable_features:Vec::new()};
    let diagnostics = ilium_plugin_api::manifest::validate_json(&manifest,&context);
    let failed = diagnostics.iter().any(|d|d.severity==ilium_plugin_api::Severity::Error);
    let records = diagnostics.into_iter().map(|d|json!({"type":"check","rule":d.code.as_str(),"severity":match d.severity {ilium_plugin_api::Severity::Error=>"error",ilium_plugin_api::Severity::Warning=>"warn",_=>"info"},"message":d.message,"key_path":d.pointer})).collect();
    finish(records,if failed {3}else{0})
}
