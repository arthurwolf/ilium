//! `ilium plugin pack` (reference-plugin-cli.md DEV-095..102, reference-package-format.md
//! PKG-143..147). Packing is a pure function of the project and its `dist/build.json`.

use super::{offline, CliEnv, CliOutput};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SIZE_CAP_WARNING: usize = 100 * 1024 * 1024;
const ROOT_DOCUMENTS: [&str; 8] = [
    "README.md",
    "CHANGELOG.md",
    "LICENSE",
    "NOTICE",
    "sbom.json",
    "build-info.json",
    "sbom.spdx.json",
    "provenance.json",
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn skipped(name: &str) -> bool {
    name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
        || matches!(name, "target" | "node_modules" | "dist")
}

/// Collects `(archive path, bytes)` under `folder`, reporting skipped names as warnings.
fn collect(
    root: &Path,
    folder: &Path,
    prefix: &str,
    files: &mut Vec<(String, Vec<u8>)>,
    warnings: &mut Vec<Value>,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(folder)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let archive = format!("{prefix}/{name}");
        if skipped(&name) {
            warnings.push(json!({"type":"warning","code":"pack.file-skipped",
                "message":format!("skipped {archive}")}));
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(root, &path, &archive, files, warnings)?;
        } else if kind.is_file() {
            files.push((archive, std::fs::read(&path)?));
        }
    }
    Ok(())
}

pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let mut positional = Vec::new();
    let mut out_arg = None;
    let (mut include_src, mut allow_debug) = (false, false);
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--include-src" => include_src = true,
            "--allow-debug" => allow_debug = true,
            "--out" => match iterator.next() {
                Some(value) => out_arg = Some(value.clone()),
                None => return offline::failure("usage.missing-argument", "--out needs a value", 2),
            },
            flag if flag.starts_with("--") => {
                return offline::failure("usage.unknown-flag", format!("Unknown flag: {flag}"), 2)
            }
            _ => positional.push(argument.clone()),
        }
    }
    let project = match positional.first() {
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        Some(dir) => env.cwd.join(dir),
        None => env.cwd.clone(),
    };
    let manifest_raw = match std::fs::read_to_string(project.join("manifest.json")) {
        Ok(text) => text,
        Err(error) => {
            return offline::failure("pack.manifest-missing", format!("manifest.json: {error}"), 1)
        }
    };
    let mut manifest: Value = match serde_json::from_str(&manifest_raw) {
        Ok(value) => value,
        Err(error) => return offline::failure("manifest.schema", error.to_string(), 1),
    };
    let (Some(id), Some(version)) = (
        manifest["id"].as_str().map(str::to_owned),
        manifest["version"].as_str().map(str::to_owned),
    ) else {
        return offline::failure("manifest.schema", "manifest id and version are required", 1);
    };
    let not_built = |message: &str| offline::failure("pack.not-built", message.to_owned(), 1);
    let build: Value = match std::fs::read(project.join("dist/build.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
    {
        Some(build) => build,
        None => return not_built("dist/build.json is missing; run ilium plugin build"),
    };
    match super::project_inputs::project_input_hash(&project) {
        Ok(hash) if build["input_hash"].as_str() == Some(hash.as_str()) => {}
        Ok(_) => {
            return offline::failure(
                "pack.stale-build",
                "the sources changed since the last build; run ilium plugin build",
                1,
            )
        }
        Err(message) => return offline::failure("pack.stale-build", message, 1),
    }
    let debug = build["profile"].as_str() == Some("debug");
    let mut warnings = Vec::new();
    if debug {
        if !allow_debug {
            return offline::failure(
                "pack.debug-build",
                "the build is a debug build; pass --allow-debug to pack it anyway",
                1,
            );
        }
        warnings.push(json!({"type":"warning","code":"pack.debug-build",
            "message":"packing a debug build"}));
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut sides: Vec<String> = build["sides"]
        .as_array()
        .map(|list| {
            list.iter().filter_map(|side| side["name"].as_str().map(str::to_owned)).collect()
        })
        .unwrap_or_default();
    sides.sort();
    for side in &sides {
        let name = format!("{side}.wasm");
        match std::fs::read(project.join("dist").join(&name)) {
            Ok(bytes) => files.push((name, bytes)),
            Err(_) => return not_built(&format!("dist/{name} is missing; run ilium plugin build")),
        }
    }
    let progress = |path: &str| json!({"type":"progress","step":"inventory","message":path});
    let mut records: Vec<Value> = Vec::new();
    for folder in ["assets", "help", "sounds", "screenshots"] {
        let path = project.join(folder);
        if path.is_dir() {
            if let Err(error) = collect(&project, &path, folder, &mut files, &mut warnings) {
                return offline::failure("pack.read-failed", error.to_string(), 1);
            }
        }
    }
    if include_src {
        let path = project.join("src");
        if path.is_dir() {
            if let Err(error) = collect(&project, &path, "src", &mut files, &mut warnings) {
                return offline::failure("pack.read-failed", error.to_string(), 1);
            }
        }
        if debug {
            for side in &sides {
                let map = project.join("dist").join(format!("{side}.wasm.map"));
                if let Ok(bytes) = std::fs::read(&map) {
                    files.push((format!("src/{side}.wasm.map"), bytes));
                }
            }
        }
    }
    for document in ROOT_DOCUMENTS {
        if let Ok(bytes) = std::fs::read(project.join(document)) {
            files.push((document.to_owned(), bytes));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files.dedup_by(|left, right| left.0 == right.0);
    let listing: Vec<Value> = files
        .iter()
        .map(|(path, bytes)| {
            records.push(progress(path));
            json!({"path":path,"length":bytes.len(),"sha256":hex(&Sha256::digest(bytes))})
        })
        .collect();
    if let Some(object) = manifest.as_object_mut() {
        object.insert("files".into(), Value::Array(listing));
    }
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
    manifest_bytes.push(b'\n');
    files.push(("manifest.json".into(), manifest_bytes));
    let total: usize = files.iter().map(|(_, bytes)| bytes.len()).sum();
    if total > SIZE_CAP_WARNING {
        warnings.push(json!({"type":"warning","code":"pack.size-cap",
            "message":"the package is over 100 MiB"}));
    }
    let package = match crate::package::write_package(&files) {
        Ok(package) => package,
        Err(diagnostic) => {
            return offline::failure(diagnostic.code.as_str(), diagnostic.message, 1)
        }
    };
    let relative = format!("dist/{id}-{version}.iliumpkg");
    let target = match &out_arg {
        Some(out) if Path::new(out).is_absolute() => PathBuf::from(out),
        Some(out) => env.cwd.join(out),
        None => project.join(&relative),
    };
    if target.exists() {
        let same_package = std::fs::read(&target)
            .ok()
            .and_then(|raw| crate::package::read_package(&raw).ok())
            .is_some_and(|existing| existing.manifest.id == id);
        if !same_package {
            return offline::failure(
                "pack.out-exists",
                format!("{} exists and is not a package of {id}", target.display()),
                1,
            );
        }
    }
    let written = target
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            let temporary = target.with_extension("iliumpkg.tmp");
            std::fs::write(&temporary, &package)?;
            std::fs::rename(&temporary, &target)
        });
    if let Err(error) = written {
        return offline::failure("pack.write-failed", error.to_string(), 1);
    }
    let shown = match &out_arg {
        None => relative,
        Some(_) => target
            .strip_prefix(&project)
            .map_or_else(|_| target.to_string_lossy().into_owned(), |p| p.to_string_lossy().into_owned()),
    };
    records.extend(warnings);
    records.push(json!({"type":"artifact","kind":"package","path":shown,
        "sha256":hex(&Sha256::digest(&package)),"bytes":package.len()}));
    offline::finish(records, 0)
}
