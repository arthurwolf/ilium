//! Byte-preserving edits of one top-level member of a JSON settings file.
//!
//! The model status-line bridges of Claude Code and Antigravity own one structured setting of the agent's
//! `settings.json` while they are enabled (`statusLine`) and must put the user's exact formatting back when
//! they are disabled. The files belong to the user and are also rewritten by the agent CLIs themselves, so
//! nothing here re-serializes them: a small span scanner locates the top-level member, the value is replaced
//! in place, or the member is inserted as the first key reusing the file's own indentation, and `serde_json`
//! only validates the text and cross-checks that the edit changed that one key only.
//!
//! The auto-compact threshold writer of the compaction optimizer (apply and revert of `autoCompactWindow` and
//! `model_auto_compact_token_limit`) lives in the `ilium.core.compaction` plugin.

use std::ops::Range;

fn replace_range(text: &str, range: Range<usize>, replacement: &str) -> Result<String, String> {
    let (Some(head), Some(tail)) = (text.get(..range.start), text.get(range.end..)) else {
        return Err("internal span error".into());
    };
    Ok(format!("{head}{replacement}{tail}"))
}

fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(index) if index > 0 && text.as_bytes().get(index - 1) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

// --- JSON ------------------------------------------------------------------

struct JsonMember {
    key: String,
    key_start: usize,
    key_end: usize,
    value_start: usize,
    value_end: usize,
}

struct JsonRoot {
    open: usize,
    close: usize,
    members: Vec<JsonMember>,
}

fn json_skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        index += 1;
    }
    index
}

/// Index just past the closing quote of the string that starts at `start`.
fn json_string_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err("unterminated JSON string".into())
}

/// Index just past the value that starts at `start`.
fn json_value_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    match bytes.get(start) {
        None => Err("missing JSON value".into()),
        Some(b'"') => json_string_end(bytes, start),
        Some(b'{' | b'[') => {
            let mut depth = 0_usize;
            let mut index = start;
            loop {
                match bytes.get(index) {
                    None => return Err("unterminated JSON container".into()),
                    Some(b'"') => {
                        index = json_string_end(bytes, index)?;
                        continue;
                    }
                    Some(b'{' | b'[') => depth += 1,
                    Some(b'}' | b']') => {
                        depth = depth.checked_sub(1).ok_or("unbalanced JSON container")?;
                        if depth == 0 {
                            return Ok(index + 1);
                        }
                    }
                    Some(_) => {}
                }
                index += 1;
            }
        }
        Some(_) => {
            let mut index = start;
            while let Some(byte) = bytes.get(index) {
                if matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                    break;
                }
                index += 1;
            }
            if index == start {
                return Err("missing JSON value".into());
            }
            Ok(index)
        }
    }
}

/// Locates every member of the top-level object. The text has already been
/// validated by `serde_json`; this only needs the byte offsets.
fn scan_json_root(text: &str) -> Result<JsonRoot, String> {
    let bytes = text.as_bytes();
    let open = json_skip_whitespace(bytes, 0);
    if bytes.get(open) != Some(&b'{') {
        return Err("the top level of the file is not a JSON object".into());
    }
    let mut members = Vec::new();
    let mut index = open + 1;
    let close = loop {
        index = json_skip_whitespace(bytes, index);
        match bytes.get(index) {
            Some(b'}') => break index,
            Some(b'"') => {}
            _ => return Err("unexpected content in the top-level JSON object".into()),
        }
        let key_start = index;
        let key_end = json_string_end(bytes, key_start)?;
        let key: String = serde_json::from_str(&text[key_start..key_end])
            .map_err(|error| format!("invalid JSON key: {error}"))?;
        index = json_skip_whitespace(bytes, key_end);
        if bytes.get(index) != Some(&b':') {
            return Err("expected ':' after a JSON key".into());
        }
        let value_start = json_skip_whitespace(bytes, index + 1);
        let value_end = json_value_end(bytes, value_start)?;
        members.push(JsonMember {
            key,
            key_start,
            key_end,
            value_start,
            value_end,
        });
        index = json_skip_whitespace(bytes, value_end);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => break index,
            _ => return Err("expected ',' or '}' in the top-level JSON object".into()),
        }
    };
    Ok(JsonRoot {
        open,
        close,
        members,
    })
}

