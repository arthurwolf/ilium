//! Text-preserving edits of the agent CLIs' own config files (core-plugin-agent-setup.md,
//! "Compaction window" setup step).
//!
//! Both editors work on the original bytes: they locate the one top-level key with a validating
//! scanner and splice only that span, so every other byte of the file (comments, spacing, line
//! endings, key order) survives. Error texts name the setting and a byte or line position, never a
//! source line, because the files can hold secrets.

use std::collections::HashSet;

/// Claude Code's setting key inside `settings.json`.
const CLAUDE_KEY: &str = "autoCompactWindow";
/// Codex's setting key at the top level of `config.toml`.
const CODEX_KEY: &str = "model_auto_compact_token_limit";
/// Nesting limit of the scanners, so hostile input cannot exhaust the stack.
const MAX_DEPTH: usize = 64;

/// Which agent setting a config edit targets (core-plugin-agent-setup.md "Compaction window").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTarget {
    /// `autoCompactWindow` in Claude Code's `settings.json`.
    ClaudeAutoCompactWindow,
    /// `model_auto_compact_token_limit` in Codex's `config.toml`.
    CodexAutoCompactTokenLimit,
}

/// What to do with the targeted key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChange {
    /// Set the key to this value.
    Set(u64),
    /// Remove the key.
    Remove,
}

/// Applies `change` to `target` inside `text`, keeping every other byte of the file.
pub fn edit_config_text(
    target: ConfigTarget,
    text: &str,
    change: KeyChange,
) -> Result<String, String> {
    match target {
        ConfigTarget::ClaudeAutoCompactWindow => edit_json_text(text, CLAUDE_KEY, change),
        ConfigTarget::CodexAutoCompactTokenLimit => edit_toml_text(text, CODEX_KEY, change),
    }
}

/// Reads the current value of `target` from `text`; `Ok(None)` when the key is absent.
pub fn parse_setting(target: ConfigTarget, text: &str) -> Result<Option<u64>, String> {
    match target {
        ConfigTarget::ClaudeAutoCompactWindow => parse_json_setting(text, CLAUDE_KEY),
        ConfigTarget::CodexAutoCompactTokenLimit => parse_toml_setting(text, CODEX_KEY),
    }
}

// ---------------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------------

/// One member of the root object, as byte offsets into the source text.
struct JsonMember {
    key: String,
    key_start: usize,
    key_end: usize,
    value_start: usize,
    value_end: usize,
}

struct JsonDocument {
    /// Offset of the root `{`.
    open: usize,
    /// Offset of the root `}`.
    close: usize,
    members: Vec<JsonMember>,
}

