//! Project-local agent chatroom storage and integration setup.
//!
//! `CHATROOM.md` is the durable, human-readable source of truth. This module
//! is shared by the TUI and the `ilium chat` CLI so every writer takes the
//! same advisory lock, emits the same one-line record format, and repairs the
//! provider integrations idempotently.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::Local;
use serde_json::{json, Map, Value};

pub const CHATROOM_FILE_NAME: &str = "CHATROOM.md";
const CHATROOM_MARKER: &str = "<!-- ilium-chatroom: v1 -->";
/// Lifecycle hooks print only messages the reader has not seen yet, capped in
/// bytes, because hook output is re-sent with every later model request.
const HOOK_COMMAND: &str = "ilium chat context --limit 40 --since-last-read --max-bytes 2048";
/// Any earlier spelling of the chatroom hook is recognized and upgraded in
/// place, so a changed command never leaves a second registration behind.
const HOOK_COMMAND_PREFIX: &str = "ilium chat context";
/// Archive written when an oversized room is rotated (global working-file rule).
const CHATROOM_ARCHIVE_IGNORE: &str = "/CHATROOM.archive.md";
/// Unread tracking scans at most this many recent records for the reader's cursor.
const UNREAD_SCAN_RECORDS: usize = 2_000;
/// A session start or compaction gets this many times the per-turn byte cap,
/// because the agent has no earlier chatroom context at that point.
const SESSION_START_BYTE_FACTOR: usize = 4;

const COORDINATION_POSTING_GUIDANCE: &str = ilium_prompts::agent::COORDINATION_GUIDANCE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub timestamp: String,
    pub author: String,
    pub content: String,
}

/// Returns the exact chatroom path for a canonical ilium project root.
pub fn path_for_project(project_root: &Path) -> PathBuf {
    project_root.join(CHATROOM_FILE_NAME)
}

