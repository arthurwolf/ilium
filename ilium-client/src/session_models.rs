//! Bounded latest-recorded-model cache for agent tree icons.
//! Uses the existing execution client; no thread or independent quota.
use crate::session_stats_store::StatsRequest;
use ilium_agent_session::{TranscriptLocator, TranscriptReadLimits};
use ilium_core::{AgentClass, NodeId};
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, StorageAdmission,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

const WINDOW: usize = 1024 * 1024;
const REFRESH: Duration = Duration::from_secs(3);
const CODEX_SCREEN_REFRESH: Duration = Duration::from_secs(1);
const MAX_PATH: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}
impl Stamp {
    fn read(meta: &Metadata) -> Self {
        Self {
            length: meta.len(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino())
            },
        }
    }
    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity == other.identity
        }
        #[cfg(not(unix))]
        {
            self.created.is_some() && self.created == other.created
        }
    }
    fn can_extend(&self, previous: &Self) -> bool {
        self.same_file(previous)
            && self.length >= previous.length
            && (self.length != previous.length || self.modified == previous.modified)
    }
}
#[derive(Clone, Debug)]
struct FileState {
    path: PathBuf,
    stamp: Stamp,
    complete_end: u64,
}
#[derive(Clone, Debug)]
struct Scan {
    file: FileState,
    end: u64,
    floor: u64,
    top: u64,
    reset: bool,
}
struct Entry {
    request: StatsRequest,
    generation: u64,
    model: Option<String>,
    model_is_statusline_observation: bool,
    diagnostic: Option<String>,
    file: Option<FileState>,
    scan: Option<Scan>,
    due: Instant,
    _storage: Arc<StorageAdmission>,
}
#[derive(Clone)]
struct Owner {
    pane: NodeId,
    request: StatsRequest,
    generation: u64,
}

pub(crate) struct SessionModelCache {
    client: Option<Client>,
    entries: BTreeMap<NodeId, Entry>,
    codex_screen_models: BTreeMap<NodeId, (StatsRequest, String)>,
    last_codex_screen_scan: Option<Instant>,
    pending: Option<Pending>,
    last_scheduled: Option<NodeId>,
    generation: u64,
    #[cfg(test)]
    claude_config_dir_for_test: Option<PathBuf>,
}
impl Default for SessionModelCache {
    fn default() -> Self {
        Self {
            client: None,
            entries: BTreeMap::new(),
            codex_screen_models: BTreeMap::new(),
            last_codex_screen_scan: None,
            pending: None,
            last_scheduled: None,
            generation: 0,
            #[cfg(test)]
            claude_config_dir_for_test: None,
        }
    }
}
impl std::fmt::Debug for SessionModelCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionModelCache")
            .field("entries", &self.entries.len())
            .finish()
    }
}
enum Pending {
    Reading(Owner, Receipt<ReadWindow>),
    ParseReady(Owner, ParseWindow),
    Parsing(Owner, Receipt<ParseWindow>),
}
impl Pending {
    fn owner(&self) -> &Owner {
        match self {
            Self::Reading(owner, _) | Self::ParseReady(owner, _) | Self::Parsing(owner, _) => owner,
        }
    }
    fn cancel(&self) {
        match self {
            Self::Reading(_, receipt) => receipt.cancel(),
            Self::Parsing(_, receipt) => receipt.cancel(),
            Self::ParseReady(..) => {}
        }
    }
}
impl SessionModelCache {
    #[cfg(test)]
    fn configure_claude_config_dir_for_test(&mut self, config_dir: PathBuf) {
        self.claude_config_dir_for_test = Some(config_dir);
    }

