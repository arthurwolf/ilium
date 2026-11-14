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
//!   one candidate per line, in reading order.
//! * The picker shows three numbered candidates at a time; `1`-`3` or Enter copy one through the
//!   clipboard, `n` and `p` page through the rest, Esc closes and unfreezes the pane.

pub mod light;
pub mod recognise;
pub mod regions;
pub mod tokens;

use std::cell::RefCell;

use ilium_plugin_sdk::generated::host::{
    ChatMessage, CompletionRequest, HostClipboard, HostInference,
};
use ilium_plugin_sdk::generated::types::events::{Event, MessageEvent};
use ilium_plugin_sdk::generated::types::intents::Submission;
use ilium_plugin_sdk::primitives::{HostTerminal, ScreenPart};
use ilium_plugin_sdk::types::{
    Activation, Alignment, Area, BlockSpec, BlockTitle, BorderType, Borders, FreezePaneSpec,
    Intent, MessageSpec, MessageTarget, NodeFlags, NodeId, OpenViewSpec, Padding, ParagraphSpec,
    PluginError, PluginId, PluginSide, RequestRef, RichSpan, SetStatusSegmentSpec, Span,
    SpanTarget, Style, TitlePosition, UiNode, UiTree, UnfreezePaneSpec, ViewTarget, Widget,
};
use ilium_plugin_sdk::{host, ilium_plugin, Handler, Lifecycle, Views};

/// The plugin id, also the target of the messages between the two sides.
pub const PLUGIN_ID: &str = "ilium.core.smart-copy";
/// The command of the manifest: the palette, the right-click menu and the toolbar invoke it.
pub const COMMAND: &str = "smart-copy";
/// Client to server: scan the visible rows of a pane (payload: [`scan_payload`]).
pub const SCAN_CHANNEL: &str = "scan";
/// Server to client: the candidates of one scan, one per line.
pub const CANDIDATES_CHANNEL: &str = "candidates";
/// Most candidates one scan reports.
pub const MAXIMUM_CANDIDATES: usize = 1024;
/// Candidates the picker shows at a time.
pub const PAGE_SIZE: usize = 3;
/// Rows of the pane a scan reads.
const SCREEN_ROWS: u32 = 500;
/// The system message of the ranking request.
const RANKING_PROMPT: &str = "Return the three most useful strings to copy, one per line.";
/// The status bar segment that reports a copy.
const STATUS_SEGMENT: &str = "smart-copy";

/// The payload of a `scan` message: the pane id (little endian), then optionally the first and
/// last row to scan (little endian `u32`). Without rows the whole visible screen is scanned.
pub fn scan_payload(pane: u64, rows: Option<(u32, u32)>) -> Vec<u8> {
    let mut payload = pane.to_le_bytes().to_vec();
    if let Some((first, last)) = rows {
        payload.extend_from_slice(&first.to_le_bytes());
        payload.extend_from_slice(&last.to_le_bytes());
    }
    payload
}

/// Reads a [`scan_payload`]: the pane and the optional row range.
fn read_scan_payload(payload: &[u8]) -> Option<(u64, Option<(u32, u32)>)> {
    let pane = u64::from_le_bytes(payload.get(..8)?.try_into().ok()?);
    let rows = match (payload.get(8..12), payload.get(12..16)) {
        (Some(first), Some(last)) => Some((
            u32::from_le_bytes(first.try_into().ok()?),
            u32::from_le_bytes(last.try_into().ok()?),
        )),
        _ => None,
    };
    Some((pane, rows))
}

