//! The deterministic ledger: facts computed by code from the tool calls, so
//! they are exact even when a summarizer paraphrases or forgets them.

use serde_json::{json, Value};

use crate::neutral::{Part, Role, Turn};

const MAX_FILES: usize = 150;
const MAX_COMMANDS: usize = 80;
const MAX_ERRORS: usize = 40;
const MAX_URLS: usize = 40;
const MAX_IDS: usize = 40;
const MAX_COMMAND_CHARS: usize = 200;
const MAX_ERROR_CHARS: usize = 240;
const MAX_URL_CHARS: usize = 300;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Ledger {
    pub(crate) files_modified: Vec<String>,
    pub(crate) files_read: Vec<String>,
    pub(crate) commands: Vec<String>,
    pub(crate) errors: Vec<String>,
    pub(crate) urls: Vec<String>,
    pub(crate) ids: Vec<String>,
}

/// Appends `entry`, moving an existing equal entry to the end so the list is
/// ordered by recency, and keeps only the newest `cap` entries.
fn remember(list: &mut Vec<String>, entry: String, cap: usize) {
    if entry.is_empty() {
        return;
    }
    if let Some(position) = list.iter().position(|existing| *existing == entry) {
        list.remove(position);
    }
    list.push(entry);
    if list.len() > cap {
        list.remove(0);
    }
}

pub(crate) fn first_line(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    truncate_chars(line, max_chars)
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{kept}...")
}

fn string_argument<'a>(arguments: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

/// The command line of a shell-like tool call.
fn command_text(arguments: &Value) -> Option<String> {
    let value = arguments.get("command").or_else(|| arguments.get("cmd"))?;
    match value {
        Value::String(command) => Some(command.clone()),
        Value::Array(words) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            // ["bash", "-lc", "script"] runs the script; show the script.
            match words.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => {
                    Some((*script).to_string())
                }
                [] => None,
                _ => Some(words.join(" ")),
            }
        }
        _ => None,
    }
}

fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "bash"
            | "shell"
            | "exec_command"
            | "local_shell"
            | "container.exec"
            | "run_command"
            | "shell_command"
            | "unified_exec"
            | "powershell"
    ) || name.ends_with("__bash")
}

fn patch_files(text: &str) -> Vec<String> {
    const MARKERS: [&str; 4] = [
        "*** Add File: ",
        "*** Update File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    text.lines()
        .filter_map(|line| {
            MARKERS
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
                .map(|path| path.trim().to_string())
        })
        .collect()
}

fn find_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("http") {
        let start = cursor + found;
        cursor = start + 4;
        let rest = &text[start..];
        if !(rest.starts_with("http://") || rest.starts_with("https://")) {
            continue;
        }
        let end = rest
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(
                        character,
                        '"' | '\'' | '<' | '>' | ')' | ']' | '}' | '`' | ','
                    )
            })
            .unwrap_or(rest.len());
        let url = rest[..end].trim_end_matches(['.', ';', ':']);
        if url.len() > "https://".len() {
            urls.push(truncate_chars(url, MAX_URL_CHARS));
        }
        cursor = start + end.max(4);
    }
    urls
}

