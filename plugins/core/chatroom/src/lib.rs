//! Core plugin `ilium.core.chatroom` (manual page core-plugin-chatroom.md).
//!
//! The server side owns the room format and the tree row; the client side draws the message view
//! and the composer. Plugin structs stay unit structs (tests build them as literals), so state
//! lives in thread-local cells.
//!
//! Host-dependent work (reading and appending `project:CHATROOM.md`, the `ilium chat` command line,
//! publishing the `chatroom` topic) needs the SDK `host()` calls, which the SDK lane has not
//! implemented yet; see the report of the implementation lane.

use std::cell::RefCell;

use ilium_plugin_sdk::exports::VirtualRowChild;
use ilium_plugin_sdk::types::{
    ActionKind, ActionValue, Activation, Alignment, Area, BlockSpec, BlockTitle, BorderType,
    Borders, Constraint, Direction, ErrorCode, Event, Flex, Intent, LayoutSpec, Margin,
    MessageSpec, MessageTarget, NodeFlags, Padding, ParagraphSpec, PluginError, PluginId,
    PluginSide, RichSpan, ScrollViewSpec, SeparatorSpec, Span, SpanTarget, Spacing, Style,
    Submission, TextInputSpec, TitlePosition, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::{ilium_plugin, Handler, Lifecycle, Views, VirtualRowsProvider};

/// The plugin id, also the target of the composer message.
const PLUGIN_ID: &str = "ilium.core.chatroom";
/// Topic the server publishes new records on.
const TOPIC: &str = "chatroom";
/// Channel the composer posts on.
const POST_CHANNEL: &str = "post";
/// Widget keys of the panel (assumed; see tests/API_GAPS.md).
const COMPOSER_KEY: &str = "composer";
const SCROLL_KEY: &str = "scroll";
/// The view shows the newest messages only.
const VIEW_MESSAGE_LIMIT: usize = 200;
/// Longest message of one page of the record format (chatroom-B02).
pub const MAX_MESSAGE_CHARS: usize = 4000;
/// Length of `YYYY-MM-DD HH:mm:ss +HH:MM`.
const TIMESTAMP_LEN: usize = 25;

/// One record of the room: timestamp, author and the unescaped content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub timestamp: String,
    pub author: String,
    pub content: String,
}

/// Escapes a field for the one-line record format: backslash, newline and tab (chatroom-B01).
pub fn escape_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Reverses [`escape_field`]; an unknown escape keeps its backslash.
pub fn unescape_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// True when `text` is `YYYY-MM-DD HH:mm:ss +HH:MM` (or with a minus sign).
pub fn is_record_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != TIMESTAMP_LEN {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 21, 22, 24];
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b' '),
        (13, b':'),
        (16, b':'),
        (19, b' '),
        (23, b':'),
    ];
    digits.iter().all(|&index| bytes[index].is_ascii_digit())
        && separators
            .iter()
            .all(|&(index, expected)| bytes[index] == expected)
        && (bytes[20] == b'+' || bytes[20] == b'-')
}

/// Formats one record line without the leading `- ` and without a line end.
pub fn format_record(timestamp: &str, author: &str, content: &str) -> String {
    format!(
        "{timestamp} | {} | {}",
        escape_field(author),
        escape_field(content)
    )
}

/// Parses `<timestamp> | <author> | <content>` (the text after the leading `- `).
pub fn parse_record(text: &str) -> Option<Record> {
    let timestamp = text.get(..TIMESTAMP_LEN)?;
    if !is_record_timestamp(timestamp) {
        return None;
    }
    let rest = text.get(TIMESTAMP_LEN..)?.strip_prefix(" | ")?;
    let (author, content) = rest.split_once(" | ")?;
    Some(Record {
        timestamp: timestamp.to_string(),
        author: unescape_field(author),
        content: unescape_field(content),
    })
}

/// Extracts the record lines (`- ` prefix) of a room file, oldest first.
pub fn parse_room(text: &str) -> Vec<Record> {
    text.lines()
        .filter_map(|line| line.strip_prefix("- "))
        .filter_map(parse_record)
        .collect()
}

/// The title of the tree row for `unread` unread messages (chatroom-B16).
pub fn row_title(unread: usize) -> String {
    if unread == 0 {
        "Chatroom".to_string()
    } else {
        format!("Chatroom ({unread} new)")
    }
}

