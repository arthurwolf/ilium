//! The live server side: follows `CHATROOM.md`, forwards each new record to the panel and to the
//! agent panes it names, and writes what the user posts in the panel.
//!
//! The room is read with `fs.tail` from a byte cursor held in memory: when the plugin starts, the
//! cursor is the end of the file (records that already exist were routed before), a change of the
//! project directory (`fs-changed`) reads what was appended since. A file that shrank restarts at
//! the top, like a replaced file did for the baseline router.

use std::cell::RefCell;

use ilium_plugin_sdk::host::{HostFs, HostTree};
use ilium_plugin_sdk::types::{
    ClientId, Delivery, InputStage, Intent, MessageEvent, MessageSpec, MessageTarget, NodeQuery,
    PluginId, PluginSide, SendInputSpec, Submission,
};

use crate::room::{
    append_message, initialize, project_path, read_room, recent_record_lines, room_state, RoomState,
};
use crate::router::{notification_lines, referenced_names};
use crate::{MAX_MESSAGE_CHARS, PLUGIN_ID};
use ilium_plugin_sdk::HostApi;

/// Channel of one new record, server to every panel. The payload is the stored record text
/// without its leading `- `.
pub const RECORD_CHANNEL: &str = "record";
/// Channel of the room history, server to the asking panel. The payload is JSON:
/// `{"exists": bool, "lines": [record text, ...]}`.
pub const HISTORY_CHANNEL: &str = "history";
/// Channel on which a panel asks for the history.
pub const HISTORY_REQUEST_CHANNEL: &str = "history-request";
/// Channel on which a panel asks the server to create the room.
pub const INIT_CHANNEL: &str = "init";
/// The author of a message typed in the panel.
const USER_AUTHOR: &str = "user";
/// The panel shows this many newest records when it opens.
pub const HISTORY_RECORDS: usize = 200;
/// Most bytes one tail call reads.
const TAIL_BYTES: u32 = 64 * 1024;
/// Most tail calls of one wake-up, so a very large append cannot hold the plugin.
const MAX_TAILS_PER_WAKE: usize = 16;
/// Most lines of one routed message sent as one input submission (the host allows 16 stages).
const STAGES_PER_SUBMISSION: usize = 16;
/// Pause between the lines of one routed message, in milliseconds.
const STAGE_DELAY_MS: u16 = 30;
/// A line longer than this makes the cursor skip it (the writer never produces one).
const MAX_RECORD_BYTES: usize = 20 * 1024;

#[derive(Default)]
struct ServerState {
    /// The next byte of the room to read.
    cursor: u64,
    /// Whether the cursor was initialised from the file (it is at the end at first sight).
    synced: bool,
}

thread_local! {
    static STATE: RefCell<ServerState> = RefCell::new(ServerState::default());
}

/// Starts following the room: the cursor goes to the end of the current file and the project
/// directory is watched, so a room created later is noticed too.
pub fn start(api: &mut dyn HostApi) {
    STATE.with(|state| *state.borrow_mut() = ServerState::default());
    let _ = HostFs::watch(api, project_path(""), false);
    sync_to_end(api);
}

fn sync_to_end(api: &mut dyn HostApi) {
    if let RoomState::Present { length } = room_state(api) {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.cursor = length;
            state.synced = true;
        });
    }
}

/// Splits the bytes read at `start` into whole lines. Returns the lines (without line end) and
/// how many bytes they consumed; a trailing unterminated line waits for the next read.
pub fn complete_lines(bytes: &[u8]) -> (Vec<String>, usize) {
    let Some(last_newline) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return (Vec::new(), 0);
    };
    let consumed = last_newline + 1;
    let lines = bytes[..last_newline]
        .split(|byte| *byte == b'\n')
        .map(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            String::from_utf8_lossy(line).into_owned()
        })
        .collect();
    (lines, consumed)
}

/// Reads what was appended since the cursor and returns the submissions it causes: one record
/// message to the panels per record, and the input for each agent pane a record names.
pub fn poll(api: &mut dyn HostApi) -> Vec<Submission> {
    let mut submissions = Vec::new();
    let length = match room_state(api) {
        RoomState::Present { length } => length,
        _ => return submissions,
    };
    let (mut cursor, synced) = STATE.with(|state| {
        let state = state.borrow();
        (state.cursor, state.synced)
    });
    if !synced {
        // The room appeared after the plugin started: everything in it is new.
        cursor = 0;
    }
    if length < cursor {
        cursor = 0;
    }
    for _ in 0..MAX_TAILS_PER_WAKE {
        let chunk = match HostFs::tail(
            api,
            project_path(crate::messages::CHATROOM_FILE_NAME),
            cursor,
            TAIL_BYTES,
        ) {
            Ok(chunk) => chunk,
            Err(_) => break,
        };
        if chunk.truncated {
            cursor = 0;
            continue;
        }
        if chunk.bytes.is_empty() {
            break;
        }
        let (lines, consumed) = complete_lines(&chunk.bytes);
        if consumed == 0 {
            if chunk.bytes.len() > MAX_RECORD_BYTES {
                // An unterminated line beyond any valid record: skip what was read.
                cursor += chunk.bytes.len() as u64;
                continue;
            }
            break;
        }
        cursor += consumed as u64;
        for line in lines {
            submissions.extend(handle_line(api, &line));
        }
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.cursor = cursor;
        state.synced = true;
    });
    submissions
}

