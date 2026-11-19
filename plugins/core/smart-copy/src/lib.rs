//! Core plugin `ilium.core.smart-copy` (manual page core-plugin-smart-copy.md).
//!
//! Smart Copy finds the useful strings on a terminal screen (paths, commands, hashes, URLs,
//! tables, code blocks, addresses and more) and lets the person copy one with a single key.
//!
//! * The client command `smart-copy` freezes the pane, asks the server side to scan the visible
//!   rows (channel `scan`), asks the inference provider for the three most useful strings, and
//!   opens the picker popup when the `candidates` reply arrives.
//! * The server side reads the rows with `terminal.read-screen`, runs the region detection of
//!   [`regions`] and the recognisers of [`recognise`], and answers with one `candidates` message:
//!   a correlated list of candidates in reading order.
//! * The picker shows three numbered candidates at a time; `1`-`3` or Enter copy one through the
//!   clipboard, `n` and `p` page through the rest, Esc closes and drops the held snapshot.

pub mod light;
mod client_input;
mod model_candidates;
#[cfg(test)]
mod transfer;
pub mod recognise;
pub mod regions;
pub mod tokens;

use std::cell::RefCell;

use ilium_plugin_sdk::generated::host::{
    ChatMessage, CompletionRequest, HostClipboard, HostExtensions, HostInference, HostTopics,
};
use ilium_plugin_sdk::generated::types::events::{Event, MessageEvent};
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::{
    Activation, Alignment, Area, BlockSpec, BlockTitle, BorderType, Borders,
    Intent, MessageSpec, MessageTarget, NodeFlags, NodeId, OpenViewSpec, Padding, ParagraphSpec,
    Outcome, PluginError, PluginId, PluginSide, RequestRef, RichSpan, SetStatusSegmentSpec, Span,
    SpanTarget, Style, TitlePosition, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::generated::exports::menu_provider::{MenuContext, MenuEntry};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, HostApi, Lifecycle, MenuProvider, Views};

/// The plugin id, also the target of the messages between the two sides.
pub const PLUGIN_ID: &str = "ilium.core.smart-copy";
/// The command of the manifest: the palette, the right-click menu and the toolbar invoke it.
pub const COMMAND: &str = "smart-copy";
/// Client to server: scan the visible rows of a pane (payload: [`scan_payload`]).
pub const SCAN_CHANNEL: &str = "scan";
/// Server to client: the correlated candidates of one scan.
pub const CANDIDATES_CHANNEL: &str = "candidates";
/// Most candidates one scan reports.
pub const MAXIMUM_CANDIDATES: usize = 1024;
/// Candidates the picker shows at a time.
pub const PAGE_SIZE: usize = 3;
/// Rows of the pane a scan reads.
const SCREEN_ROWS: u32 = 500;
/// The system message of the ranking request.
const RANKING_PROMPT: &str = include_str!("ranking-prompt.txt");
/// The status bar segment that reports a copy.
/// The hook point that may change the copied text.
const AFTER_COPY: &str = "ilium.core.smart-copy/after-copy";
/// The topic published after every copy.
const COPIED_TOPIC: &str = "ilium.core.smart-copy/copied";
const STATUS_SEGMENT: &str = "smart-copy";

/// Encodes a scan and the client instance and invocation that own its reply.
pub fn scan_payload(pane: u64, rows: Option<(u32, u32)>, instance: u64, sequence: u64) -> Vec<u8> {
    serde_json::json!({"pane": pane, "rows": rows, "instance": instance, "sequence": sequence})
        .to_string().into_bytes()
}

fn read_scan_payload(payload: &[u8]) -> Option<(u64, Option<(u32, u32)>, (u64, u64))> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let pane = value.get("pane")?.as_u64()?;
    let rows = match value.get("rows")? {
        serde_json::Value::Null => None,
        serde_json::Value::Array(rows) if rows.len() == 2 => Some((
            u32::try_from(rows[0].as_u64()?).ok()?,
            u32::try_from(rows[1].as_u64()?).ok()?,
        )),
        _ => return None,
    };
    Some((pane, rows, (value.get("instance")?.as_u64()?, value.get("sequence")?.as_u64()?)))
}

