//! Finds the Claude and Codex sessions on disk that were active in the usage
//! view's selected range, whether or not Ilium has a pane for them.
//!
//! Everything here runs inside one background job: it lists transcript files
//! by modification time, reads each (bounded), keeps only the spend inside the
//! range and recovers a display name. Nothing here touches the UI thread.
//!
//! Counting rule: a session that an Ilium pane owns (or owned, within the
//! ownership history) is covered by that pane's row and is skipped here, so no
//! spend is counted twice. Every other session is counted for the whole range.
use crate::cost_model::PriceTable;
use crate::session_stats::{SpendBucket, StatsAccumulator};
use crate::weekly_usage::{model_workloads, ModelWorkload};
use ilium_core::AgentClass;
use ilium_execution::{Job, JobContext};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Transcript files considered per scan, newest first.
const MAX_CANDIDATES: usize = 400;
/// Bytes of never-seen transcript read per scan; the rest waits for the next.
const MAX_FRESH_READ_BYTES: u64 = 3 * 1024 * 1024 * 1024;
/// Entries listed per scan, a guard against pathological directory trees.
const MAX_LISTED_ENTRIES: usize = 400_000;
/// Spend buckets kept in the process-wide cache.
const CACHE_BUCKET_LIMIT: usize = 400_000;
/// Bytes of a transcript head searched for the working directory and prompt.
const HEAD_BYTES: usize = 512 * 1024;
/// Backup/snapshot files read per project when recovering names.
const ARCHIVE_FILES_PER_PROJECT: usize = 150;
/// Largest Ilium snapshot file parsed for names.
const ARCHIVE_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Parsed snapshot files remembered across scans.
const ARCHIVE_CACHE_FILES: usize = 1_000;
/// Longest fallback name taken from a first prompt.
const PROMPT_NAME_CHARS: usize = 60;
const DAY_MS: i64 = 86_400_000;

/// One session found on disk with its usage inside the range.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiscoveredSession {
    pub class: AgentClass,
    /// The transcript shows a Kilo Proxy launcher (own backend, not the
    /// provider's account), so the session is listed in its own section.
    pub proxy_backed: bool,
    pub session_id: String,
    pub name: String,
    pub models: Vec<ModelWorkload>,
    pub last_ms: Option<i64>,
}

#[derive(Debug, Default)]
pub(crate) struct DiscoveryOutput {
    pub sessions: Vec<DiscoveredSession>,
    /// Some transcripts were not read (candidate or byte budget reached).
    pub partial: bool,
    /// Some transcript could not be read at all.
    pub unreadable: usize,
}

/// Transcript stores to search, as the directories that contain `projects/`
/// (Claude) and `sessions/` (Codex).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderRoots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
}

impl ProviderRoots {
    /// The default stores, the environment overrides, and every store a known
    /// pane transcript lives in (custom provider roots).
    pub(crate) fn discover<'a>(
        home: &Path,
        transcript_hints: impl Iterator<Item = (&'a AgentClass, &'a Path)>,
    ) -> Self {
        let mut roots = Self::default();
        let mut push = |list: &mut Vec<PathBuf>, path: PathBuf| {
            if !list.contains(&path) {
                list.push(path);
            }
        };
        push(&mut roots.claude, home.join(".claude"));
        push(&mut roots.codex, home.join(".codex"));
        if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
            push(&mut roots.claude, PathBuf::from(dir));
        }
        if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
            push(&mut roots.codex, PathBuf::from(dir));
        }
        for (class, path) in transcript_hints {
            let (marker, list) = match class {
                AgentClass::Claude => ("projects", &mut roots.claude),
                AgentClass::Codex => ("sessions", &mut roots.codex),
                _ => continue,
            };
            if let Some(root) = path
                .ancestors()
                .find(|ancestor| ancestor.file_name().is_some_and(|name| name == marker))
                .and_then(Path::parent)
            {
                push(list, root.to_path_buf());
            }
        }
        roots
    }
}

pub(crate) struct DiscoverSessions {
    pub roots: ProviderRoots,
    pub start_ms: i64,
    pub end_ms: i64,
    pub prices: PriceTable,
    /// Sessions already covered by a pane row, never read here.
    pub exclude: Vec<(AgentClass, String)>,
    /// Project folders whose `.ilium` archive may name sessions.
    pub archive_projects: Vec<PathBuf>,
}

impl Job for DiscoverSessions {
    type Output = DiscoveryOutput;
    type Error = std::convert::Infallible;
    fn run(self, context: JobContext) -> Result<Self::Output, Self::Error> {
        Ok(discover(&self, || context.stop_requested()))
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    class: AgentClass,
    path: PathBuf,
    session_id: String,
    modified_ms: i64,
    length: u64,
}

fn modified_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    let elapsed = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(elapsed.as_millis()).ok()
}

