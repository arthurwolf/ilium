//! The `ilium chat` command line: `init`, `send`, `context` and `tail`.
//!
//! The launcher of the CLI (ilium/src/plugin_cli.rs) resolves the project the room belongs to
//! (the manifest's `project_marker`: the nearest parent holding `CHATROOM.md`, or the main
//! checkout of a linked worktree) and hands the caller's variables over as leading
//! `--ilium-env=NAME=VALUE` words (the manifest's `env`). This module strips them, parses the
//! rest, runs the command against the `project` scope and finishes the invocation.
//!
//! Exit codes follow the launcher: 0 success, 1 failure, 2 usage.

use std::cell::RefCell;
use std::collections::HashMap;

use ilium_plugin_sdk::host::HostCli;
use ilium_plugin_sdk::HostApi;

use crate::messages::{capped_context, tail_messages, unread_context, UnreadReader};
use crate::room::{
    append_message, initialize, read_reader_cursor, read_room, write_reader_cursor, PostFailure,
};
use crate::root::NO_CHATROOM_MESSAGE;

/// The reserved prefix of the leading words that carry the caller's environment variables.
pub const ENV_ARGUMENT_PREFIX: &str = "--ilium-env=";
/// Records `context` prints by default.
const DEFAULT_CONTEXT_LIMIT: usize = 40;
/// Records `tail` prints by default.
const DEFAULT_TAIL_LIMIT: usize = 100;
/// The most bytes of the hook input read from standard input.
const HOOK_INPUT_LIMIT: u32 = 1024 * 1024;
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

/// One parsed `ilium chat` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Init,
    Send {
        message: String,
        author: Option<String>,
    },
    Context {
        limit: usize,
        since_last_read: bool,
        max_bytes: Option<usize>,
        reader: Option<String>,
    },
    Tail {
        limit: usize,
    },
}

/// The words of one invocation: the caller's variables and the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub env: Vec<(String, String)>,
    pub command: Command,
}

impl Parsed {
    /// The caller's value of a variable the manifest forwards; empty values were dropped.
    fn env(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.trim().is_empty())
    }
}

/// Splits the leading environment words off `argv`.
fn split_env(argv: &[String]) -> (Vec<(String, String)>, &[String]) {
    let count = argv
        .iter()
        .take_while(|word| word.starts_with(ENV_ARGUMENT_PREFIX))
        .count();
    let env = argv[..count]
        .iter()
        .filter_map(|word| word.strip_prefix(ENV_ARGUMENT_PREFIX))
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    (env, &argv[count..])
}

/// Reads the value of option `name` (`--name value` or `--name=value`) at `words[*index]`.
fn option_value(words: &[String], index: &mut usize, name: &str) -> Result<Option<String>, String> {
    let word = &words[*index];
    if word == name {
        *index += 1;
        return match words.get(*index) {
            Some(value) => {
                *index += 1;
                Ok(Some(value.clone()))
            }
            None => Err(format!("a value is required for '{name} <VALUE>'")),
        };
    }
    if let Some(value) = word
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('='))
    {
        *index += 1;
        return Ok(Some(value.to_string()));
    }
    Ok(None)
}

fn number(name: &str, text: &str) -> Result<usize, String> {
    text.parse::<usize>()
        .map_err(|_| format!("invalid value '{text}' for '{name} <VALUE>': not a number"))
}

/// Parses the words after `ilium chat`.
pub fn parse(argv: &[String]) -> Result<Parsed, String> {
    let (env, words) = split_env(argv);
    let (name, rest) = words
        .split_first()
        .ok_or_else(|| "a subcommand is required: init, send, context or tail".to_string())?;
    let mut message = None;
    let mut author = None;
    let mut limit = None;
    let mut since_last_read = false;
    let mut max_bytes = None;
    let mut reader = None;
    let mut index = 0;
    while index < rest.len() {
        let word = &rest[index];
        if word == "--since-last-read" && name == "context" {
            since_last_read = true;
            index += 1;
            continue;
        }
        let mut matched = false;
        for option in [
            "--message",
            "--author",
            "--limit",
            "--max-bytes",
            "--reader",
        ] {
            let Some(value) = option_value(rest, &mut index, option)? else {
                continue;
            };
            let allowed = match option {
                "--message" | "--author" => name == "send",
                "--limit" => name == "context" || name == "tail",
                _ => name == "context",
            };
            if !allowed {
                return Err(format!("unexpected argument '{option}' for 'chat {name}'"));
            }
            match option {
                "--message" => message = Some(value),
                "--author" => author = Some(value),
                "--limit" => limit = Some(number(option, &value)?),
                "--max-bytes" => max_bytes = Some(number(option, &value)?),
                _ => reader = Some(value),
            }
            matched = true;
            break;
        }
        if !matched {
            return Err(format!("unexpected argument '{word}' for 'chat {name}'"));
        }
    }
    let command = match name.as_str() {
        "init" => Command::Init,
        "send" => Command::Send {
            message: message.ok_or_else(|| {
                "the following required arguments were not provided: --message <MESSAGE>"
                    .to_string()
            })?,
            author,
        },
        "context" => Command::Context {
            limit: limit.unwrap_or(DEFAULT_CONTEXT_LIMIT),
            since_last_read,
            max_bytes,
            reader,
        },
        "tail" => Command::Tail {
            limit: limit.unwrap_or(DEFAULT_TAIL_LIMIT),
        },
        other => return Err(format!("unrecognized subcommand '{other}'")),
    };
    Ok(Parsed { env, command })
}