    pub(crate) fn configure_execution(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub(crate) fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
    }
    pub(crate) fn invalidate_model_icons(&mut self) {
        self.cancel_pending();
        self.entries.clear();
        self.codex_screen_models.clear();
        self.last_codex_screen_scan = None;
        self.last_scheduled = None;
    }
    pub(crate) fn model(&self, pane: NodeId, request: &StatsRequest) -> Option<&str> {
        self.entries
            .get(&pane)
            .filter(|entry| {
                entry.request == *request
                    && entry.scan.is_none()
                    && (request.class != AgentClass::Claude
                        || entry.model_is_statusline_observation)
            })?
            .model
            .as_deref()
    }
    pub(crate) fn diagnostic(&self, pane: NodeId, request: &StatsRequest) -> Option<&str> {
        self.entries
            .get(&pane)
            .filter(|entry| entry.request == *request)?
            .diagnostic
            .as_deref()
    }
    fn refresh_codex_screen_models(
        &mut self,
        screens: BTreeMap<NodeId, (StatsRequest, String)>,
        now: Instant,
    ) -> bool {
        if self
            .last_codex_screen_scan
            .is_some_and(|last| now.saturating_duration_since(last) < CODEX_SCREEN_REFRESH)
        {
            return false;
        }
        self.last_codex_screen_scan = Some(now);
        let next = screens
            .into_iter()
            .filter_map(|(pane, (request, contents))| {
                codex_status_line_model(&contents).map(|model| (pane, (request, model)))
            })
            .collect::<BTreeMap<_, _>>();
        if self.codex_screen_models == next {
            return false;
        }
        self.codex_screen_models = next;
        true
    }
    fn codex_screen_model(&self, pane: NodeId, request: &StatsRequest) -> Option<&str> {
        self.codex_screen_models
            .get(&pane)
            .filter(|(observed_request, _)| observed_request == request)
            .map(|(_, model)| model.as_str())
    }
    fn live(&self, owner: &Owner) -> bool {
        self.entries.get(&owner.pane).is_some_and(|entry| {
            entry.generation == owner.generation && entry.request == owner.request
        })
    }
    /// Call with only live supported panes; an empty map disables and clears.
    /// Context reconciliation precedes completion collection, fencing ABA changes.
    pub(crate) fn tick(&mut self, contexts: BTreeMap<NodeId, StatsRequest>, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|pane, entry| contexts.get(pane) == Some(&entry.request));
        let mut changed = before != self.entries.len();
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !self.live(pending.owner()))
        {
            self.cancel_pending();
        }
        for (pane, request) in contexts {
            if self.entries.contains_key(&pane) {
                continue;
            }
            // Charge retained paths, request and map entry before insertion.
            if request.session_id.len() > MAX_PATH
                || request.project_path.capacity() > MAX_PATH
                || request.home.capacity() > MAX_PATH
            {
                continue;
            }
            let bytes = request.session_id.capacity()
                + request.project_path.capacity()
                + request.home.capacity()
                + MAX_PATH * 2
                + 4096;
            let Ok(storage) = crate::execution::process_quota().reserve_external_storage(bytes)
            else {
                continue;
            };
            let Some(generation) = self.generation.checked_add(1) else {
                continue;
            };
            self.generation = generation;
            self.entries.insert(
                pane,
                Entry {
                    request,
                    generation,
                    model: None,
                    model_is_statusline_observation: false,
                    diagnostic: None,
                    file: None,
                    scan: None,
                    due: now,
                    _storage: Arc::new(storage),
                },
            );
        }
        changed |= self.collect(now);
        if self.pending.is_none() {
            self.schedule(now);
        }
        changed
    }
    fn schedule(&mut self, now: Instant) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let eligible = |entry: &&Entry| entry.due <= now;
        let pane = self
            .entries
            .iter()
            .filter(|(_, entry)| eligible(entry))
            .find(|(pane, _)| self.last_scheduled.is_none_or(|last| **pane > last))
            .or_else(|| self.entries.iter().find(|(_, entry)| entry.due <= now))
            .map(|(pane, _)| *pane);
        let Some(pane) = pane else {
            return;
        };
        self.last_scheduled = Some(pane);
        let entry = self.entries.get_mut(&pane).expect("selected entry exists");
        entry.due = now + REFRESH;
        let Ok(storage) = crate::execution::process_quota().reserve_external_storage(2 * WINDOW)
        else {
            return;
        };
        let owner = Owner {
            pane,
            request: entry.request.clone(),
            generation: entry.generation,
        };
        let job = ReadWindow {
            request: entry.request.clone(),
            previous: entry.file.clone(),
            scan: entry.scan.clone(),
            storage: Arc::new(storage),
            #[cfg(test)]
            claude_config_dir: self.claude_config_dir_for_test.clone(),
        };
        match client.try_submit(
            Lane::Io,
            JobCost {
                input_bytes: 16 * WINDOW,
                result_bytes: 2 * WINDOW,
            },
            job,
        ) {
            Ok(receipt) => self.pending = Some(Pending::Reading(owner, receipt)),
            Err(_) => { /* due time and cursor retain fair, bounded retries */ }
        }
    }
    fn collect(&mut self, now: Instant) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        match pending {
            Pending::Reading(owner, mut receipt) => match receipt.try_take() {
                JobPoll::Pending => self.pending = Some(Pending::Reading(owner, receipt)),
                JobPoll::Lost | JobPoll::Taken => {
                    return self.fail(&owner, "Model read result unavailable".into(), now);
                }
                JobPoll::Ready(outcome) => {
                    let (outcome, hold) = outcome.into_parts();
                    match outcome {
                        JobOutcome::Finished(Ok(ReadResult::Unchanged)) => {}
                        JobOutcome::Finished(Ok(ReadResult::RecordedModel(model))) => {
                            let entry = self.entries.get_mut(&owner.pane).expect("live owner");
                            let changed = entry.model != model;
                            entry.model = model;
                            entry.model_is_statusline_observation = true;
                            entry.file = None;
                            entry.scan = None;
                            entry.diagnostic = None;
                            entry.due = now + REFRESH;
                            drop(hold);
                            return changed;
                        }
                        JobOutcome::Finished(Ok(ReadResult::Window(window))) => {
                            self.pending = Some(Pending::ParseReady(
                                owner,
                                ParseWindow {
                                    class: window.class.clone(),
                                    window,
                                },
                            ))
                        }
                        JobOutcome::Finished(Err(error)) => {
                            drop(hold);
                            return self.fail(&owner, error, now);
                        }
                        _ => {
                            drop(hold);
                            return self.fail(
                                &owner,
                                "Model observation cancelled or failed".into(),
                                now,
                            );
                        }
                    }
                    drop(hold);
                }
            },
            Pending::ParseReady(owner, job) => {
                let Some(client) = self.client.clone() else {
                    return self.fail(&owner, "Model worker unavailable".into(), now);
                };
                match client.try_submit(
                    Lane::Cpu,
                    JobCost {
                        input_bytes: 4 * WINDOW,
                        result_bytes: 256 * 1024,
                    },
                    job,
                ) {
                    Ok(receipt) => self.pending = Some(Pending::Parsing(owner, receipt)),
                    Err(rejected) if rejected.reason == ilium_execution::RejectReason::Busy => {
                        self.pending = Some(Pending::ParseReady(owner, rejected.value))
                    }
                    Err(rejected) => {
                        return self.fail(
                            &owner,
                            format!("Model parser admission: {:?}", rejected.reason),
                            now,
                        );
                    }
                }
            }
            Pending::Parsing(owner, mut receipt) => match receipt.try_take() {
                JobPoll::Pending => self.pending = Some(Pending::Parsing(owner, receipt)),
                JobPoll::Lost | JobPoll::Taken => {
                    return self.fail(&owner, "Model parser result unavailable".into(), now);
                }
                JobPoll::Ready(outcome) => {
                    let (outcome, hold) = outcome.into_parts();
                    if !self.live(&owner) {
                        return false;
                    }
                    match outcome {
                        JobOutcome::Finished(Ok(parsed)) => {
                            let entry = self.entries.get_mut(&owner.pane).expect("live owner");
                            let previous = entry.model.clone();
                            let was_scanning = entry.scan.is_some();
                            match parsed {
                                Parsed::Continue(scan) => {
                                    // A newer completed record remains unexamined.
                                    // Preserve prior evidence internally, but render fallback during scanning.
                                    entry.scan = Some(scan);
                                    entry.due = now;
                                }
                                Parsed::Complete { file, model, reset } => {
                                    if model.is_some() || reset {
                                        entry.model = model;
                                    }
                                    entry.model_is_statusline_observation = false;
                                    entry.file = Some(file);
                                    entry.scan = None;
                                    entry.diagnostic = None;
                                    entry.due = now + REFRESH;
                                }
                            }
                            drop(hold);
                            return previous != entry.model || entry.scan.is_some() || was_scanning;
                        }
                        JobOutcome::Finished(Err(error)) => {
                            drop(hold);
                            return self.fail(&owner, error, now);
                        }
                        _ => {
                            drop(hold);
                            return self.fail(
                                &owner,
                                "Model parsing cancelled or failed".into(),
                                now,
                            );
                        }
                    }
                }
            },
        }
        false
    }
    fn fail(&mut self, owner: &Owner, error: String, now: Instant) -> bool {
        if !self.live(owner) {
            return false;
        }
        let entry = self.entries.get_mut(&owner.pane).expect("live owner");
        let changed = entry.model.take().is_some();
        entry.file = None;
        entry.scan = None;
        entry.diagnostic = Some(error);
        entry.due = now + REFRESH;
        changed
    }
}
impl Drop for SessionModelCache {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

impl crate::app::App {
    fn live_model_contexts(&self) -> BTreeMap<NodeId, StatsRequest> {
        if !self.ui_settings.agent_tree_model_icons {
            return BTreeMap::new();
        }
        let Some(home) =
            directories::BaseDirs::new().map(|directories| directories.home_dir().to_path_buf())
        else {
            return BTreeMap::new();
        };
        self.tree
            .panes()
            .filter_map(|node| {
                let (class, session_id, project_path) =
                    self.known_agent_history_context(node.id)?;
                matches!(
                    &class,
                    AgentClass::Claude | AgentClass::Codex | AgentClass::Antigravity
                )
                .then_some((
                    node.id,
                    StatsRequest {
                        class,
                        session_id,
                        project_path,
                        home: home.clone(),
                    },
                ))
            })
            .collect()
    }