/// True only for a regular project-root chatroom file. Symlinks are rejected
/// so a project cannot accidentally make ilium read or append outside itself.
pub fn exists(project_root: &Path) -> bool {
    let path = path_for_project(project_root);
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

/// Creates the room and runtime hooks needed by participating Claude/Codex
/// agents. Agent instruction files are managed independently by
/// `agent_feature_setup`, so disabling guidance in Settings cannot be undone
/// by this integration repair path.
pub fn initialize(project_root: &Path) -> anyhow::Result<()> {
    if !project_root.is_dir() {
        anyhow::bail!("project root {} is unavailable", project_root.display());
    }
    let chatroom_path = path_for_project(project_root);

    with_project_lock(project_root, || {
        match fs::symlink_metadata(&chatroom_path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(metadata) if metadata.file_type().is_symlink() => anyhow::bail!(
                "refusing to use symlinked chatroom {}",
                chatroom_path.display()
            ),
            Ok(_) => anyhow::bail!(
                "chatroom path {} exists but is not a regular file",
                chatroom_path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                write_new_chatroom(&chatroom_path)?;
            }
            Err(error) => return Err(error.into()),
        }
        ensure_gitignore(project_root)?;
        ensure_codex_hooks(project_root)?;
        ensure_claude_hooks(project_root)?;
        crate::progress_guard::ensure_installed(project_root)?;
        Ok(())
    })
}

/// Repairs integrations for an existing room. It deliberately does nothing
/// when no room exists, so ilium startup never creates project files merely by
/// inspecting a project.
pub fn ensure_integrations(project_root: &Path) -> anyhow::Result<bool> {
    if !exists(project_root) {
        return Ok(false);
    }
    with_project_lock(project_root, || {
        ensure_gitignore(project_root)?;
        ensure_codex_hooks(project_root)?;
        ensure_claude_hooks(project_root)?;
        crate::progress_guard::ensure_installed(project_root)?;
        Ok(true)
    })
}

/// Appends a single escaped Markdown record. The advisory lock covers the
/// complete append, which avoids interleaving when several agents respond at
/// once through `ilium chat send` or the TUI composer.
pub fn append_message(project_root: &Path, author: &str, content: &str) -> anyhow::Result<()> {
    if !exists(project_root) {
        anyhow::bail!("this project does not have a chatroom");
    }
    let author = sanitize_field(author);
    let content = sanitize_field(content);
    if author.is_empty() || content.is_empty() {
        anyhow::bail!("chatroom author and message must not be empty");
    }
    if content.chars().count() > 4_000 {
        anyhow::bail!("chatroom messages are limited to 4000 characters");
    }
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S %:z").to_string();
    with_project_lock(project_root, || {
        let path = path_for_project(project_root);
        // CHATROOM.md is human-editable (see module doc), so a prior manual
        // edit may have left the file without a trailing newline. Appending
        // straight onto that would silently splice our new record onto the
        // end of the human's last line, corrupting both.
        let mut existing = ilium_platform::secure_fs::open_regular_file(&path)?;
        let length = existing.metadata()?.len();
        let needs_leading_newline = if length == 0 {
            false
        } else {
            existing.seek(SeekFrom::End(-1))?;
            let mut last = [0_u8; 1];
            existing.read_exact(&mut last)?;
            last[0] != b'\n'
        };
        let mut file = OpenOptions::new().append(true).open(&path)?;
        if needs_leading_newline {
            writeln!(file)?;
        }
        writeln!(
            file,
            "{}",
            ilium_prompts::render_value(
                "agent/chatroom-record-row",
                &serde_json::json!({"v0": (timestamp).to_string(), "v1": (author).to_string(), "v2": (content).to_string()})
            )
        )?;
        file.sync_data()?;
        Ok(())
    })
}

/// Reads the tail of valid chat records. Human edits outside the record shape
/// remain visible in the file but are safely ignored by the structured UI.
pub fn read_messages(project_root: &Path, limit: usize) -> anyhow::Result<Vec<ChatMessage>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if !exists(project_root) {
        return Ok(Vec::new());
    }
    let mut contents = String::new();
    File::open(path_for_project(project_root))?.read_to_string(&mut contents)?;
    let mut messages = contents
        .lines()
        .filter_map(parse_message_line)
        .collect::<Vec<_>>();
    let keep_from = messages.len().saturating_sub(limit);
    Ok(messages.split_off(keep_from))
}

/// Produces context suitable for a provider lifecycle hook. Keep this plain
/// text: hook output becomes model context and must never carry terminal
/// controls or be mistaken for executable instructions.
pub fn context(project_root: &Path, limit: usize) -> anyhow::Result<String> {
    let messages = read_messages(project_root, limit)?;
    let mut output = full_context_header(&messages);
    for message in &messages {
        output.push_str(&render_context_row(message));
    }
    Ok(output)
}

/// Hook context capped at `max_bytes`, without unread tracking.
pub fn capped_context(
    project_root: &Path,
    limit: usize,
    max_bytes: usize,
) -> anyhow::Result<String> {
    let messages = read_messages(project_root, limit)?;
    Ok(render_capped(
        full_context_header(&messages),
        &messages,
        limit,
        max_bytes,
    ))
}

/// Identifies the agent reading the room through a lifecycle hook.
pub struct UnreadReader<'a> {
    /// Stable reader identity, such as an ilium pane id or an agent session id.
    pub key: &'a str,
    /// A session start or compaction: the agent holds no earlier chatroom context.
    pub is_session_start: bool,
}

