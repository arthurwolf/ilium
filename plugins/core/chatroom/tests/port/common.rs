#![allow(dead_code)]
//! Shared helpers: hosts for both sides, the room file, the command line, exact templates of the baseline.

use std::path::PathBuf;

use ilium_plugin_core_chatroom::{ChatroomClient, ChatroomServer};
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::events::{MessageEvent, TopicPublishedEvent};
use ilium_plugin_sdk::types::{Event, Intent, PluginId, PluginSide, Submission};

/// Panel view of the client side. The page names no id; the tests assume this one (see API_GAPS.md).
pub const PANEL: &str = "chatroom.panel";
/// The project-scope path of the room.
pub const ROOM: &str = "project:CHATROOM.md";
/// The plugin id.
pub const PLUGIN: &str = "ilium.core.chatroom";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/chatroom).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-chatroom.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The feature traceability page of the manual.
pub fn traceability_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-feature-traceability.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A prompt template of the baseline (`ilium-prompts/templates/agent/<name>.hbs`), byte for byte.
pub fn template(name: &str) -> String {
    let path = workspace_root().join("ilium-prompts/templates/agent").join(format!("{name}.hbs"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("template {path:?}: {error}"))
}

/// Renders a baseline template with the values `v0`, `v1`, `v2` (no escaping, like the baseline).
pub fn render(name: &str, values: &[&str]) -> String {
    let mut text = template(name);
    for (index, value) in values.iter().enumerate() {
        text = text.replace(&format!("{{{{v{index}}}}}"), value);
    }
    text
}

/// The coordination guidance paragraph (`agent/coordination-guidance`).
pub fn guidance() -> String {
    template("coordination-guidance")
}

/// The exact initial file of chatroom-B05.
pub fn initial_room() -> String {
    format!(
        "# ILIUM CHATROOM\n<!-- ilium-chatroom: v1 -->\n\n{}\n\n## Messages\n",
        render("chatroom-header-guidance", &[&guidance()])
    )
}

/// A server-side host with the manifest grants and the plugin activated.
pub fn server_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("server").build();
    host.activate::<ChatroomServer>();
    host
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<ChatroomClient>();
    host
}

/// Timestamp of the n-th test record, in the format of chatroom-B01.
pub fn stamp(n: usize) -> String {
    format!("2026-10-12 {:02}:{:02}:{:02} +02:00", (n / 3600) % 24, (n / 60) % 60, n % 60)
}

/// One record line of chatroom-B01 with the given (already escaped) fields.
pub fn record_line(timestamp: &str, author: &str, content: &str) -> String {
    format!("- {timestamp} | {author} | {content}")
}

/// The n-th test record line: author `agent`, content `record <n>`.
pub fn numbered_line(n: usize) -> String {
    record_line(&stamp(n), "agent", &format!("record {n}"))
}

/// The initial room followed by the given record lines, each ended by a newline.
pub fn room_with(lines: &[String]) -> String {
    let mut text = initial_room();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// Writes the room file with `lines` (the initial room when empty) into the host file system.
pub fn seed_room(host: &mut FakeHost, lines: &[String]) {
    host.fs().write(ROOM, room_with(lines).as_bytes());
    host.settle();
}

/// A server host with a room holding `count` numbered records.
pub fn host_with_records(count: usize) -> FakeHost {
    let mut host = server_host();
    let lines: Vec<String> = (0..count).map(numbered_line).collect();
    seed_room(&mut host, &lines);
    host
}

/// The room file as text.
pub fn room_text(host: &mut FakeHost) -> String {
    String::from_utf8(host.fs().read(ROOM).expect("room file exists")).expect("room is utf-8")
}

/// The record lines (starting with `- `) of the room file.
pub fn room_records(host: &mut FakeHost) -> Vec<String> {
    room_text(host).lines().filter(|line| line.starts_with("- ")).map(str::to_string).collect()
}

/// The result of one `ilium chat` invocation.
#[derive(Debug, Clone)]
pub struct Run {
    pub out: String,
    pub err: String,
    pub code: Option<u8>,
}

/// Runs `ilium chat <args>` with empty stdin.
pub fn chat(host: &mut FakeHost, args: &[&str]) -> Run {
    chat_stdin(host, args, b"")
}

/// Runs `ilium chat <args>` with this stdin (the hook input of an agent).
pub fn chat_stdin(host: &mut FakeHost, args: &[&str], stdin: &[u8]) -> Run {
    let mut full = vec!["chat"];
    full.extend_from_slice(args);
    let invocation = host.cli().invoke(&full, stdin);
    Run { out: invocation.stdout(), err: invocation.stderr(), code: invocation.exit_code() }
}

/// Posts through `ilium chat send --message <text>` and returns the run.
pub fn send(host: &mut FakeHost, text: &str) -> Run {
    chat(host, &["send", "--message", text])
}

/// The state check of a timestamp `YYYY-MM-DD HH:mm:ss +HH:MM` (also with a minus sign).
pub fn is_record_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 25 {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 21, 22, 24];
    let separators = [(4, b'-'), (7, b'-'), (10, b' '), (13, b':'), (16, b':'), (19, b' '), (23, b':')];
    digits.iter().all(|&index| bytes[index].is_ascii_digit())
        && separators.iter().all(|&(index, expected)| bytes[index] == expected)
        && (bytes[20] == b'+' || bytes[20] == b'-')
}

