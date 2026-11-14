//! The room file and its sibling project files through the `fs` host import.
//!
//! Every function takes the host handle, so the CLI side and the live server side share one
//! implementation of the file rules. The `project` scope is the session project directory (the
//! launcher of `ilium chat` resolves the owning project first, see `project_marker` of the
//! manifest). Writes are performed by the host before the call returns; the later
//! `request-completed` of a write carries no information the plugin needs.

use ilium_plugin_sdk::host::{EntryKind, HostClock, HostFs, WriteMode};
use ilium_plugin_sdk::types::{ErrorCode, FsPath, PluginError};
use ilium_plugin_sdk::HostApi;

use crate::hooks::{merge_hook_file, CLAUDE_HOOKS_PATH, CODEX_HOOKS_PATH, HOOK_EVENTS};
use crate::messages::{
    format_post, gitignore_with_room, new_room_text, parse_messages, PostError, CHATROOM_FILE_NAME,
};

/// The most bytes read from the end of the room for one tail, context or unread read.
pub const TAIL_READ_BYTES: u32 = 1024 * 1024;
/// The most bytes of one integration file (`.gitignore`, hook files) the plugin edits.
const INTEGRATION_FILE_LIMIT: u32 = 512 * 1024;
/// Folder of the reader cursors in the `data` scope.
const READERS_DIR: &str = "readers";

/// A path in the session project.
pub fn project_path(path: &str) -> FsPath {
    FsPath {
        scope: "project".to_string(),
        path: path.to_string(),
    }
}

/// A path in the plugin's data scope.
fn data_path(path: &str) -> FsPath {
    FsPath {
        scope: "data".to_string(),
        path: path.to_string(),
    }
}

/// What the room path holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    /// No file at the path.
    Missing,
    /// A regular file.
    Present { length: u64 },
    /// Something else (a directory, a symlink): refused, never written through.
    Unusable,
}

/// Looks at the room path without following a symlink.
pub fn room_state(api: &mut dyn HostApi) -> RoomState {
    match HostFs::metadata(api, project_path(CHATROOM_FILE_NAME)) {
        Ok(metadata) if metadata.kind == EntryKind::File => RoomState::Present {
            length: metadata.length,
        },
        Ok(_) => RoomState::Unusable,
        Err(error) if error.code == ErrorCode::NotFound => RoomState::Missing,
        Err(_) => RoomState::Unusable,
    }
}

/// The text of the newest part of the room (at most [`TAIL_READ_BYTES`]); a cut first line is
/// dropped so every returned line is whole.
pub fn read_room_tail(api: &mut dyn HostApi, length: u64) -> Result<String, PluginError> {
    let start = length.saturating_sub(u64::from(TAIL_READ_BYTES));
    let bytes = HostFs::read(
        api,
        project_path(CHATROOM_FILE_NAME),
        start,
        TAIL_READ_BYTES,
    )?;
    let begin = if start == 0 {
        0
    } else {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    };
    Ok(String::from_utf8_lossy(&bytes[begin..]).into_owned())
}

/// The room text for a reader; an absent room reads as empty and an unusable one is an error.
pub fn read_room(api: &mut dyn HostApi) -> Result<Option<String>, PluginError> {
    match room_state(api) {
        RoomState::Missing => Ok(None),
        RoomState::Unusable => Err(PluginError::new(
            ErrorCode::InvalidArgument,
            "CHATROOM.md is not a regular file",
        )),
        RoomState::Present { length } => read_room_tail(api, length).map(Some),
    }
}