/// Hook context holding only records this reader has not seen yet, newest
/// last, within `max_bytes`. Returns an empty string when nothing is new.
/// Records that do not fit are counted in a note, never silently dropped. A
/// session start, a first read, or a cursor that rotated out of the room gets
/// the full guidance header and `SESSION_START_BYTE_FACTOR` times the budget.
pub fn unread_context(
    project_root: &Path,
    limit: usize,
    max_bytes: usize,
    reader: &UnreadReader<'_>,
) -> anyhow::Result<String> {
    let messages = read_messages(project_root, UNREAD_SCAN_RECORDS)?;
    let cursor_path = reader_cursor_path(project_root, reader.key);
    let unread_start = if reader.is_session_start {
        None
    } else {
        read_reader_cursor(&cursor_path).and_then(|fingerprint| {
            messages
                .iter()
                .rposition(|message| message_fingerprint(message) == fingerprint)
                .map(|index| index + 1)
        })
    };
    if let Some(last) = messages.last() {
        write_reader_cursor(&cursor_path, &message_fingerprint(last))?;
    }
    let output = match unread_start {
        Some(start) if start == messages.len() => String::new(),
        Some(start) => render_capped(
            ilium_prompts::agent::CHATROOM_UNREAD_CONTEXT.to_string(),
            &messages[start..],
            limit,
            max_bytes,
        ),
        None => {
            let recent = &messages[messages.len().saturating_sub(limit)..];
            render_capped(
                full_context_header(recent),
                recent,
                limit,
                max_bytes.saturating_mul(SESSION_START_BYTE_FACTOR),
            )
        }
    };
    Ok(output)
}

fn full_context_header(messages: &[ChatMessage]) -> String {
    let template = if messages.is_empty() {
        "agent/chatroom-empty-context"
    } else {
        "agent/chatroom-context"
    };
    ilium_prompts::render_value(
        template,
        &serde_json::json!({"v0": (COORDINATION_POSTING_GUIDANCE).to_string()}),
    )
}

/// Bytes kept free for the omitted-records note.
const OMITTED_NOTE_RESERVE: usize = 160;

/// Renders `header` plus the newest of `messages` (at most `limit`) that fit in
/// `max_bytes`. Newer records win; a single oversized newest record is cut at
/// a character boundary rather than dropped.
fn render_capped(
    header: String,
    messages: &[ChatMessage],
    limit: usize,
    max_bytes: usize,
) -> String {
    let candidates = &messages[messages.len().saturating_sub(limit)..];
    let mut budget = max_bytes
        .saturating_sub(header.len())
        .saturating_sub(OMITTED_NOTE_RESERVE);
    let mut rows = Vec::new();
    for message in candidates.iter().rev() {
        let row = render_context_row(message);
        if row.len() <= budget {
            budget -= row.len();
            rows.push(row);
            continue;
        }
        if rows.is_empty() {
            rows.push(truncate_row(&row, budget));
        }
        break;
    }
    let omitted = messages.len() - rows.len();
    let mut output = header;
    if omitted > 0 {
        output.push_str(&ilium_prompts::render_value(
            "agent/chatroom-omitted",
            &serde_json::json!({"v0": omitted.to_string(), "v1": messages.len().to_string()}),
        ));
    }
    for row in rows.into_iter().rev() {
        output.push_str(&row);
    }
    output
}

fn render_context_row(message: &ChatMessage) -> String {
    ilium_prompts::render_value(
        "agent/chatroom-context-row",
        &serde_json::json!({"v0": (flatten_for_hook_output(&message.timestamp)).to_string(), "v1": (flatten_for_hook_output(&message.author)).to_string(), "v2": (flatten_for_hook_output(&message.content)).to_string()}),
    )
}

fn truncate_row(row: &str, budget: usize) -> String {
    let marker = "…\n";
    let keep = budget.saturating_sub(marker.len());
    let mut end = keep.min(row.len());
    while !row.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{marker}", &row[..end])
}