/// Splits a record line `- <timestamp> | <author> | <content>` into its three fields.
pub fn split_record(line: &str) -> (String, String, String) {
    let body = line.strip_prefix("- ").unwrap_or_else(|| panic!("not a record line: {line:?}"));
    let mut fields = body.splitn(3, " | ");
    let timestamp = fields.next().expect("timestamp").to_string();
    let author = fields.next().unwrap_or_else(|| panic!("no author in {line:?}")).to_string();
    let content = fields.next().unwrap_or_else(|| panic!("no content in {line:?}")).to_string();
    (timestamp, author, content)
}

/// The `topic-published` event the server sends to the client for one record line.
pub fn topic_event(line: &str) -> Event {
    Event::TopicPublished(TopicPublishedEvent {
        topic: "chatroom".to_string(),
        from: PluginId(PLUGIN.to_string()),
        payload_json: serde_json::json!({ "text": line }).to_string(),
    })
}

/// The `message` event the composer of the client side sends to the server side.
pub fn composer_message(text: &str) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(PLUGIN.to_string()),
        from_side: PluginSide::Client,
        from_client: None,
        channel: "post".to_string(),
        payload: text.as_bytes().to_vec(),
    })
}

/// The `message` event of another plugin posting to the room.
pub fn peer_message(from: &str, text: &str) -> Event {
    Event::Message(MessageEvent {
        from: PluginId(from.to_string()),
        from_side: PluginSide::Server,
        from_client: None,
        channel: "post".to_string(),
        payload: text.as_bytes().to_vec(),
    })
}

/// Titles of every `set-virtual-rows` submission row of kind `chatroom`, in submission order.
pub fn row_titles(submissions: &[Submission]) -> Vec<String> {
    let mut titles = Vec::new();
    for submission in submissions {
        if let Intent::SetVirtualRows(spec) = &submission.intent {
            if spec.row_kind != "chatroom" {
                continue;
            }
            for row in &spec.rows {
                titles.push(row.label.iter().map(|span| span.text.as_str()).collect::<String>());
            }
        }
    }
    titles
}

/// Topic publications of the plugin on `topic`, as parsed JSON payloads in order.
pub fn published(host: &FakeHost, topic: &str) -> Vec<serde_json::Value> {
    host.published_topics()
        .into_iter()
        .filter(|(name, _)| name == topic)
        .map(|(_, payload)| serde_json::from_str(&payload).expect("payload is JSON"))
        .collect()
}

/// The `text` fields of the `chatroom` topic publications, in order.
pub fn published_texts(host: &FakeHost) -> Vec<String> {
    published(host, "chatroom").iter().map(|value| value["text"].as_str().expect("text field").to_string()).collect()
}

/// The `<pre class="term">` figure that precedes `caption`, as plain screen text.
pub fn page_art(caption: &str) -> String {
    let page = page_text();
    let caption_at = page.find(caption).unwrap_or_else(|| panic!("figcaption {caption:?} missing"));
    let before = &page[..caption_at];
    let open = before.rfind("<pre class=\"term\">").expect("pre") + "<pre class=\"term\">".len();
    let close = before[open..].find("</pre>").expect("pre end");
    let mut out = String::new();
    let mut in_tag = false;
    for character in before[open..open + close].chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => out.push(other),
            _ => {}
        }
    }
    let text = out.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&");
    text.lines().map(|line| line.trim_end().to_string()).collect::<Vec<_>>().join("\n") + "\n"
}