/// The candidates of some rows in reading order, each offered once.
pub fn candidates_of(rows: &[String]) -> Vec<String> {
    candidates_with_recognisers(rows, &[])
}

fn candidates_with_recognisers(rows: &[String], entries: &[String]) -> Vec<String> {
    let joined = rows.join("\n");
    let mut found: Vec<(usize, usize, std::borrow::Cow<'_, str>)> = regions::detect_regions_for_candidates(rows)
        .into_iter()
        .filter_map(|region| {
            let span = region.spans.first()?;
            Some((usize::from(span.row), usize::from(span.start_column), std::borrow::Cow::Owned(region.text)))
        })
        .collect();
    for entry in entries.iter().take(64) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(entry) else { continue };
        let Some(pattern) = value.get("regex").and_then(|value| value.as_str()) else { continue };
        if pattern.len() > 4096 { continue; }
        let Ok(regex) = regex::RegexBuilder::new(pattern).size_limit(1 << 20).build() else { continue };
        // Bound work per recogniser so an empty or prolific one cannot consume another's budget.
        // Borrow matches until the final limit is applied instead of cloning up to 64 scans.
        let mut inspected = 0;
        if value.get("multiline").and_then(|value| value.as_bool()).unwrap_or(false) {
            for matched in regex.find_iter(&joined).take(MAXIMUM_CANDIDATES) {
                let prefix = &joined[..matched.start()];
                let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
                let column = prefix.rsplit('\n').next().unwrap_or("").chars()
                    .map(|character| unicode_width::UnicodeWidthChar::width(character).unwrap_or(0)).sum();
                found.push((row, column, std::borrow::Cow::Borrowed(matched.as_str())));
            }
        } else {
            for (row, text) in rows.iter().enumerate() {
                for matched in regex.find_iter(text).take(MAXIMUM_CANDIDATES - inspected) {
                    let column = text[..matched.start()].chars()
                        .map(|character| unicode_width::UnicodeWidthChar::width(character).unwrap_or(0)).sum();
                    found.push((row, column, std::borrow::Cow::Borrowed(matched.as_str())));
                    inspected += 1;
                }
                if inspected >= MAXIMUM_CANDIDATES { break; }
            }
        }
    }
    found.sort_by_key(|(row, column, _)| (*row, *column));
    let mut seen = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for (_, _, text) in found {
        let text = text.trim().to_string();
        if text.is_empty() || !seen.insert(text.clone()) {
            continue;
        }
        candidates.push(text);
        if candidates.len() >= MAXIMUM_CANDIDATES {
            break;
        }
    }
    candidates
}

fn message_to(
    side: PluginSide,
    client: Option<ilium_plugin_sdk::types::ClientId>,
    channel: &str,
    payload: Vec<u8>,
) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side,
            client,
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// The server side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = [])]
#[derive(Default)]
pub struct SmartCopyServer {
    activated: bool,
}

impl Lifecycle for SmartCopyServer {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        self.activated = true;
        Ok(())
    }
}

impl Handler for SmartCopyServer {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            if let Event::Message(message) = event {
                if message.channel == SCAN_CHANNEL {
                    submissions.extend(scan_reply(&message));
                }
            }
        }
        submissions
    }
}

/// Scans the pane a `scan` message names and answers the sender with the candidates.
fn scan_reply(message: &MessageEvent) -> Option<Submission> {
    if message.from_side != PluginSide::Client || message.from.0 != PLUGIN_ID {
        return None;
    }
    let client = message.from_client?;
    let (pane, range, (instance, sequence)) = read_scan_payload(&message.payload)?;
    let screen = {
        let mut api = host();
        HostTerminal::read_screen(&mut *api, NodeId(pane), ScreenPart::Visible, SCREEN_ROWS)
    }?;
    let rows: Vec<String> = match range {
        Some((first, last)) => screen
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let index = index as u32;
                if index >= first && index <= last {
                    row.clone()
                } else {
                    String::new()
                }
            })
            .collect(),
        None => screen.rows.clone(),
    };
    let entries = HostExtensions::list(&mut *host(), format!("{PLUGIN_ID}/recognisers"))
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.entry_json)
        .collect::<Vec<_>>();
    let candidates = candidates_with_recognisers(&rows, &entries);
    Some(message_to(
        PluginSide::Client,
        Some(client),
        CANDIDATES_CHANNEL,
        serde_json::json!({"instance": instance, "sequence": sequence, "candidates": candidates})
            .to_string().into_bytes(),
    ))
}