/// The author of `send` without `--author`: `ILIUM_CHATROOM_AUTHOR`, then `AGENT_NAME`, then
/// `agent`.
fn default_author(parsed: &Parsed) -> String {
    parsed
        .env("ILIUM_CHATROOM_AUTHOR")
        .or_else(|| parsed.env("AGENT_NAME"))
        .unwrap_or("agent")
        .to_string()
}

/// A `context --since-last-read` waiting for the hook input on standard input.
struct PendingContext {
    invocation: u64,
    parsed: Parsed,
}

thread_local! {
    static PENDING: RefCell<HashMap<u64, PendingContext>> = RefCell::new(HashMap::new());
}

fn finish(api: &mut dyn HostApi, invocation: u64, code: u8, stdout: &str, stderr: &str) {
    if !stdout.is_empty() {
        let _ = HostCli::write_text(api, invocation, stdout.to_string());
    }
    if !stderr.is_empty() {
        let _ = HostCli::write_error(api, invocation, stderr.to_string());
    }
    HostCli::finish(api, invocation, code);
}

fn fail(api: &mut dyn HostApi, invocation: u64, message: &str) {
    finish(
        api,
        invocation,
        EXIT_FAILURE,
        "",
        &format!("ilium chat: {message}\n"),
    );
}

/// Starts one invocation. Everything finishes at once except `context --since-last-read`, which
/// waits for the hook input.
pub fn on_invoked(api: &mut dyn HostApi, invocation: u64, argv: &[String], cwd: &str) {
    let parsed = match parse(argv) {
        Ok(parsed) => parsed,
        Err(message) => {
            finish(
                api,
                invocation,
                EXIT_USAGE,
                "",
                &format!("ilium chat: {message}\n"),
            );
            return;
        }
    };
    match &parsed.command {
        Command::Init => match initialize(api) {
            Ok(()) => finish(
                api,
                invocation,
                0,
                &format!(
                    "chatroom ready at {}/CHATROOM.md\n",
                    cwd.trim_end_matches('/')
                ),
                "",
            ),
            Err(message) => fail(api, invocation, &message),
        },
        Command::Send { message, author } => {
            let author = author.clone().unwrap_or_else(|| default_author(&parsed));
            match append_message(api, &author, message) {
                Ok(()) => finish(api, invocation, 0, "chatroom message sent\n", ""),
                Err(PostFailure::NoRoom) => fail(api, invocation, NO_CHATROOM_MESSAGE),
                Err(other) => fail(api, invocation, &other.to_string()),
            }
        }
        Command::Tail { limit } => match read_room(api) {
            Ok(None) => fail(api, invocation, NO_CHATROOM_MESSAGE),
            Ok(Some(text)) => {
                let mut output = String::new();
                for message in tail_messages(&text, *limit) {
                    output.push_str(&format!(
                        "{} | {} | {}\n",
                        message.timestamp, message.author, message.content
                    ));
                }
                finish(api, invocation, 0, &output, "");
            }
            Err(error) => fail(api, invocation, &error.message),
        },
        Command::Context {
            since_last_read: true,
            ..
        } => match HostCli::read_stdin(api, invocation, HOOK_INPUT_LIMIT) {
            Ok(request) => PENDING.with(|pending| {
                pending.borrow_mut().insert(
                    request,
                    PendingContext {
                        invocation,
                        parsed: parsed.clone(),
                    },
                );
            }),
            // No input channel: the reader is whatever the flags and pane variable name.
            Err(_) => run_context(api, invocation, &parsed, HookInput::default()),
        },
        Command::Context { .. } => run_context(api, invocation, &parsed, HookInput::default()),
    }
}

/// A cancelled invocation stops waiting for its input.
pub fn on_cancelled(invocation: u64) {
    PENDING.with(|pending| {
        pending
            .borrow_mut()
            .retain(|_, waiting| waiting.invocation != invocation);
    });
}

/// Continues the `context` that waited for the hook input; false when `request` is not one.
pub fn on_stdin_ready(api: &mut dyn HostApi, request: u64) -> bool {
    let Some(waiting) = PENDING.with(|pending| pending.borrow_mut().remove(&request)) else {
        return false;
    };
    let bytes = HostCli::take_stdin(api, request).unwrap_or_default();
    run_context(
        api,
        waiting.invocation,
        &waiting.parsed,
        parse_hook_input(&bytes),
    );
    true
}