fn message_fingerprint(message: &ChatMessage) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for field in [&message.timestamp, &message.author, &message.content] {
        hasher.update(field.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn reader_cursor_path(project_root: &Path, reader_key: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(reader_key.as_bytes()));
    project_root
        .join(".ilium")
        .join("chat-readers")
        .join(&digest[..32])
}

fn read_reader_cursor(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let fingerprint = contents.trim();
    (fingerprint.len() == 64 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| fingerprint.to_string())
}

/// Each reader owns its cursor file, so a plain write-then-rename suffices.
fn write_reader_cursor(path: &Path, fingerprint: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&temporary, format!("{fingerprint}\n"))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

/// Collapses a stored field to a single safe line for hook output. Stored
/// content can carry real newlines/tabs (from a sent multi-line message) or,
/// since `CHATROOM.md` is human-editable, arbitrary control bytes such as raw
/// terminal escapes. Left verbatim, a real newline could forge additional
/// `- timestamp | author | content` records in the hook text and raw control
/// codes could reach the model context unsanitized, which the doc comment on
/// `context` promises never happens. `|` is left untouched: hook consumers
/// read this as prose, not as another escaped record to re-split.
fn flatten_for_hook_output(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            other if other.is_control() => String::new(),
            other => other.to_string(),
        })
        .collect()
}

fn write_new_chatroom(path: &Path) -> anyhow::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    writeln!(file, "{}", ilium_prompts::agent::CHATROOM_TITLE)?;
    writeln!(file, "{CHATROOM_MARKER}")?;
    writeln!(file)?;
    writeln!(
        file,
        "{}",
        ilium_prompts::render_value(
            "agent/chatroom-header-guidance",
            &serde_json::json!({"v0": (COORDINATION_POSTING_GUIDANCE).to_string()})
        )
    )?;
    writeln!(file)?;
    writeln!(file, "{}", ilium_prompts::agent::CHATROOM_MESSAGES_HEADING)?;
    file.sync_all()?;
    let _ = ilium_platform::secure_fs::sync_parent_directory_if_supported(path)?;
    Ok(())
}

/// Reads a text file that may legitimately not exist yet. A missing file
/// reads as empty (the normal "nothing to merge into" case); any other read
/// failure (permissions, non-UTF-8 content, ...) is propagated instead of
/// being papered over, because every caller here follows up with a full
/// `fs::write` that would otherwise silently truncate a real, unreadable
/// file down to just the generated ilium block.
fn read_existing_or_empty(path: &Path) -> anyhow::Result<String> {
    let file = match ilium_platform::secure_fs::open_regular_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => {
            return Err(anyhow::anyhow!(
                "failed to read {}: {error}",
                path.display()
            ));
        }
    };
    let mut bytes = Vec::new();
    file.take(512 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 512 * 1024 {
        anyhow::bail!("{} exceeds 512 KiB integration file limit", path.display());
    }
    Ok(String::from_utf8(bytes)?)
}

/// Tail preparation for the TUI. History remains in the authoritative file;
/// oversized human-edited records are refused rather than truncated silently.
pub(crate) fn read_messages_bounded(
    project_root: &Path,
    limit: usize,
) -> anyhow::Result<Vec<ChatMessage>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if !exists(project_root) {
        return Ok(Vec::new());
    }
    let mut file = ilium_platform::secure_fs::open_regular_file(&path_for_project(project_root))?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(8 * 1024 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        anyhow::bail!("Chatroom tail grew beyond preparation limit");
    }
    let begin = if start == 0 {
        0
    } else {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    };
    let text = std::str::from_utf8(&bytes[begin..])?;
    let mut messages = std::collections::VecDeque::new();
    let mut retained = 0_usize;
    for line in text.lines().rev() {
        if let Some(message) = parse_message_line(line) {
            retained += message.timestamp.capacity()
                + message.author.capacity()
                + message.content.capacity()
                + std::mem::size_of::<ChatMessage>();
            if retained > 4 * 1024 * 1024 {
                anyhow::bail!("Chatroom messages exceed 4 MiB retained limit");
            }
            messages.push_front(message);
            if messages.len() >= limit.min(200) {
                break;
            }
        }
    }
    Ok(messages.into())
}

fn ensure_gitignore(project_root: &Path) -> anyhow::Result<()> {
    let path = project_root.join(".gitignore");
    let existing = read_existing_or_empty(&path)?;
    let missing = ["/CHATROOM.md", CHATROOM_ARCHIVE_IGNORE]
        .into_iter()
        .filter(|entry| !existing.lines().any(|line| line.trim() == *entry))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let expected = existing.clone();
    let mut contents = existing;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    for entry in missing {
        contents.push_str(entry);
        contents.push('\n');
    }
    publish_integration_file(&path, contents.as_bytes(), &expected)?;
    Ok(())
}