/// What the picker shows and what it waits for.
#[derive(Default)]
struct Picker {
    /// The pane this picker was opened for, when the command opened it.
    pane: Option<u64>,
    /// The stable terminal snapshot held while the picker is open; dropped when it closes.
    held_snapshot: Option<u64>,
    /// The invocation whose scan may update this picker.
    scan_request: Option<(u64, u64)>,
    /// The host admission outcome for the current scan request.
    scan_submission: Option<u64>,
    /// The scan result, in reading order.
    candidates: Vec<String>,
    /// Whether the current scan has replied, including an empty result.
    scan_done: bool,
    /// Resolved model suggestions followed by distinct detected candidates.
    ranked: Vec<String>,
    /// Model suggestions, kept separately so a later scan cannot displace built-in candidates.
    model_candidates: Vec<String>,
    /// First candidate of the page the picker shows.
    page_start: usize,
    /// The ranking request in flight.
    ranking: Option<RequestRef>,
    /// The screen content captured for this ranking request.
    snapshot: Option<model_candidates::ScreenSnapshot>,
    /// Whether the popup is open.
    open: bool,
}

impl Picker {
    fn rebuild_ranking(&mut self) {
        self.ranked.clear();
        if self.model_candidates.is_empty() {
            return;
        }
        let model_limit = 2048usize.saturating_sub(self.candidates.len());
        self.ranked.extend(self.model_candidates.iter().take(model_limit).cloned());
        for candidate in &self.candidates {
            if !self.ranked.contains(candidate) {
                self.ranked.push(candidate.clone());
            }
        }
    }

    /// The candidates the picker offers: the model's ranking when there is one, else the scan.
    fn offered(&self) -> &[String] {
        if self.ranked.is_empty() {
            &self.candidates
        } else {
            &self.ranked
        }
    }

    fn page(&self) -> &[String] {
        let offered = self.offered();
        let start = self.page_start.min(offered.len());
        &offered[start..(start + PAGE_SIZE).min(offered.len())]
    }
}

thread_local! {
    static PICKER: RefCell<Picker> = RefCell::new(Picker::default());
}

fn popup() -> ViewTarget {
    ViewTarget::Popup(COMMAND.to_string())
}

fn plain(text: &str) -> RichSpan {
    RichSpan {
        text: text.to_string(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }
}

fn status(text: String) -> Submission {
    Submission::new(Intent::SetStatusSegment(SetStatusSegmentSpec {
        segment: STATUS_SEGMENT.to_string(),
        content: vec![plain(&text)],
        tooltip: None,
        tone: None,
    }))
}

/// Captures the visible rows used to resolve a ranking request.
fn screen_snapshot(pane: u64) -> Option<model_candidates::ScreenSnapshot> {
    let mut api = host();
    HostTerminal::read_screen(&mut *api, NodeId(pane), ScreenPart::Visible, SCREEN_ROWS)
        .map(|screen| model_candidates::ScreenSnapshot::new(screen.rows))
}

/// Starts the ranking request for a pane and returns what to wait for.
fn start_ranking(snapshot: &model_candidates::ScreenSnapshot) -> Option<RequestRef> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: RANKING_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: snapshot.prompt(),
            },
        ],
        max_output_tokens: None,
        json_schema: None,
        purpose: COMMAND.to_string(),
    };
    let mut api = host();
    HostInference::complete(&mut *api, request)
        .ok()
        .map(RequestRef::Call)
}