fn find_ids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let is_run_byte = |byte: u8| byte.is_ascii_hexdigit() || byte == b'-';
    let mut ids = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !is_run_byte(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && is_run_byte(bytes[index]) {
            index += 1;
        }
        let bounded = (start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
            && (index == bytes.len() || !bytes[index].is_ascii_alphanumeric());
        if !bounded {
            continue;
        }
        let run = &text[start..index];
        let is_uuid = run.len() == 36
            && run.bytes().enumerate().all(|(position, byte)| {
                if matches!(position, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            });
        let is_commit = run.len() == 40 && run.bytes().all(|byte| byte.is_ascii_hexdigit());
        if is_uuid || is_commit {
            ids.push(run.to_ascii_lowercase());
        }
    }
    ids
}

/// Exit code stated by a command's output, when it is non-zero.
fn failing_exit_line(text: &str) -> Option<String> {
    const PHRASES: [&str; 4] = [
        "exit code:",
        "exit code ",
        "exited with code ",
        "exit status ",
    ];
    let lowered = text.to_ascii_lowercase();
    for phrase in PHRASES {
        let Some(position) = lowered.find(phrase) else {
            continue;
        };
        let after = lowered[position + phrase.len()..].trim_start();
        let digits: String = after
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if digits.is_empty() || digits.chars().all(|digit| digit == '0') {
            continue;
        }
        let line_start = text[..position]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let line = first_line(&text[line_start..], MAX_ERROR_CHARS);
        let detail = text
            .split_once("Output:")
            .map(|(_, output)| first_line(output, MAX_ERROR_CHARS))
            .filter(|detail| !detail.is_empty());
        return Some(match detail {
            Some(detail) => format!("{line} | {detail}"),
            None => line,
        });
    }
    None
}

impl Ledger {
    pub(crate) fn from_turns(turns: &[Turn]) -> Self {
        Self::build(turns, true)
    }

    /// Ledger of tool activity only, ignoring URLs and ids in message text.
    pub(crate) fn from_tool_activity(turns: &[Turn]) -> Self {
        Self::build(turns, false)
    }

    fn build(turns: &[Turn], include_text: bool) -> Self {
        let mut ledger = Self::default();
        for turn in turns {
            for part in &turn.parts {
                match part {
                    Part::Text(text) => {
                        if include_text && matches!(turn.role, Role::User | Role::Assistant) {
                            ledger.note_text(text, true);
                        }
                    }
                    Part::ToolCall {
                        name, arguments, ..
                    } => ledger.note_call(name, arguments),
                    Part::ToolResult { text, is_error, .. } => ledger.note_result(text, *is_error),
                    Part::Omitted(_) => {}
                }
            }
        }
        ledger
    }

    fn note_text(&mut self, text: &str, with_ids: bool) {
        for url in find_urls(text) {
            remember(&mut self.urls, url, MAX_URLS);
        }
        if with_ids {
            for id in find_ids(text) {
                remember(&mut self.ids, id, MAX_IDS);
            }
        }
    }

    fn note_result(&mut self, text: &str, is_error: bool) {
        let line = if is_error {
            Some(first_line(text, MAX_ERROR_CHARS))
        } else {
            failing_exit_line(text)
        };
        if let Some(line) = line.filter(|line| !line.is_empty()) {
            remember(&mut self.errors, line, MAX_ERRORS);
        }
    }

    fn note_call(&mut self, name: &str, arguments: &Value) {
        let lowered = name.to_ascii_lowercase();
        let raw_text = match arguments {
            Value::String(text) => Some(text.as_str()),
            other => string_argument(other, &["input", "patch"]),
        };
        if lowered.contains("apply_patch") || lowered.contains("applypatch") {
            for path in raw_text.map(patch_files).unwrap_or_default() {
                remember(&mut self.files_modified, path, MAX_FILES);
            }
            return;
        }
        if is_shell_tool(&lowered) {
            let command = command_text(arguments).or_else(|| raw_text.map(str::to_string));
            if let Some(command) = command {
                let mut line = first_line(&command, MAX_COMMAND_CHARS);
                if command.trim().lines().count() > 1 && !line.ends_with("...") {
                    line.push_str(" ...");
                }
                remember(&mut self.commands, line, MAX_COMMANDS);
                self.note_text(&command, false);
            }
            return;
        }
        if let Some(path) = string_argument(arguments, &["file_path", "path", "notebook_path"]) {
            let path = path.to_string();
            if ["grep", "glob", "search", "list", "ls"]
                .iter()
                .any(|word| lowered.contains(word))
            {
                return;
            }
            if ["edit", "write", "create", "replace", "patch", "insert"]
                .iter()
                .any(|word| lowered.contains(word))
            {
                remember(&mut self.files_modified, path, MAX_FILES);
            } else if ["read", "view", "open", "cat"]
                .iter()
                .any(|word| lowered.contains(word))
            {
                remember(&mut self.files_read, path, MAX_FILES);
            }
        }
        if let Value::Object(_) = arguments {
            if let Some(url) = string_argument(arguments, &["url"]) {
                self.note_text(url, false);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.files_modified.is_empty()
            && self.files_read.is_empty()
            && self.commands.is_empty()
            && self.errors.is_empty()
            && self.urls.is_empty()
            && self.ids.is_empty()
    }

    /// The ledger as the plain-text list used inside prompts and appendices.
    pub(crate) fn render(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        ilium_prompts::render_value(
            "compaction/ledger",
            &json!({
                "files_modified": self.files_modified,
                "files_read": self.files_read,
                "commands": self.commands,
                "errors": self.errors,
                "urls": self.urls,
                "ids": self.ids,
            }),
        )
        .trim()
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, arguments: Value) -> Turn {
        Turn::new(
            Role::Assistant,
            vec![Part::ToolCall {
                id: "c1".into(),
                name: name.into(),
                arguments,
            }],
        )
    }

    fn result(text: &str, is_error: bool) -> Turn {
        Turn::new(
            Role::ToolResults,
            vec![Part::ToolResult {
                call_id: "c1".into(),
                text: text.into(),
                is_error,
            }],
        )
    }

    #[test]
    fn claude_and_codex_tool_calls_fill_the_lists() {
        let turns = vec![
            call("Read", json!({"file_path": "/src/a.rs"})),
            call("Edit", json!({"file_path": "/src/b.rs", "old_string": "x", "new_string": "y"})),
            call("Bash", json!({"command": "cargo test\nsecond line"})),
            call("shell", json!({"command": ["bash", "-lc", "ls -la"]})),
            call(
                "apply_patch",
                json!("*** Begin Patch\n*** Update File: src/c.rs\n@@\n-a\n+b\n*** Add File: src/d.rs\n+x\n*** End Patch"),
            ),
            call("Grep", json!({"path": "/src", "pattern": "x"})),
        ];
        let ledger = Ledger::from_turns(&turns);
        assert_eq!(ledger.files_read, ["/src/a.rs"]);
        assert_eq!(ledger.files_modified, ["/src/b.rs", "src/c.rs", "src/d.rs"]);
        assert_eq!(ledger.commands, ["cargo test ...", "ls -la"]);
    }

    #[test]
    fn repeated_entries_move_to_the_end_and_lists_are_capped() {
        let mut list = Vec::new();
        for entry in ["a", "b", "a"] {
            remember(&mut list, entry.to_string(), 10);
        }
        assert_eq!(list, ["b", "a"]);
        for number in 0..20 {
            remember(&mut list, number.to_string(), 5);
        }
        assert_eq!(list, ["15", "16", "17", "18", "19"]);
    }

    #[test]
    fn errors_come_from_flagged_results_and_nonzero_exit_codes() {
        let turns = vec![
            result("error: could not compile `x`\nmore", true),
            result(
                "Exit code: 2\nWall time: 1s\nOutput:\nboom happened\nrest",
                false,
            ),
            result("Exit code: 0\nOutput:\nfine", false),
            result("all good", false),
        ];
        let ledger = Ledger::from_turns(&turns);
        assert_eq!(
            ledger.errors,
            [
                "error: could not compile `x`",
                "Exit code: 2 | boom happened"
            ]
        );
    }

    #[test]
    fn urls_and_ids_are_found_in_prose() {
        let user = Turn::new(
            Role::User,
            vec![Part::Text(
                "see https://example.com/a/b?x=1. and (https://example.org/z) id 123e4567-e89b-12d3-a456-426614174000 sha 0123456789abcdef0123456789abcdef01234567".into(),
            )],
        );
        let ledger = Ledger::from_turns(&[user]);
        assert_eq!(
            ledger.urls,
            ["https://example.com/a/b?x=1", "https://example.org/z"]
        );
        assert_eq!(
            ledger.ids,
            [
                "123e4567-e89b-12d3-a456-426614174000",
                "0123456789abcdef0123456789abcdef01234567"
            ]
        );
    }

    #[test]
    fn rendering_lists_only_non_empty_sections() {
        let ledger = Ledger::from_turns(&[call("Read", json!({"file_path": "/a"}))]);
        assert_eq!(ledger.render(), "Files read:\n- /a");
        assert_eq!(Ledger::default().render(), "");
    }
}