/// The one member named `key`; two of them are refused.
fn json_find_member<'a>(root: &'a JsonRoot, key: &str) -> Result<Option<&'a JsonMember>, String> {
    let mut matches = root.members.iter().filter(|member| member.key == key);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(format!("the key {key} appears more than once"));
    }
    Ok(first)
}

/// Edits one top-level JSON value while preserving every other byte. Used by
/// managed integrations that need to own a structured setting temporarily.
pub(crate) fn edit_top_level_json_value(
    text: &str,
    key: &str,
    replacement: Option<serde_json::Value>,
) -> Result<String, String> {
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = expected
        .as_object_mut()
        .ok_or_else(|| "the top level of the file is not a JSON object".to_owned())?;
    let root = scan_json_root(text)?;
    let member = json_find_member(&root, key)?;
    let edited = match (member, replacement.as_ref()) {
        (Some(member), Some(value)) => replace_range(
            text,
            member.value_start..member.value_end,
            &serde_json::to_string(value).map_err(|error| error.to_string())?,
        )?,
        (None, Some(value)) => insert_json_member(text, &root, key, value)?,
        (Some(member), None) => remove_json_member(text, &root, member)?,
        (None, None) => text.to_owned(),
    };
    match replacement {
        Some(value) => {
            object.insert(key.to_owned(), value);
        }
        None => {
            object.remove(key);
        }
    }
    let actual: serde_json::Value = serde_json::from_str(&edited)
        .map_err(|error| format!("edited JSON is invalid: {error}"))?;
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(edited)
}

/// Restores one top-level JSON value from its original source slice so a
/// managed setting can put the user's exact formatting back when disabling.
pub(crate) fn restore_top_level_json_value_text(
    text: &str,
    key: &str,
    replacement: Option<&str>,
) -> Result<String, String> {
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = expected
        .as_object_mut()
        .ok_or_else(|| "the top level of the file is not a JSON object".to_owned())?;
    let replacement_value: Option<serde_json::Value> = replacement
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| format!("invalid replacement JSON: {error}"))?;
    let root = scan_json_root(text)?;
    let member = json_find_member(&root, key)?;
    let edited = match (member, replacement) {
        (Some(member), Some(raw)) => {
            replace_range(text, member.value_start..member.value_end, raw)?
        }
        (None, Some(_)) => {
            let value = replacement_value
                .as_ref()
                .expect("parsed replacement exists");
            insert_json_member(text, &root, key, value)?
        }
        (Some(member), None) => remove_json_member(text, &root, member)?,
        (None, None) => text.to_owned(),
    };
    match replacement_value {
        Some(value) => {
            object.insert(key.to_owned(), value);
        }
        None => {
            object.remove(key);
        }
    }
    let actual: serde_json::Value = serde_json::from_str(&edited)
        .map_err(|error| format!("edited JSON is invalid: {error}"))?;
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(edited)
}

pub(crate) fn top_level_json_value_text(text: &str, key: &str) -> Result<Option<String>, String> {
    let _: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let root = scan_json_root(text)?;
    let Some(member) = json_find_member(&root, key)? else {
        return Ok(None);
    };
    Ok(Some(
        text.get(member.value_start..member.value_end)
            .ok_or("internal JSON span error")?
            .to_owned(),
    ))
}