fn plain_rich(text: &str) -> Vec<RichSpan> {
    vec![RichSpan {
        text: text.to_string(),
        style: Style::default(),
        target: SpanTarget::None,
        tooltip: None,
    }]
}

/// The server side of `ilium.core.chatroom` (core-plugin-chatroom.md, world `ilium:server/base@0.1.0`).
#[ilium_plugin(side = "server", exports = ["virtual-rows-provider"])]
#[derive(Default)]
pub struct ChatroomServer;

impl Lifecycle for ChatroomServer {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for ChatroomServer {
    /// Appending to the room file and publishing the topic need the SDK host file calls; until
    /// they exist the server side accepts the events and submits nothing.
    fn on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// The `virtual-rows-provider` export of the server: the unread-messages tree row (core-plugin-chatroom.md "Tree row", reference-wit.md section 8.12).
impl VirtualRowsProvider for ChatroomServer {
    fn list_virtual_children(
        &mut self,
        row_kind: String,
        _parent: String,
        _cursor: Option<String>,
    ) -> Result<(Vec<VirtualRowChild>, Option<String>), PluginError> {
        if row_kind != TOPIC {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                format!("unknown virtual row kind {row_kind}"),
            ));
        }
        let child = VirtualRowChild {
            id: "file".to_string(),
            label: plain_rich("CHATROOM.md"),
            icon: None,
            expandable: false,
            command: None,
            tooltip: None,
            accepts_drop: false,
        };
        Ok((vec![child], None))
    }
}

/// Client state: the messages seen on the topic and the draft of the composer.
#[derive(Default)]
struct ClientState {
    records: Vec<Record>,
    draft: String,
}

thread_local! {
    static CLIENT: RefCell<ClientState> = RefCell::new(ClientState::default());
}

/// Reads the string value of `"text"` out of the flat JSON payload of a topic event.
fn payload_text(payload_json: &str) -> Option<String> {
    let after_key = payload_json.split_once("\"text\"")?.1;
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    let mut characters = after_colon.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(character) = characters.next() {
        match character {
            '"' => return Some(out),
            '\\' => match characters.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = characters.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

/// The client side of `ilium.core.chatroom` (core-plugin-chatroom.md, world `ilium:client/base@0.1.0`).
#[ilium_plugin(side = "client", exports = ["views"])]
#[derive(Default)]
pub struct ChatroomClient;

impl Lifecycle for ChatroomClient {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        CLIENT.with(|cell| *cell.borrow_mut() = ClientState::default());
        Ok(())
    }
}

impl Handler for ChatroomClient {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            match event {
                Event::TopicPublished(published) if published.topic == TOPIC => {
                    let record = payload_text(&published.payload_json)
                        .and_then(|text| parse_record(&text));
                    if let Some(record) = record {
                        CLIENT.with(|cell| cell.borrow_mut().records.push(record));
                    }
                }
                Event::UiAction(action) if action.key == COMPOSER_KEY => {
                    if let Some(submission) = composer_action(action.kind, action.value) {
                        submissions.push(submission);
                    }
                }
                _ => {}
            }
        }
        submissions
    }
}

/// Handles one composer action: `Changed` stores the draft, `Submitted` posts it and empties it.
fn composer_action(kind: ActionKind, value: Option<ActionValue>) -> Option<Submission> {
    let text = match value {
        Some(ActionValue::Text(text)) => text,
        _ => String::new(),
    };
    match kind {
        ActionKind::Changed => {
            CLIENT.with(|cell| cell.borrow_mut().draft = text);
            None
        }
        ActionKind::Submitted => {
            CLIENT.with(|cell| cell.borrow_mut().draft.clear());
            let message: String = text.trim().chars().take(MAX_MESSAGE_CHARS).collect();
            if message.is_empty() {
                return None;
            }
            Some(Submission::new(Intent::Message(MessageSpec {
                to: MessageTarget {
                    plugin: PluginId(PLUGIN_ID.to_string()),
                    side: PluginSide::Server,
                    client: None,
                },
                channel: POST_CHANNEL.to_string(),
                payload: message.into_bytes(),
            })))
        }
        _ => None,
    }
}

impl Views for ChatroomClient {
    fn render(&mut self, _view: ViewTarget, area: Area) -> UiTree {
        CLIENT.with(|cell| build_view(&cell.borrow(), usize::from(area.columns)))
    }
}

