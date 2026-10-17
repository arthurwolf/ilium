//! Helpers of part 2 (install failures, uninstall, enable/disable, list, grants, dev, logs).
//! Session fake methods that the skeleton lacks are listed in API_GAPS_p2.md.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ilium_plugin_host::cli::{CliEnv, CliOutput, MemorySession};

use crate::common::*;

/// A low-risk grant the grant catalogue knows (see API_GAPS_p2.md: adjust to the grants page).
pub const EXTRA_LOW_RISK_GRANT: &str = "workspace.read";

pub fn s(path: &Path) -> &str {
    path.to_str().expect("utf8")
}

/// Packs the `server` project (id acme.demo) and installs it unsigned with `--yes`.
pub fn install_demo(env: &CliEnv, root: &Path) -> PathBuf {
    let (_, package) = packed_project(env, root);
    let output = run(env, &["install", s(&package), "--allow-unsigned", "--yes"]);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
    package
}

/// A fresh session, an environment attached to it, and the installed demo plugin.
pub fn demo_session(root: &Path) -> (Arc<MemorySession>, CliEnv) {
    let session = MemorySession::new();
    let env = env_session(root, &session);
    install_demo(&env, root);
    (session, env)
}

/// The `both` (server and client) project, id acme.both.
pub fn both_project(env: &CliEnv, root: &Path) -> PathBuf {
    new_project(env, root, "both", "both", "rust")
}

/// Every message text of the error records.
pub fn error_messages(output: &CliOutput) -> Vec<String> {
    recs(output, "error")
        .iter()
        .filter_map(|record| {
            record
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_owned)
        })
        .collect()
}

/// Position of every key in a raw JSONL line, in the order given; true when ascending.
pub fn keys_in_order(raw_line: &str, keys: &[&str]) -> bool {
    let mut last = 0usize;
    for key in keys {
        let needle = format!("\"{key}\"");
        match raw_line.find(&needle) {
            Some(position) if position >= last => last = position,
            _ => return false,
        }
    }
    true
}

/// The raw stdout lines of one record type.
pub fn raw_lines(output: &CliOutput, record_type: &str) -> Vec<String> {
    output
        .stdout_raw
        .lines()
        .filter(|line| line.contains(&format!("\"type\":\"{record_type}\"")))
        .map(str::to_owned)
        .collect()
}
