//! Installed-file verification (SEC-153/156). Never instantiates or repairs a guest.
use std::{
    ffi::OsStr,
    io::Read,
    path::{Component, Path},
    time::UNIX_EPOCH,
};

use ilium_platform::secure_fs::NoFollowDirectory;
use ilium_plugin_api::{ids::parse_plugin_id, manifest::Diagnostic, Code};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{offline, CliEnv, CliOutput};
use crate::{
    audit::{AuditContext, AuditLog, AuditRecord, AuditRetention},
    registry::{EnableFlags, InstalledRegistry, PackageStore},
};

fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(Code::from_static(code), "", message)
}
fn io(error: impl std::fmt::Display) -> Diagnostic {
    self::error("verify.io", error.to_string())
}

/// Verify an installed ID using actual private roots and explicit host provenance.
/// Corruption disables the persisted user enable flag even if audit subsequently
/// fails; that failure is explicit and never reported as a successful transition.
/// A live instance must separately be stopped by its runtime owner.
pub fn run(id: &str, env: &CliEnv, audit_context: AuditContext) -> CliOutput {
    match verify(id, env, audit_context) {
        Ok(()) => offline::finish(
            vec![
                json!({"type":"check", "id":id, "code":"verify.ok", "severity":"info", "message":"Installed files match the recorded inventory"}),
            ],
            0,
        ),
        Err(diagnostic) => {
            let exit = if diagnostic.code.as_str() == "plugin.not-found" {
                5
            } else {
                1
            };
            offline::failure(diagnostic.code.as_str(), diagnostic.message, exit)
        }
    }
}
fn verify(id: &str, env: &CliEnv, audit_context: AuditContext) -> Result<(), Diagnostic> {
    let plugin = parse_plugin_id(id)?;
    for root in [&env.data_dir, &env.state_dir, &env.config_dir] {
        validate_absolute(root)?;
    }
    if audit_context.actor.kind != "cli" || audit_context.session.is_empty() {
        return Err(error(
            "audit.context-required",
            "Installed verification requires actual CLI audit provenance",
        ));
    }
    let root = env.data_dir.join("plugins");
    match directory(&root, false) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(error("plugin.not-found", "Plugin is not installed"))
        }
        Err(e) => return Err(io(e)),
    }
    let registry = InstalledRegistry::open(&root.join("installed.json")).map_err(io)?;
    // Keep the active version and exact hash baseline stable throughout verification
    // and the persisted disable operation, excluding concurrent update/uninstall.
    let transaction = registry.transaction().map_err(io)?;
    let row = transaction
        .record(id)
        .ok_or_else(|| error("plugin.not-found", "Plugin is not installed"))?;
    let version = row["active_version"]
        .as_str()
        .ok_or_else(|| error("verify.context-required", "Missing installed version"))?;
    let path = PackageStore {
        root: root.join("store"),
    }
    .checked_version_dir(&plugin, version)
    .map_err(io)?;
    let result = verify_files(&path, row, id, version);
    if let Err(corruption) = result {
        directory(&env.config_dir, true).map_err(io)?;
        let mut flags = EnableFlags::open(&env.config_dir.join("config.toml")).map_err(io)?;
        flags.set_enabled(&plugin, false).map_err(io)?;
        directory(&env.state_dir, true).map_err(io)?;
        let now = u64::try_from(env.now.duration_since(UNIX_EPOCH).map_err(io)?.as_millis())
            .map_err(io)?;
        let audit_result = (|| {
            let mut audit = AuditLog::open_with_context(
                env.state_dir.join("audit/audit.jsonl"),
                Some(audit_context),
                AuditRetention::default(),
            )
            .map_err(io)?;
            let tier = if row["source"] == "bundled" {
                "t1"
            } else if row["key_id"].is_string() {
                "t2"
            } else {
                "t3"
            };
            audit.append_at(&AuditRecord { at_unix_millis:now, plugin:id.into(), action:"integrity-failure".into(),
                detail_json:json!({"version":version,"tier":tier,"result":"failed","reason":"manifest.file-hash",
                    "package_sha256":row["package_sha256"],"target":corruption.message}).to_string() },now).map_err(io)?;
            let status = audit.status();
            if status.queued != 0
                || status.write_failed
                || status.durability_uncertain
                || status.dropped != 0
            {
                return Err(error(
                    "audit.io",
                    "Integrity failure audit record is not durable",
                ));
            }
            Ok(())
        })();
        if let Err(audit_error) = audit_result {
            return Err(error("audit.io", format!("Files changed after install; enable flag persisted false, but integrity audit failed: {}",audit_error.message)));
        }
        return Err(error(
            "manifest.file-hash",
            format!(
                "Files changed after install; plugin disabled: {}",
                corruption.message
            ),
        ));
    }
    Ok(())
}
fn verify_files(path: &Path, row: &Value, id: &str, version: &str) -> Result<(), Diagnostic> {
    let installed = directory(path, false).map_err(io)?;
    let bytes = read_bounded(&installed, "package.iliumpkg", 300 * 1024 * 1024)?;
    if hash(&bytes) != row["package_sha256"].as_str().unwrap_or_default() {
        return Err(error("manifest.file-hash", "Stored package bytes changed"));
    }
    let package = crate::package::read_package(&bytes).map_err(|errors| {
        error(
            "manifest.file-hash",
            format!("Stored package no longer parses: {errors:?}"),
        )
    })?;
    if package.manifest.id != id || package.manifest.version != version {
        return Err(error(
            "manifest.file-hash",
            "Stored package identity differs from installed record",
        ));
    }
    let manifest = package
        .entries
        .iter()
        .find(|entry| entry.path == "manifest.json")
        .ok_or_else(|| error("manifest.file-hash", "Stored manifest is missing"))?;
    if hash(&manifest.bytes) != row["manifest_sha256"].as_str().unwrap_or_default() {
        return Err(error(
            "manifest.file-hash",
            "Manifest differs from installed hash",
        ));
    }
    for entry in &package.entries {
        let mut file = relative_file(&installed, &entry.path).map_err(io)?;
        if file.metadata().map_err(io)?.len() != entry.bytes.len() as u64 {
            return Err(error(
                "manifest.file-hash",
                format!("{} length changed", entry.path),
            ));
        }
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let read = file.read(&mut buffer).map_err(io)?;
            if read == 0 {
                break;
            }
            count = count.saturating_add(read as u64);
            if count > entry.bytes.len() as u64 {
                return Err(error(
                    "manifest.file-hash",
                    format!("{} grew during verification", entry.path),
                ));
            }
            digest.update(&buffer[..read]);
        }
        if count != entry.bytes.len() as u64
            || format!("{:x}", digest.finalize()) != hash(&entry.bytes)
        {
            return Err(error(
                "manifest.file-hash",
                format!("{} bytes changed", entry.path),
            ));
        }
    }
    // The archive is the exact registry-bound baseline. Validate its inventory too,
    // rather than treating an archive payload as an arbitrary list of expected files.
    for expected in &package.manifest.files {
        let entry = package
            .entries
            .iter()
            .find(|entry| entry.path == expected.path)
            .ok_or_else(|| error("manifest.file-hash", "Inventory entry is absent"))?;
        if entry.bytes.len() as u64 != expected.length || hash(&entry.bytes) != expected.sha256 {
            return Err(error(
                "manifest.file-hash",
                format!("{} does not match inventory", expected.path),
            ));
        }
    }
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_bounded(
    directory: &NoFollowDirectory,
    name: &str,
    bound: u64,
) -> Result<Vec<u8>, Diagnostic> {
    let file = directory.open_regular(OsStr::new(name)).map_err(io)?;
    if file.metadata().map_err(io)?.len() > bound {
        return Err(error(
            "manifest.file-hash",
            "Stored package exceeds size bound",
        ));
    }
    let mut bytes = Vec::new();
    file.take(bound + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > bound {
        return Err(error(
            "manifest.file-hash",
            "Stored package grew beyond size bound",
        ));
    }
    Ok(bytes)
}
fn relative_file(root: &NoFollowDirectory, path: &str) -> std::io::Result<std::fs::File> {
    let parts: Vec<_> = Path::new(path).components().collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.contains('\\')
    {
        return Err(std::io::Error::other("Unsafe inventory path"));
    }
    let mut directory = None;
    for part in &parts[..parts.len() - 1] {
        let parent = directory.as_ref().unwrap_or(root);
        directory = Some(parent.open_directory(part.as_os_str())?);
    }
    directory
        .as_ref()
        .unwrap_or(root)
        .open_regular(parts[parts.len() - 1].as_os_str())
}
fn validate_absolute(path: &Path) -> Result<(), Diagnostic> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(error(
            "verify.context-required",
            "Absolute roots without dot traversal are required",
        ));
    }
    Ok(())
}
fn directory(path: &Path, create: bool) -> std::io::Result<NoFollowDirectory> {
    let root = path
        .ancestors()
        .last()
        .ok_or_else(|| std::io::Error::other("Missing filesystem root"))?;
    let mut directory = NoFollowDirectory::open_root(root)?;
    for component in path.components() {
        if let Component::Normal(name) = component {
            if create {
                directory.create_directory_if_missing(name)?;
            }
            directory = directory.open_directory(name)?;
        }
    }
    Ok(directory)
}
