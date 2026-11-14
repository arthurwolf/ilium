//! The server side of `ilium.core.search`: answers query messages from the clients.
//!
//! One query is answered with one `results` message to the asking client. The answer joins:
//!
//! - pane hits (agents and terminals) found in the tree by name and by the text on their screen
//!   and scrollback, read through `terminal.read-screen`;
//! - file hits from one shared `scan-files` job that reads the project files;
//! - meaning hits from the host vector index (`inference.embed`), left out when the grant,
//!   the provider or the index is missing;
//! - hits of the sources other plugins contribute to `ilium.core.search/sources`;
//!
//! and then runs the `ilium.core.search/rank` hook over them before sending. Every wait is an
//! asynchronous host request, so the handler never blocks. State lives in a thread-local cell
//! because the plugin structs are unit structs.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use ilium_plugin_sdk::host;
use ilium_plugin_sdk::host::{
    EntryKind, HostExtensions, HostFs, HostJobs, HostServices, HostTree, HostVector,
};
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::{
    ClientId, ErrorCode, Event, FsPath, IndexId, Intent, MessageEvent, MessageSpec, MessageTarget,
    NodeClass, NodeId, Outcome, PluginError, PluginId, PluginSide, RequestId, RequestRef,
    RequestResult, Submission,
};
use serde_json::{json, Value};

use crate::matching::{
    context_parts, last_command_from_text, single_line, strip_terminal_controls, MAX_RESULTS,
};

/// The plugin id, also the target of the answers.
pub const PLUGIN_ID: &str = "ilium.core.search";
/// Channel the clients send a query on: `{ "revision": u64, "query": String }`.
pub const QUERY_CHANNEL: &str = "ilium.core.search/query";
/// Channel the server answers on: `{ "revision": u64, "hits": [...] }`.
pub const RESULTS_CHANNEL: &str = "ilium.core.search/results";
/// The extension point that adds sources of results.
pub const SOURCES_POINT: &str = "ilium.core.search/sources";
/// The extension point that reorders results.
pub const RANK_POINT: &str = "ilium.core.search/rank";
/// The kind of the file scanning job.
pub const SCAN_JOB: &str = "scan-files";

/// Files over this size are never read (parity with the built-in search).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// One `fs.read` call reads at most 1 MiB.
const READ_CHUNK: u32 = 1024 * 1024;
/// The job answers with at most this much text; paths beyond it come back as `unread`.
const JOB_OUTPUT_BUDGET: usize = 12 * 1024 * 1024;
/// The server reads at most this much text itself when the job could not.
const INLINE_BUDGET: usize = 8 * 1024 * 1024;
/// Most paths one scan lists.
const MAX_SCAN_ENTRIES: u32 = 20_000;
/// Text of one embedded pane, at most 8 KiB (the `embed` limit).
const EMBED_TEXT_BYTES: usize = 8 * 1024;
/// `embed` takes at most 256 texts.
pub const EMBED_BATCH: usize = 256;
/// Meaning hits asked of the vector index.
pub const TOP_K: u16 = 20;
/// The purpose every embed call states (AI admission gate).
const EMBED_PURPOSE: &str = "workspace search";
/// Rows read from a terminal screen and scrollback.
const SCREEN_ROWS: u32 = 500;
const SCROLLBACK_ROWS: u32 = 2_000;
/// Most nodes visited by one tree walk.
const TREE_WALK_LIMIT: usize = 8_192;

// ---- state -------------------------------------------------------------------------------

/// Where one part of a query stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Skipped,
    Waiting,
    Done,
}

/// One client's newest query and what it still waits for.
#[derive(Debug)]
struct Query {
    serial: u64,
    revision: u64,
    text: String,
    hits: Vec<Value>,
    files: Stage,
    meaning: Stage,
    sources: usize,
    ranking: bool,
}

/// What a host request that is in flight was started for.
#[derive(Debug, Clone)]
enum Purpose {
    Scan,
    Job,
    CorpusEmbed,
    QueryEmbed {
        client: u64,
        serial: u64,
    },
    Source {
        client: u64,
        serial: u64,
        kind: String,
    },
    Rank {
        client: u64,
        serial: u64,
    },
}

