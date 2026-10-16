//! Shared helpers of the Progress feature tests (manual page core-plugin-progress.md).
//!
//! Every use of a testkit item that the skeleton lacks lives in the "gap helpers" section at the
//! end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_progress::{ProgressClient, ProgressServer};
use ilium_plugin_sdk::host::process::ProcessExit;
use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentView, Event, Intent, MessageEvent, NodeClass, NodeId, PluginId, PluginSide, SendInputSpec, Submission,
};
use serde_json::{Value, json};

pub const PLUGIN_ID: &str = "ilium.core.progress";
pub const FOOTER_VIEW: &str = "progress.footer";
pub const STATUS_SEGMENT: &str = "progress";
/// The channel on which the server side hands monitor records to the client side (see API_GAPS).
pub const MONITORS_CHANNEL: &str = "monitors";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-progress`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-progress")
}

/// A user document of the repository by file name, for example `cli-reference.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

pub fn golden_path(name: &str) -> String {
    format!("{}/tests/features/golden/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else { break };
        blocks.push(after[..end].to_string());
        rest = &after[end + 4..];
    }
    blocks
}

/// The first json block after the heading `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page.find(heading).unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else { break };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        blocks.push(text.trim_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the table row of `text` whose first cell is `first_cell`.
pub fn table_row(text: &str, first_cell: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            cells.len() > 2 && cells[1] == first_cell
        })
        .unwrap_or_else(|| panic!("no table row starting with {first_cell}"));
    let mut cells: Vec<String> = line.split('|').map(|cell| cell.trim().to_string()).collect();
    cells.remove(0);
    cells.pop();
    cells
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// Hosts and panes
// ---------------------------------------------------------------------------------------------

pub fn server_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build()
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build()
}

pub fn activated_server() -> FakeHost {
    let mut host = server_host();
    host.activate::<ProgressServer>();
    host
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<ProgressClient>();
    host
}

/// A plain terminal pane in the fake tree; returns its node id.
pub fn add_shell(host: &mut FakeHost, name: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    })
}

/// An agent pane in the fake tree; returns its node id.
pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity: ilium_plugin_sdk::types::AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    })
}

/// An activated server host and one terminal pane.
pub fn host_with_pane() -> (FakeHost, u64) {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "shell");
    (host, pane)
}

// ---------------------------------------------------------------------------------------------
// Probes
// ---------------------------------------------------------------------------------------------

/// A successful probe exit with the given standard output.
pub fn probe_exit(stdout: &str) -> ProcessExit {
    ProcessExit { status: Some(0), signal: None, stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() }
}

pub fn failing_exit(status: i32) -> ProcessExit {
    ProcessExit { status: Some(status), signal: None, stdout: Vec::new(), stderr: b"boom".to_vec() }
}

/// The probe output for a running task.
pub fn running(job_id: &str, percent: u32, message: &str) -> String {
    json!({"job_id": job_id, "status": "running", "percent": percent, "message": message}).to_string()
}

/// The probe output for a finished task.
pub fn done(job_id: &str, message: &str) -> String {
    json!({"job_id": job_id, "status": "done", "percent": 100, "message": message}).to_string()
}

/// The probe output for a failed task.
pub fn failed(job_id: &str, percent: u32, message: &str, error: &str) -> String {
    json!({"job_id": job_id, "status": "error", "percent": percent, "message": message, "error": error})
        .to_string()
}

/// Registers one expected probe run: `sh -c <command>` printing `stdout` and exiting 0.
pub fn expect_probe(host: &mut FakeHost, command: &str, stdout: &str) {
    host.process().expect("sh", &["-c", command]).returns(probe_exit(stdout));
}

/// Registers one expected probe run with a prepared exit.
pub fn expect_exit(host: &mut FakeHost, command: &str, exit: ProcessExit) {
    host.process().expect("sh", &["-c", command]).returns(exit);
}