struct JsonScanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonScanner<'a> {
    fn fail<T>(&self, reason: &str) -> Result<T, String> {
        Err(format!("invalid JSON at byte {}: {reason}", self.pos))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8, reason: &str) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            return Ok(());
        }
        self.fail(reason)
    }

    /// Scans a string from the opening quote and returns its decoded content.
    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"', "expected a string")?;
        let mut decoded = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return self.fail("unterminated string");
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(decoded);
                }
                0..=0x1f => return self.fail("control character in string"),
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut decoded)?;
                }
                _ => {
                    let chunk_start = self.pos;
                    while let Some(next) = self.peek() {
                        if next == b'"' || next == b'\\' || next < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    // The source is a &str, so a run delimited by ASCII bytes is valid UTF-8.
                    decoded.push_str(&String::from_utf8_lossy(&self.bytes[chunk_start..self.pos]));
                }
            }
        }
    }

    fn escape(&mut self, decoded: &mut String) -> Result<(), String> {
        let Some(code) = self.peek() else {
            return self.fail("unterminated escape");
        };
        self.pos += 1;
        let plain = match code {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let unit = self.hex4()?;
                let scalar = self.combine_surrogates(unit)?;
                decoded.push(scalar);
                return Ok(());
            }
            _ => return self.fail("unknown escape"),
        };
        decoded.push(plain);
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self.peek().and_then(|byte| (byte as char).to_digit(16));
            let Some(digit) = digit else {
                return self.fail("bad unicode escape");
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn combine_surrogates(&mut self, unit: u32) -> Result<char, String> {
        if !(0xD800..0xDC00).contains(&unit) {
            return Ok(char::from_u32(unit).unwrap_or('\u{fffd}'));
        }
        if self.peek() == Some(b'\\') && self.bytes.get(self.pos + 1) == Some(&b'u') {
            self.pos += 2;
            let low = self.hex4()?;
            if (0xDC00..0xE000).contains(&low) {
                let scalar = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                return Ok(char::from_u32(scalar).unwrap_or('\u{fffd}'));
            }
        }
        Ok('\u{fffd}')
    }

    fn digits(&mut self) -> Result<(), String> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == start {
            return self.fail("expected digits");
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits()?,
            _ => return self.fail("expected a number"),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits()?;
        }
        Ok(())
    }

    fn literal(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            return Ok(());
        }
        self.fail("unknown literal")
    }

    fn value(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return self.fail("nesting too deep");
        }
        match self.peek() {
            Some(b'{') => self.object(depth, None),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(|_| ()),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fail("expected a value"),
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), String> {
        self.expect(b'[', "expected '['")?;
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.skip_whitespace();
            self.value(depth + 1)?;
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    /// Scans an object; when `members` is given, records each member's spans (root object only).
    fn object(
        &mut self,
        depth: usize,
        mut members: Option<&mut Vec<JsonMember>>,
    ) -> Result<(), String> {
        self.expect(b'{', "expected '{'")?;
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.skip_whitespace();
            let key_start = self.pos;
            let key = self.string()?;
            let key_end = self.pos;
            self.skip_whitespace();
            self.expect(b':', "expected ':'")?;
            self.skip_whitespace();
            let value_start = self.pos;
            self.value(depth + 1)?;
            let value_end = self.pos;
            if let Some(list) = members.as_deref_mut() {
                list.push(JsonMember { key, key_start, key_end, value_start, value_end });
            }
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }
}

fn scan_json_document(text: &str) -> Result<JsonDocument, String> {
    let mut scanner = JsonScanner { bytes: text.as_bytes(), pos: 0 };
    scanner.skip_whitespace();
    if scanner.peek().is_none() {
        return scanner.fail("empty document");
    }
    if scanner.peek() != Some(b'{') {
        scanner.value(0)?;
        scanner.skip_whitespace();
        if scanner.peek().is_some() {
            return scanner.fail("unexpected text after the value");
        }
        return Err("the JSON document is not a JSON object".to_string());
    }
    let open = scanner.pos;
    let mut members = Vec::new();
    scanner.object(0, Some(&mut members))?;
    let close = scanner.pos - 1;
    scanner.skip_whitespace();
    if scanner.peek().is_some() {
        return scanner.fail("unexpected text after the object");
    }
    Ok(JsonDocument { open, close, members })
}

/// The one member named `key`; more than one is refused because the intended value is unclear.
fn find_unique_member<'a>(
    document: &'a JsonDocument,
    key: &str,
) -> Result<Option<(usize, &'a JsonMember)>, String> {
    let mut found = document.members.iter().enumerate().filter(|(_, member)| member.key == key);
    let first = found.next();
    if found.next().is_some() {
        return Err(format!("`{key}` appears more than once in the JSON object"));
    }
    Ok(first)
}

fn json_integer(text: &str, member: &JsonMember, key: &str) -> Result<u64, String> {
    let raw = &text[member.value_start..member.value_end];
    let is_digits = !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit());
    match raw.parse::<u64>() {
        Ok(value) if is_digits => Ok(value),
        _ => Err(format!("`{key}` is not a non-negative integer")),
    }
}

fn parse_json_setting(text: &str, key: &str) -> Result<Option<u64>, String> {
    let document = scan_json_document(text)?;
    match find_unique_member(&document, key)? {
        Some((_, member)) => json_integer(text, member, key).map(Some),
        None => Ok(None),
    }
}