fn looks_like_session_id(text: &str) -> bool {
    text.len() == 36
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn list_claude(root: &Path, start_ms: i64, listed: &mut usize, out: &mut Vec<Candidate>) {
    let Ok(projects) = std::fs::read_dir(root.join("projects")) else {
        return;
    };
    for project in projects.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            *listed += 1;
            if *listed > MAX_LISTED_ENTRIES {
                return;
            }
            let path = file.path();
            let Some(stem) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".jsonl"))
            else {
                continue;
            };
            if !looks_like_session_id(stem) {
                continue;
            }
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            let Some(modified) = modified_ms(&metadata).filter(|ms| *ms >= start_ms) else {
                continue;
            };
            out.push(Candidate {
                class: AgentClass::Claude,
                session_id: stem.to_owned(),
                path,
                modified_ms: modified,
                length: metadata.len(),
            });
        }
    }
}

fn numeric_dirs_desc(directory: &Path) -> Vec<(u32, PathBuf)> {
    let mut dirs: Vec<(u32, PathBuf)> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let number = entry.file_name().to_str()?.parse::<u32>().ok()?;
            entry
                .file_type()
                .ok()
                .filter(std::fs::FileType::is_dir)
                .map(|_| (number, entry.path()))
        })
        .collect();
    dirs.sort_by(|left, right| right.0.cmp(&left.0));
    dirs
}

fn list_codex(root: &Path, start_ms: i64, listed: &mut usize, out: &mut Vec<Candidate>) {
    // A rollout is filed under the day it was created but may be appended to
    // for weeks (resume), so look back well before the range start by day
    // folder and filter by modification time.
    let oldest_day = chrono::DateTime::from_timestamp_millis(start_ms.saturating_sub(45 * DAY_MS))
        .map(|time| time.date_naive());
    let sessions = root.join("sessions");
    for (year, year_dir) in numeric_dirs_desc(&sessions) {
        for (month, month_dir) in numeric_dirs_desc(&year_dir) {
            for (day, day_dir) in numeric_dirs_desc(&month_dir) {
                let too_old = oldest_day
                    .zip(chrono::NaiveDate::from_ymd_opt(year as i32, month, day))
                    .is_some_and(|(oldest, this)| this < oldest);
                if too_old {
                    return;
                }
                let Ok(files) = std::fs::read_dir(&day_dir) else {
                    continue;
                };
                for file in files.flatten() {
                    *listed += 1;
                    if *listed > MAX_LISTED_ENTRIES {
                        return;
                    }
                    let path = file.path();
                    let Some(stem) = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| name.starts_with("rollout-"))
                        .and_then(|name| name.strip_suffix(".jsonl"))
                    else {
                        continue;
                    };
                    let Some(id) = stem
                        .get(stem.len().saturating_sub(36)..)
                        .filter(|id| looks_like_session_id(id))
                    else {
                        continue;
                    };
                    let Ok(metadata) = file.metadata() else {
                        continue;
                    };
                    let Some(modified) = modified_ms(&metadata).filter(|ms| *ms >= start_ms) else {
                        continue;
                    };
                    out.push(Candidate {
                        class: AgentClass::Codex,
                        session_id: id.to_owned(),
                        path,
                        modified_ms: modified,
                        length: metadata.len(),
                    });
                }
            }
        }
    }
}

// ------------------------------------------------------------- read + cache

/// Range-independent facts about one session, cached by file signature so a
/// range change or refresh re-slices the spend without re-reading.
struct CachedSession {
    signature: (u64, i64, u64, i64),
    spend: Arc<Vec<SpendBucket>>,
    project: Option<PathBuf>,
    first_prompt: Option<String>,
    last_ms: Option<i64>,
    proxy_backed: bool,
}

fn session_cache() -> &'static Mutex<HashMap<PathBuf, CachedSession>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedSession>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Files that belong to the session besides its main transcript: Claude
/// sub-agent transcripts, Codex sub-agent threads.
fn extra_files(candidate: &Candidate) -> Vec<PathBuf> {
    match candidate.class {
        AgentClass::Claude => crate::session_stats_store::ClaudeSubagentFiles::new(&candidate.path)
            .map(|files| files.flatten().take(5_000).collect())
            .unwrap_or_default(),
        AgentClass::Codex => crate::session_stats_codex_children::children_of(&candidate.path),
        _ => Vec::new(),
    }
}