/// The newest `limit` records of the room as stored lines without the `- ` prefix.
pub fn recent_record_lines(text: &str, limit: usize) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .filter(|line| crate::parse_record(line).is_some())
        .collect();
    let from = lines.len().saturating_sub(limit);
    lines[from..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// `YYYY-MM-DD HH:mm:ss +00:00` for a UTC wall clock in milliseconds. The plugin sandbox has no
/// time zone, so every record is stamped in UTC.
pub fn format_timestamp(unix_millis: u64) -> String {
    let seconds = unix_millis / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let second_of_day = seconds % 86_400;
    // Civil-from-days (Howard Hinnant), valid for every date of the proleptic Gregorian calendar.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} +00:00",
        second_of_day / 3600,
        (second_of_day % 3600) / 60,
        second_of_day % 60
    )
}

/// Why a post failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostFailure {
    /// The project has no room.
    NoRoom,
    /// The record was refused before any write.
    Refused(PostError),
    /// The host refused the read or the write.
    Host(String),
}

impl std::fmt::Display for PostFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRoom => formatter.write_str(crate::root::NO_CHATROOM_MESSAGE),
            Self::Refused(error) => std::fmt::Display::fmt(error, formatter),
            Self::Host(message) => formatter.write_str(message),
        }
    }
}

/// Appends one record. The record is one single append write, so concurrent writers (other
/// `ilium chat send` processes, the panel composer) never interleave inside a line.
pub fn append_message(
    api: &mut dyn HostApi,
    author: &str,
    content: &str,
) -> Result<(), PostFailure> {
    let length = match room_state(api) {
        RoomState::Present { length } => length,
        RoomState::Missing => return Err(PostFailure::NoRoom),
        RoomState::Unusable => {
            return Err(PostFailure::Host(
                "CHATROOM.md is not a regular file".to_string(),
            ))
        }
    };
    // Only the last byte matters: a human edit may have dropped the final line end.
    let existing = if length == 0 {
        String::new()
    } else {
        let last = HostFs::read(api, project_path(CHATROOM_FILE_NAME), length - 1, 1)
            .map_err(|error| PostFailure::Host(error.message))?;
        if last.first() == Some(&b'\n') {
            "\n".to_string()
        } else {
            "x".to_string()
        }
    };
    let timestamp = format_timestamp(HostClock::now_unix_millis(api));
    let record =
        format_post(&existing, &timestamp, author, content).map_err(PostFailure::Refused)?;
    HostFs::write(
        api,
        project_path(CHATROOM_FILE_NAME),
        record.into_bytes(),
        WriteMode::Append,
    )
    .map(|_| ())
    .map_err(|error| PostFailure::Host(error.message))
}

/// Creates the room (when absent) and the project integrations: the `.gitignore` entries and the
/// Codex and Claude hook files. Safe to repeat; existing records and foreign hooks stay.
pub fn initialize(api: &mut dyn HostApi) -> Result<(), String> {
    match room_state(api) {
        RoomState::Unusable => return Err("CHATROOM.md is not a regular file".to_string()),
        RoomState::Missing => {
            HostFs::write(
                api,
                project_path(CHATROOM_FILE_NAME),
                new_room_text().into_bytes(),
                WriteMode::CreateNew,
            )
            .map_err(|error| error.message)?;
        }
        RoomState::Present { .. } => {}
    }
    ensure_integrations(api)
}

/// Adds the git-ignore entries and the hook files; reports the first failure.
pub fn ensure_integrations(api: &mut dyn HostApi) -> Result<(), String> {
    let gitignore = read_text_or_empty(api, ".gitignore")?;
    if let Some(updated) = gitignore_with_room(&gitignore) {
        write_replace(api, ".gitignore", updated)?;
    }
    for (path, folder) in [(CODEX_HOOKS_PATH, ".codex"), (CLAUDE_HOOKS_PATH, ".claude")] {
        let existing = read_text_or_empty(api, path)?;
        if let Some(updated) = merge_hook_file(&existing, path, &HOOK_EVENTS)? {
            // The folder may not exist yet; an existing one is fine.
            let _ = HostFs::create_dir(api, project_path(folder));
            write_replace(api, path, updated)?;
        }
    }
    Ok(())
}