/// The shared file scan.
#[derive(Debug, Default)]
enum FilesFlow {
    #[default]
    Idle,
    Scanning,
    Running(Vec<String>),
}

/// One batch chain that embeds the changed panes.
#[derive(Debug)]
struct Chain {
    items: Vec<(String, u64, String)>,
    next: usize,
    batch_len: usize,
    vectors: Vec<Vec<f32>>,
}

#[derive(Debug, Default)]
struct Meaning {
    index: Option<(IndexId, usize)>,
    indexed: BTreeMap<String, u64>,
    chain: Option<Chain>,
}

#[derive(Debug, Default)]
struct ServerState {
    next_serial: u64,
    queries: BTreeMap<u64, Query>,
    requests: BTreeMap<u64, Purpose>,
    files: FilesFlow,
    meaning: Meaning,
}

thread_local! {
    static STATE: RefCell<ServerState> = RefCell::new(ServerState::default());
}

/// Forgets everything; called on activation.
pub fn reset() {
    STATE.with(|state| *state.borrow_mut() = ServerState::default());
}

/// Handles one batch of events.
pub fn on_events(batch: Vec<Event>) -> Vec<Submission> {
    let mut out = Vec::new();
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        for event in batch {
            match event {
                Event::Message(message) if message.channel == QUERY_CHANNEL => {
                    state.on_query(&message, &mut out);
                }
                Event::RequestCompleted(result) => state.on_completed(&result, &mut out),
                _ => {}
            }
        }
    });
    out
}

// ---- tree and screens --------------------------------------------------------------------

/// One terminal or agent pane of the tree with the text read from its screen.
#[derive(Debug, Clone)]
struct PaneText {
    id: u64,
    name: String,
    /// `agent` for a pane with an identified agent, else `terminal`.
    kind: &'static str,
    visible: String,
    scrollback: String,
}

fn read_part(node: NodeId, part: ScreenPart, rows: u32) -> String {
    let mut api = host();
    HostTerminal::read_screen(&mut *api, node, part, rows)
        .map(|screen| screen.rows.join("\n"))
        .unwrap_or_default()
}

/// Every terminal pane of the tree in tree order, with the text of its screen and, when asked,
/// of its scrollback. Panes of other kinds (editors, boards, ...) have no terminal text.
fn collect_panes(with_scrollback: bool) -> Vec<PaneText> {
    let mut api = host();
    let mut stack: Vec<NodeId> = HostTree::roots(&mut *api);
    stack.reverse();
    let mut panes = Vec::new();
    let mut visited = 0usize;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > TREE_WALK_LIMIT {
            break;
        }
        let Some(view) = HostTree::get(&mut *api, node) else {
            continue;
        };
        match view.class {
            NodeClass::Pane => {
                let is_terminal = view
                    .content_kind
                    .as_deref()
                    .is_none_or(|kind| kind == "ilium.terminal");
                if !is_terminal {
                    continue;
                }
                let visible = read_part(node, ScreenPart::Visible, SCREEN_ROWS);
                let scrollback = if with_scrollback {
                    read_part(node, ScreenPart::Scrollback, SCROLLBACK_ROWS)
                } else {
                    String::new()
                };
                panes.push(PaneText {
                    id: node.0,
                    name: view.title,
                    kind: if view.agent.is_some() {
                        "agent"
                    } else {
                        "terminal"
                    },
                    visible,
                    scrollback,
                });
            }
            NodeClass::Container | NodeClass::Folder => {
                let mut children = HostTree::children(&mut *api, node);
                children.reverse();
                stack.extend(children);
            }
        }
    }
    panes
}

/// The first case-insensitive match of `query` in `text` as (before, matched, after) with the
/// context cut to 72 characters and newlines turned into spaces.
pub fn first_match(text: &str, query: &str) -> Option<(String, String, String)> {
    if query.is_empty() {
        return None;
    }
    let folded_text = text.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0;
    while let Some(found) = folded_text.get(start..)?.find(&folded_query) {
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if text.is_char_boundary(match_start) && text.is_char_boundary(match_end) {
            let (before, matched, after) = context_parts(text, match_start, match_end);
            return Some((
                single_line(before),
                single_line(matched),
                single_line(after),
            ));
        }
        start = match_end;
    }
    None
}