/// The client side of `ilium.core.smart-copy` (core-plugin-smart-copy.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views", "menu-provider"])]
#[derive(Default)]
pub struct SmartCopyClient {
    activated: bool,
    instance: u64,
    next_scan: u64,
    claims: client_input::Claims,
}

impl Lifecycle for SmartCopyClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.instance = context.instance;
        self.next_scan = 0;
        self.claims = client_input::Claims::default();
        {
            let mut handle = host();
            let api: &mut dyn HostApi = &mut *handle;
            self.claims.adopt_existing(api);
        }
        PICKER.with(|cell| *cell.borrow_mut() = Picker::default());
        self.activated = true;
        Ok(())
    }
}

impl Handler for SmartCopyClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            {
                let mut handle = host();
                let api: &mut dyn HostApi = &mut *handle;
                self.claims.on_event(api, &event);
            }
            match event {
                Event::CommandInvoked(command) if command.command == "copy-path" => {
                    if let Some(pane) = command.context.pane {
                        let mut handle = host();
                        let api: &mut dyn HostApi = &mut *handle;
                        submissions.push(status(client_input::copy_path(api, pane)));
                    }
                }
                Event::CommandInvoked(command) if command.command == "transfer" => {
                    if let Some(pane) = command.context.pane {
                        let mut handle = host();
                        let api: &mut dyn HostApi = &mut *handle;
                        submissions.push(status(client_input::transfer_screen(api, pane)));
                    }
                }
                Event::CommandInvoked(command) if command.command == "copy" => {
                    if let Some(pane) = command.context.pane {
                        submissions.extend(copy_selection(pane));
                    }
                }
                Event::CommandInvoked(command) if command.command == COMMAND => {
                    if let Some(pane) = command.context.pane {
                        if let Some(sequence) = self.next_scan.checked_add(1) {
                            self.next_scan = sequence;
                            submissions.extend(begin(pane.0, (self.instance, sequence)));
                        } else {
                            submissions.push(status("Smart Copy request limit reached".to_owned()));
                        }
                    }
                }
                Event::Message(message)
                    if message.channel == CANDIDATES_CHANNEL
                        && message.from.0 == PLUGIN_ID
                        && message.from_side == PluginSide::Server =>
                {
                    submissions.extend(offer(&message.payload));
                }
                Event::RequestCompleted(done) => {
                    submissions.extend(hook_completed(&done.request));
                    submissions.extend(scan_admission(&done.request, &done.outcome));
                    submissions.extend(rank(&done.request));
                }
                Event::KeyPressed(key) => submissions.extend(press(&key.chord)),
                _ => {}
            }
        }
        submissions
    }
}

/// Copies the selection belonging to the command's target pane.
fn copy_selection(pane: NodeId) -> Vec<Submission> {
    let mut api = host();
    let Some(selection) = HostTerminal::get_selection(&mut *api, pane) else {
        return vec![status("Nothing selected".to_string())];
    };
    if selection.text.is_empty() {
        return vec![status("Nothing selected".to_string())];
    }
    match HostClipboard::write_text(&mut *api, selection.text.clone()) {
        Ok(()) => vec![status(format!("Copied {} characters", selection.text.chars().count()))],
        Err(error) => vec![status(format!("Copy failed: {}", error.message))],
    }
}

/// The command: hold a stable snapshot of the pane, ask the server to scan it and the model to rank it.
fn begin(pane: u64, request: (u64, u64)) -> Vec<Submission> {
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        let mut submissions = Vec::new();
        release_snapshot(&mut picker);
        picker.pane = Some(pane);
        picker.scan_request = Some(request);
        picker.candidates.clear();
        picker.scan_done = false;
        picker.ranked.clear();
        picker.model_candidates.clear();
        picker.page_start = 0;
        picker.snapshot = screen_snapshot(pane);
        picker.ranking = picker.snapshot.as_ref().and_then(start_ranking);
        picker.held_snapshot = {
            let mut api = host();
            HostTerminal::freeze_snapshot(&mut *api, NodeId(pane)).ok()
        };
        let scan = message_to(
            PluginSide::Server,
            None,
            SCAN_CHANNEL,
            scan_payload(pane, None, request.0, request.1),
        );
        picker.scan_submission = Some(scan.correlation);
        submissions.push(scan);
        submissions
    })
}