/// Advances the fake clock by whole seconds and lets every due timer run.
pub fn tick(host: &mut FakeHost, seconds: u64) {
    host.advance_clock(seconds * 1000);
    host.settle();
}

/// Number of probe runs the fake process has seen.
pub fn probe_runs(host: &mut FakeHost) -> usize {
    host.process().calls().len()
}

// ---------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------

/// Parses JSONL text into one value per non-empty line.
pub fn records(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("not JSON: {line:?}: {error}")))
        .collect()
}

/// The top-level keys of one JSON object text, in the order they are written.
pub fn top_level_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    let characters: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let symbol = characters[index];
        if in_string {
            if escaped {
                escaped = false;
                current.push(symbol);
            } else if symbol == '\\' {
                escaped = true;
                current.push(symbol);
            } else if symbol == '"' {
                in_string = false;
                if depth == 1 {
                    let mut look = index + 1;
                    while look < characters.len() && characters[look].is_whitespace() {
                        look += 1;
                    }
                    if look < characters.len() && characters[look] == ':' {
                        keys.push(current.clone());
                    }
                }
            } else {
                current.push(symbol);
            }
        } else {
            match symbol {
                '"' => {
                    in_string = true;
                    current.clear();
                }
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                _ => {}
            }
        }
        index += 1;
    }
    keys
}

/// The record of `type` among `stdout`'s lines (the first one).
pub fn record_of(stdout: &str, kind: &str) -> Value {
    records(stdout)
        .into_iter()
        .find(|record| record["type"] == kind)
        .unwrap_or_else(|| panic!("no {kind} record in {stdout:?}"))
}

/// The raw line of the record of `type` (to check key order).
pub fn line_of(stdout: &str, kind: &str) -> String {
    let needle = format!("\"type\":\"{kind}\"");
    stdout
        .lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("no {kind} line in {stdout:?}"))
        .to_string()
}

/// Output of one command line run: stdout text, stderr text and exit code.
pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub exit: Option<u8>,
}

/// Runs `ilium progress <args>` as typed in `pane`.
pub fn progress(host: &mut FakeHost, pane: u64, args: &[&str]) -> Run {
    let mut full = vec!["progress"];
    full.extend_from_slice(args);
    run_in_pane(host, pane, &full)
}

/// Runs `ilium <args>` as typed in `pane` (the pane identity comes from `ILIUM_PANE_ID`).
pub fn run_in_pane(host: &mut FakeHost, pane: u64, args: &[&str]) -> Run {
    set_pane_env(host, Some(pane));
    let invocation = host.cli().invoke(args, b"");
    Run { stdout: invocation.stdout(), stderr: invocation.stderr(), exit: invocation.exit_code() }
}

/// A command line that stays open (a `--wait`); read it again with [`Open::read`] after time passes.
pub struct Open {
    invocation: ilium_plugin_sdk::testing::fakes::cli::Invocation,
}

impl Open {
    pub fn read(&self) -> Run {
        Run { stdout: self.invocation.stdout(), stderr: self.invocation.stderr(), exit: self.invocation.exit_code() }
    }
}

impl Open {
    /// Presses Ctrl+C on the open command line: the host sends `cli-cancelled` (gap helper).
    pub fn cancel(&self, host: &mut FakeHost) {
        host.cli().cancel(&self.invocation);
        host.settle();
    }
}

/// Starts `ilium <args>` in `pane` without reading its result yet.
pub fn start_in_pane(host: &mut FakeHost, pane: u64, args: &[&str]) -> Open {
    set_pane_env(host, Some(pane));
    Open { invocation: host.cli().invoke(args, b"") }
}

/// Registers a monitor with `progress set`, one probe run answering `first_stdout`; returns its id.
pub fn register(host: &mut FakeHost, pane: u64, command: &str, first_stdout: &str, extra: &[&str]) -> u64 {
    expect_probe(host, command, first_stdout);
    let mut args = vec!["set", "--command", command];
    args.extend_from_slice(extra);
    let run = progress(host, pane, &args);
    let record = record_of(&run.stdout, "progress_set");
    record["monitor_id"].as_u64().unwrap_or_else(|| panic!("monitor_id in {record}"))
}