fn contains_ignoring_case(text: &str, query: &str) -> bool {
    !query.is_empty()
        && text
            .to_ascii_lowercase()
            .contains(&query.to_ascii_lowercase())
}

fn pane_hit(pane: &PaneText, query: &str) -> Option<Value> {
    let found = first_match(&pane.visible, query).or_else(|| first_match(&pane.scrollback, query));
    let name_match = contains_ignoring_case(&pane.name, query);
    if found.is_none() && !name_match {
        return None;
    }
    let mut hit = json!({
        "id": pane.id.to_string(),
        "kind": pane.kind,
        "title": pane.name,
    });
    if let Some((before, matched, after)) = found {
        hit["before"] = json!(before);
        hit["matched"] = json!(matched);
        hit["after"] = json!(after);
        if let Some(command) = last_command_from_text(&pane.visible) {
            hit["last_command"] = json!(command);
        }
    }
    Some(hit)
}

/// Pane hits of `query`: agents first, then the other panes, each in tree order.
fn pane_hits(query: &str) -> Vec<Value> {
    let panes = collect_panes(true);
    let mut hits: Vec<Value> = Vec::new();
    for agents_first in [true, false] {
        for pane in &panes {
            if (pane.kind == "agent") == agents_first {
                if let Some(hit) = pane_hit(pane, query) {
                    hits.push(hit);
                }
            }
        }
    }
    hits
}

/// The hits the `query` service answers at once: panes only, no files or meaning.
pub fn quick_hits(query: &str) -> Vec<Value> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    pane_hits(query)
}

// ---- files -------------------------------------------------------------------------------

fn project_path(path: &str) -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: path.to_string(),
    }
}

fn skipped_directory(path: &str) -> bool {
    path.split('/')
        .any(|part| matches!(part, ".git" | "target" | "node_modules" | ".ilium"))
}

/// Reads one file up to `MAX_FILE_BYTES` as searchable text (control sequences removed, invalid
/// UTF-8 dropped); `None` when it is too large or unreadable.
fn read_file(path: &str) -> Option<String> {
    let mut api = host();
    let metadata = HostFs::metadata(&mut *api, project_path(path)).ok()?;
    if metadata.kind != EntryKind::File || metadata.length > MAX_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    let mut offset = 0u64;
    loop {
        let chunk = HostFs::read(&mut *api, project_path(path), offset, READ_CHUNK).ok()?;
        if chunk.is_empty() {
            break;
        }
        offset += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if offset >= metadata.length {
            break;
        }
    }
    Some(strip_terminal_controls(&bytes).text)
}

/// True when the file is known to be over the size limit, so nothing reads it.
fn too_large(path: &str) -> bool {
    let mut api = host();
    HostFs::metadata(&mut *api, project_path(path))
        .is_ok_and(|metadata| metadata.length > MAX_FILE_BYTES)
}

/// The `scan-files` job: reads the listed project files and returns
/// `{ "files": [{ "path", "text" }], "unread": [path] }`. A file that cannot be read here, or
/// that would exceed the output budget, is listed as unread so the server can read it itself.
pub fn run_scan_job(input: &[u8]) -> Vec<u8> {
    let listing = String::from_utf8_lossy(input);
    let mut files: Vec<Value> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    let mut produced = 0usize;
    for path in listing.split('\n').filter(|path| !path.is_empty()) {
        if produced >= JOB_OUTPUT_BUDGET {
            unread.push(path.to_string());
            continue;
        }
        match read_file(path) {
            Some(text) => {
                produced += text.len();
                files.push(json!({ "path": path, "text": text }));
            }
            None if too_large(path) => {}
            None => unread.push(path.to_string()),
        }
    }
    json!({ "files": files, "unread": unread })
        .to_string()
        .into_bytes()
}