fn signature(candidate: &Candidate, extras: &[PathBuf]) -> (u64, i64, u64, i64) {
    let mut newest = 0_i64;
    let mut bytes = 0_u64;
    for extra in extras {
        if let Ok(metadata) = std::fs::metadata(extra) {
            newest = newest.max(modified_ms(&metadata).unwrap_or(0));
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    (candidate.length, candidate.modified_ms, bytes, newest)
}

fn cached(candidate: &Candidate, signature: (u64, i64, u64, i64)) -> Option<CachedHit> {
    let cache = session_cache().lock().ok()?;
    let entry = cache.get(&candidate.path)?;
    (entry.signature == signature).then(|| CachedHit {
        spend: Arc::clone(&entry.spend),
        project: entry.project.clone(),
        first_prompt: entry.first_prompt.clone(),
        last_ms: entry.last_ms,
        proxy_backed: entry.proxy_backed,
    })
}

struct CachedHit {
    spend: Arc<Vec<SpendBucket>>,
    project: Option<PathBuf>,
    first_prompt: Option<String>,
    last_ms: Option<i64>,
    proxy_backed: bool,
}

fn remember(candidate: &Candidate, signature: (u64, i64, u64, i64), hit: &CachedHit) {
    let Ok(mut cache) = session_cache().lock() else {
        return;
    };
    let held: usize = cache.values().map(|entry| entry.spend.len()).sum();
    if held + hit.spend.len() > CACHE_BUCKET_LIMIT {
        cache.clear();
    }
    if hit.spend.len() <= CACHE_BUCKET_LIMIT {
        cache.insert(
            candidate.path.clone(),
            CachedSession {
                signature,
                spend: Arc::clone(&hit.spend),
                project: hit.project.clone(),
                first_prompt: hit.first_prompt.clone(),
                last_ms: hit.last_ms,
                proxy_backed: hit.proxy_backed,
            },
        );
    }
}

/// Reads a session's main transcript and its extra files to the end.
fn read_spend(
    candidate: &Candidate,
    extras: &[PathBuf],
    cancelled: &impl Fn() -> bool,
) -> std::io::Result<(Vec<SpendBucket>, Option<i64>)> {
    let mut accumulator = StatsAccumulator::new(candidate.class.clone());
    // One pass reads a bounded slice; repeat until the file is consumed.
    loop {
        let before = accumulator.offset();
        accumulator.ingest_file_cancellable(&candidate.path, cancelled, |_, _, _| {})?;
        if accumulator.offset() == before {
            break;
        }
    }
    for extra in extras {
        let result = match candidate.class {
            AgentClass::Codex => accumulator.ingest_codex_child_to_end(extra, cancelled),
            _ => accumulator.ingest_extra_file_to_end(extra, cancelled),
        };
        // A file removed since listing adds nothing more.
        match result {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    let stats = accumulator.snapshot();
    Ok((stats.spend, stats.last_at_ms))
}

/// Section id of sessions found on disk that ran through a Kilo Proxy
/// launcher; they cannot be tied to one configured custom agent.
pub(crate) const PROXY_PROFILE_ID: &str = "kilo-proxy-outside-ilium";

/// Only the head of a transcript is searched for the proxy marker.
const PROXY_MARKER_MAX_RECORDS: usize = 64;
const PROXY_MARKER_MAX_BYTES: usize = 1024 * 1024;
/// Tool name of the web-search MCP server that the Kilo Proxy launchers
/// (`claude-proxy`, `claude-brainiac`) register for every run; it appears in
/// the tool list recorded at the start of the transcript.
const PROXY_MARKER: &[u8] = b"mcp__kilo-search__";

/// True when the first records of a Claude transcript show a Kilo Proxy
/// launcher. Those sessions use another backend than the Anthropic account.
fn is_kilo_proxy_transcript(path: &Path) -> bool {
    use std::io::{BufRead, Read};
    let Ok(file) = ilium_platform::secure_fs::open_regular_file(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file.take(PROXY_MARKER_MAX_BYTES as u64));
    let mut line = Vec::new();
    for _ in 0..PROXY_MARKER_MAX_RECORDS {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        if line
            .windows(PROXY_MARKER.len())
            .any(|window| window == PROXY_MARKER)
        {
            return true;
        }
    }
    false
}

/// `You are Claude Code running through the Kilo Proxy "Brainiac" mode in
/// /work/dir (…). …` says nothing about the task; show mode and folder.
fn proxy_boilerplate_label(text: &str) -> Option<String> {
    let rest = text
        .trim_start()
        .strip_prefix("You are Claude Code running through the Kilo Proxy")?;
    let rest = rest.trim_start();
    let (mode, rest) = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"')?,
        None => ("", rest),
    };
    let directory = rest
        .split_once(" in ")
        .map(|(_, after)| after.split_whitespace().next().unwrap_or(""))
        .unwrap_or("");
    let folder = Path::new(directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let mut label = String::from("Kilo Proxy");
    if !mode.is_empty() {
        label.push_str(&format!(" {mode}"));
    }
    if !folder.is_empty() {
        label.push_str(&format!(" · {folder}"));
    }
    Some(label)
}

/// Working directory and the first real user prompt, from the transcript head.
fn read_head(candidate: &Candidate) -> (Option<PathBuf>, Option<String>) {
    use std::io::Read;
    let Ok(file) = ilium_platform::secure_fs::open_regular_file(&candidate.path) else {
        return (None, None);
    };
    let mut head = Vec::new();
    if file.take(HEAD_BYTES as u64).read_to_end(&mut head).is_err() {
        return (None, None);
    }
    let mut project = None;
    let mut prompt = None;
    for line in head.split(|byte| *byte == b'\n') {
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if project.is_none() {
            let cwd = record
                .get("cwd")
                .or_else(|| record.get("payload").and_then(|payload| payload.get("cwd")))
                .and_then(serde_json::Value::as_str);
            project = cwd.filter(|cwd| !cwd.is_empty()).map(PathBuf::from);
        }
        if prompt.is_none() {
            prompt = first_prompt_of(&candidate.class, &record);
        }
        if project.is_some() && prompt.is_some() {
            break;
        }
    }
    (project, prompt)
}

fn first_prompt_of(class: &AgentClass, record: &serde_json::Value) -> Option<String> {
    use serde_json::Value;
    let text = match class {
        AgentClass::Claude => {
            if record.get("type").and_then(Value::as_str) != Some("user")
                || record.get("isMeta").and_then(Value::as_bool) == Some(true)
                || record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            match record.get("message")?.get("content")? {
                Value::String(text) => text.clone(),
                Value::Array(blocks) => blocks
                    .iter()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => return None,
            }
        }
        AgentClass::Codex => {
            let payload = record.get("payload")?;
            if record.get("type").and_then(Value::as_str) != Some("event_msg")
                || payload.get("type").and_then(Value::as_str) != Some("user_message")
            {
                return None;
            }
            payload.get("message")?.as_str()?.to_owned()
        }
        _ => return None,
    };
    prompt_label(&proxy_boilerplate_label(&text).unwrap_or(text))
}

/// One line, trimmed, without tag-only system text, cut to a readable length.
fn prompt_label(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("Caveat:") {
        return None;
    }
    let line = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= PROMPT_NAME_CHARS {
        return Some(line);
    }
    let cut: String = line.chars().take(PROMPT_NAME_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

// -------------------------------------------------------------------- names

type ArchiveNames = Arc<Vec<(String, String)>>;

fn archive_cache() -> &'static Mutex<HashMap<PathBuf, (u64, i64, ArchiveNames)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (u64, i64, ArchiveNames)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Every session id found in a pane's resume command, mapped to the pane's
/// name in this snapshot.
fn names_in_snapshot(bytes: &[u8]) -> Vec<(String, String)> {
    use serde_json::Value;
    let Ok(snapshot) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let Some(nodes) = snapshot.pointer("/tree/nodes") else {
        return Vec::new();
    };
    let name_of = |node_id: &Value| -> Option<String> {
        let key = node_id.as_u64()?.to_string();
        let node = match nodes {
            Value::Object(map) => map.get(&key)?,
            Value::Array(list) => list
                .iter()
                .find(|node| node.get("id").and_then(Value::as_u64) == node_id.as_u64())?,
            _ => return None,
        };
        let name = node.get("name")?.as_str()?.trim();
        (!name.is_empty()).then(|| name.to_owned())
    };
    let mut found = Vec::new();
    for pane in snapshot
        .get("panes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = pane.get("node_id").and_then(name_of) else {
            continue;
        };
        let text = pane.get("kind").map(Value::to_string).unwrap_or_default();
        for id in session_ids_in(&text) {
            found.push((id, name.clone()));
        }
    }
    found
}

fn session_ids_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut ids = Vec::new();
    let mut index = 0;
    while index + 36 <= bytes.len() {
        let window = &text[index..index + 36];
        let shaped = text.is_char_boundary(index + 36)
            && looks_like_session_id(window)
            && window.as_bytes()[8] == b'-'
            && window.as_bytes()[13] == b'-'
            && window.as_bytes()[18] == b'-'
            && window.as_bytes()[23] == b'-';
        if shaped {
            ids.push(window.to_ascii_lowercase());
            index += 36;
        } else {
            index += 1;
        }
    }
    ids
}

/// Session id to Ilium name, newest snapshot first so a rename wins.
pub(crate) struct ArchiveIndex {
    names: HashMap<String, String>,
}

impl ArchiveIndex {
    pub(crate) fn load(projects: &[PathBuf], cancelled: &impl Fn() -> bool) -> Self {
        let mut names = HashMap::new();
        for project in projects {
            let mut files: Vec<(i64, u64, PathBuf)> = Vec::new();
            let ilium = project.join(".ilium");
            let mut directories = vec![ilium.join("sessions")];
            if let Ok(entries) = std::fs::read_dir(ilium.join("backups")) {
                directories.extend(entries.flatten().map(|entry| entry.path()));
            }
            for directory in directories {
                let Ok(entries) = std::fs::read_dir(directory) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_snapshot = path.extension().is_some_and(|ext| ext == "json")
                        || path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.contains(".json") || name.contains(".bak"));
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };
                    if is_snapshot && metadata.is_file() && metadata.len() <= ARCHIVE_FILE_BYTES {
                        files.push((modified_ms(&metadata).unwrap_or(0), metadata.len(), path));
                    }
                }
            }
            files.sort_by(|left, right| right.0.cmp(&left.0));
            files.truncate(ARCHIVE_FILES_PER_PROJECT);
            for (modified, length, path) in files {
                if cancelled() {
                    return Self { names };
                }
                for (id, name) in snapshot_names(&path, length, modified).iter() {
                    names.entry(id.clone()).or_insert_with(|| name.clone());
                }
            }
        }
        Self { names }
    }

    pub(crate) fn name_of(&self, session_id: &str) -> Option<&str> {
        self.names
            .get(&session_id.to_ascii_lowercase())
            .map(String::as_str)
    }
}

fn snapshot_names(path: &Path, length: u64, modified: i64) -> ArchiveNames {
    if let Some((cached_length, cached_modified, names)) = archive_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(path).cloned())
    {
        if cached_length == length && cached_modified == modified {
            return names;
        }
    }
    let names: ArchiveNames = Arc::new(
        std::fs::read(path)
            .map(|bytes| names_in_snapshot(&bytes))
            .unwrap_or_default(),
    );
    if let Ok(mut cache) = archive_cache().lock() {
        if cache.len() >= ARCHIVE_CACHE_FILES {
            cache.clear();
        }
        cache.insert(path.to_path_buf(), (length, modified, Arc::clone(&names)));
    }
    names
}