/// What the lifecycle hook tells on standard input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookInput {
    pub session_id: Option<String>,
    pub is_session_start: bool,
}

/// Reads `session_id` and `hook_event_name` of the JSON a Claude Code or Codex hook writes;
/// anything else reads as no input.
pub fn parse_hook_input(bytes: &[u8]) -> HookInput {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return HookInput::default();
    };
    HookInput {
        session_id: value
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        is_session_start: value
            .get("hook_event_name")
            .and_then(serde_json::Value::as_str)
            == Some("SessionStart"),
    }
}

/// Prints the room context; with `--since-last-read` only the records the reader has not seen,
/// and nothing when there are none. The reader is `--reader`, else the caller's pane
/// (`ILIUM_PANE_ID`), else the hook's session id.
fn run_context(api: &mut dyn HostApi, invocation: u64, parsed: &Parsed, hook: HookInput) {
    let Command::Context {
        limit,
        since_last_read,
        max_bytes,
        reader,
    } = &parsed.command
    else {
        return;
    };
    let text = match read_room(api) {
        // A hook in a directory without a room prints nothing and succeeds.
        Ok(None) => {
            finish(api, invocation, 0, "", "");
            return;
        }
        Ok(Some(text)) => text,
        Err(error) => {
            fail(api, invocation, &error.message);
            return;
        }
    };
    let max_bytes = max_bytes.unwrap_or(usize::MAX);
    let reader_key = reader
        .clone()
        .or_else(|| parsed.env("ILIUM_PANE_ID").map(str::to_string))
        .or(hook.session_id);
    let output = match reader_key.as_deref() {
        Some(key) if *since_last_read => {
            let stored = read_reader_cursor(api, key);
            let outcome = unread_context(
                &text,
                *limit,
                max_bytes,
                &UnreadReader {
                    key,
                    is_session_start: hook.is_session_start,
                },
                stored.as_deref(),
            );
            if let Some(cursor) = &outcome.new_cursor {
                if stored.as_deref() != Some(cursor.as_str()) {
                    write_reader_cursor(api, key, cursor);
                }
            }
            outcome.output
        }
        _ => capped_context(&text, *limit, max_bytes),
    };
    let output = if output.is_empty() {
        output
    } else {
        format!("{output}\n")
    };
    finish(api, invocation, 0, &output, "");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn the_leading_environment_words_are_stripped_and_read() {
        let parsed = parse(&words(&[
            "--ilium-env=AGENT_NAME=codex",
            "--ilium-env=ILIUM_PANE_ID=7",
            "send",
            "--message",
            "hello",
        ]))
        .unwrap();
        assert_eq!(
            parsed.command,
            Command::Send {
                message: "hello".into(),
                author: None
            }
        );
        assert_eq!(parsed.env("AGENT_NAME"), Some("codex"));
        assert_eq!(default_author(&parsed), "codex");
        assert_eq!(parsed.env("ILIUM_CHATROOM_AUTHOR"), None);
    }

    #[test]
    fn the_author_falls_back_in_the_documented_order() {
        let parsed = parse(&words(&[
            "--ilium-env=ILIUM_CHATROOM_AUTHOR=first",
            "--ilium-env=AGENT_NAME=second",
            "tail",
        ]))
        .unwrap();
        assert_eq!(default_author(&parsed), "first");
        let bare = parse(&words(&["tail"])).unwrap();
        assert_eq!(default_author(&bare), "agent");
    }

    #[test]
    fn context_options_parse_with_both_spellings() {
        let parsed = parse(&words(&[
            "context",
            "--limit=40",
            "--since-last-read",
            "--max-bytes",
            "2048",
            "--reader",
            "pane-1",
        ]))
        .unwrap();
        assert_eq!(
            parsed.command,
            Command::Context {
                limit: 40,
                since_last_read: true,
                max_bytes: Some(2048),
                reader: Some("pane-1".into())
            }
        );
    }

    #[test]
    fn usage_errors_name_the_problem() {
        assert!(parse(&words(&[])).is_err());
        assert!(parse(&words(&["send"])).unwrap_err().contains("--message"));
        assert!(parse(&words(&["tail", "--limit", "x"])).is_err());
        assert!(parse(&words(&["init", "--force"])).is_err());
        assert!(parse(&words(&["rm"])).is_err());
    }

    #[test]
    fn the_hook_input_names_the_session_and_its_start() {
        let input = parse_hook_input(br#"{"session_id":"abc","hook_event_name":"SessionStart"}"#);
        assert_eq!(input.session_id.as_deref(), Some("abc"));
        assert!(input.is_session_start);
        assert_eq!(parse_hook_input(b"not json"), HookInput::default());
        assert_eq!(parse_hook_input(b""), HookInput::default());
    }
}