/// Matches of `query` in one file: case-insensitive, control sequences removed, 72 characters
/// of context, the zero-based line of the match, at most `limit` hits.
fn file_hits(path: &str, text: &str, query: &str, limit: usize, hits: &mut Vec<Value>) {
    if query.is_empty() || limit == 0 {
        return;
    }
    let plain = text;
    let folded_text = plain.to_ascii_lowercase();
    let folded_query = query.to_ascii_lowercase();
    let mut start = 0usize;
    let mut counted_to = 0usize;
    let mut line = 0usize;
    let mut added = 0usize;
    while let Some(found) = folded_text
        .get(start..)
        .and_then(|rest| rest.find(&folded_query))
    {
        let match_start = start + found;
        let match_end = match_start + folded_query.len();
        if !plain.is_char_boundary(match_start) || !plain.is_char_boundary(match_end) {
            start = match_end;
            continue;
        }
        line += plain.as_bytes()[counted_to..match_start]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        counted_to = match_start;
        let (before, matched, after) = context_parts(plain, match_start, match_end);
        hits.push(json!({
            "id": path,
            "kind": "file",
            "title": path,
            "before": single_line(before),
            "matched": single_line(matched),
            "after": single_line(after),
            "line": line,
        }));
        added += 1;
        if added >= limit {
            return;
        }
        start = match_end.max(start + 1);
        if start >= folded_text.len() {
            return;
        }
    }
}

/// The text of the project files a finished scan produced.
#[derive(Debug, Default)]
struct FileSnapshot {
    files: Vec<(String, String)>,
}

fn read_unread(paths: &[String], snapshot: &mut FileSnapshot) {
    let mut budget = INLINE_BUDGET;
    for path in paths {
        if budget == 0 {
            break;
        }
        let Some(text) = read_file(path) else {
            continue;
        };
        budget = budget.saturating_sub(text.len());
        snapshot.files.push((path.clone(), text));
    }
}

fn snapshot_from_output(output: Option<Vec<u8>>, paths: &[String]) -> FileSnapshot {
    let mut snapshot = FileSnapshot::default();
    let parsed = output
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object);
    let Some(parsed) = parsed else {
        read_unread(paths, &mut snapshot);
        return snapshot;
    };
    for file in parsed["files"].as_array().into_iter().flatten() {
        if let (Some(path), Some(text)) = (file["path"].as_str(), file["text"].as_str()) {
            snapshot.files.push((path.to_string(), text.to_string()));
        }
    }
    let unread: Vec<String> = parsed["unread"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|path| path.as_str().map(str::to_string))
        .collect();
    read_unread(&unread, &mut snapshot);
    snapshot
}

// ---- meaning -----------------------------------------------------------------------------

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn truncate_bytes(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The text embedded for a pane: its name and the visible screen, at most 8 KiB.
fn corpus() -> Vec<(String, u64, String)> {
    collect_panes(false)
        .into_iter()
        .map(|pane| {
            let text = truncate_bytes(
                &format!("{}\n{}", pane.name, pane.visible),
                EMBED_TEXT_BYTES,
            );
            (pane.id.to_string(), hash_text(&text), text)
        })
        .collect()
}

// ---- the state machine -------------------------------------------------------------------

fn answer(client: u64, revision: u64, hits: &[Value]) -> Submission {
    let payload = json!({ "revision": revision, "hits": hits })
        .to_string()
        .into_bytes();
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client: Some(ClientId(client)),
        },
        channel: RESULTS_CHANNEL.to_string(),
        payload,
    }))
}

/// True when the request finished without error.
fn succeeded(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Ok | Outcome::OkNode(_) | Outcome::OkEpoch(_)
    )
}

fn request_number(result: &RequestResult) -> Option<u64> {
    match result.request {
        RequestRef::Call(RequestId(number)) => Some(number),
        RequestRef::Submission(_) => None,
    }
}

impl ServerState {
    fn on_query(&mut self, message: &MessageEvent, out: &mut Vec<Submission>) {
        let Some(client) = message.from_client.map(|client| client.0) else {
            return;
        };
        let Ok(payload) = serde_json::from_slice::<Value>(&message.payload) else {
            return;
        };
        let (Some(revision), Some(text)) =
            (payload["revision"].as_u64(), payload["query"].as_str())
        else {
            return;
        };
        if text.trim().is_empty() {
            self.queries.remove(&client);
            out.push(answer(client, revision, &[]));
            return;
        }
        self.next_serial += 1;
        let query = Query {
            serial: self.next_serial,
            revision,
            text: text.to_string(),
            hits: pane_hits(text),
            files: Stage::Skipped,
            meaning: Stage::Skipped,
            sources: 0,
            ranking: false,
        };
        self.queries.insert(client, query);
        self.start_files(client);
        self.start_meaning(client);
        self.start_sources(client);
        self.advance(client, out);
    }

