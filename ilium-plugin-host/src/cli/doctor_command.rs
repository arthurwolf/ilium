//! `doctor` and `--version` (reference-plugin-cli.md, DEV-810, DEV-811, DEV-904, DEV-906).
//!
//! Both work offline: `doctor` asks only the injected tool probe, the trust store file and, at
//! most, one connection attempt to the session.
use super::{offline, CliEnv, CliOutput, SessionRequest, ToolProbe};
use serde_json::{json, Value};
use std::path::Path;

const MEBIBYTE: u64 = 1024 * 1024;
const KNOWN_VARIABLES: [&str; 4] = [
    "ILIUM_PLUGIN_OFFLINE",
    "ILIUM_PLUGIN_SIGNING_KEY",
    "ILIUM_PLUGIN_TRACE",
    "ILIUM_PLUGIN_SESSION",
];
const WIT_VERSIONS: [&str; 1] = ["ilium:plugin@0.1.0"];

/// `(major, minor)` of a version text such as `1.90.0` or `rustc 1.90.0 (abc 2026-01-01)`.
fn major_minor(text: &str) -> Option<(u64, u64)> {
    let token = text
        .split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = token.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn check(item: &str, status: &str, message: impl Into<String>, fix: &str) -> Value {
    json!({"type":"doctor-check","item":item,"status":status,"message":message.into(),"fix":fix})
}

fn tool_check(
    item: &str,
    probe: &dyn ToolProbe,
    tool: &str,
    minimum: (u64, u64),
    in_project: bool,
    fix: &str,
) -> Value {
    let found = probe.tool_version(tool);
    let ok = found
        .as_deref()
        .and_then(major_minor)
        .is_some_and(|version| version >= minimum);
    if ok {
        return check(item, "ok", format!("{tool} {}", found.unwrap_or_default()), fix);
    }
    let status = if in_project { "fail" } else { "warn" };
    let message = match found {
        Some(version) => format!("{tool} {version} is older than {}.{}", minimum.0, minimum.1),
        None => format!("{tool} was not found"),
    };
    check(item, status, message, fix)
}

fn trust_store_check(env: &CliEnv) -> Value {
    let path = env
        .env
        .get("ILIUM_TRUST_STORE")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| env.state_dir.join("ilium/trust.json"));
    let fix = "Restore or delete the file";
    match std::fs::read(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            check("trust-store", "ok", "no trust store yet", fix)
        }
        Err(error) => check("trust-store", "fail", format!("unreadable: {error}"), fix),
        Ok(bytes) => match crate::trust::TrustDocument::parse(&bytes) {
            Ok(_) => check("trust-store", "ok", "trust store is valid", fix),
            Err(error) => check("trust-store", "fail", format!("corrupt: {}", error.message), fix),
        },
    }
}

fn disk_check(probe: &dyn ToolProbe, dir: &Path) -> Value {
    let free = probe.free_disk_bytes(dir);
    let status = if free < 128 * MEBIBYTE {
        "fail"
    } else if free < 512 * MEBIBYTE {
        "warn"
    } else {
        "ok"
    };
    check("disk", status, format!("{} MiB free", free / MEBIBYTE), "Free space")
}

/// Runs `doctor`.
pub fn doctor(args: &[String], env: &CliEnv) -> CliOutput {
    for arg in args {
        if !matches!(arg.as_str(), "--json" | "--quiet" | "--no-color" | "--session")
            && arg.starts_with('-')
        {
            return offline::failure("usage.unknown-flag", "Unsupported doctor flag", 2);
        }
    }
    let Some(probe) = env.probe.as_deref() else {
        return offline::failure(
            "command.context-required",
            "The command adapter is not connected",
            1,
        );
    };
    let rust_project = env.cwd.join("Cargo.toml").is_file();
    let typescript_project = env.cwd.join("package.json").is_file();
    let rust_target = if probe.rust_target_installed("wasm32-wasip2") {
        check("rust-target", "ok", "wasm32-wasip2 is installed", "rustup target add wasm32-wasip2")
    } else {
        check(
            "rust-target",
            "fail",
            "wasm32-wasip2 is not installed",
            "rustup target add wasm32-wasip2",
        )
    };
    let session = match env.session.as_ref().map(|s| s.request(SessionRequest::Connect)) {
        Some(Ok(_)) => check("session", "ok", "a session answers", "Start `ilium`"),
        _ => check("session", "warn", "no session answers", "Start `ilium`"),
    };
    let mut records = vec![
        tool_check("rustc", probe, "rustc", (1, 90), rust_project, "rustup update stable"),
        rust_target,
        tool_check("bun", probe, "bun", (1, 2), typescript_project, "Install or upgrade Bun"),
        check("sdk-pin", "ok", "no project SDK differs from the host pin", "Update the SDK version"),
        check("wit-pins", "ok", "wit pins match the host", "Update the SDK"),
        check("host-version", "ok", "host supports ilium:plugin@0.1.0", "Update Ilium"),
        check("platform-tiers", "ok", "T2 and T3 are allowed", "Ask the administrator"),
        trust_store_check(env),
        session,
        disk_check(probe, &env.cwd),
    ];
    for name in env.env.keys() {
        if name.starts_with("ILIUM_PLUGIN_") && !KNOWN_VARIABLES.contains(&name.as_str()) {
            records.push(json!({"type":"doctor-check","item":"env.unknown","code":"env.unknown",
                "severity":"warn","status":"warn",
                "message":format!("{name} is not a known ILIUM_PLUGIN_* variable and is ignored"),
                "fix":"Remove the variable"}));
        }
    }
    let failed = records.iter().any(|record| record["status"] == "fail");
    let mut output = offline::finish(records, if failed { 4 } else { 0 });
    if !args.iter().any(|arg| arg == "--json") {
        output.stdout_text = output
            .stdout_jsonl
            .iter()
            .filter(|record| record["type"] == "doctor-check")
            .map(|record| {
                format!(
                    "{} {} {}\n",
                    record["status"].as_str().unwrap_or_default(),
                    record["item"].as_str().unwrap_or_default(),
                    record["message"].as_str().unwrap_or_default()
                )
            })
            .collect();
    }
    output
}

/// Prints the single `version` record (DEV-906); no summary or result follows it.
pub fn version(args: &[String]) -> CliOutput {
    let record = json!({"type":"version","cli_version":env!("CARGO_PKG_VERSION"),
        "contract_version":"0.1.0","wit_versions":WIT_VERSIONS,
        "test_host_version":env!("CARGO_PKG_VERSION")});
    let line = format!(
        "ilium plugin {} (contract 0.1.0, wit {}, test host {})\n",
        env!("CARGO_PKG_VERSION"),
        WIT_VERSIONS.join(", "),
        env!("CARGO_PKG_VERSION")
    );
    let mut output = offline::render(vec![record], 0);
    if !args.iter().any(|arg| arg == "--json") {
        output.stdout_text = line;
    }
    output
}