fn scan_admission(request: &RequestRef, outcome: &Outcome) -> Vec<Submission> {
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        if picker.scan_submission.map(RequestRef::Submission).as_ref() != Some(request) {
            return Vec::new();
        }
        picker.scan_submission = None;
        if outcome.is_ok() || picker.scan_done {
            return Vec::new();
        }
        close(&mut picker, vec![status("Smart Copy scan failed; please retry".to_owned())])
    })
}

/// The `candidates` reply: remember the scan and open the popup once.
fn offer(payload: &[u8]) -> Vec<Submission> {
    let Some((request, candidates)) = (|| {
        let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
        let request = (value.get("instance")?.as_u64()?, value.get("sequence")?.as_u64()?);
        let values = value.get("candidates")?.as_array()?;
        if values.len() > MAXIMUM_CANDIDATES { return None; }
        let candidates = values.iter().map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        Some((request, candidates))
    })() else { return Vec::new(); };
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        if picker.scan_request != Some(request) || picker.scan_done {
            return Vec::new();
        }
        picker.candidates = candidates;
        picker.rebuild_ranking();
        picker.scan_done = true;
        if picker.offered().is_empty() {
            if picker.pane.is_some() && picker.ranking.is_none() {
                return close(&mut picker, vec![status("No copy candidates found".to_owned())]);
            }
            return Vec::new();
        }
        picker.page_start = 0;
        open_popup(&mut picker)
    })
}

/// Resolve model records against the captured rows and retain the detected candidates.
fn rank(request: &RequestRef) -> Vec<Submission> {
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        if picker.ranking.as_ref() != Some(request) {
            return Vec::new();
        }
        picker.ranking = None;
        let RequestRef::Call(id) = request else {
            return Vec::new();
        };
        let completion = {
            let mut api = host();
            HostInference::take_completion(&mut *api, *id)
        };
        let ranked = completion.and_then(|completion| picker.snapshot.as_ref()
            .map(|snapshot| snapshot.resolve(&completion.text))).unwrap_or_default();
        if ranked.is_empty() {
            // A failed or empty ranking leaves the scan as the offer.
            if picker.scan_done && picker.offered().is_empty() && picker.pane.is_some() {
                return close(&mut picker, vec![status("No copy candidates found".to_owned())]);
            }
            return Vec::new();
        }
        picker.model_candidates = ranked;
        picker.rebuild_ranking();
        picker.page_start = 0;
        open_popup(&mut picker)
    })
}

/// Opens the popup unless it is open already.
fn open_popup(picker: &mut Picker) -> Vec<Submission> {
    if picker.open {
        return Vec::new();
    }
    picker.open = true;
    vec![Submission::new(Intent::OpenView(OpenViewSpec {
        view: popup(),
        focus: true,
    }))]
}

/// A key while the picker is open.
fn press(chord: &str) -> Vec<Submission> {
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        if !picker.open {
            return Vec::new();
        }
        match chord {
            "esc" | "escape" => close(&mut picker, Vec::new()),
            "enter" | "return" => copy(&mut picker, 0),
            "n" | "down" | "right" | "tab" | "pagedown" => {
                let next = picker.page_start + PAGE_SIZE;
                if next < picker.offered().len() {
                    picker.page_start = next;
                }
                Vec::new()
            }
            "p" | "up" | "left" | "backtab" | "pageup" => {
                picker.page_start = picker.page_start.saturating_sub(PAGE_SIZE);
                Vec::new()
            }
            digit => match digit.parse::<usize>() {
                Ok(number) if number >= 1 => copy(&mut picker, number - 1),
                _ => Vec::new(),
            },
        }
    })
}

/// A copy whose `after-copy` hook chain has not answered yet.
struct PendingCopy {
    request: RequestRef,
    text: String,
    pane: u64,
}