    // ---- files ----

    fn start_files(&mut self, client: u64) {
        if !matches!(self.files, FilesFlow::Idle) {
            self.set_files(client, Stage::Waiting);
            return;
        }
        let started = {
            let mut api = host();
            HostFs::scan(
                &mut *api,
                project_path(""),
                "**/*".to_string(),
                MAX_SCAN_ENTRIES,
            )
        };
        match started {
            Ok(request) => {
                self.requests.insert(request.0, Purpose::Scan);
                self.files = FilesFlow::Scanning;
                self.set_files(client, Stage::Waiting);
            }
            Err(_) => self.set_files(client, Stage::Done),
        }
    }

    fn set_files(&mut self, client: u64, stage: Stage) {
        if let Some(query) = self.queries.get_mut(&client) {
            query.files = stage;
        }
    }

    fn on_scan_completed(
        &mut self,
        request: u64,
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let paths: Vec<String> = if succeeded(&result.outcome) {
            let mut api = host();
            HostFs::take_entries(&mut *api, RequestId(request))
                .unwrap_or_default()
                .into_iter()
                .filter(|entry| {
                    entry.entry.metadata.kind == EntryKind::File
                        && entry.entry.metadata.length <= MAX_FILE_BYTES
                })
                .map(|entry| entry.relative_path)
                .filter(|path| !skipped_directory(path))
                .collect()
        } else {
            Vec::new()
        };
        if paths.is_empty() {
            self.finish_files(FileSnapshot::default(), out);
            return;
        }
        let started = {
            let mut api = host();
            HostJobs::start(
                &mut *api,
                SCAN_JOB.to_string(),
                paths.join("\n").into_bytes(),
            )
        };
        match started {
            Ok(job) => {
                self.requests.insert(job.0, Purpose::Job);
                self.files = FilesFlow::Running(paths);
            }
            Err(_) => {
                let mut snapshot = FileSnapshot::default();
                read_unread(&paths, &mut snapshot);
                self.finish_files(snapshot, out);
            }
        }
    }

    fn on_job_completed(
        &mut self,
        request: u64,
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let paths = match std::mem::take(&mut self.files) {
            FilesFlow::Running(paths) => paths,
            _ => Vec::new(),
        };
        let snapshot = if succeeded(&result.outcome) {
            let output = {
                let mut api = host();
                HostJobs::take_output(&mut *api, RequestId(request))
            };
            snapshot_from_output(output, &paths)
        } else {
            FileSnapshot::default()
        };
        self.finish_files(snapshot, out);
    }

    /// Gives every query that waits for files its file hits.
    fn finish_files(&mut self, snapshot: FileSnapshot, out: &mut Vec<Submission>) {
        self.files = FilesFlow::Idle;
        let waiting: Vec<u64> = self
            .queries
            .iter()
            .filter(|(_, query)| query.files == Stage::Waiting)
            .map(|(client, _)| *client)
            .collect();
        for client in waiting {
            if let Some(query) = self.queries.get_mut(&client) {
                let mut room = MAX_RESULTS.saturating_sub(query.hits.len());
                let needle = query.text.clone();
                for (path, text) in &snapshot.files {
                    if room == 0 {
                        break;
                    }
                    let before = query.hits.len();
                    file_hits(path, text, &needle, room, &mut query.hits);
                    room -= query.hits.len() - before;
                }
                query.files = Stage::Done;
            }
            self.advance(client, out);
        }
    }

    // ---- meaning ----

    fn start_meaning(&mut self, client: u64) {
        let items = corpus();
        if items.is_empty() {
            return;
        }
        let present: Vec<&String> = items.iter().map(|(id, _, _)| id).collect();
        if let Some((index, _)) = self.meaning.index {
            let stale: Vec<String> = self
                .meaning
                .indexed
                .keys()
                .filter(|id| !present.contains(id))
                .cloned()
                .collect();
            if !stale.is_empty() {
                let mut api = host();
                let _ = HostVector::remove_entries(&mut *api, index, stale.clone());
                for id in stale {
                    self.meaning.indexed.remove(&id);
                }
            }
        }
        let changed: Vec<(String, u64, String)> = items
            .into_iter()
            .filter(|(id, hash, _)| self.meaning.indexed.get(id) != Some(hash))
            .collect();
        if let Some(query) = self.queries.get_mut(&client) {
            query.meaning = Stage::Waiting;
        }
        if self.meaning.chain.is_some() {
            return;
        }
        if changed.is_empty() {
            self.embed_query(client);
            return;
        }
        self.meaning.chain = Some(Chain {
            items: changed,
            next: 0,
            batch_len: 0,
            vectors: Vec::new(),
        });
        if !self.send_batch() {
            self.fail_meaning();
        }
    }