/// The candidates of some rows in reading order: single-line texts only, each offered once.
pub fn candidates_of(rows: &[String]) -> Vec<String> {
    let mut regions = regions::detect_regions(rows);
    regions.sort_by_key(|region| {
        region
            .spans
            .first()
            .map_or((u16::MAX, u16::MAX), |span| (span.row, span.start_column))
    });
    let mut seen = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for region in regions {
        let text = region.text.trim().to_string();
        if text.is_empty() || text.contains('\n') || !seen.insert(text.clone()) {
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
    let (pane, range) = read_scan_payload(&message.payload)?;
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
    let candidates = candidates_of(&rows);
    Some(message_to(
        PluginSide::Client,
        message.from_client,
        CANDIDATES_CHANNEL,
        candidates.join("\n").into_bytes(),
    ))
}

/// What the picker shows and what it waits for.
#[derive(Default)]
struct Picker {
    /// The pane frozen for this picker, when the command opened it.
    pane: Option<u64>,
    /// The scan result, in reading order.
    candidates: Vec<String>,
    /// The ranking of the model, at most three lines; replaces `candidates` when it arrives.
    ranked: Vec<String>,
    /// First candidate of the page the picker shows.
    page_start: usize,
    /// The ranking request in flight.
    ranking: Option<RequestRef>,
    /// Whether the popup is open.
    open: bool,
}

impl Picker {
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

/// The server's reading of a screen as text for the ranking request.
fn screen_text(pane: u64) -> String {
    let mut api = host();
    HostTerminal::read_screen(&mut *api, NodeId(pane), ScreenPart::Visible, SCREEN_ROWS)
        .map(|screen| screen.rows.join("\n"))
        .unwrap_or_default()
}

/// Starts the ranking request for a pane and returns what to wait for.
fn start_ranking(pane: u64) -> Option<RequestRef> {
    let request = CompletionRequest {
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: RANKING_PROMPT.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: screen_text(pane),
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
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct SmartCopyClient {
    activated: bool,
}

impl Lifecycle for SmartCopyClient {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        let _ = &context;
        PICKER.with(|cell| *cell.borrow_mut() = Picker::default());
        self.activated = true;
        Ok(())
    }
}

impl Handler for SmartCopyClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::CommandInvoked(command) if command.command == COMMAND => {
                    if let Some(pane) = command.context.pane {
                        submissions.extend(begin(pane.0));
                    }
                }
                Event::Message(message) if message.channel == CANDIDATES_CHANNEL => {
                    submissions.extend(offer(&message.payload));
                }
                Event::RequestCompleted(done) => submissions.extend(rank(&done.request)),
                Event::KeyPressed(key) => submissions.extend(press(&key.chord)),
                _ => {}
            }
        }
        submissions
    }
}

/// The command: freeze the pane, ask the server to scan it and the model to rank it.
fn begin(pane: u64) -> Vec<Submission> {
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        let mut submissions = Vec::new();
        if let Some(previous) = picker.pane.filter(|previous| *previous != pane) {
            submissions.push(Submission::new(Intent::UnfreezePane(UnfreezePaneSpec {
                pane: NodeId(previous),
            })));
        }
        picker.pane = Some(pane);
        picker.ranked.clear();
        picker.page_start = 0;
        picker.ranking = start_ranking(pane);
        submissions.push(Submission::new(Intent::FreezePane(FreezePaneSpec {
            pane: NodeId(pane),
        })));
        submissions.push(message_to(
            PluginSide::Server,
            None,
            SCAN_CHANNEL,
            scan_payload(pane, None),
        ));
        submissions
    })
}

/// The `candidates` reply: remember the scan and open the popup once.
fn offer(payload: &[u8]) -> Vec<Submission> {
    let candidates: Vec<String> = String::from_utf8_lossy(payload)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    PICKER.with(|cell| {
        let mut picker = cell.borrow_mut();
        picker.candidates = candidates;
        picker.page_start = 0;
        open_popup(&mut picker)
    })
}

/// The ranking request finished: keep the first three lines of the reply and show them.
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
        let ranked: Vec<String> = completion
            .map(|completion| {
                completion
                    .text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .take(PAGE_SIZE)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if ranked.is_empty() {
            // A failed or empty ranking leaves the scan as the offer.
            return Vec::new();
        }
        picker.ranked = ranked;
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

/// Copies the candidate at `index` of the page, reports it, then closes the picker.
fn copy(picker: &mut Picker, index: usize) -> Vec<Submission> {
    let Some(text) = picker.page().get(index).cloned() else {
        return Vec::new();
    };
    let written = {
        let mut api = host();
        HostClipboard::write_text(&mut *api, text.clone())
    };
    match written {
        Ok(()) => {
            let report = status(format!("Copied {} characters", text.chars().count()));
            close(picker, vec![report])
        }
        Err(error) => vec![status(format!("Copy failed: {}", error.message))],
    }
}

/// Closes the popup, unfreezes the pane and forgets the scan.
fn close(picker: &mut Picker, mut submissions: Vec<Submission>) -> Vec<Submission> {
    submissions.push(Submission::new(Intent::CloseView(popup())));
    if let Some(pane) = picker.pane.take() {
        submissions.push(Submission::new(Intent::UnfreezePane(UnfreezePaneSpec {
            pane: NodeId(pane),
        })));
    }
    *picker = Picker::default();
    submissions
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
            vec!["command:ilium.core.smart-copy/smart-copy".to_string()]
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