    fn live_codex_screens(
        &self,
        contexts: &BTreeMap<NodeId, StatsRequest>,
    ) -> BTreeMap<NodeId, (StatsRequest, String)> {
        contexts
            .iter()
            .filter_map(|(pane, request)| {
                if request.class != AgentClass::Codex {
                    return None;
                }
                let Some(crate::app::PaneRuntime::Terminal(view)) = self.panes.get(pane) else {
                    return None;
                };
                Some((
                    *pane,
                    (
                        request.clone(),
                        view.with_screen(|screen| screen.contents()),
                    ),
                ))
            })
            .collect()
    }

    pub(crate) fn tick_session_models(&mut self, now: Instant) -> bool {
        let contexts = self.live_model_contexts();
        let claude_projects = contexts
            .values()
            .filter(|request| request.class == AgentClass::Claude)
            .map(|request| request.project_path.clone())
            .collect::<Vec<_>>();
        let enabled = self.ui_settings.agent_tree_model_icons;
        let executable = enabled.then(|| std::env::current_exe().ok()).flatten();
        if let Err(error) = crate::claude_model_statusline::reconcile_app_setting(
            enabled,
            &claude_projects,
            executable.as_deref(),
        ) {
            self.status_message = Some(format!(
                "Claude model capture configuration could not be updated: {error}"
            ));
        }
        let screens = self.live_codex_screens(&contexts);
        let transcript_changed = self.session_models.tick(contexts, now);
        transcript_changed
            | self
                .session_models
                .refresh_codex_screen_models(screens, now)
    }

