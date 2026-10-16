//! Helpers of the port twins. Every item the skeleton lacks is named in ../API_GAPS.md.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_worktrees::WorktreesServer;
use ilium_plugin_sdk::host::process::ProcessExit;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::*;

pub fn stdout_exit(text: &str) -> ProcessExit {
    ProcessExit { status: Some(0), signal: None, stdout: text.as_bytes().to_vec(), stderr: Vec::new() }
}

/// A server host whose project (main checkout) is `root`; every git run succeeds with empty output.
pub fn server_host_at(root: &str) -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").project_root(PathBuf::from(root)).build();
    host.process().default_exit(ok_exit());
    host.activate::<WorktreesServer>();
    host
}

/// The repository facts of the original dialog tests: branches `main` and `agent/task`, checked out `topic`.
pub fn with_repository_facts(host: &mut FakeHost) {
    host.process().on_args(&["branch", "--format=%(refname:short)"], stdout_exit("main\nagent/task\n"));
    host.process().on_args(&["rev-parse", "--abbrev-ref", "HEAD"], stdout_exit("topic\n"));
    host.process().on_args(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], stdout_exit("origin/main\n"));
}

pub fn add_arguments(host: &mut FakeHost) -> Vec<String> {
    let calls = worktree_add_calls(&host.process().calls());
    assert_eq!(calls.len(), 1, "exactly one git worktree add");
    calls[0].arguments.clone()
}

/// The latest `changed` publication as JSON.
pub fn last_changed(host: &mut FakeHost) -> Value {
    let published = host.published(TOPIC_CHANGED);
    serde_json::from_str(published.last().expect("a changed publication")).expect("JSON payload")
}

pub fn removal_calls(host: &mut FakeHost) -> Vec<Vec<String>> {
    host.process().calls().into_iter().filter(|c| c.program == "git" && c.arguments.get(1).map(String::as_str) == Some("remove")).map(|c| c.arguments).collect()
}

pub fn remove_request(host: &mut FakeHost, payload: Value) {
    host.deliver_one(channel_message("remove", payload.to_string().into_bytes()));
    host.settle();
}

pub fn request(branch: &str) -> Value {
    json!({ "branch": branch })
}