fn read_text_or_empty(api: &mut dyn HostApi, path: &str) -> Result<String, String> {
    match HostFs::metadata(api, project_path(path)) {
        Err(error) if error.code == ErrorCode::NotFound => return Ok(String::new()),
        Err(error) => return Err(format!("failed to read {path}: {}", error.message)),
        Ok(metadata) if metadata.kind != EntryKind::File => {
            return Err(format!("{path} is not a regular file"))
        }
        Ok(metadata) if metadata.length > u64::from(INTEGRATION_FILE_LIMIT) => {
            return Err(format!("{path} exceeds 512 KiB integration file limit"))
        }
        Ok(_) => {}
    }
    let bytes = HostFs::read(api, project_path(path), 0, INTEGRATION_FILE_LIMIT)
        .map_err(|error| format!("failed to read {path}: {}", error.message))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not valid UTF-8"))
}

fn write_replace(api: &mut dyn HostApi, path: &str, text: String) -> Result<(), String> {
    HostFs::write(
        api,
        project_path(path),
        text.into_bytes(),
        WriteMode::Replace,
    )
    .map(|_| ())
    .map_err(|error| format!("failed to write {path}: {}", error.message))
}

/// The stored cursor of one reader (a record fingerprint), when present.
pub fn read_reader_cursor(api: &mut dyn HostApi, reader_key: &str) -> Option<String> {
    let path = format!(
        "{READERS_DIR}/{}",
        crate::messages::reader_cursor_file_name(reader_key)
    );
    let bytes = HostFs::read(api, data_path(&path), 0, 128).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let fingerprint = text.trim();
    (fingerprint.len() == 32 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| fingerprint.to_string())
}

/// Stores the cursor of one reader. Best effort: a failure only means the reader sees some
/// records again.
pub fn write_reader_cursor(api: &mut dyn HostApi, reader_key: &str, fingerprint: &str) {
    let _ = HostFs::create_dir(api, data_path(READERS_DIR));
    let path = format!(
        "{READERS_DIR}/{}",
        crate::messages::reader_cursor_file_name(reader_key)
    );
    let _ = HostFs::write(
        api,
        data_path(&path),
        format!("{fingerprint}\n").into_bytes(),
        WriteMode::Replace,
    );
}

/// How many records follow the reader's cursor (the unread count of the tree row, chatroom-B16).
pub fn unread_count(contents: &str, stored_cursor: Option<&str>) -> usize {
    let messages = parse_messages(contents);
    match stored_cursor.and_then(|fingerprint| {
        messages
            .iter()
            .rposition(|message| crate::messages::message_fingerprint(message) == fingerprint)
    }) {
        Some(index) => messages.len() - index - 1,
        None => messages.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_civil_dates() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00 +00:00");
        assert_eq!(
            format_timestamp(1_709_164_800_000 + 3_723_000),
            "2024-02-29 01:02:03 +00:00"
        );
        assert!(crate::is_record_timestamp(&format_timestamp(
            1_800_000_000_000
        )));
    }

    #[test]
    fn recent_lines_keep_only_valid_records_newest_last() {
        let text = "# Chatroom\n- 2026-11-14 10:00:00 +00:00 | a | one\nnot a record\n- 2026-11-14 10:00:01 +00:00 | b | two\n";
        let lines = recent_record_lines(text, 1);
        assert_eq!(
            lines,
            vec!["2026-11-14 10:00:01 +00:00 | b | two".to_string()]
        );
    }

    #[test]
    fn unread_counts_records_after_the_cursor() {
        let text =
            "- 2026-11-14 10:00:00 +00:00 | a | one\n- 2026-11-14 10:00:01 +00:00 | b | two\n";
        let all = crate::messages::parse_messages(text);
        let cursor = crate::messages::message_fingerprint(&all[0]);
        assert_eq!(unread_count(text, Some(&cursor)), 1);
        assert_eq!(unread_count(text, None), 2);
    }
}