thread_local! {
    static PENDING_COPY: RefCell<Option<PendingCopy>> = const { RefCell::new(None) };
}

/// The kind a candidate is reported with to the hooks and on the `copied` topic.
const COPY_KIND: &str = "candidate";

/// Copies the candidate at `index` of the page: the `after-copy` chain may change the text first.
fn copy(picker: &mut Picker, index: usize) -> Vec<Submission> {
    if PENDING_COPY.with(|cell| cell.borrow().is_some()) {
        return Vec::new();
    }
    let Some(text) = picker.page().get(index).cloned() else {
        return Vec::new();
    };
    let pane = picker.pane.unwrap_or(0);
    let payload = serde_json::json!({ "text": text, "kind": COPY_KIND, "pane": pane.to_string() });
    let started = {
        let mut api = host();
        let hooked = HostExtensions::list(&mut *api, AFTER_COPY.to_owned())
            .map(|entries| !entries.is_empty())
            .unwrap_or(false);
        hooked.then(|| HostExtensions::run_hook(&mut *api, AFTER_COPY.to_owned(), payload.to_string()))
    };
    match started {
        Some(Ok(id)) => {
            PENDING_COPY.with(|cell| {
                *cell.borrow_mut() = Some(PendingCopy { request: RequestRef::Call(id), text, pane });
            });
            Vec::new()
        }
        Some(Err(_)) | None => finish_copy(picker, text),
    }
}

/// The hook chain answered: a reply replaces the text; no reply (pass or refusal) keeps it.
fn hook_completed(request: &RequestRef) -> Vec<Submission> {
    let Some(pending) = PENDING_COPY.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_some_and(|pending| &pending.request == request) {
            slot.take()
        } else {
            None
        }
    }) else {
        return Vec::new();
    };
    let RequestRef::Call(id) = &pending.request else {
        return Vec::new();
    };
    let reply = {
        let mut api = host();
        HostExtensions::take_reply(&mut *api, *id)
    };
    let text = reply.unwrap_or(pending.text);
    PICKER.with(|cell| finish_copy(&mut cell.borrow_mut(), text))
}

/// Writes the final text, publishes `copied` and closes the picker; a failed write keeps it open.
fn finish_copy(picker: &mut Picker, text: String) -> Vec<Submission> {
    let characters = text.chars().count();
    let written = {
        let mut api = host();
        HostClipboard::write_text(&mut *api, text)
    };
    match written {
        Ok(()) => {
            {
                let mut api = host();
                let payload = serde_json::json!({ "kind": COPY_KIND, "length": characters });
                let _ = HostTopics::publish(&mut *api, COPIED_TOPIC.to_owned(), payload.to_string());
            }
            let report = status(format!("Copied {characters} characters"));
            close(picker, vec![report])
        }
        Err(error) => vec![status(format!("Copy failed: {}", error.message))],
    }
}

/// Drops the held terminal snapshot, if any.
fn release_snapshot(picker: &mut Picker) {
    if let Some(handle) = picker.held_snapshot.take() {
        let mut api = host();
        HostTerminal::drop_snapshot(&mut *api, handle);
    }
}

/// Closes the popup, drops the held snapshot and forgets the scan.
fn close(picker: &mut Picker, mut submissions: Vec<Submission>) -> Vec<Submission> {
    submissions.push(Submission::new(Intent::CloseView(popup())));
    picker.pane = None;
    release_snapshot(picker);
    *picker = Picker::default();
    submissions
}

impl MenuProvider for SmartCopyClient {
    fn menu_items(&mut self, context: MenuContext) -> Vec<MenuEntry> {
        let mut handle = host();
        let api: &mut dyn HostApi = &mut *handle;
        client_input::menu_items(api, context.pane)
    }
}

impl Views for SmartCopyClient {
    fn render(&mut self, view: ViewTarget, area: Area) -> UiTree {
        let _ = area;
        PICKER.with(|cell| {
            let picker = cell.borrow();
            if view != popup() || picker.offered().is_empty() {
                return empty_tree();
            }
            picker_tree(&picker)
        })
    }
}