fn span(text: &str) -> Span {
    Span {
        text: text.to_string(),
        style: Style::default(),
    }
}

/// Wraps `text` to `width` cells; continuation lines start with two spaces.
fn wrap_message(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let current_len = current.chars().count();
        if current.is_empty() {
            current = word.to_string();
        } else if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = format!("  {word}");
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn message_lines(records: &[Record], width: usize) -> Vec<String> {
    let start = records.len().saturating_sub(VIEW_MESSAGE_LIMIT);
    let mut lines = Vec::new();
    for record in &records[start..] {
        let stamp: String = record.timestamp.chars().take(19).collect();
        let content = record.content.replace('\n', " ");
        let text = format!("{stamp} {} {content}", record.author);
        lines.extend(wrap_message(&text, width));
    }
    lines
}

fn agents_line(records: &[Record]) -> String {
    let mut agents: Vec<&str> = Vec::new();
    for record in records {
        if !agents.contains(&record.author.as_str()) {
            agents.push(&record.author);
        }
    }
    agents.join(" \u{b7} ")
}

fn build_view(state: &ClientState, columns: usize) -> UiTree {
    let inner_width = columns.saturating_sub(4);
    let mut lines: Vec<String> = vec!["Agents in this project".to_string()];
    if !state.records.is_empty() {
        lines.push(agents_line(&state.records));
    }
    if state.records.is_empty() {
        lines.push("No messages yet. Type below to post as user.".to_string());
    } else {
        lines.extend(message_lines(&state.records, inner_width));
    }
    let content_rows = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let paragraph = ParagraphSpec {
        lines: lines.iter().map(|line| vec![span(line)]).collect(),
        rich: None,
        wrap: Some(false),
        alignment: Alignment::Left,
        style: Style::default(),
        scrollable: false,
    };
    let composer = TextInputSpec {
        label: String::new(),
        initial: state.draft.clone(),
        placeholder: Some("Write message - Enter sends".to_string()),
        submit_on_enter: true,
        ..TextInputSpec::default()
    };
    let node = |key: Option<&str>, widget: Widget, children: Vec<u32>, focusable: bool| UiNode {
        key: key.map(str::to_string),
        widget,
        children,
        flags: if focusable {
            NodeFlags::FOCUSABLE
        } else {
            NodeFlags::default()
        },
        tooltip: None,
        tab_index: None,
        disabled_reason: None,
    };
    let block = BlockSpec {
        borders: Borders::ALL,
        border_type: BorderType::Rounded,
        titles: vec![BlockTitle {
            text: vec![span("Chatroom")],
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
    let layout = LayoutSpec {
        direction: Direction::Vertical,
        constraints: vec![
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
        flex: Flex::Legacy,
        spacing: Spacing::Space(0),
        margin: Margin::default(),
    };
    let scroll = ScrollViewSpec {
        content_rows,
        show_scrollbar: true,
        follow_tail: true,
    };
    let separator = SeparatorSpec {
        direction: Direction::Horizontal,
        label: None,
        style: Style::default(),
    };
    UiTree {
        version: 1,
        nodes: vec![
            node(None, Widget::Block(block), vec![1], false),
            node(None, Widget::Layout(layout), vec![2, 4, 5], false),
            node(Some(SCROLL_KEY), Widget::ScrollView(scroll), vec![3], true),
            node(None, Widget::Paragraph(paragraph), vec![], false),
            node(None, Widget::Separator(separator), vec![], false),
            node(Some(COMPOSER_KEY), Widget::TextInput(composer), vec![], true),
        ],
        root: 0,
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// The shipped manifest, read at compile time so the tests check the same bytes the host loads.
    fn shipped_manifest() -> serde_json::Value {
        serde_json::from_str(include_str!("../manifest.json")).expect("manifest.json is valid JSON")
    }

    fn array_contains(values: &serde_json::Value, expected: &str) -> bool {
        values
            .as_array()
            .map(|entries| entries.iter().any(|entry| entry.as_str() == Some(expected)))
            .unwrap_or(false)
    }

    fn extension_point<'a>(
        manifest: &'a serde_json::Value,
        point_id: &str,
    ) -> Option<&'a serde_json::Value> {
        manifest["contributes"]["extension_points"]
            .as_array()?
            .iter()
            .find(|point| point["id"].as_str() == Some(point_id))
    }

    /// REQ: COR-050
    #[test]
    fn activation_is_startup_and_chatroom_topic() {
        let manifest = shipped_manifest();
        assert!(array_contains(&manifest["activation"], "startup"));
        assert!(array_contains(&manifest["activation"], "topic:chatroom"));
    }

    #[test]
    fn manifest_identifies_the_chatroom_plugin() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["api"].as_str(), Some("ilium:plugin@0.1.0"));
        assert_eq!(manifest["id"].as_str(), Some("ilium.core.chatroom"));
        assert_eq!(manifest["version"].as_str(), Some("0.1.0"));
    }

    #[test]
    fn each_side_names_its_entry_and_world() {
        let manifest = shipped_manifest();
        assert_eq!(manifest["server"]["entry"].as_str(), Some("server.wasm"));
        assert_eq!(
            manifest["server"]["world"].as_str(),
            Some("ilium:server/base@0.1.0")
        );
        assert_eq!(manifest["client"]["entry"].as_str(), Some("client.wasm"));
        assert_eq!(
            manifest["client"]["world"].as_str(),
            Some("ilium:client/base@0.1.0")
        );
    }

    /// and the server grants agent-setup services and the project file scope).
    #[test]
    fn grants_cover_project_files_and_agent_setup_without_agent_config() {
        let manifest = shipped_manifest();
        let server_grants = &manifest["capabilities"]["server"];
        assert!(array_contains(server_grants, "fs.write:project"));
        assert!(array_contains(server_grants, "topics.subscribe:chatroom"));
        assert!(array_contains(
            server_grants,
            "services.use:ilium.core.agent-setup"
        ));
        assert!(!array_contains(server_grants, "agent-config"));
        assert!(!array_contains(
            &manifest["capabilities"]["client"],
            "agent-config"
        ));
    }

    /// Compile-time check: the struct for each side implements the trait its manifest export names.
    #[test]
    fn side_structs_implement_their_manifest_exports() {
        fn assert_virtual_rows_provider<T: VirtualRowsProvider>() {}
        fn assert_views<T: Views>() {}
        assert_virtual_rows_provider::<ChatroomServer>();
        assert_views::<ChatroomClient>();
    }

    #[test]
    fn chatroom_row_lists_the_room_file_for_an_empty_parent() {
        let mut server = ChatroomServer;
        let result = server.list_virtual_children("chatroom".to_string(), String::new(), None);
        let (children, cursor) = match result {
            Ok(page) => page,
            Err(_) => panic!("the chatroom row must list its children"),
        };
        assert_eq!(children.len(), 1);
        assert!(cursor.is_none());
    }

    #[test]
    fn unknown_row_kind_is_refused() {
        let mut server = ChatroomServer;
        let result = server.list_virtual_children("not-chatroom".to_string(), String::new(), None);
        assert!(result.is_err());
    }

    /// and 8 handlers; message-renderers is a client collect point with 64 entries). Defaults are the manual's.
    /// REQ: EXT-052
    #[test]
    fn owned_extension_points_declare_their_limits() {
        let manifest = shipped_manifest();

        let before_post =
            extension_point(&manifest, "before-post").expect("before-post is declared");
        assert_eq!(before_post["kind"].as_str(), Some("hook"));
        assert_eq!(before_post["mode"].as_str(), Some("transform"));
        assert_eq!(before_post["deadline_ms"].as_u64(), Some(20));
        assert_eq!(before_post["max_handlers"].as_u64(), Some(8));

        let message_renderers =
            extension_point(&manifest, "message-renderers").expect("message-renderers is declared");
        assert_eq!(message_renderers["kind"].as_str(), Some("collect"));
        assert_eq!(message_renderers["max_entries"].as_u64(), Some(64));
    }

    /// REQ: SEC-293
    #[test]
    fn extension_point_risk_marks_follow_the_security_table() {
        let manifest = shipped_manifest();
        let before_post =
            extension_point(&manifest, "before-post").expect("before-post is declared");
        let message_renderers =
            extension_point(&manifest, "message-renderers").expect("message-renderers is declared");
        assert_eq!(before_post["risk"].as_str(), Some("high"));
        assert_eq!(message_renderers["risk"].as_str(), Some("standard"));
    }
}