    /// Sends the next embed batch of the chain; false when the call is refused.
    fn send_batch(&mut self) -> bool {
        let Some(chain) = self.meaning.chain.as_mut() else {
            return false;
        };
        let end = (chain.next + EMBED_BATCH).min(chain.items.len());
        let texts: Vec<String> = chain.items[chain.next..end]
            .iter()
            .map(|(_, _, text)| text.clone())
            .collect();
        chain.batch_len = texts.len();
        let started = {
            let mut api = host();
            HostVector::embed(&mut *api, texts, EMBED_PURPOSE.to_string())
        };
        match started {
            Ok(request) => {
                self.requests.insert(request.0, Purpose::CorpusEmbed);
                true
            }
            Err(_) => false,
        }
    }

    /// Marks every query that waits for meaning as done without meaning hits.
    fn fail_meaning(&mut self) {
        self.meaning.chain = None;
        for query in self.queries.values_mut() {
            if query.meaning == Stage::Waiting {
                query.meaning = Stage::Done;
            }
        }
    }

    fn on_corpus_embedded(
        &mut self,
        request: u64,
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let vectors = if succeeded(&result.outcome) {
            let mut api = host();
            HostVector::take_vectors(&mut *api, RequestId(request))
        } else {
            None
        };
        let Some(chain) = self.meaning.chain.as_mut() else {
            return;
        };
        let Some(vectors) = vectors.filter(|vectors| vectors.len() == chain.batch_len) else {
            self.fail_meaning();
            self.advance_all(out);
            return;
        };
        chain.vectors.extend(vectors);
        chain.next += chain.batch_len;
        if chain.next < chain.items.len() {
            if !self.send_batch() {
                self.fail_meaning();
                self.advance_all(out);
            }
            return;
        }
        let Some(chain) = self.meaning.chain.take() else {
            return;
        };
        if !self.store_vectors(chain) {
            self.fail_meaning();
            self.advance_all(out);
            return;
        }
        let waiting: Vec<u64> = self
            .queries
            .iter()
            .filter(|(_, query)| query.meaning == Stage::Waiting)
            .map(|(client, _)| *client)
            .collect();
        for client in waiting {
            self.embed_query(client);
            self.advance(client, out);
        }
    }

    /// Puts the embedded panes into the index, creating or rebuilding it as needed.
    fn store_vectors(&mut self, chain: Chain) -> bool {
        let Some(dimensions) = chain.vectors.first().map(Vec::len) else {
            return false;
        };
        let mut api = host();
        if let Some((index, known)) = self.meaning.index {
            if known != dimensions {
                HostVector::drop_index(&mut *api, index);
                self.meaning.index = None;
                self.meaning.indexed.clear();
            }
        }
        let index = match self.meaning.index {
            Some((index, _)) => index,
            None => {
                let Ok(dimensions_u16) = u16::try_from(dimensions) else {
                    return false;
                };
                match HostVector::create_index(&mut *api, "workspace".to_string(), dimensions_u16) {
                    Ok(index) => {
                        self.meaning.index = Some((index, dimensions));
                        index
                    }
                    Err(_) => return false,
                }
            }
        };
        let entries: Vec<(String, Vec<f32>)> = chain
            .items
            .iter()
            .zip(chain.vectors)
            .map(|((id, _, _), vector)| (id.clone(), vector))
            .collect();
        if HostVector::upsert(&mut *api, index, entries).is_err() {
            return false;
        }
        for (id, hash, _) in chain.items {
            self.meaning.indexed.insert(id, hash);
        }
        true
    }