fn ensure_codex_hooks(project_root: &Path) -> anyhow::Result<()> {
    let path = project_root.join(".codex/hooks.json");
    ensure_hook_file(&path, &["SessionStart", "UserPromptSubmit"])
}

fn ensure_claude_hooks(project_root: &Path) -> anyhow::Result<()> {
    let path = project_root.join(".claude/settings.local.json");
    ensure_hook_file(&path, &["SessionStart", "UserPromptSubmit"])
}

fn ensure_hook_file(path: &Path, events: &[&str]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = read_existing_or_empty(path)?;
    let expected = existing.clone();
    let existing = if existing.trim().is_empty() {
        "{}".to_string()
    } else {
        existing
    };
    let mut root: Value = serde_json::from_str(&existing)
        .map_err(|error| anyhow::anyhow!("could not merge {}: {error}", path.display()))?;
    let root_object = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} must contain a JSON object", path.display()))?;
    let mut changed = false;
    let hooks = root_object
        .entry("hooks".to_string())
        .or_insert_with(|| {
            changed = true;
            Value::Object(Map::new())
        })
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{}.hooks must be a JSON object", path.display()))?;
    for event in events {
        let groups = hooks
            .entry((*event).to_string())
            .or_insert_with(|| {
                changed = true;
                Value::Array(Vec::new())
            })
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("{}.hooks.{event} must be an array", path.display()))?;
        for group in groups.iter_mut() {
            changed |= upgrade_ilium_hook_command(group);
        }
        if !groups.iter().any(is_ilium_hook_group) {
            groups.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": HOOK_COMMAND,
                    "timeout": 10,
                    "statusMessage": "Checking ilium chatroom"
                }]
            }));
            changed = true;
        }
    }
    if changed {
        let updated = format!("{}\n", serde_json::to_string_pretty(&root)?);
        publish_integration_file(path, updated.as_bytes(), &expected)?;
    }
    Ok(())
}

fn publish_integration_file(path: &Path, contents: &[u8], expected: &str) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = parent.join(format!(
        ".{name}.ilium-integration-{}",
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        if read_existing_or_empty(path)? != expected {
            anyhow::bail!(
                "{} changed while preparing integrations; no replacement made",
                path.display()
            );
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn is_ilium_hook_group(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.starts_with(HOOK_COMMAND_PREFIX))
            })
        })
}

/// Rewrites an older chatroom hook command to the current one. Returns whether
/// anything changed.
fn upgrade_ilium_hook_command(group: &mut Value) -> bool {
    let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for hook in hooks {
        let is_outdated = hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| {
                command.starts_with(HOOK_COMMAND_PREFIX) && command != HOOK_COMMAND
            });
        if is_outdated {
            hook["command"] = Value::String(HOOK_COMMAND.to_string());
            changed = true;
        }
    }
    changed
}