/// Stable synthetic row key for a session with no pane; the high bit keeps it
/// clear of real pane ids.
pub(crate) fn row_key(session_id: &str) -> u64 {
    let hash = session_id
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    hash | (1 << 63)
}

/// Name shown for a session with no Ilium name and no readable prompt.
pub(crate) fn short_session_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

// --------------------------------------------------------------------- scan

fn discover(job: &DiscoverSessions, cancelled: impl Fn() -> bool) -> DiscoveryOutput {
    let mut output = DiscoveryOutput::default();
    let mut candidates = Vec::new();
    let mut listed = 0;
    for root in &job.roots.claude {
        list_claude(root, job.start_ms, &mut listed, &mut candidates);
    }
    for root in &job.roots.codex {
        list_codex(root, job.start_ms, &mut listed, &mut candidates);
    }
    candidates.retain(|candidate| {
        !job.exclude
            .iter()
            .any(|(class, id)| *class == candidate.class && *id == candidate.session_id)
            && (candidate.class != AgentClass::Codex
                || !crate::session_stats_codex_children::is_sub_agent_thread(&candidate.path))
    });
    candidates.sort_by(|left, right| right.modified_ms.cmp(&left.modified_ms));
    let mut seen = HashSet::new();
    candidates.retain(|candidate| {
        seen.insert((
            candidate.class == AgentClass::Codex,
            candidate.session_id.clone(),
        ))
    });
    if candidates.len() > MAX_CANDIDATES {
        candidates.truncate(MAX_CANDIDATES);
        output.partial = true;
    }
    let mut fresh_bytes = 0_u64;
    let mut loaded: Vec<(Candidate, CachedHit)> = Vec::new();
    for candidate in candidates {
        if cancelled() {
            return output;
        }
        let extras = extra_files(&candidate);
        let signature = signature(&candidate, &extras);
        let hit = match cached(&candidate, signature) {
            Some(hit) => hit,
            None => {
                let weight = signature.0.saturating_add(signature.2);
                if fresh_bytes.saturating_add(weight) > MAX_FRESH_READ_BYTES {
                    output.partial = true;
                    continue;
                }
                fresh_bytes += weight;
                let Ok((spend, last_ms)) = read_spend(&candidate, &extras, &cancelled) else {
                    if cancelled() {
                        return output;
                    }
                    output.unreadable += 1;
                    continue;
                };
                let (project, first_prompt) = read_head(&candidate);
                let hit = CachedHit {
                    spend: Arc::new(spend),
                    project,
                    first_prompt,
                    last_ms,
                    proxy_backed: candidate.class == AgentClass::Claude
                        && is_kilo_proxy_transcript(&candidate.path),
                };
                remember(&candidate, signature, &hit);
                hit
            }
        };
        loaded.push((candidate, hit));
    }
    let mut projects = job.archive_projects.clone();
    for (_, hit) in &loaded {
        if let Some(project) = &hit.project {
            if !projects.contains(project) {
                projects.push(project.clone());
            }
        }
    }
    projects.truncate(64);
    let archive = ArchiveIndex::load(&projects, &cancelled);
    for (candidate, hit) in loaded {
        let (models, _) = model_workloads(&hit.spend, job.start_ms, job.end_ms, &job.prices);
        // A session with nothing spent inside the range does not appear.
        if !models.iter().any(|model| model.tokens.total() > 0) {
            continue;
        }
        let name = archive
            .name_of(&candidate.session_id)
            .map(str::to_owned)
            .or(hit.first_prompt)
            .unwrap_or_else(|| short_session_id(&candidate.session_id));
        output.sessions.push(DiscoveredSession {
            class: candidate.class,
            proxy_backed: hit.proxy_backed,
            session_id: candidate.session_id,
            name,
            models,
            last_ms: hit.last_ms,
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_ID: &str = "11111111-2222-4333-8444-555555555555";
    const CODEX_ID: &str = "01a1c2cd-0199-7f22-8a9c-c67c741d0ad0";

    fn job(home: &Path, start_ms: i64, end_ms: i64) -> DiscoverSessions {
        DiscoverSessions {
            roots: ProviderRoots {
                claude: vec![home.join(".claude")],
                codex: vec![home.join(".codex")],
            },
            start_ms,
            end_ms,
            prices: PriceTable::default(),
            exclude: Vec::new(),
            archive_projects: Vec::new(),
        }
    }

    fn claude_line(id: &str, model: &str, at: &str, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{at}","cwd":"/work/demo","message":{{"id":"{id}","model":"{model}","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":1,"output_tokens":{output}}}}}}}"#
        )
    }

    fn write_claude(home: &Path, id: &str, lines: &[String]) -> PathBuf {
        let directory = home.join(".claude/projects/-work-demo");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{id}.jsonl"));
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn user_line(text: &str) -> String {
        format!(
            r#"{{"type":"user","timestamp":"2026-11-10T09:59:00.000Z","cwd":"/work/demo","message":{{"role":"user","content":"{text}"}}}}"#
        )
    }

    /// Manual check on the real home folder:
    /// `<test binary> real_home_discovery_report --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_home_discovery_report() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME is set"));
        let end_ms = chrono::Utc::now().timestamp_millis();
        // Real project folders that hold an `.ilium` archive.
        let mut archive_projects = Vec::new();
        let mut pending = vec![(home.join("dev"), 0_u8)];
        while let Some((directory, depth)) = pending.pop() {
            if directory.join(".ilium").is_dir() {
                archive_projects.push(directory.clone());
            }
            if depth >= 4 {
                continue;
            }
            for entry in std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && !name.starts_with('.')
                    && name != "node_modules"
                    && name != "target"
                {
                    pending.push((entry.path(), depth + 1));
                }
            }
        }
        println!("archive_projects={}", archive_projects.len());
        let plain = discover(
            &DiscoverSessions {
                roots: ProviderRoots::discover(&home, std::iter::empty()),
                start_ms: end_ms - DAY_MS,
                end_ms,
                prices: PriceTable::default(),
                exclude: Vec::new(),
                archive_projects: Vec::new(),
            },
            || false,
        );
        for (label, days) in [("1d-with-archive", 1_i64), ("7d-with-archive", 7)] {
            let started = std::time::Instant::now();
            let output = discover(
                &DiscoverSessions {
                    roots: ProviderRoots::discover(&home, std::iter::empty()),
                    start_ms: end_ms - days * DAY_MS,
                    end_ms,
                    prices: PriceTable::default(),
                    exclude: Vec::new(),
                    archive_projects: archive_projects.clone(),
                },
                || false,
            );
            if days == 1 {
                let renamed: Vec<_> = output
                    .sessions
                    .iter()
                    .filter_map(|session| {
                        let before = plain
                            .sessions
                            .iter()
                            .find(|other| other.session_id == session.session_id)?;
                        (before.name != session.name)
                            .then(|| format!("{:?} -> {:?}", before.name, session.name))
                    })
                    .collect();
                println!("names_changed_by_archive={}", renamed.len());
                for line in renamed.iter().take(8) {
                    println!("  renamed {line}");
                }
            }
            let mut by_model: std::collections::BTreeMap<String, (u64, u64)> =
                std::collections::BTreeMap::new();
            for session in &output.sessions {
                for model in &session.models {
                    let entry = by_model.entry(model.model.clone()).or_default();
                    entry.0 += model.tokens.input + model.tokens.cache_read;
                    entry.1 += model.tokens.output;
                }
            }
            println!(
                "range={label} sessions={} partial={} unreadable={} elapsed_ms={}",
                output.sessions.len(),
                output.partial,
                output.unreadable,
                started.elapsed().as_millis()
            );
            for (model, (input, output_tokens)) in &by_model {
                println!("  model={model} prompt_side={input} output={output_tokens}");
            }
            for session in output.sessions.iter().take(5) {
                println!(
                    "  sample class={:?} id={} name={:?}",
                    session.class, session.session_id, session.name
                );
            }
        }
    }

    #[test]
    fn kilo_proxy_sessions_are_marked_and_plain_sessions_are_not() {
        let home = tempfile::tempdir().unwrap();
        let proxy_attachment = r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["WebSearch","mcp__kilo-search__web_fetch"]}}"#;
        let proxy_id = "aaaaaaaa-2222-4333-8444-555555555555";
        write_claude(
            home.path(),
            proxy_id,
            &[
                proxy_attachment.to_owned(),
                user_line("You are Claude Code running through the Kilo Proxy \\\"Brainiac\\\" mode in /home/arthur/dev/ai/kilo-proxy (Bun/TypeScript). Work in parallel."),
                claude_line("m1", "claude-sonnet-5-5", "2026-11-10T10:00:00.000Z", 10),
            ],
        );
        write_claude(
            home.path(),
            CLAUDE_ID,
            &[
                user_line("Fix the build please"),
                claude_line("m2", "claude-sonnet-5-5", "2026-11-10T10:00:00.000Z", 10),
            ],
        );
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .timestamp_millis()
        };
        let output = discover(
            &job(
                home.path(),
                at("2026-11-09T00:00:00Z"),
                at("2026-11-11T00:00:00Z"),
            ),
            || false,
        );
        let find = |id: &str| output.sessions.iter().find(|s| s.session_id == id).unwrap();
        assert!(find(proxy_id).proxy_backed);
        assert_eq!(find(proxy_id).name, "Kilo Proxy Brainiac · kilo-proxy");
        assert!(!find(CLAUDE_ID).proxy_backed);
        assert_eq!(find(CLAUDE_ID).name, "Fix the build please");
    }

    #[test]
    fn the_proxy_marker_is_only_searched_in_the_transcript_head() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("late.jsonl");
        let mut text = String::new();
        for _ in 0..80 {
            text.push_str("{\"type\":\"user\"}\n");
        }
        text.push_str("mcp__kilo-search__web_fetch\n");
        std::fs::write(&path, text).unwrap();
        assert!(!is_kilo_proxy_transcript(&path));
        std::fs::write(&path, "x\nmcp__kilo-search__web_fetch\n").unwrap();
        assert!(is_kilo_proxy_transcript(&path));
    }

    #[test]
    fn a_session_with_no_pane_is_found_and_counts_only_the_selected_range() {
        let home = tempfile::tempdir().unwrap();
        write_claude(
            home.path(),
            CLAUDE_ID,
            &[
                user_line("Fix the build please"),
                claude_line("m1", "claude-sonnet-5-5", "2026-11-01T10:00:00.000Z", 1000),
                claude_line("m2", "claude-sonnet-5-5", "2026-11-10T10:00:00.000Z", 500),
            ],
        );
        let at = |text: &str| {
            chrono::DateTime::parse_from_rfc3339(text)
                .unwrap()
                .timestamp_millis()
        };
        // The file's modification time is "now", so every start before it
        // passes the cheap prefilter; slicing is by event time.
        let output = discover(
            &job(
                home.path(),
                at("2026-11-09T00:00:00Z"),
                at("2026-11-11T00:00:00Z"),
            ),
            || false,
        );
        assert_eq!(output.sessions.len(), 1);
        let session = &output.sessions[0];
        assert_eq!(session.session_id, CLAUDE_ID);
        assert_eq!(session.name, "Fix the build please");
        let output_tokens: u64 = session.models.iter().map(|model| model.tokens.output).sum();
        assert_eq!(
            output_tokens, 500,
            "the 2026-11-01 event is outside the range"
        );
        let none = discover(
            &job(
                home.path(),
                at("2026-12-01T00:00:00Z"),
                at("2026-12-02T00:00:00Z"),
            ),
            || false,
        );
        assert!(none.sessions.is_empty(), "no spend in range means no row");
    }

    #[test]
    fn sessions_already_owned_by_a_pane_are_not_counted_again() {
        let home = tempfile::tempdir().unwrap();
        write_claude(
            home.path(),
            CLAUDE_ID,
            &[claude_line(
                "m1",
                "claude-sonnet-5-5",
                "2026-11-10T10:00:00.000Z",
                10,
            )],
        );
        let mut request = job(home.path(), 0, i64::MAX / 2);
        request
            .exclude
            .push((AgentClass::Claude, CLAUDE_ID.to_owned()));
        assert!(discover(&request, || false).sessions.is_empty());
    }

    #[test]
    fn codex_threads_fold_into_their_parent_and_are_not_listed_alone() {
        let home = tempfile::tempdir().unwrap();
        let day = home.path().join(".codex/sessions/2026/11/10");
        std::fs::create_dir_all(&day).unwrap();
        let child_id = "01a1c2f8-90da-7e43-b09d-9815212356ff";
        let usage = |total: u64| {
            format!(
                r#"{{"timestamp":"2026-11-10T10:00:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{total},"cached_input_tokens":0,"output_tokens":{total},"reasoning_output_tokens":0,"total_tokens":{}}}}}}}}}"#,
                total * 2
            )
        };
        let meta = |parent: Option<&str>| match parent {
            Some(parent) => format!(
                r#"{{"type":"session_meta","payload":{{"cwd":"/work/demo","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"{parent}"}}}}}}}}}}"#
            ),
            None => r#"{"type":"session_meta","payload":{"cwd":"/work/demo","source":"cli"}}"#
                .to_owned(),
        };
        let turn = r#"{"type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#;
        std::fs::write(
            day.join(format!("rollout-2026-11-10T10-00-00-{CODEX_ID}.jsonl")),
            format!("{}\n{turn}\n{}\n", meta(None), usage(100)),
        )
        .unwrap();
        std::fs::write(
            day.join(format!("rollout-2026-11-10T10-01-00-{child_id}.jsonl")),
            format!("{}\n{turn}\n{}\n", meta(Some(CODEX_ID)), usage(40)),
        )
        .unwrap();
        let output = discover(&job(home.path(), 0, i64::MAX / 2), || false);
        assert_eq!(output.sessions.len(), 1, "the child is not a top-level row");
        assert_eq!(output.sessions[0].session_id, CODEX_ID);
        let total_output: u64 = output.sessions[0]
            .models
            .iter()
            .map(|model| model.tokens.output)
            .sum();
        assert_eq!(total_output, 140, "parent 100 plus child 40");
    }

    #[test]
    fn names_come_from_archived_ilium_snapshots_newest_first() {
        let project = tempfile::tempdir().unwrap();
        let backups = project.path().join(".ilium/backups/default");
        std::fs::create_dir_all(&backups).unwrap();
        let snapshot = |name: &str| {
            format!(
                r#"{{"version":1,"tree":{{"nodes":{{"7":{{"name":"{name}"}}}}}},"panes":[{{"node_id":7,"kind":{{"Terminal":{{"Command":"claude --resume '{CLAUDE_ID}'"}}}}}}]}}"#
            )
        };
        std::fs::write(backups.join("1-a.json"), snapshot("old name")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(backups.join("2-b.json"), snapshot("renamed")).unwrap();
        let index = ArchiveIndex::load(&[project.path().to_path_buf()], &|| false);
        assert_eq!(index.name_of(CLAUDE_ID), Some("renamed"));
        assert_eq!(index.name_of("ffffffff-ffff-4fff-8fff-ffffffffffff"), None);
    }

    #[test]
    fn unnamed_sessions_fall_back_to_the_first_prompt_then_the_short_id() {
        assert_eq!(
            prompt_label("  Fix   the\nbuild  ").as_deref(),
            Some("Fix the build")
        );
        assert_eq!(prompt_label("<system-reminder>x"), None);
        let long = "word ".repeat(40);
        let label = prompt_label(&long).unwrap();
        assert!(label.chars().count() <= PROMPT_NAME_CHARS && label.ends_with('…'));
        assert_eq!(short_session_id(CLAUDE_ID), "11111111");
    }

    #[test]
    fn custom_provider_roots_come_from_known_transcript_paths() {
        let hint = PathBuf::from("/data/custom-claude/projects/-work/abc.jsonl");
        let roots = ProviderRoots::discover(
            Path::new("/home/x"),
            std::iter::once((&AgentClass::Claude, hint.as_path())),
        );
        assert!(roots.claude.contains(&PathBuf::from("/data/custom-claude")));
        assert!(roots.claude.contains(&PathBuf::from("/home/x/.claude")));
        assert!(roots.codex.contains(&PathBuf::from("/home/x/.codex")));
    }
}