/// Inserts `"key": value` as the first member, reusing the whitespace that
/// already follows `{` and the separator style of the existing first member.
fn insert_json_member(
    text: &str,
    root: &JsonRoot,
    key: &str,
    value: &serde_json::Value,
) -> Result<String, String> {
    let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
    if let Some(first) = root.members.first() {
        let leading = text
            .get(root.open + 1..first.key_start)
            .ok_or("internal span error")?;
        let separator = text
            .get(first.key_end..first.value_start)
            .filter(|separator| !separator.contains('\n'))
            .unwrap_or(": ");
        let member = format!("\"{key}\"{separator}{value},{leading}");
        return replace_range(text, first.key_start..first.key_start, &member);
    }
    let interior = text
        .get(root.open + 1..root.close)
        .ok_or("internal span error")?;
    let replacement = match interior.rfind('\n') {
        Some(index) => {
            let eol = line_ending(text);
            let closing_indent = &interior[index + 1..];
            format!("{eol}{closing_indent}  \"{key}\": {value}{eol}{closing_indent}")
        }
        None => format!("\"{key}\": {value}"),
    };
    replace_range(text, root.open + 1..root.close, &replacement)
}

/// Removes one member together with the comma and whitespace that tie it to a
/// neighbour. For a member this module inserted as the first key, this restores
/// the original bytes exactly.
fn remove_json_member(text: &str, root: &JsonRoot, member: &JsonMember) -> Result<String, String> {
    let position = root
        .members
        .iter()
        .position(|candidate| candidate.key_start == member.key_start)
        .ok_or("internal member lookup error")?;
    if let Some(next) = root.members.get(position + 1) {
        return replace_range(text, member.key_start..next.key_start, "");
    }
    match position
        .checked_sub(1)
        .and_then(|prior| root.members.get(prior))
    {
        Some(previous) => replace_range(text, previous.value_end..member.value_end, ""),
        None => replace_range(text, root.open + 1..root.close, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = "{\n  \"model\": \"opus\",\n  \"theme\":   \"dark\"\n}\n";

    #[test]
    fn replacing_a_value_keeps_every_other_byte() {
        let text = "{\n  \"statusLine\": {\"type\": \"command\"},\n  \"theme\":   \"dark\"\n}\n";
        let edited = edit_top_level_json_value(
            text,
            "statusLine",
            Some(serde_json::json!({"type": "static"})),
        )
        .expect("edit");
        assert_eq!(
            edited,
            "{\n  \"statusLine\": {\"type\":\"static\"},\n  \"theme\":   \"dark\"\n}\n"
        );
    }

    #[test]
    fn inserting_then_removing_restores_the_original_bytes() {
        let inserted =
            edit_top_level_json_value(SETTINGS, "statusLine", Some(serde_json::json!(1)))
                .expect("insert");
        assert!(inserted.contains("\"statusLine\": 1"));
        let restored = edit_top_level_json_value(&inserted, "statusLine", None).expect("remove");
        assert_eq!(restored, SETTINGS);
    }

    #[test]
    fn restoring_the_original_source_text_puts_back_the_users_formatting() {
        let text = "{\n  \"statusLine\":   {\"a\":1}\n}\n";
        let original = top_level_json_value_text(text, "statusLine")
            .expect("read")
            .expect("present");
        assert_eq!(original, "{\"a\":1}");
        let changed =
            edit_top_level_json_value(text, "statusLine", Some(serde_json::json!({"b": 2})))
                .expect("change");
        let restored = restore_top_level_json_value_text(&changed, "statusLine", Some(&original))
            .expect("restore");
        assert_eq!(restored, text);
    }

    #[test]
    fn a_nested_key_of_the_same_name_is_not_the_top_level_key() {
        let text = "{\n  \"env\": {\"statusLine\": 1}\n}\n";
        assert_eq!(top_level_json_value_text(text, "statusLine"), Ok(None));
    }

    #[test]
    fn invalid_duplicate_and_non_object_input_is_refused() {
        assert!(top_level_json_value_text("{ not json", "a").is_err());
        assert!(top_level_json_value_text("[1]", "a").is_err());
        assert!(top_level_json_value_text("{\"a\": 1, \"a\": 2}", "a").is_err());
        assert!(edit_top_level_json_value("{\"a\": 1, \"a\": 2}", "a", None).is_err());
    }
}