/// All probe outputs for a command, used with [`feed`].
pub fn feed(host: &mut FakeHost, command: &str, outputs: &[String]) {
    for output in outputs {
        expect_probe(host, command, output);
    }
}

/// `progress_status` record of the pane.
pub fn status(host: &mut FakeHost, pane: u64) -> Value {
    let run = progress(host, pane, &["status"]);
    record_of(&run.stdout, "progress_status")
}

/// The `monitor_health.state` of monitor `id` in a status record.
pub fn health_of(status: &Value, id: u64) -> String {
    status["monitors"]
        .as_array()
        .expect("monitors")
        .iter()
        .find(|monitor| monitor["monitor_id"] == id)
        .unwrap_or_else(|| panic!("monitor {id} in {status}"))["monitor_health"]["state"]
        .as_str()
        .expect("state")
        .to_string()
}

// ---------------------------------------------------------------------------------------------
// Submissions
// ---------------------------------------------------------------------------------------------

/// Every `send-input` among the submissions the plugin made so far.
pub fn sent_inputs(host: &FakeHost) -> Vec<SendInputSpec> {
    host.submitted()
        .iter()
        .filter_map(|submission: &Submission| match &submission.intent {
            Intent::SendInput(spec) => Some(spec.clone()),
            _ => None,
        })
        .collect()
}

/// The text of every `send-input` to `pane`.
pub fn typed_texts(host: &FakeHost, pane: u64) -> Vec<String> {
    sent_inputs(host).into_iter().filter(|spec| spec.pane == NodeId(pane)).map(|spec| spec.text).collect()
}

/// A `message` event from the server side of the plugin to the client side.
pub fn monitors_message(payload: &Value) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN_ID.to_string()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: MONITORS_CHANNEL.to_string(),
        payload: payload.to_string().into_bytes(),
    })
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: every use of an item the skeleton lacks (see ../API_GAPS.md)
// ---------------------------------------------------------------------------------------------

/// Sets `ILIUM_PANE_ID` for the next invocations, or removes it (`None`).
pub fn set_pane_env(host: &mut FakeHost, pane: Option<u64>) {
    match pane {
        Some(pane) => host.cli().set_env("ILIUM_PANE_ID", &pane.to_string()),
        None => host.cli().unset_env("ILIUM_PANE_ID"),
    }
}

/// Every reply the plugin gave to an `http-called` call: `(call, status, headers, body)`.
pub fn http_replies(host: &FakeHost) -> Vec<(u64, u16, Vec<(String, String)>, Vec<u8>)> {
    host.http_replies()
}

/// Sets a kernel setting of the tab `agent-monitoring` and delivers `kernel-settings-changed`.
pub fn set_kernel_setting(host: &mut FakeHost, key: &str, value: Value) -> Result<(), ilium_plugin_sdk::types::PluginError> {
    host.settings().set(key, value)?;
    host.settle();
    Ok(())
}

/// Contributes an entry to an extension point the plugin owns, as a package manifest would.
pub fn contribute(host: &mut FakeHost, point: &str, id: &str, priority: i32, entry: Value) {
    host.contribute_extension(point, id, priority, entry);
    host.settle();
}

/// Calls the service `ilium.core.progress/report` of the plugin; returns the reply text.
pub fn call_report(host: &mut FakeHost, request_json: &str) -> Result<String, ilium_plugin_sdk::types::PluginError> {
    host.call_own_service("ilium.core.progress/report", request_json)
}

/// `(topic, payload_json)` of every topic publication the plugin made.
pub fn published_topics(host: &FakeHost) -> Vec<(String, String)> {
    host.published_topics()
}