    fn embed_query(&mut self, client: u64) {
        let Some(query) = self.queries.get(&client) else {
            return;
        };
        let serial = query.serial;
        let text = truncate_bytes(&query.text, EMBED_TEXT_BYTES);
        let started = {
            let mut api = host();
            HostVector::embed(&mut *api, vec![text], EMBED_PURPOSE.to_string())
        };
        match started {
            Ok(request) => {
                self.requests
                    .insert(request.0, Purpose::QueryEmbed { client, serial });
            }
            Err(_) => {
                if let Some(query) = self.queries.get_mut(&client) {
                    query.meaning = Stage::Done;
                }
            }
        }
    }

    fn on_query_embedded(
        &mut self,
        request: u64,
        client: u64,
        serial: u64,
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let vector = if succeeded(&result.outcome) {
            let mut api = host();
            HostVector::take_vectors(&mut *api, RequestId(request))
                .and_then(|vectors| vectors.into_iter().next())
        } else {
            None
        };
        let index = self.meaning.index.map(|(index, _)| index);
        let current = self
            .queries
            .get(&client)
            .is_some_and(|query| query.serial == serial);
        if !current {
            return;
        }
        let ranked = match (index, vector) {
            (Some(index), Some(vector)) => {
                let mut api = host();
                HostVector::top_k(&mut *api, index, vector, TOP_K).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let mut meaning_hits = Vec::new();
        {
            let mut api = host();
            for (id, score) in ranked {
                let Ok(number) = id.parse::<u64>() else {
                    continue;
                };
                let Some(view) = HostTree::get(&mut *api, NodeId(number)) else {
                    continue;
                };
                let pane = view
                    .agent
                    .as_ref()
                    .map_or_else(|| view.title.clone(), |agent| agent.signature.clone());
                meaning_hits.push(json!({
                    "id": id,
                    "kind": "meaning",
                    "title": view.title,
                    "score": score,
                    "pane": pane,
                }));
            }
        }
        if let Some(query) = self.queries.get_mut(&client) {
            query.hits.extend(meaning_hits);
            query.meaning = Stage::Done;
        }
        self.advance(client, out);
    }

    // ---- sources and ranking ----

    fn start_sources(&mut self, client: u64) {
        let entries = {
            let mut api = host();
            HostExtensions::list(&mut *api, SOURCES_POINT.to_string()).unwrap_or_default()
        };
        let Some(query) = self.queries.get(&client) else {
            return;
        };
        let (serial, text) = (query.serial, query.text.clone());
        let mut started = 0usize;
        for entry in entries {
            let Ok(description) = serde_json::from_str::<Value>(&entry.entry_json) else {
                continue;
            };
            let (Some(service), Some(kind)) = (
                description["service"].as_str(),
                description["kind"].as_str(),
            ) else {
                continue;
            };
            let call = {
                let mut api = host();
                HostServices::call(
                    &mut *api,
                    service.to_string(),
                    "query".to_string(),
                    json!({ "query": text }).to_string(),
                )
            };
            if let Ok(request) = call {
                self.requests.insert(
                    request.0,
                    Purpose::Source {
                        client,
                        serial,
                        kind: kind.to_string(),
                    },
                );
                started += 1;
            }
        }
        if let Some(query) = self.queries.get_mut(&client) {
            query.sources = started;
        }
    }

    fn on_source_replied(
        &mut self,
        request: u64,
        target: (u64, u64, String),
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let (client, serial, kind) = target;
        let reply = if succeeded(&result.outcome) {
            let mut api = host();
            HostServices::take_reply(&mut *api, RequestId(request))
        } else {
            None
        };
        let Some(query) = self.queries.get_mut(&client) else {
            return;
        };
        if query.serial != serial {
            return;
        }
        query.sources = query.sources.saturating_sub(1);
        let hits = reply
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value["hits"].as_array().cloned())
            .unwrap_or_default();
        for mut hit in hits {
            if !(hit["id"].is_string() && hit["title"].is_string()) {
                continue;
            }
            if hit["kind"].as_str().is_none() {
                hit["kind"] = json!(kind);
            }
            query.hits.push(hit);
        }
        self.advance(client, out);
    }

    fn on_rank_replied(
        &mut self,
        request: u64,
        client: u64,
        serial: u64,
        result: &RequestResult,
        out: &mut Vec<Submission>,
    ) {
        let reply = if succeeded(&result.outcome) {
            let mut api = host();
            HostExtensions::take_reply(&mut *api, RequestId(request))
        } else {
            None
        };
        let Some(query) = self.queries.remove(&client) else {
            return;
        };
        if query.serial != serial {
            self.queries.insert(client, query);
            return;
        }
        let hits = match reply.and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
            Some(reply) => reorder(query.hits, &reply),
            None => query.hits,
        };
        out.push(answer(client, query.revision, &hits));
    }