fn with_project_lock<T>(
    project_root: &Path,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let state_directory = project_root.join(".ilium");
    fs::create_dir_all(&state_directory)?;
    let lock_path = state_directory.join("chatroom.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    // `File::lock` is the portable exclusive lock: `flock` on Unix,
    // `LockFileEx` on Windows.
    lock.lock()?;
    let result = operation();
    // `lock` (the fd) releases the flock unconditionally on drop, so a failed
    // explicit unlock here is not itself a correctness problem. Treating it
    // as fatal would turn an already-succeeded `operation()` (e.g. a message
    // already appended and fsynced) into a reported failure, inviting a
    // caller retry that re-runs the operation and duplicates its effect.
    let _ = lock.unlock();
    result
}

fn parse_message_line(line: &str) -> Option<ChatMessage> {
    let line = line.strip_prefix("- ")?;
    let mut fields = line.splitn(3, " | ");
    let mut parse_field =
        || -> Option<String> { Some(unescape_field(&strip_raw_control_bytes(fields.next()?))) };
    Some(ChatMessage {
        timestamp: parse_field()?,
        author: parse_field()?,
        content: parse_field()?,
    })
}

/// Drops raw control bytes from a record field as read from the file.
/// `sanitize_field` guarantees ilium itself never writes them, but
/// `CHATROOM.md` is human-editable, so a well-formed hand-edited record line
/// can still smuggle raw terminal escape bytes into a field. Left in place
/// they would reach every structured consumer unsanitized — `ilium chat tail`
/// prints fields straight to the user's terminal and the TUI renders them —
/// which the module contract ("human edits ... are safely ignored by the
/// structured UI") promises never happens. A raw tab is kept as benign
/// whitespace, and escaped `\n`/`\t` sequences are untouched here so
/// `unescape_field` still reconstructs real newlines and tabs from them.
fn strip_raw_control_bytes(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect()
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn unescape_field(value: &str) -> String {
    let mut result = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some(other) => result.push(other),
            None => result.push('\\'),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        append_message, capped_context, context, ensure_integrations, exists, initialize,
        read_messages, unread_context, UnreadReader, HOOK_COMMAND,
    };

    fn reader(key: &str, is_session_start: bool) -> UnreadReader<'_> {
        UnreadReader {
            key,
            is_session_start,
        }
    }

    #[test]
    fn unread_context_prints_only_records_the_reader_has_not_seen() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        append_message(directory.path(), "agent", "first claim").unwrap();
        append_message(directory.path(), "agent", "second claim").unwrap();

        let first = unread_context(directory.path(), 40, 2048, &reader("pane-1", false)).unwrap();
        assert!(first.contains("first claim") && first.contains("second claim"));
        assert!(first.contains("Ilium chatroom is enabled"));

        let unchanged =
            unread_context(directory.path(), 40, 2048, &reader("pane-1", false)).unwrap();
        assert_eq!(unchanged, "");

        append_message(directory.path(), "agent", "third claim").unwrap();
        let next = unread_context(directory.path(), 40, 2048, &reader("pane-1", false)).unwrap();
        assert!(next.starts_with("New ilium chatroom messages"));
        assert!(next.contains("third claim"));
        assert!(!next.contains("second claim"));

        let other = unread_context(directory.path(), 40, 2048, &reader("pane-2", false)).unwrap();
        assert!(other.contains("first claim") && other.contains("third claim"));

        let restart = unread_context(directory.path(), 40, 2048, &reader("pane-1", true)).unwrap();
        assert!(restart.contains("first claim") && restart.contains("third claim"));
    }

    #[test]
    fn capped_context_keeps_newest_records_and_counts_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        for index in 0..30 {
            append_message(
                directory.path(),
                "agent",
                &format!("record {index} {}", "x".repeat(200)),
            )
            .unwrap();
        }
        let output = capped_context(directory.path(), 40, 2048).unwrap();
        assert!(output.len() <= 2048, "{} bytes", output.len());
        assert!(output.contains("record 29 "));
        assert!(!output.contains("record 0 "));
        assert!(output.contains("earlier unread messages omitted"));

        append_message(directory.path(), "agent", &"y".repeat(3900)).unwrap();
        let oversized = capped_context(directory.path(), 40, 2048).unwrap();
        assert!(oversized.len() <= 2048, "{} bytes", oversized.len());
        assert!(oversized.contains("yyyy"));
    }

    #[test]
    fn integration_repair_upgrades_an_older_hook_command_in_place() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        let path = directory.path().join(".claude/settings.local.json");
        let legacy = std::fs::read_to_string(&path)
            .unwrap()
            .replace(HOOK_COMMAND, "ilium chat context --limit 40");
        std::fs::write(&path, legacy).unwrap();

        ensure_integrations(directory.path()).unwrap();

        let repaired = std::fs::read_to_string(&path).unwrap();
        assert_eq!(repaired.matches("ilium chat context").count(), 2);
        assert_eq!(repaired.matches(HOOK_COMMAND).count(), 2);
        assert!(std::fs::read_to_string(directory.path().join(".gitignore"))
            .unwrap()
            .contains("/CHATROOM.archive.md"));
    }

    #[test]
    fn initialization_creates_an_ignored_room_and_hooks_without_editing_guidance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("AGENTS.md"), "# Existing\n").unwrap();
        initialize(directory.path()).unwrap();
        initialize(directory.path()).unwrap();

        assert!(exists(directory.path()));
        assert!(std::fs::read_to_string(directory.path().join(".gitignore"))
            .unwrap()
            .contains("/CHATROOM.md"));
        assert_eq!(
            std::fs::read_to_string(directory.path().join("AGENTS.md")).unwrap(),
            "# Existing\n"
        );
        let chatroom_contents =
            std::fs::read_to_string(directory.path().join("CHATROOM.md")).unwrap();
        assert!(chatroom_contents.contains("Do not post routine progress narration"));
        for path in [".codex/hooks.json", ".claude/settings.local.json"] {
            let contents = std::fs::read_to_string(directory.path().join(path)).unwrap();
            assert_eq!(contents.matches("ilium chat context --limit 40").count(), 2);
        }
    }

    #[test]
    fn append_and_read_preserve_multiline_message_content() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        append_message(
            directory.path(),
            "agent:codex",
            "claiming | work\nthen testing",
        )
        .unwrap();
        let messages = read_messages(directory.path(), 40).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].author, "agent:codex");
        assert_eq!(messages[0].content, "claiming | work\nthen testing");
        let hook_context = context(directory.path(), 40).unwrap();
        assert!(hook_context.contains("claiming | work"));
        assert!(hook_context.contains("Use the chatroom sparingly"));
        assert!(hook_context.contains("Do not post routine progress narration"));
    }

    #[test]
    fn initialization_rejects_a_directory_at_the_chatroom_path_before_writing_integrations() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("CHATROOM.md")).unwrap();

        let error = initialize(directory.path()).unwrap_err().to_string();

        assert!(error.contains("not a regular file"));
        assert!(!directory.path().join(".codex/hooks.json").exists());
        assert!(!directory
            .path()
            .join(".claude/settings.local.json")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn initialization_rejects_a_dangling_chatroom_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        symlink(
            directory.path().join("missing-target"),
            directory.path().join("CHATROOM.md"),
        )
        .unwrap();

        let error = initialize(directory.path()).unwrap_err().to_string();

        assert!(error.contains("symlinked chatroom"));
    }

    #[test]
    fn append_repairs_a_missing_trailing_newline_left_by_a_human_edit() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        let chatroom_path = directory.path().join(super::CHATROOM_FILE_NAME);
        let mut contents = std::fs::read_to_string(&chatroom_path).unwrap();
        assert!(contents.ends_with('\n'));
        while contents.ends_with('\n') {
            contents.pop();
        }
        std::fs::write(&chatroom_path, &contents).unwrap();

        append_message(directory.path(), "agent:codex", "hello").unwrap();

        let messages = read_messages(directory.path(), 40).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn hand_edited_records_cannot_smuggle_raw_terminal_escapes() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path()).unwrap();
        let chatroom_path = directory.path().join(super::CHATROOM_FILE_NAME);
        let mut contents = std::fs::read_to_string(&chatroom_path).unwrap();
        // A human-edited but well-formed record carrying a raw ANSI escape.
        contents
            .push_str("- 2026-08-19 10:00:00 +02:00 | human | red \u{1b}[31mtext\u{1b}[0m\tok\n");
        std::fs::write(&chatroom_path, contents).unwrap();

        let messages = read_messages(directory.path(), 40).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].author, "human");
        assert_eq!(messages[0].content, "red [31mtext[0m\tok");
    }

    #[test]
    fn boot_repair_leaves_projects_without_a_room_untouched() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!ensure_integrations(directory.path()).unwrap());
        assert!(!directory.path().join(".codex").exists());
    }
}