fn node(key: Option<&str>, widget: Widget, children: Vec<u32>) -> UiNode {
    UiNode {
        key: key.map(str::to_string),
        widget,
        children,
        flags: NodeFlags::default(),
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    }
}

fn empty_tree() -> UiTree {
    UiTree {
        version: 1,
        nodes: vec![node(
            None,
            Widget::Paragraph(ParagraphSpec {
                lines: Vec::new(),
                rich: None,
                wrap: Some(false),
                alignment: Alignment::Left,
                style: Style::default(),
                scrollable: false,
            }),
            Vec::new(),
        )],
        root: 0,
    }
}

fn picker_tree(picker: &Picker) -> UiTree {
    let numbers = Style {
        add: ilium_plugin_sdk::types::Modifiers::BOLD,
        ..Style::default()
    };
    let mut lines: Vec<Vec<Span>> = picker
        .page()
        .iter()
        .enumerate()
        .map(|(index, text)| {
            vec![
                Span {
                    text: (index + 1).to_string(),
                    style: numbers.clone(),
                },
                Span {
                    text: format!(" {text}"),
                    style: Style::default(),
                },
            ]
        })
        .collect();
    let offered = picker.offered().len();
    let paged = offered > PAGE_SIZE;
    lines.push(Vec::new());
    let hint = if paged {
        format!(
            "1-3 copy   Enter copy   n/p page {}/{}   Esc close",
            picker.page_start / PAGE_SIZE + 1,
            offered.div_ceil(PAGE_SIZE)
        )
    } else {
        "1-3 copy   Enter copy   Esc close".to_string()
    };
    lines.push(vec![Span {
        text: hint,
        style: Style::default(),
    }]);
    let paragraph = ParagraphSpec {
        lines,
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    };
    let block = BlockSpec {
        borders: Borders::ALL,
        border_type: BorderType::Rounded,
        titles: vec![BlockTitle {
            text: vec![Span {
                text: "Smart Copy".to_string(),
                style: Style::default(),
            }],
            position: TitlePosition::Top,
            alignment: Alignment::Left,
        }],
        padding: Padding {
            left: 1,
            right: 1,
            top: 0,
            bottom: 0,
        },
        style: Style::default(),
        border_style: Style::default(),
    };
    UiTree {
        version: 1,
        nodes: vec![
            node(None, Widget::Block(block), vec![1]),
            node(Some("candidates"), Widget::Paragraph(paragraph), Vec::new()),
        ],
        root: 0,
    }
}

#[cfg(test)]
mod picker_capacity_tests {
    use super::Picker;

    #[test]
    fn late_detected_candidates_fit_within_the_full_candidate_budget() {
        let mut picker = Picker::default();
        picker.model_candidates = (0..2048).map(|index| format!("model-{index}")).collect();
        picker.rebuild_ranking();
        assert_eq!(picker.offered().len(), 2048);
        picker.candidates = (0..1024).map(|index| format!("detected-{index}")).collect();
        picker.rebuild_ranking();
        assert_eq!(picker.offered().len(), 2048);
        assert!(picker.candidates.iter().all(|candidate| picker.offered().contains(candidate)));
    }
}

#[cfg(test)]
mod spec_tests {
    use serde_json::Value;

    const MANIFEST_TEXT: &str = include_str!("../manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST_TEXT).expect("manifest.json is valid JSON")
    }