    // ---- completion ----

    /// Sends the answer of `client` once everything it waits for is in; runs the rank hook first.
    fn advance(&mut self, client: u64, out: &mut Vec<Submission>) {
        let Some(query) = self.queries.get_mut(&client) else {
            return;
        };
        let waiting = query.files == Stage::Waiting
            || query.meaning == Stage::Waiting
            || query.sources > 0
            || query.ranking;
        if waiting {
            return;
        }
        if query.hits.is_empty() {
            if let Some(query) = self.queries.remove(&client) {
                out.push(answer(client, query.revision, &[]));
            }
            return;
        }
        let payload = json!({
            "query": query.text,
            "hits": query.hits.iter().map(rank_entry).collect::<Vec<_>>(),
        })
        .to_string();
        let serial = query.serial;
        let started = {
            let mut api = host();
            HostExtensions::run_hook(&mut *api, RANK_POINT.to_string(), payload)
        };
        match started {
            Ok(request) => {
                query.ranking = true;
                self.requests
                    .insert(request.0, Purpose::Rank { client, serial });
            }
            Err(_) => {
                if let Some(query) = self.queries.remove(&client) {
                    out.push(answer(client, query.revision, &query.hits));
                }
            }
        }
    }

    fn advance_all(&mut self, out: &mut Vec<Submission>) {
        let clients: Vec<u64> = self.queries.keys().copied().collect();
        for client in clients {
            self.advance(client, out);
        }
    }

    fn on_completed(&mut self, result: &RequestResult, out: &mut Vec<Submission>) {
        let Some(request) = request_number(result) else {
            return;
        };
        let Some(purpose) = self.requests.remove(&request) else {
            return;
        };
        match purpose {
            Purpose::Scan => self.on_scan_completed(request, result, out),
            Purpose::Job => self.on_job_completed(request, result, out),
            Purpose::CorpusEmbed => self.on_corpus_embedded(request, result, out),
            Purpose::QueryEmbed { client, serial } => {
                self.on_query_embedded(request, client, serial, result, out);
            }
            Purpose::Source {
                client,
                serial,
                kind,
            } => self.on_source_replied(request, (client, serial, kind), result, out),
            Purpose::Rank { client, serial } => {
                self.on_rank_replied(request, client, serial, result, out);
            }
        }
    }
}

/// The rank hook entry of one hit: its id, kind and score (1 for text hits).
fn rank_entry(hit: &Value) -> Value {
    json!({
        "id": hit["id"],
        "kind": hit["kind"],
        "score": hit["score"].as_f64().unwrap_or(1.0),
    })
}

/// Puts the hits in the order a rank reply lists them; unlisted hits keep their order after.
fn reorder(hits: Vec<Value>, reply: &Value) -> Vec<Value> {
    let Some(listed) = reply["hits"].as_array() else {
        return hits;
    };
    let mut remaining: Vec<Option<Value>> = hits.into_iter().map(Some).collect();
    let mut ordered = Vec::with_capacity(remaining.len());
    for entry in listed {
        let position = remaining.iter().position(|hit| {
            hit.as_ref()
                .is_some_and(|hit| hit["id"] == entry["id"] && hit["kind"] == entry["kind"])
        });
        if let Some(position) = position {
            if let Some(hit) = remaining[position].take() {
                ordered.push(hit);
            }
        }
    }
    ordered.extend(remaining.into_iter().flatten());
    ordered
}

/// The refusal text of an unknown job kind.
pub fn unknown_job(kind: &str) -> PluginError {
    PluginError {
        code: ErrorCode::InvalidArgument,
        message: format!("ilium.core.search: unknown job kind {kind}"),
        retry_after_ms: None,
    }
}