fn json_quote(key: &str) -> String {
    let mut quoted = String::from("\"");
    for character in key.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            control if (control as u32) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn splice(text: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut edited = String::with_capacity(text.len() + replacement.len());
    edited.push_str(&text[..start]);
    edited.push_str(replacement);
    edited.push_str(&text[end..]);
    edited
}

/// Applies `change` to the top-level `key` of a JSON document, keeping formatting.
pub fn edit_json_text(text: &str, key: &str, change: KeyChange) -> Result<String, String> {
    let document = scan_json_document(text)?;
    let existing = find_unique_member(&document, key)?;
    match (change, existing) {
        (KeyChange::Set(value), Some((_, member))) => {
            json_integer(text, member, key)?;
            Ok(splice(text, member.value_start, member.value_end, &value.to_string()))
        }
        (KeyChange::Set(value), None) => Ok(insert_json_member(text, &document, key, value)),
        (KeyChange::Remove, Some((index, _))) => Ok(remove_json_member(text, &document, index)),
        (KeyChange::Remove, None) => Ok(text.to_string()),
    }
}

/// Inserts the key as the first member, copying the indentation and colon style of the object.
fn insert_json_member(text: &str, document: &JsonDocument, key: &str, value: u64) -> String {
    let quoted = json_quote(key);
    let Some(first) = document.members.first() else {
        let inner = &text[document.open + 1..document.close];
        if !inner.contains('\n') {
            let member = format!("{quoted}: {value}");
            return splice(text, document.open + 1, document.open + 1, &member);
        }
        let newline = if inner.contains("\r\n") { "\r\n" } else { "\n" };
        return splice(
            text,
            document.open + 1,
            document.open + 1,
            &format!("{newline}  {quoted}: {value}"),
        );
    };
    let gap = &text[document.open + 1..first.key_start];
    let colon = &text[first.key_end..first.value_start];
    let member = format!("{quoted}{colon}{value},{gap}");
    splice(text, first.key_start, first.key_start, &member)
}

fn remove_json_member(text: &str, document: &JsonDocument, index: usize) -> String {
    let members = &document.members;
    let member = &members[index];
    if members.len() == 1 {
        return splice(text, document.open + 1, member.value_end, "");
    }
    if let Some(next) = members.get(index + 1) {
        return splice(text, member.key_start, next.key_start, "");
    }
    let previous = &members[index - 1];
    splice(text, previous.value_end, member.value_end, "")
}

// ---------------------------------------------------------------------------------------------
// TOML
// ---------------------------------------------------------------------------------------------

/// One `key = value` line before the first table header.
struct TomlEntry {
    path: String,
    line_start: usize,
    line_end: usize,
    value_start: usize,
    value_end: usize,
    has_comment: bool,
}

struct TomlDocument {
    entries: Vec<TomlEntry>,
    /// Offset of the first table header line, when the file has tables.
    first_table_start: Option<usize>,
    line_ending: &'static str,
}

struct TomlScanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> TomlScanner<'a> {
    fn fail<T>(&self, reason: &str) -> Result<T, String> {
        let line = self.bytes[..self.pos.min(self.bytes.len())].iter().filter(|b| **b == b'\n').count() + 1;
        Err(format!("invalid TOML at line {line}: {reason}"))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            if self.peek() == Some(b'\r') && self.bytes.get(self.pos + 1) == Some(&b'\n') {
                break;
            }
            self.pos += 1;
        }
    }

    /// Consumes one line ending when present; false at a character that is not a line ending.
    fn eat_newline(&mut self) -> bool {
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                true
            }
            Some(b'\r') if self.bytes.get(self.pos + 1) == Some(&b'\n') => {
                self.pos += 2;
                true
            }
            _ => false,
        }
    }

    fn key_part(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(quote @ (b'"' | b'\'')) => {
                let start = self.pos + 1;
                self.quoted_single_line(quote)?;
                Ok(String::from_utf8_lossy(&self.bytes[start..self.pos - 1]).into_owned())
            }
            _ => {
                let start = self.pos;
                while matches!(self.peek(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')) {
                    self.pos += 1;
                }
                if self.pos == start {
                    return self.fail("expected a key");
                }
                Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
            }
        }
    }

    fn key_path(&mut self) -> Result<String, String> {
        let mut path = String::new();
        loop {
            self.skip_blanks();
            path.push_str(&self.key_part()?);
            self.skip_blanks();
            if self.peek() != Some(b'.') {
                return Ok(path);
            }
            self.pos += 1;
            path.push('.');
        }
    }

    /// A single-line quoted string starting at the opening quote; escapes only apply to `"`.
    fn quoted_single_line(&mut self, quote: u8) -> Result<(), String> {
        self.pos += 1;
        loop {
            match self.peek() {
                None | Some(b'\n') | Some(b'\r') => return self.fail("unterminated string"),
                Some(byte) if byte == quote => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') if quote == b'"' => self.pos += 2,
                Some(_) => self.pos += 1,
            }
        }
    }

    fn quoted_multi_line(&mut self, quote: u8) -> Result<(), String> {
        self.pos += 3;
        loop {
            match self.peek() {
                None => return self.fail("unterminated multi-line string"),
                Some(b'\\') if quote == b'"' => self.pos += 2,
                Some(byte) if byte == quote && self.bytes[self.pos..].starts_with(&[quote; 3]) => {
                    self.pos += 3;
                    // Up to two further quotes belong to the content.
                    for _ in 0..2 {
                        if self.peek() == Some(quote) {
                            self.pos += 1;
                        }
                    }
                    return Ok(());
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn scalar(&mut self) -> Result<(), String> {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-' | b'.' | b':') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return self.fail("missing or unreadable value");
        }
        let token = &self.bytes[start..self.pos];
        let unsigned = token.strip_prefix(b"+").or_else(|| token.strip_prefix(b"-")).unwrap_or(token);
        let is_word = matches!(token, b"true" | b"false") || matches!(unsigned, b"inf" | b"nan");
        if is_word || unsigned.first().is_some_and(u8::is_ascii_digit) {
            return Ok(());
        }
        self.fail("unreadable value")
    }

    fn skip_array_filler(&mut self) {
        loop {
            self.skip_blanks();
            match self.peek() {
                Some(b'#') => self.skip_comment(),
                _ if self.eat_newline() => {}
                _ => return,
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return self.fail("nesting too deep");
        }
        match self.peek() {
            None | Some(b'\n') | Some(b'\r') | Some(b'#') => self.fail("missing value"),
            Some(quote @ (b'"' | b'\'')) => {
                if self.bytes[self.pos..].starts_with(&[quote; 3]) {
                    self.quoted_multi_line(quote)
                } else {
                    self.quoted_single_line(quote)
                }
            }
            Some(b'[') => self.array(depth),
            Some(b'{') => self.inline_table(depth),
            Some(_) => self.scalar(),
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), String> {
        self.pos += 1;
        loop {
            self.skip_array_filler();
            if self.peek() == Some(b']') {
                self.pos += 1;
                return Ok(());
            }
            self.value(depth + 1)?;
            self.skip_array_filler();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    fn inline_table(&mut self, depth: usize) -> Result<(), String> {
        self.pos += 1;
        self.skip_blanks();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.key_path()?;
            self.skip_blanks();
            if self.peek() != Some(b'=') {
                return self.fail("expected '='");
            }
            self.pos += 1;
            self.skip_blanks();
            self.value(depth + 1)?;
            self.skip_blanks();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }

    /// Everything after a statement on its line: blanks, an optional comment, then the line end.
    fn finish_line(&mut self) -> Result<bool, String> {
        self.skip_blanks();
        let has_comment = self.peek() == Some(b'#');
        if has_comment {
            self.skip_comment();
        }
        if self.peek().is_none() || self.eat_newline() {
            return Ok(has_comment);
        }
        self.fail("unexpected characters after the value")
    }

    fn table_header(&mut self) -> Result<String, String> {
        let is_array = self.bytes.get(self.pos + 1) == Some(&b'[');
        self.pos += if is_array { 2 } else { 1 };
        let path = self.key_path()?;
        for _ in 0..if is_array { 2 } else { 1 } {
            if self.peek() != Some(b']') {
                return self.fail("unterminated table header");
            }
            self.pos += 1;
        }
        self.finish_line()?;
        Ok(if is_array { format!("[[{path}]]") } else { format!("[{path}]") })
    }
}

fn detect_line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(index) if index > 0 && text.as_bytes()[index - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

fn scan_toml_document(text: &str) -> Result<TomlDocument, String> {
    let mut scanner = TomlScanner { bytes: text.as_bytes(), pos: 0 };
    let mut document = TomlDocument {
        entries: Vec::new(),
        first_table_start: None,
        line_ending: detect_line_ending(text),
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut table = String::new();
    let mut array_tables = 0usize;
    loop {
        scanner.skip_blanks();
        let Some(byte) = scanner.peek() else {
            return Ok(document);
        };
        if byte == b'#' {
            scanner.skip_comment();
            continue;
        }
        if scanner.eat_newline() {
            continue;
        }
        let line_start = line_start_of(text, scanner.pos);
        if byte == b'[' {
            let header = scanner.table_header()?;
            document.first_table_start.get_or_insert(line_start);
            if header.starts_with("[[") {
                array_tables += 1;
                table = format!("{header}#{array_tables}");
            } else if !seen.insert(header.clone()) {
                scanner.pos = line_start;
                return scanner.fail("table defined more than once");
            } else {
                table = header;
            }
            continue;
        }
        let path = scanner.key_path()?;
        scanner.skip_blanks();
        if scanner.peek() != Some(b'=') {
            return scanner.fail("expected '='");
        }
        scanner.pos += 1;
        scanner.skip_blanks();
        let value_start = scanner.pos;
        scanner.value(0)?;
        let value_end = scanner.pos;
        let has_comment = scanner.finish_line()?;
        if !seen.insert(format!("{table}\u{0}{path}")) {
            scanner.pos = line_start;
            return scanner.fail("key defined more than once");
        }
        if document.first_table_start.is_none() {
            document.entries.push(TomlEntry {
                path,
                line_start,
                line_end: scanner.pos,
                value_start,
                value_end,
                has_comment,
            });
        }
    }
}

fn line_start_of(text: &str, position: usize) -> usize {
    text[..position].rfind('\n').map_or(0, |index| index + 1)
}

/// Parses a TOML integer token into a `u64`.
fn toml_integer(raw: &str, key: &str) -> Result<u64, String> {
    let not_integer = || format!("`{key}` is not an integer");
    let (negative, unsigned) = match raw.as_bytes().first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    let (radix, digits) = match unsigned.get(..2) {
        Some("0x") if !negative && raw.len() > 2 => (16, &unsigned[2..]),
        Some("0o") if !negative && raw.len() > 2 => (8, &unsigned[2..]),
        Some("0b") if !negative && raw.len() > 2 => (2, &unsigned[2..]),
        _ => (10, unsigned),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c == '_' || c.is_digit(radix));
    if !well_formed {
        return Err(not_integer());
    }
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    let value = u64::from_str_radix(&cleaned, radix).map_err(|_| not_integer())?;
    if negative && value != 0 {
        return Err(format!("`{key}` is not a non-negative integer"));
    }
    Ok(value)
}

/// Reads the top-level integer `key` of a TOML document; `Ok(None)` when absent.
pub fn parse_toml_setting(text: &str, key: &str) -> Result<Option<u64>, String> {
    let document = scan_toml_document(text)?;
    match document.entries.iter().find(|entry| entry.path == key) {
        Some(entry) => toml_integer(&text[entry.value_start..entry.value_end], key).map(Some),
        None => Ok(None),
    }
}

fn edit_toml_text(text: &str, key: &str, change: KeyChange) -> Result<String, String> {
    let document = scan_toml_document(text)?;
    let existing = document.entries.iter().find(|entry| entry.path == key);
    match (change, existing) {
        (KeyChange::Set(value), Some(entry)) => {
            toml_integer(&text[entry.value_start..entry.value_end], key)?;
            Ok(splice(text, entry.value_start, entry.value_end, &value.to_string()))
        }
        (KeyChange::Set(value), None) => Ok(insert_toml_line(text, &document, key, value)),
        (KeyChange::Remove, Some(entry)) => remove_toml_line(text, entry, key),
        (KeyChange::Remove, None) => Ok(text.to_string()),
    }
}

/// Offset where a new top-level line goes: after the last top-level entry, else before the first
/// table together with the comment lines directly above it, else at the end of the file.
fn toml_insertion_point(text: &str, document: &TomlDocument) -> usize {
    if let Some(last) = document.entries.last() {
        return last.line_end;
    }
    let Some(mut point) = document.first_table_start else {
        return text.len();
    };
    while point > 0 {
        let previous_start = line_start_of(text, point - 1);
        if !text[previous_start..point].trim_start().starts_with('#') {
            break;
        }
        point = previous_start;
    }
    point
}

fn insert_toml_line(text: &str, document: &TomlDocument, key: &str, value: u64) -> String {
    let point = toml_insertion_point(text, document);
    let ending = document.line_ending;
    let line = format!("{key} = {value}");
    let insertion = if point < text.len() {
        format!("{line}{ending}")
    } else if text.is_empty() {
        line
    } else if text.ends_with('\n') {
        format!("{line}{ending}")
    } else {
        format!("{ending}{line}")
    };
    splice(text, point, point, &insertion)
}

fn remove_toml_line(text: &str, entry: &TomlEntry, key: &str) -> Result<String, String> {
    if entry.has_comment {
        return Err(format!(
            "`{key}` line carries a comment; remove the line by hand to keep the comment"
        ));
    }
    let mut start = entry.line_start;
    // A last line without a line ending takes the preceding line ending with it.
    let is_unterminated = !text[..entry.line_end].ends_with('\n');
    if is_unterminated && start > 0 {
        start -= if text[..start].ends_with("\r\n") { 2 } else { 1 };
    }
    Ok(splice(text, start, entry.line_end, ""))
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// safety rules: "text outside the markers is kept". Unrelated keys keep their exact bytes.
    #[test]
    fn json_set_keeps_unrelated_keys_verbatim() {
        let text = "{\n  \"model\": \"opus\",\n  \"theme\":   \"dark\"\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(250_000)).unwrap();
        assert!(edited.contains("\"model\": \"opus\""));
        assert!(edited.contains("\"theme\":   \"dark\""));
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(250_000)
        );
    }

    /// the top-level key. An existing key is replaced in place, not duplicated.
    #[test]
    fn json_set_replaces_existing_value_without_duplicating_key() {
        let text = "{\n  \"autoCompactWindow\": 100\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(200)).unwrap();
        assert_eq!(edited.matches("autoCompactWindow").count(), 1);
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(200)
        );
    }

    /// the top-level key, and the other keys stay.
    #[test]
    fn json_remove_drops_key_and_keeps_other_keys() {
        let text = "{\n  \"autoCompactWindow\": 100,\n  \"model\": \"opus\"\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Remove).unwrap();
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            None
        );
        assert!(edited.contains("\"model\": \"opus\""));
    }

    /// top-level key is targeted. A nested key of the same name keeps its value and text.
    #[test]
    fn json_set_does_not_touch_nested_key_of_same_name() {
        let text = "{\n  \"env\": {\"autoCompactWindow\": 1}\n}\n";
        let edited = edit_json_text(text, "autoCompactWindow", KeyChange::Set(5)).unwrap();
        assert!(edited.contains("\"env\": {\"autoCompactWindow\": 1}"));
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(5)
        );
    }

    /// the key is absent.
    #[test]
    fn parse_setting_absent_key_is_none() {
        let text = "{\n  \"model\": \"opus\"\n}\n";
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, text).unwrap(),
            None
        );
    }

    /// settings object (no keys yet) accepts a `Set` and reads it back.
    #[test]
    fn config_text_set_on_empty_object_adds_claude_key() {
        let edited = edit_config_text(
            ConfigTarget::ClaudeAutoCompactWindow,
            "{}",
            KeyChange::Set(1),
        )
        .unwrap();
        assert_eq!(
            parse_setting(ConfigTarget::ClaudeAutoCompactWindow, &edited).unwrap(),
            Some(1)
        );
    }

    /// top-level integer key of a TOML document.
    #[test]
    fn parse_toml_reads_top_level_integer() {
        let text = "model = \"x\"\nauto_compact_token_limit = 180000\n\n[profiles.fast]\nauto_compact_token_limit = 1\n";
        assert_eq!(
            parse_toml_setting(text, "auto_compact_token_limit").unwrap(),
            Some(180_000)
        );
    }

    /// top-level table counts. A same-named key inside `[profiles.fast]` is not the setting.
    #[test]
    fn parse_toml_ignores_table_scoped_key() {
        let text = "[profiles.fast]\nauto_compact_token_limit = 1\n";
        assert_eq!(
            parse_toml_setting(text, "auto_compact_token_limit").unwrap(),
            None
        );
    }

    /// `u64`, so the parse returns `Err` instead of a made-up number.
    #[test]
    fn parse_toml_non_integer_value_is_error() {
        let text = "auto_compact_token_limit = \"big\"\n";
        assert!(parse_toml_setting(text, "auto_compact_token_limit").is_err());
    }

    /// absent key.
    #[test]
    fn parse_setting_malformed_json_is_error() {
        assert!(parse_setting(ConfigTarget::ClaudeAutoCompactWindow, "{ not json").is_err());
    }
}