    pub(crate) fn current_tree_models(&self) -> std::collections::HashMap<NodeId, String> {
        self.live_model_contexts()
            .into_iter()
            .filter_map(|(pane, request)| {
                if request.class == AgentClass::Codex {
                    return self
                        .session_models
                        .codex_screen_model(pane, &request)
                        .map(|model| (pane, model.to_owned()));
                }
                self.session_models
                    .model(pane, &request)
                    .map(|model| (pane, model.to_owned()))
            })
            .collect()
    }
}

/// Codex's default TUI status line places the selected model first and joins
/// status items with ` · `. Limit inspection to the visible footer area and
/// require the same model family the toolbar can render; absent or custom
/// status lines deliberately leave the tree on its provider fallback.
fn codex_status_line_model(contents: &str) -> Option<String> {
    contents
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .find_map(|line| {
            let leading_segment = line.split_once(" · ")?.0.trim();
            let candidate = leading_segment.split_ascii_whitespace().next()?;
            crate::agent_toolbar::model_icon_glyph(
                AgentClass::Codex,
                candidate,
                &crate::icon_settings::IconSettings::default(),
            )?;
            Some(candidate.to_owned())
        })
}

struct ReadWindow {
    request: StatsRequest,
    previous: Option<FileState>,
    scan: Option<Scan>,
    storage: Arc<StorageAdmission>,
    #[cfg(test)]
    claude_config_dir: Option<PathBuf>,
}
enum ReadResult {
    Unchanged,
    Window(Window),
    RecordedModel(Option<String>),
}
struct Window {
    class: AgentClass,
    file: FileState,
    start: u64,
    end: u64,
    floor: u64,
    top: Option<u64>,
    reset: bool,
    bytes: Vec<u8>,
    _storage: Arc<StorageAdmission>,
}
impl Job for ReadWindow {
    type Output = ReadResult;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Self::Output, String> {
        if context.stop_requested() {
            return Err("Model read cancelled".into());
        }
        let locator = TranscriptLocator::new_bounded(
            &self.request.home,
            &self.request.project_path,
            TranscriptReadLimits {
                line_bytes: WINDOW,
                total_read_bytes: 16 * WINDOW,
                scanned_entries: 1_000_000,
                retained_path_bytes: WINDOW,
            },
        );
        if self.request.class == AgentClass::Claude {
            #[cfg(test)]
            let model = if let Some(config_dir) = self.claude_config_dir.as_deref() {
                crate::claude_model_statusline::model_for_verified_session_in_config(
                    &self.request.home,
                    &self.request.project_path,
                    &self.request.session_id,
                    config_dir,
                )
            } else {
                crate::claude_model_statusline::model_for_verified_session(
                    &self.request.home,
                    &self.request.project_path,
                    &self.request.session_id,
                )
            }
            .map_err(|error| error.to_string())?;
            #[cfg(not(test))]
            let model = crate::claude_model_statusline::model_for_verified_session(
                &self.request.home,
                &self.request.project_path,
                &self.request.session_id,
            )
            .map_err(|error| error.to_string())?;
            return Ok(ReadResult::RecordedModel(model));
        }
        if self.request.class == AgentClass::Antigravity {
            let transcript = locator
                .transcript_for_session(&self.request.class, &self.request.session_id)
                .filter(|transcript| transcript.session_id == self.request.session_id)
                .ok_or_else(|| "No verified Antigravity conversation yet".to_owned())?;
            let model = crate::antigravity_model_statusline::model_for_verified_session(
                &self.request.home,
                &self.request.project_path,
                &transcript.session_id,
            )
            .map_err(|error| error.to_string())?;
            return Ok(ReadResult::RecordedModel(model));
        }
        let previous_path = self
            .scan
            .as_ref()
            .map(|scan| &scan.file.path)
            .or_else(|| self.previous.as_ref().map(|file| &file.path));
        let transcript = previous_path
            .and_then(|path| locator.transcript_from_path(&self.request.class, path))
            .filter(|transcript| transcript.session_id == self.request.session_id)
            .or_else(|| {
                locator.transcript_for_session(&self.request.class, &self.request.session_id)
            });
        if locator.read_limit_reached() {
            return Err(
                "Model transcript discovery did not establish ownership within its limits".into(),
            );
        }
        let transcript = transcript.ok_or_else(|| "No verified model transcript yet".to_owned())?;
        if transcript.path.capacity() > MAX_PATH {
            return Err("Model transcript path exceeds bounds".into());
        }
        let mut reader = File::open(&transcript.path).map_err(|error| error.to_string())?;
        let stamp = Stamp::read(&reader.metadata().map_err(|error| error.to_string())?);
        let previous = self
            .previous
            .filter(|file| file.path == transcript.path && stamp.can_extend(&file.stamp));
        let scan = self
            .scan
            .filter(|scan| scan.file.path == transcript.path && stamp.can_extend(&scan.file.stamp));
        if scan.is_none() && previous.as_ref().is_some_and(|file| file.stamp == stamp) {
            return Ok(ReadResult::Unchanged);
        }
        let (end, floor, top, reset, snapshot_stamp) = match scan {
            Some(scan) => (
                scan.end,
                scan.floor,
                Some(scan.top),
                scan.reset,
                scan.file.stamp,
            ),
            None => (
                stamp.length,
                previous.as_ref().map_or(0, |file| file.complete_end),
                None,
                previous.is_none(),
                stamp,
            ),
        };
        let start = end.saturating_sub(WINDOW as u64).max(floor);
        reader
            .seek(SeekFrom::Start(start))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; (end - start) as usize];
        reader
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        if context.stop_requested() {
            return Err("Model read cancelled".into());
        }
        Ok(ReadResult::Window(Window {
            class: self.request.class,
            file: FileState {
                path: transcript.path,
                stamp: snapshot_stamp,
                complete_end: 0,
            },
            start,
            end,
            floor,
            top,
            reset,
            bytes,
            _storage: self.storage,
        }))
    }
}
struct ParseWindow {
    class: AgentClass,
    window: Window,
}
enum Parsed {
    Continue(Scan),
    Complete {
        file: FileState,
        model: Option<String>,
        reset: bool,
    },
}
impl Job for ParseWindow {
    type Output = Parsed;
    type Error = String;
    fn run(self, context: JobContext) -> Result<Parsed, String> {
        parse_window(self.class, self.window, || context.stop_requested())
    }
}
fn parse_window(
    class: AgentClass,
    mut window: Window,
    cancelled: impl Fn() -> bool,
) -> Result<Parsed, String> {
    // A non-newline-terminated tail is incomplete and must be re-read on append.
    let end = window
        .bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if end == 0 && window.start > window.floor {
        return Err("Latest model evidence exceeds the 1 MiB record bound".into());
    }
    let top = window.top.unwrap_or(window.start + end as u64);
    let begin = if window.start == window.floor {
        0
    } else {
        window.bytes[..end]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(end, |index| index + 1)
    };
    for line in window.bytes[begin..end].rsplit(|byte| *byte == b'\n') {
        if cancelled() {
            return Err("Model parsing cancelled".into());
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if let Some(model) = parse_model_record(&class, line)? {
            window.file.complete_end = top;
            return Ok(Parsed::Complete {
                file: window.file,
                model: Some(model),
                reset: window.reset,
            });
        }
    }
    if window.start == window.floor {
        window.file.complete_end = top;
        return Ok(Parsed::Complete {
            file: window.file,
            model: None,
            reset: window.reset,
        });
    }
    let next = window.start + begin as u64;
    if next >= window.end {
        return Err("Latest model evidence exceeds the 1 MiB record bound".into());
    }
    Ok(Parsed::Continue(Scan {
        file: window.file,
        end: next,
        floor: window.floor,
        top,
        reset: window.reset,
    }))
}

#[derive(Deserialize)]
struct RecordKind<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
}
#[derive(Deserialize)]
struct AssistantRecord<'a> {
    #[serde(borrow)]
    message: Option<ModelField<'a>>,
}
#[derive(Deserialize)]
struct ContextRecord<'a> {
    #[serde(borrow)]
    payload: Option<ModelField<'a>>,
}
#[derive(Deserialize)]
struct ModelField<'a> {
    model: Option<&'a str>,
}
/// Pure CPU parser. Unknown fields are skipped without retaining tool output.
fn parse_model_record(class: &AgentClass, line: &[u8]) -> Result<Option<String>, String> {
    let invalid = |_| "Incomplete or malformed model evidence".to_owned();
    let record: RecordKind<'_> = serde_json::from_slice(line).map_err(invalid)?;
    let model = match (class, record.kind) {
        (AgentClass::Claude, Some("assistant")) if !record.sidechain => {
            let record: AssistantRecord<'_> = serde_json::from_slice(line).map_err(invalid)?;
            record.message.and_then(|message| message.model)
        }
        (AgentClass::Codex, Some("turn_context")) => {
            let record: ContextRecord<'_> = serde_json::from_slice(line).map_err(invalid)?;
            record.payload.and_then(|payload| payload.model)
        }
        _ => None,
    };
    match model {
        Some("<synthetic>") | None => Ok(None),
        Some(model) if model.len() <= 256 => Ok(Some(model.to_owned())),
        Some(_) => Err("Recorded model identifier exceeds 256 bytes".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentActivity, PaneContentKind, PaneStatus, ROOT_ID};
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };

    fn isolated_execution() -> (Execution, Client) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 1,
            jobs: 16,
            service_jobs: 0,
            input_bytes: 64 * WINDOW,
            result_bytes: 16 * WINDOW,
            worker_threads: 2,
            // The admission also reserves bank metadata, in addition to both
            // workers' resident memory.
            worker_bytes: 8 * WINDOW,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: if threads == 0 { 0 } else { 2 * WINDOW },
        };
        let owner = Execution::start(
            quota,
            ExecutionConfig {
                cpu: lane(1),
                io: lane(1),
                service: lane(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: 8,
                service_jobs: 0,
                input_bytes: 64 * WINDOW,
                result_bytes: 16 * WINDOW,
            })
            .unwrap();
        (owner, client)
    }

    fn request(session: &str) -> StatsRequest {
        StatsRequest {
            class: AgentClass::Codex,
            session_id: session.into(),
            project_path: "/work".into(),
            home: "/home/test".into(),
        }
    }

    #[test]
    fn enabled_model_icons_request_antigravity_without_enabling_stats_support() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Antigravity, AgentActivity::Idle, None),
            )
            .unwrap();
        app.agent_session_ids
            .insert(pane, "33333333-3333-4333-8333-333333333333".to_string());
        assert!(app.live_model_contexts().is_empty());
        app.ui_settings.agent_tree_model_icons = true;

        assert!(app.session_stats_request(pane).is_none());
        let contexts = app.live_model_contexts();
        let request = contexts.get(&pane).expect("Antigravity model request");
        assert_eq!(request.class, AgentClass::Antigravity);
        assert_eq!(request.session_id, "33333333-3333-4333-8333-333333333333");
        assert_eq!(request.project_path, std::path::PathBuf::from("/work"));
    }

    #[test]
    fn enabled_model_icons_include_verified_unavailable_sessions() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let session_id = "33333333-3333-4333-8333-333333333333";
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::AgentUnavailable(Box::new(ilium_core::AgentRecovery {
                    last_known_state: ilium_core::AgentState::from_activity(
                        AgentClass::Claude,
                        AgentActivity::Idle,
                        None,
                    ),
                    process: ilium_core::AgentProcessKey {
                        class: AgentClass::Claude,
                        process_id: 42,
                        started_at_unix_seconds: 1,
                    },
                    availability: ilium_core::AgentAvailability::Exited(
                        ilium_core::AgentExitOutcome::ExitCode(0),
                    ),
                    signal_name: None,
                    session_id: Some(session_id.into()),
                    last_prompt: None,
                    previous_exact_prompt: None,
                    latest_prompt_unavailable: false,
                })),
            )
            .unwrap();
        assert!(app.live_model_contexts().is_empty());
        app.ui_settings.agent_tree_model_icons = true;

        let contexts = app.live_model_contexts();
        let request = contexts
            .get(&pane)
            .expect("verified retained model request");
        assert_eq!(request.class, AgentClass::Claude);
        assert_eq!(request.session_id, session_id);
        assert_eq!(request.project_path, std::path::PathBuf::from("/work"));
    }

    #[test]
    fn active_claude_uses_provider_fallback_until_live_model_is_observed() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        app.agent_session_ids
            .insert(pane, "33333333-3333-4333-8333-333333333333".to_string());
        app.ui_settings.agent_tree_model_icons = true;

        let contexts = app.live_model_contexts();
        assert_eq!(
            contexts.get(&pane).expect("active Claude context").class,
            AgentClass::Claude
        );
        app.session_models.tick(contexts, Instant::now());
        app.session_models.entries.get_mut(&pane).unwrap().model = Some("claude-sonnet-5".into());

        assert!(
            app.current_tree_models().get(&pane).is_none(),
            "an active Claude session must not show a stale transcript model before a live status-line observation"
        );
    }

    #[test]
    fn active_claude_uses_then_invalidates_verified_statusline_model() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        app.agent_session_ids
            .insert(pane, "44444444-4444-4444-8444-444444444444".to_string());
        app.ui_settings.agent_tree_model_icons = true;

        let contexts = app.live_model_contexts();
        app.session_models.tick(contexts, Instant::now());
        let entry = app.session_models.entries.get_mut(&pane).unwrap();
        entry.model = Some("claude-opus-4-1".into());
        entry.model_is_statusline_observation = true;

        assert_eq!(
            app.current_tree_models().get(&pane).map(String::as_str),
            Some("claude-opus-4-1")
        );

        app.session_models.invalidate_model_icons();

        assert!(app.current_tree_models().get(&pane).is_none());
    }

    #[test]
    fn model_icon_transition_clears_models_for_every_provider() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let panes = [
            AgentClass::Claude,
            AgentClass::Codex,
            AgentClass::Antigravity,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, class)| {
            let pane = app
                .tree
                .add_pane(group, format!("agent-{index}"), PaneContentKind::Terminal)
                .unwrap();
            let mut request = request(&format!("session-{index}"));
            request.class = class;
            (pane, request)
        })
        .collect::<BTreeMap<_, _>>();
        app.ui_settings.agent_tree_model_icons = true;
        app.session_models.tick(panes.clone(), Instant::now());
        for (pane, entry) in &mut app.session_models.entries {
            entry.model = Some("claude-opus-4-1".into());
            entry.model_is_statusline_observation = true;
            app.session_models
                .codex_screen_models
                .insert(*pane, (panes[pane].clone(), "gpt-5".into()));
        }

        let mut settings = app.ui_settings.clone();
        settings.agent_tree_model_icons = false;
        app.apply_ui_settings(settings);

        assert!(
            app.session_models.entries.is_empty(),
            "turning model icons off must discard cached model observations for every provider"
        );
        assert!(
            app.session_models.codex_screen_models.is_empty(),
            "turning model icons off must discard cached Codex screen observations"
        );
    }

    #[test]
    fn unavailable_codex_uses_retained_footer_model_and_never_transcript_history() {
        let mut app = crate::app::App::new("test-session".to_string(), "/work".into());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let session_id = "33333333-3333-4333-8333-333333333333";
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::AgentUnavailable(Box::new(ilium_core::AgentRecovery {
                    last_known_state: ilium_core::AgentState::from_activity(
                        AgentClass::Codex,
                        AgentActivity::Idle,
                        None,
                    ),
                    process: ilium_core::AgentProcessKey {
                        class: AgentClass::Codex,
                        process_id: 42,
                        started_at_unix_seconds: 1,
                    },
                    availability: ilium_core::AgentAvailability::Exited(
                        ilium_core::AgentExitOutcome::ExitCode(0),
                    ),
                    signal_name: None,
                    session_id: Some(session_id.into()),
                    last_prompt: None,
                    previous_exact_prompt: None,
                    latest_prompt_unavailable: false,
                })),
            )
            .unwrap();
        app.ui_settings.agent_tree_model_icons = true;

        let footer = "gpt-6-luna high · ~/work";
        let mut view = crate::terminal_view::TerminalView::new(24, 80);
        view.apply_replay(footer.as_bytes(), 1, true);
        app.panes
            .insert(pane, crate::app::PaneRuntime::Terminal(Box::new(view)));

        let contexts = app.live_model_contexts();
        let request = contexts
            .get(&pane)
            .expect("verified retained Codex context")
            .clone();
        let now = Instant::now();
        let screens = app.live_codex_screens(&contexts);
        assert_eq!(
            screens
                .get(&pane)
                .and_then(|(_, screen)| codex_status_line_model(screen)),
            Some("gpt-6-luna".into()),
        );
        assert!(app.session_models.refresh_codex_screen_models(screens, now));
        app.session_models.entries.insert(
            pane,
            Entry {
                request: request.clone(),
                generation: 0,
                model: Some("gpt-6.1-sol".into()),
                model_is_statusline_observation: false,
                diagnostic: None,
                file: None,
                scan: None,
                due: now,
                _storage: Arc::new(
                    crate::execution::process_quota()
                        .reserve_external_storage(2 * WINDOW)
                        .unwrap(),
                ),
            },
        );

        assert_eq!(
            app.current_tree_models().get(&pane).map(String::as_str),
            Some("gpt-6-luna"),
            "the retained terminal footer is the selected model; transcript history is stale"
        );

        app.panes.remove(&pane);
        let no_screen_scan = now + CODEX_SCREEN_REFRESH;
        let screens = app.live_codex_screens(&contexts);
        assert!(screens.is_empty());
        assert!(app
            .session_models
            .refresh_codex_screen_models(screens, no_screen_scan));
        assert!(
            !app.current_tree_models().contains_key(&pane),
            "without a retained terminal footer, stale transcript history must use provider fallback"
        );

        let mut unknown_view = crate::terminal_view::TerminalView::new(24, 80);
        unknown_view.apply_replay("unknown-model high · ~/work".as_bytes(), 2, true);
        app.panes.insert(
            pane,
            crate::app::PaneRuntime::Terminal(Box::new(unknown_view)),
        );
        let next_scan = no_screen_scan + CODEX_SCREEN_REFRESH;
        let screens = app.live_codex_screens(&contexts);
        assert!(app
            .session_models
            .refresh_codex_screen_models(screens, next_scan));
        assert!(
            !app.current_tree_models().contains_key(&pane),
            "an unknown retained footer must use the provider icon instead of a historical model"
        );
    }

    fn write_codex_model_transcript(
        home: &std::path::Path,
        project: &std::path::Path,
        id: &str,
        model: &str,
    ) {
        let directory = home.join(".codex").join("sessions").join("2026/10/07");
        std::fs::create_dir_all(&directory).unwrap();
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": id, "cwd": project}
        });
        let context = serde_json::json!({
            "type": "turn_context",
            "payload": {"model": model}
        });
        let path = directory.join(format!("rollout-2026-10-07T10-00-00-{id}.jsonl"));
        std::fs::write(path, format!("{metadata}\n{context}\n")).unwrap();
    }
    fn window(bytes: &[u8], floor: u64) -> Window {
        Window {
            class: AgentClass::Codex,
            file: FileState {
                path: "/fixture.jsonl".into(),
                stamp: Stamp {
                    length: floor + bytes.len() as u64,
                    modified: None,
                    created: None,
                    #[cfg(unix)]
                    identity: (1, 2),
                },
                complete_end: 0,
            },
            start: floor,
            end: floor + bytes.len() as u64,
            floor,
            top: None,
            reset: floor == 0,
            bytes: bytes.to_vec(),
            _storage: Arc::new(
                crate::execution::process_quota()
                    .reserve_external_storage(2 * WINDOW)
                    .unwrap(),
            ),
        }
    }
    #[test]
    fn latest_completed_context_wins_and_partial_tail_is_revisited() {
        let bytes = b"{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6-sol\"}}\n{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6-astra\"}}\n{\"type\":\"turn_context\"";
        let Parsed::Complete { file, model, .. } =
            parse_window(AgentClass::Codex, window(bytes, 0), || false).unwrap()
        else {
            panic!("complete fixture")
        };
        assert_eq!(model.as_deref(), Some("gpt-6-astra"));
        assert!(file.complete_end < bytes.len() as u64);
        let appended = b"{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6-luna\"}}\n";
        let Parsed::Complete { model, reset, .. } = parse_window(
            AgentClass::Codex,
            window(appended, file.complete_end),
            || false,
        )
        .unwrap() else {
            panic!("complete suffix")
        };
        assert_eq!(model.as_deref(), Some("gpt-6-luna"));
        assert!(!reset);
    }
    #[test]
    fn claude_sidechain_and_synthetic_messages_do_not_replace_main_model() {
        for line in [
            br#"{"type":"assistant","isSidechain":true,"message":{"model":"claude-opus-4"}}"#
                .as_slice(),
            br#"{"type":"assistant","message":{"model":"<synthetic>"}}"#.as_slice(),
            br#"{"type":"user","message":"plain user message"}"#.as_slice(),
        ] {
            assert_eq!(parse_model_record(&AgentClass::Claude, line).unwrap(), None);
        }
        assert_eq!(
            parse_model_record(
                &AgentClass::Claude,
                br#"{"type":"assistant","message":{"model":"claude-sonnet-5"}}"#
            )
            .unwrap()
            .as_deref(),
            Some("claude-sonnet-5")
        );
    }
    #[test]
    fn forty_sessions_retain_small_model_records_without_stats_cache_capacity() {
        let now = Instant::now();
        let mut cache = SessionModelCache::default();
        let contexts: BTreeMap<_, _> = (0..40)
            .map(|id| (NodeId(id), request(&format!("session-{id}"))))
            .collect();
        cache.tick(contexts.clone(), now);
        assert_eq!(cache.entries.len(), 40);
        for entry in cache.entries.values_mut() {
            entry.model = Some("gpt-6-sol".into());
        }
        for (pane, context) in &contexts {
            assert_eq!(cache.model(*pane, context), Some("gpt-6-sol"));
        }
        cache.tick(BTreeMap::new(), now);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn replacing_a_session_on_the_same_pane_requests_a_redraw() {
        let now = Instant::now();
        let mut cache = SessionModelCache::default();
        let original = request("original");
        assert!(!cache.tick(BTreeMap::from([(NodeId(1), original.clone())]), now));
        cache.entries.get_mut(&NodeId(1)).unwrap().model = Some("gpt-6-sol".into());

        let replacement = request("replacement");
        assert!(cache.tick(
            BTreeMap::from([(NodeId(1), replacement.clone())]),
            now + Duration::from_millis(1),
        ));
        assert_eq!(cache.model(NodeId(1), &original), None);
        assert_eq!(cache.model(NodeId(1), &replacement), None);
    }

    #[test]
    fn model_icons_read_mixed_provider_models_with_more_than_sixteen_sessions() {
        let home = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let project_path = ilium_platform::paths::canonicalize(project.path()).unwrap();
        let mut contexts = BTreeMap::new();
        let mut expected_models = BTreeMap::new();
        let antigravity_conversations = home.path().join(".gemini/antigravity-cli/conversations");
        std::fs::create_dir_all(&antigravity_conversations).unwrap();
        for index in 0..21_u64 {
            let id = format!("00000000-0000-4000-8000-{index:012x}");
            let (class, model) = match index % 3 {
                0 => (AgentClass::Codex, "gpt-6.1-sol"),
                1 => (AgentClass::Claude, "claude-opus-4-1"),
                _ => (AgentClass::Antigravity, "Gemini 3.8 Flash (High)"),
            };
            match class {
                AgentClass::Codex => {
                    write_codex_model_transcript(home.path(), &project_path, &id, model);
                }
                AgentClass::Claude => {
                    crate::claude_model_statusline::record_model_observation_for_test(
                        config.path(),
                        home.path(),
                        &id,
                        &project_path,
                        model,
                    )
                    .unwrap();
                }
                AgentClass::Antigravity => {
                    std::fs::write(
                        antigravity_conversations.join(format!("{id}.db")),
                        "opaque SQLite fixture",
                    )
                    .unwrap();
                    use std::io::Write as _;
                    writeln!(
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(home.path().join(".gemini/antigravity-cli/history.jsonl"))
                            .unwrap(),
                        "{}",
                        serde_json::json!({
                            "conversationId": id,
                            "workspace": project_path,
                        })
                    )
                    .unwrap();
                    crate::antigravity_model_statusline::record_observation_for_test(
                        home.path(),
                        &id,
                        &project_path,
                        model,
                    )
                    .unwrap();
                }
                _ => unreachable!("test only creates supported providers"),
            }
            expected_models.insert(NodeId(index), model);
            contexts.insert(
                NodeId(index),
                StatsRequest {
                    class,
                    session_id: id,
                    project_path: project_path.clone(),
                    home: home.path().to_path_buf(),
                },
            );
        }

        let (mut execution, client) = isolated_execution();
        let mut cache = SessionModelCache::default();
        cache.configure_execution(client);
        cache.configure_claude_config_dir_for_test(config.path().to_path_buf());
        let deadline = Instant::now() + Duration::from_secs(20);
        cache.tick(contexts.clone(), Instant::now());
        while contexts
            .keys()
            .any(|pane| cache.model(*pane, &contexts[pane]).is_none())
        {
            if Instant::now() >= deadline {
                let missing = contexts
                    .iter()
                    .filter(|(pane, request)| cache.model(**pane, request).is_none())
                    .map(|(pane, request)| {
                        format!(
                            "pane {} {:?} session {}",
                            pane.0, request.class, request.session_id
                        )
                    })
                    .collect::<Vec<_>>();
                panic!("mixed-provider models were not read: {missing:?}");
            }
            cache.tick(contexts.clone(), Instant::now());
            std::thread::sleep(Duration::from_millis(2));
        }
        for (pane, request) in &contexts {
            assert_eq!(cache.model(*pane, request), Some(expected_models[pane]));
        }

        drop(cache);
        execution.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }

    #[test]
    fn antigravity_statusline_model_requires_a_matching_project_owned_conversation() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let other_project = tempfile::tempdir().unwrap();
        let project_path = ilium_platform::paths::canonicalize(project.path()).unwrap();
        let id = "12345678-abcd-ef01-2345-6789abcdef01";
        let conversation_dir = home.path().join(".gemini/antigravity-cli/conversations");
        std::fs::create_dir_all(&conversation_dir).unwrap();
        std::fs::write(
            conversation_dir.join(format!("{id}.db")),
            "opaque SQLite fixture",
        )
        .unwrap();
        std::fs::write(
            home.path().join(".gemini/antigravity-cli/history.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({
                    "conversationId": id,
                    "workspace": project_path,
                })
            ),
        )
        .unwrap();
        crate::antigravity_model_statusline::record_observation_for_test(
            home.path(),
            id,
            &project_path,
            "Gemini 3.5 Flash (High)",
        )
        .unwrap();

        let request = StatsRequest {
            class: AgentClass::Antigravity,
            session_id: id.into(),
            project_path: project_path.clone(),
            home: home.path().to_path_buf(),
        };
        let (mut execution, client) = isolated_execution();
        let mut cache = SessionModelCache::default();
        cache.configure_execution(client);
        let deadline = Instant::now() + Duration::from_secs(10);
        while cache.model(NodeId(1), &request).is_none() {
            assert!(
                Instant::now() < deadline,
                "Antigravity model was not captured"
            );
            cache.tick(
                BTreeMap::from([(NodeId(1), request.clone())]),
                Instant::now(),
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            cache.model(NodeId(1), &request),
            Some("Gemini 3.5 Flash (High)")
        );

        let wrong_project = StatsRequest {
            project_path: ilium_platform::paths::canonicalize(other_project.path()).unwrap(),
            ..request.clone()
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while cache.diagnostic(NodeId(1), &wrong_project).is_none() {
            assert!(
                Instant::now() < deadline,
                "wrong-project model scan did not finish"
            );
            cache.tick(
                BTreeMap::from([(NodeId(1), wrong_project.clone())]),
                Instant::now(),
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(cache.model(NodeId(1), &wrong_project), None);

        drop(cache);
        execution.request_shutdown(ShutdownMode::Drain);
        assert_eq!(
            execution
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
    }
    #[test]
    fn stale_owner_cannot_apply_after_provider_project_or_session_change() {
        let now = Instant::now();
        let mut cache = SessionModelCache::default();
        let original = request("original");
        cache.tick(BTreeMap::from([(NodeId(1), original.clone())]), now);
        let owner = Owner {
            pane: NodeId(1),
            request: original.clone(),
            generation: cache.entries[&NodeId(1)].generation,
        };
        for replacement in [
            request("replacement"),
            StatsRequest {
                class: AgentClass::Claude,
                ..original.clone()
            },
            StatsRequest {
                project_path: "/elsewhere".into(),
                ..original.clone()
            },
        ] {
            cache.tick(BTreeMap::from([(NodeId(1), replacement.clone())]), now);
            assert!(!cache.live(&owner));
            assert!(!cache.fail(&owner, "stale result".into(), now));
            assert!(cache.diagnostic(NodeId(1), &replacement).is_none());
        }
    }

    #[test]
    fn codex_status_line_reads_only_the_leading_current_model_segment() {
        assert_eq!(
            codex_status_line_model("gpt-6-astra high · ~/project · thread title"),
            Some("gpt-6-astra".to_owned())
        );
        assert_eq!(
            codex_status_line_model("/repo/gpt-6-luna · gpt-6-sol medium"),
            None
        );
    }

    #[test]
    fn codex_status_line_model_is_taken_only_from_the_terminal_footer() {
        assert_eq!(
            codex_status_line_model(
                "gpt-6.1-sol medium · ~/project\nworking on gpt-6-astra now\n> draft"
            ),
            None
        );
        assert_eq!(
            codex_status_line_model("conversation text\n\n  gpt-6-luna high · ~/project"),
            Some("gpt-6-luna".to_owned())
        );
    }

    #[test]
    fn codex_status_line_ignores_unknown_or_ambiguous_models() {
        assert_eq!(codex_status_line_model("gpt-6-nova · ~/project"), None);
        assert_eq!(codex_status_line_model("Sol · ~/project"), None);
        assert_eq!(codex_status_line_model("gpt-6-sol medium"), None);
    }

    #[test]
    fn codex_status_line_does_not_reuse_a_conversation_model_when_the_footer_is_unknown() {
        assert_eq!(
            codex_status_line_model(
                "gpt-6.1-sol medium · ~/project\nold conversation text\nunknown-model · ~/project"
            ),
            None
        );
    }
}