/// One stored line of the room: forwards a valid record, ignores anything else.
fn handle_line(api: &mut dyn HostApi, line: &str) -> Vec<Submission> {
    if line.len() > MAX_RECORD_BYTES {
        return Vec::new();
    }
    let Some(record_text) = line.strip_prefix("- ") else {
        return Vec::new();
    };
    let Some(record) = crate::router::parse_record(line) else {
        return Vec::new();
    };
    let mut submissions = vec![to_panels(
        RECORD_CHANNEL,
        record_text.as_bytes().to_vec(),
        None,
    )];
    submissions.extend(route_references(api, &record.author, &record.content));
    submissions
}

fn to_panels(channel: &str, payload: Vec<u8>, client: Option<u64>) -> Submission {
    Submission::new(Intent::Message(MessageSpec {
        to: MessageTarget {
            plugin: PluginId(PLUGIN_ID.to_string()),
            side: PluginSide::Client,
            client: client.map(ClientId),
        },
        channel: channel.to_string(),
        payload,
    }))
}

/// Types the message into every agent pane it names with `@name`.
fn route_references(api: &mut dyn HostApi, author: &str, content: &str) -> Vec<Submission> {
    let references = referenced_names(content);
    if references.is_empty() {
        return Vec::new();
    }
    let query = NodeQuery {
        content_kind: Some("ilium.terminal".to_string()),
        ..NodeQuery::default()
    };
    let mut submissions = Vec::new();
    for node in HostTree::find(api, query) {
        let Some(view) = HostTree::get(api, node) else {
            continue;
        };
        let name = view
            .title
            .strip_prefix('@')
            .unwrap_or(&view.title)
            .to_string();
        if !references.iter().any(|reference| *reference == name) {
            continue;
        }
        let message = notification(author, &name, content);
        let lines: Vec<&str> = notification_lines(&message).collect();
        for group in lines.chunks(STAGES_PER_SUBMISSION) {
            let stages = group
                .iter()
                .map(|line| InputStage {
                    text: (*line).to_string(),
                    delay_ms: STAGE_DELAY_MS,
                    delivery: Delivery::Submitted,
                })
                .collect();
            submissions.push(Submission::new(Intent::SendInput(SendInputSpec {
                pane: node,
                text: String::new(),
                delivery: Delivery::Submitted,
                prompt_epoch: None,
                stages,
                raw: None,
                mouse: None,
                if_current: None,
                lock_user_input: false,
                journal: false,
            })));
        }
    }
    submissions
}

/// The text typed into a named agent. The session id the baseline added is not readable from a
/// plugin, so the line says `unknown`.
fn notification(author: &str, called_name: &str, content: &str) -> String {
    format!(
        "new event: the agent {author} with session unknown sent you (called @{called_name}) the following message in the chatroom:\n{content}\n"
    )
}

/// Handles one plugin message from a panel.
pub fn on_message(api: &mut dyn HostApi, message: &MessageEvent) -> Vec<Submission> {
    let client = message.from_client.as_ref().map(|client| client.0);
    match message.channel.as_str() {
        crate::POST_CHANNEL => {
            let text = String::from_utf8_lossy(&message.payload);
            let content: String = text.trim().chars().take(MAX_MESSAGE_CHARS).collect();
            if !content.is_empty() {
                // The watch delivers the new record to the panels; a refused post has nothing to
                // forward and the composer keeps no draft either way.
                let _ = append_message(api, USER_AUTHOR, &content);
            }
            // The directory watch may be coalesced: read right away so the post shows at once.
            poll(api)
        }
        INIT_CHANNEL => {
            let _ = initialize(api);
            sync_to_end(api);
            vec![history(api, client)]
        }
        HISTORY_REQUEST_CHANNEL => vec![history(api, client)],
        _ => Vec::new(),
    }
}

/// The history message for one panel.
fn history(api: &mut dyn HostApi, client: Option<u64>) -> Submission {
    let (exists, lines) = match read_room(api) {
        Ok(Some(text)) => (true, recent_record_lines(&text, HISTORY_RECORDS)),
        _ => (false, Vec::new()),
    };
    let payload = serde_json::json!({ "exists": exists, "lines": lines })
        .to_string()
        .into_bytes();
    to_panels(HISTORY_CHANNEL, payload, client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whole_lines_are_consumed() {
        let (lines, consumed) = complete_lines(b"- a\r\n- b\n- c");
        assert_eq!(lines, vec!["- a".to_string(), "- b".to_string()]);
        assert_eq!(consumed, "- a\r\n- b\n".len());
        assert_eq!(complete_lines(b"no end"), (Vec::new(), 0));
        assert_eq!(complete_lines(b""), (Vec::new(), 0));
    }

    #[test]
    fn the_notification_names_the_author_and_the_called_name() {
        let text = notification("@codex", "compiler", "build\nnow");
        assert!(text.starts_with("new event: the agent @codex with session unknown sent you"));
        assert!(text.contains("(called @compiler)"));
        assert!(text.ends_with("build\nnow\n"));
    }
}