    fn string_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("value is an array")
            .iter()
            .map(|item| item.as_str().expect("item is a string").to_string())
            .collect()
    }

    fn extension_point(manifest: &Value, point_id: &str) -> Value {
        manifest["contributes"]["extension_points"]
            .as_array()
            .expect("extension_points is an array")
            .iter()
            .find(|point| point["id"] == point_id)
            .cloned()
            .expect("extension point is declared")
    }

    /// REQ: COR-050
    #[test]
    fn manifest_identifies_smart_copy_and_both_worlds() {
        let manifest = manifest();
        assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
        assert_eq!(manifest["id"], "ilium.core.smart-copy");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
        assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_the_smart_copy_command_and_the_terminal_menu_runs_it() {
        let manifest = manifest();
        assert_eq!(
            string_list(&manifest["activation"]),
            vec![
                "command:ilium.core.smart-copy/smart-copy".to_string(),
                "command:ilium.core.smart-copy/copy".to_string(),
                "command:ilium.core.smart-copy/copy-path".to_string(),
                "command:ilium.core.smart-copy/transfer".to_string(),
            ]
        );
        let entries = manifest["contributes"]["context_menu"]
            .as_array()
            .expect("context_menu is an array");
        let smart_copy = entries
            .iter()
            .find(|entry| entry["id"] == "smart-copy")
            .expect("smart-copy menu entry exists");
        assert_eq!(smart_copy["command"], "smart-copy");
        assert_eq!(smart_copy["target"], "terminal");
    }

    /// REQ: COR-050
    #[test]
    fn server_capabilities_include_the_read_grants_and_both_extension_grants() {
        let manifest = manifest();
        let server = string_list(&manifest["capabilities"]["server"]);
        for expected in [
            "terminal.read-cells",
            "terminal.read-screen",
            "inference",
            "extensions.provide:ilium.core.smart-copy/recognisers",
            "extensions.provide:ilium.core.smart-copy/after-copy",
        ] {
            assert!(
                server.iter().any(|grant| grant == expected),
                "missing server grant {expected}"
            );
        }
    }

    /// REQ: EXT-052
    #[test]
    fn recognisers_point_collects_up_to_64_entries_with_the_five_required_fields() {
        let point = extension_point(&manifest(), "recognisers");
        assert_eq!(point["kind"], "collect");
        assert_eq!(point["max_entries"], 64);
        assert_eq!(point["version"], "0.1.0");
        assert_eq!(
            string_list(&point["entry_schema"]["required"]),
            vec!["id", "kind", "regex", "multiline", "label"]
        );
    }

    /// REQ: EXT-052
    #[test]
    fn after_copy_point_is_a_transform_hook_with_a_20_ms_deadline_and_8_handlers() {
        let point = extension_point(&manifest(), "after-copy");
        assert_eq!(point["kind"], "hook");
        assert_eq!(point["mode"], "transform");
        assert_eq!(point["deadline_ms"], 20);
        assert_eq!(point["max_handlers"], 8);
    }

    /// REQ: SEC-293
    #[test]
    fn after_copy_is_marked_high_risk() {
        let point = extension_point(&manifest(), "after-copy");
        assert_eq!(point["risk"], "high");
    }

    /// REQ: EXT-052
    #[test]
    fn recogniser_entry_without_a_regex_is_invalid_under_its_schema() {
        let point = extension_point(&manifest(), "recognisers");
        let required = string_list(&point["entry_schema"]["required"]);
        let entry: Value = serde_json::json!({
            "id": "path",
            "kind": "path",
            "multiline": false,
            "label": "Path"
        });
        let missing: Vec<&String> = required
            .iter()
            .filter(|field| entry.get(field.as_str()).is_none())
            .collect();
        assert_eq!(missing, vec!["regex"]);
    }

    /// REQ: EXT-052
    #[test]
    fn after_copy_reply_without_text_is_rejected_under_its_schema() {
        let point = extension_point(&manifest(), "after-copy");
        let required = string_list(&point["reply_schema"]["required"]);
        assert_eq!(required, vec!["text"]);
        let empty_reply: Value = serde_json::json!({});
        assert!(empty_reply.get("text").is_none());
    }

    /// REQ: COR-050
    #[test]
    fn plugin_is_replaceable_and_its_surface_lists_the_copied_topic() {
        let manifest = manifest();
        assert_eq!(manifest["replaceable"], true);
        let surface = string_list(&manifest["surface"]);
        assert!(surface.iter().any(|entry| entry == "topics:copied"));
        assert!(surface.iter().any(|entry| entry == "commands:smart-copy"));
    }
}
