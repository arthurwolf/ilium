//! Classification of tool calls into the compact rework features.
//!
//! Everything here turns text (a path, a search argument, a shell command)
//! into counts, flags and 32-bit hashes; the text itself is dropped.
//!
//! Rules (ported from the research scripts, `extract.py` of both agents):
//!
//! * file-read-like calls: Claude `Read`, `Grep`, `Glob`; shell commands with
//!   a segment starting with `cat`, `head`, `tail`, `nl`, `bat`, `less`,
//!   `sed -n`, `rg`, `grep`, `ls`, `find`, `tree`, `fd`, `wc` or `git
//!   log|show|diff|status|blame`. A command counts once however many segments
//!   read.
//! * hashed read arguments: the file paths of the path readers (`cat`,
//!   `head`, `tail`, `nl`, `bat`, `less`, `sed -n`) and `Read`, resolved
//!   against the working directory; searches and listings are hashed whole.
//! * edits: Claude `Edit`/`Write`/`MultiEdit`/`NotebookEdit`, Codex
//!   `apply_patch`; shell writes (heredoc, `sed -i`, `tee`, `>` redirection,
//!   `git apply|commit`, `patch`) are flagged separately as a heuristic.
//! * command hashes: whitespace-normalised commands of at least 20
//!   characters that are not trivial (`ls`, `pwd`, `git status`...).

use crate::tool_features::{
    ToolAccumulator, HASH_KIND_MASK, HASH_KIND_PATH, HASH_KIND_SEARCH, TOOL_FLAG_EDIT,
    TOOL_FLAG_WRITE_HEURISTIC,
};
use crate::util::fnv1a_64;

/// Commands shorter than this are never hashed for duplicate detection.
const MIN_COMMAND_HASH_CHARACTERS: usize = 20;

/// Command prefixes too trivial to count as a repeated command.
const TRIVIAL_COMMANDS: [&str; 10] = [
    "ls ",
    "pwd",
    "git status",
    "git diff --stat",
    "echo ",
    "date",
    "sleep ",
    "true",
    "which ",
    "cd ",
];

/// Tools that edit through a dedicated tool call.
const EDIT_TOOLS: [&str; 5] = ["Edit", "Write", "MultiEdit", "NotebookEdit", "apply_patch"];

/// 32-bit hash with the kind in the two low bits.
pub(crate) fn hash_with_kind(bytes: &[u8], kind: u32) -> u32 {
    let wide = fnv1a_64(bytes);
    let folded = (wide >> 32) as u32 ^ wide as u32;
    (folded & !HASH_KIND_MASK) | (kind & HASH_KIND_MASK)
}

/// Makes `token` absolute against `cwd` and normalises `.` and `..`. `~/`
/// paths and absolute paths are kept as written.
pub(crate) fn absolute_path(token: &str, cwd: Option<&str>) -> String {
    let joined = if token.starts_with('/') || token.starts_with('~') {
        token.to_string()
    } else {
        format!("{}/{}", cwd.unwrap_or("/"), token)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Records a tool call that is not a shell command.
pub(crate) fn add_claude_tool(
    accumulator: &mut ToolAccumulator,
    name: &str,
    file_path: Option<&str>,
    search: [Option<&str>; 3],
    command: Option<&str>,
    cwd: Option<&str>,
) {
    accumulator.tool_calls = accumulator.tool_calls.saturating_add(1);
    match name {
        "Read" => {
            accumulator.read_calls = accumulator.read_calls.saturating_add(1);
            if let Some(path) = file_path {
                let absolute = absolute_path(path, cwd);
                accumulator.push_read_hash(hash_with_kind(absolute.as_bytes(), HASH_KIND_PATH));
            }
        }
        "Grep" | "Glob" => {
            accumulator.read_calls = accumulator.read_calls.saturating_add(1);
            let joined = format!(
                "{name}\u{1}{}\u{1}{}\u{1}{}",
                search[0].unwrap_or(""),
                search[1].unwrap_or(""),
                search[2].unwrap_or("")
            );
            accumulator.push_read_hash(hash_with_kind(joined.as_bytes(), HASH_KIND_SEARCH));
        }
        "Bash" => {
            if let Some(command) = command {
                add_command(accumulator, command, cwd, false);
            }
        }
        other if EDIT_TOOLS.contains(&other) => accumulator.flags |= TOOL_FLAG_EDIT,
        _ => {}
    }
}

/// Records a shell command (Claude `Bash`, Codex exec). The caller counts the
/// enclosing tool call. `search_paths_are_reads` makes the path-like arguments
/// of `rg`/`grep`/`ls`/`find` count as read paths too (the Codex research
/// did).
pub(crate) fn add_command(
    accumulator: &mut ToolAccumulator,
    command: &str,
    cwd: Option<&str>,
    search_paths_are_reads: bool,
) {
    accumulator.command_calls = accumulator.command_calls.saturating_add(1);
    let normalised = normalise_whitespace(command);
    if normalised.len() >= MIN_COMMAND_HASH_CHARACTERS && !is_trivial(&normalised) {
        accumulator.push_command_hash(hash_with_kind(normalised.as_bytes(), 0));
    }
    if looks_like_write(command) {
        accumulator.flags |= TOOL_FLAG_WRITE_HEURISTIC;
    }
    let mut read_like = false;
    for segment in command.split([';', '&', '|', '(', '\n']) {
        let tokens = tokenize(segment);
        let Some(kind) = classify_segment(&tokens) else {
            continue;
        };
        read_like = true;
        match kind {
            SegmentKind::PathRead { skip_script } => {
                for path in path_arguments(&tokens, skip_script, false) {
                    let absolute = absolute_path(&path, cwd);
                    accumulator.push_read_hash(hash_with_kind(absolute.as_bytes(), HASH_KIND_PATH));
                }
            }
            SegmentKind::Search { skip_pattern } => {
                let whole = normalise_whitespace(segment);
                accumulator.push_read_hash(hash_with_kind(whole.as_bytes(), HASH_KIND_SEARCH));
                if search_paths_are_reads {
                    for path in path_arguments(&tokens, skip_pattern, true) {
                        let absolute = absolute_path(&path, cwd);
                        accumulator
                            .push_read_hash(hash_with_kind(absolute.as_bytes(), HASH_KIND_PATH));
                    }
                }
            }
        }
    }
    if read_like {
        accumulator.read_calls = accumulator.read_calls.saturating_add(1);
    }
}

/// Records an `apply_patch` (or a code block that calls it).
pub(crate) fn add_edit(accumulator: &mut ToolAccumulator) {
    accumulator.flags |= TOOL_FLAG_EDIT;
}

enum SegmentKind {
    /// `cat`, `head`, `tail`, `nl`, `bat`, `less`, `sed -n`; `skip_script`
    /// drops the first non-flag argument (the sed script).
    PathRead { skip_script: bool },
    /// `rg`, `grep`, `ls`, `find`, `tree`, `fd`, `wc`, `git log|...`;
    /// `skip_pattern` drops the first non-flag argument for `rg`/`grep`.
    Search { skip_pattern: bool },
}

fn classify_segment(tokens: &[String]) -> Option<SegmentKind> {
    let first = tokens.first()?.as_str();
    match first {
        "cat" | "head" | "tail" | "nl" | "bat" | "less" => {
            Some(SegmentKind::PathRead { skip_script: false })
        }
        "sed" if tokens.iter().skip(1).any(|token| is_sed_quiet_flag(token)) => {
            Some(SegmentKind::PathRead { skip_script: true })
        }
        "rg" | "grep" => Some(SegmentKind::Search { skip_pattern: true }),
        "ls" | "find" | "tree" | "fd" | "wc" => Some(SegmentKind::Search {
            skip_pattern: false,
        }),
        "git"
            if tokens.get(1).is_some_and(|sub| {
                matches!(sub.as_str(), "log" | "show" | "diff" | "status" | "blame")
            }) =>
        {
            Some(SegmentKind::Search {
                skip_pattern: false,
            })
        }
        _ => None,
    }
}

fn is_sed_quiet_flag(token: &str) -> bool {
    token == "-n" || (token.starts_with('-') && !token.starts_with("--") && token.contains('n'))
}

/// Path-like arguments of a segment. `strict` keeps only tokens that look like
/// paths (a slash, a `~`, or a short extension).
fn path_arguments(tokens: &[String], skip_first_argument: bool, strict: bool) -> Vec<String> {
    let mut skipped = !skip_first_argument;
    let mut paths = Vec::new();
    for token in tokens.iter().skip(1) {
        if token.starts_with('-') || token.is_empty() {
            continue;
        }
        if !skipped {
            skipped = true;
            continue;
        }
        if is_line_range(token) {
            continue;
        }
        if !strict || looks_like_path(token) {
            paths.push(token.clone());
        }
    }
    paths
}

/// `10`, `10,20p`, `$p`, `1,$p`: sed addresses and head/tail counts.
fn is_line_range(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, ',' | '$' | 'p' | '+')
        })
}

fn looks_like_path(token: &str) -> bool {
    if token.contains('/') || token.starts_with('~') {
        return true;
    }
    match token.rsplit_once('.') {
        Some((stem, extension)) => {
            !stem.is_empty()
                && (1..=6).contains(&extension.len())
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// Splits on whitespace honouring single and double quotes (no escapes beyond
/// a backslash inside double quotes).
fn tokenize(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    let mut characters = segment.chars().peekable();
    while let Some(character) = characters.next() {
        match quote {
            Some(delimiter) if character == delimiter => quote = None,
            Some('"') if character == '\\' => {
                if let Some(escaped) = characters.next() {
                    current.push(escaped);
                }
            }
            Some(_) => current.push(character),
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                in_token = true;
            }
            None if character.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            None => {
                current.push(character);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

fn normalise_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_trivial(normalised: &str) -> bool {
    TRIVIAL_COMMANDS.iter().any(|prefix| {
        let word = prefix.trim_end();
        normalised.strip_prefix(word).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
        })
    })
}

/// Heuristic: the command writes files through the shell.
fn looks_like_write(command: &str) -> bool {
    if command.contains("sed -i")
        || command.contains("git apply")
        || command.contains("git commit")
        || command.contains("<<")
    {
        return true;
    }
    if command
        .split([';', '&', '|', '\n'])
        .any(|segment| matches!(segment.split_whitespace().next(), Some("tee" | "patch")))
    {
        return true;
    }
    has_file_redirect(command)
}

/// A `>` or `>>` redirect to something other than `&n` or `/dev/null`.
fn has_file_redirect(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'>' {
            index += 1;
            continue;
        }
        let previous = index.checked_sub(1).map(|at| bytes[at]);
        let is_fd_or_arrow = previous
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'&' | b'-' | b'='));
        let mut after = index + 1;
        if bytes.get(after) == Some(&b'>') {
            after += 1;
        }
        while bytes.get(after) == Some(&b' ') {
            after += 1;
        }
        let target = &command[after.min(command.len())..];
        let to_descriptor = target.starts_with('&');
        let to_null = target.starts_with("/dev/null");
        if !is_fd_or_arrow && !to_descriptor && !to_null && !target.is_empty() {
            return true;
        }
        index = after.max(index + 1);
    }
    false
}

/// Extracts the `cmd: "..."` string literals of a Codex `exec` code block
/// (`text(await tools.exec_command({cmd:"rg -n foo src", ...}))`).
pub(crate) fn extract_exec_commands(code: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let bytes = code.as_bytes();
    let mut search_from = 0;
    while let Some(relative) = code[search_from..].find("cmd") {
        let start = search_from + relative;
        search_from = start + 3;
        let boundary_before = start == 0 || !is_identifier_byte(bytes[start - 1]);
        if !boundary_before {
            continue;
        }
        let mut cursor = start + 3;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b':') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let Some(&quote) = bytes.get(cursor) else {
            continue;
        };
        if !matches!(quote, b'"' | b'\'' | b'`') {
            continue;
        }
        if let Some((literal, end)) = read_string_literal(code, cursor, quote) {
            commands.push(literal);
            search_from = end;
        }
    }
    commands
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Reads a JS string literal starting at `open` (the quote byte); returns the
/// decoded text and the index after the closing quote.
fn read_string_literal(code: &str, open: usize, quote: u8) -> Option<(String, usize)> {
    let bytes = code.as_bytes();
    let mut cursor = open + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            byte if byte == quote => {
                let raw = &code[open + 1..cursor];
                let decoded = if quote == b'"' {
                    serde_json::from_str::<String>(&code[open..=cursor])
                        .unwrap_or_else(|_| raw.to_string())
                } else {
                    raw.to_string()
                };
                return Some((decoded, cursor + 1));
            }
            _ => cursor += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(text: &str, cwd: Option<&str>) -> ToolAccumulator {
        let mut accumulator = ToolAccumulator::default();
        add_command(&mut accumulator, text, cwd, true);
        accumulator
    }

    #[test]
    fn path_readers_hash_resolved_paths_and_count_once_per_command() {
        let first = command("cat src/lib.rs src/main.rs", Some("/work/project"));
        assert_eq!(first.read_calls, 1);
        assert_eq!(first.read_hashes.len(), 2);
        let again = command("head -n 20 /work/project/src/lib.rs", None);
        assert_eq!(again.read_hashes[0], first.read_hashes[0]);
        let sed = command("sed -n '10,40p' src/lib.rs", Some("/work/project"));
        assert_eq!(sed.read_hashes, vec![first.read_hashes[0]]);
        assert!(first
            .read_hashes
            .iter()
            .all(|hash| hash & HASH_KIND_MASK == HASH_KIND_PATH));
    }

    #[test]
    fn searches_are_hashed_whole_and_only_codex_counts_their_paths() {
        let mut claude = ToolAccumulator::default();
        add_command(&mut claude, "rg -n needle src/lib.rs", Some("/w"), false);
        assert_eq!(claude.read_calls, 1);
        assert_eq!(claude.read_hashes.len(), 1);
        assert_eq!(claude.read_hashes[0] & HASH_KIND_MASK, HASH_KIND_SEARCH);
        let codex = command("rg -n needle src/lib.rs", Some("/w"));
        assert_eq!(codex.read_hashes.len(), 2);
        let direct = command("cat src/lib.rs", Some("/w"));
        assert_eq!(codex.read_hashes[1], direct.read_hashes[0]);
    }

    #[test]
    fn non_reading_commands_count_no_read_but_trivial_ones_are_not_hashed() {
        let build = command("cargo build --release --workspace", None);
        assert_eq!((build.read_calls, build.command_calls), (0, 1));
        assert_eq!(build.command_hashes.len(), 1);
        let trivial = command("git status", None);
        assert!(trivial.command_hashes.is_empty());
        assert_eq!(trivial.read_calls, 1);
        // Identical commands hash identically regardless of spacing.
        let spaced = command("cargo   build --release  --workspace", None);
        assert_eq!(spaced.command_hashes, build.command_hashes);
    }

    #[test]
    fn write_heuristics_are_flagged_separately_from_edit_tools() {
        for writer in [
            "sed -i 's/a/b/' file.txt",
            "cat <<'EOF' > notes.md",
            "echo hi > out.txt",
            "printf x | tee out.txt",
            "git apply fix.patch",
        ] {
            assert!(
                command(writer, None).flags & TOOL_FLAG_WRITE_HEURISTIC != 0,
                "{writer}"
            );
        }
        for reader in [
            "cargo test 2>&1",
            "ls > /dev/null",
            "grep -rn a src | head",
            "x=$((1+2))",
        ] {
            assert_eq!(
                command(reader, None).flags & TOOL_FLAG_WRITE_HEURISTIC,
                0,
                "{reader}"
            );
        }
        let mut accumulator = ToolAccumulator::default();
        add_claude_tool(&mut accumulator, "Edit", Some("/a"), [None; 3], None, None);
        assert_eq!(accumulator.flags, TOOL_FLAG_EDIT);
        assert_eq!(accumulator.tool_calls, 1);
    }

    #[test]
    fn claude_read_grep_and_glob_tools() {
        let mut accumulator = ToolAccumulator::default();
        add_claude_tool(
            &mut accumulator,
            "Read",
            Some("/w/a.rs"),
            [None; 3],
            None,
            None,
        );
        add_claude_tool(
            &mut accumulator,
            "Grep",
            None,
            [Some("needle"), Some("/w"), None],
            None,
            None,
        );
        add_claude_tool(
            &mut accumulator,
            "Glob",
            None,
            [Some("**/*.rs"), None, None],
            None,
            None,
        );
        add_claude_tool(&mut accumulator, "WebFetch", None, [None; 3], None, None);
        assert_eq!((accumulator.tool_calls, accumulator.read_calls), (4, 3));
        assert_eq!(accumulator.read_hashes[0] & HASH_KIND_MASK, HASH_KIND_PATH);
        assert_eq!(
            accumulator.read_hashes[1] & HASH_KIND_MASK,
            HASH_KIND_SEARCH
        );
        let again = {
            let mut other = ToolAccumulator::default();
            add_claude_tool(
                &mut other,
                "Read",
                Some("/w/./sub/../a.rs"),
                [None; 3],
                None,
                None,
            );
            other
        };
        assert_eq!(again.read_hashes[0], accumulator.read_hashes[0]);
    }

    #[test]
    fn exec_code_commands_are_extracted() {
        let code = r#"text(await tools.exec_command({cmd:"pwd; rg -n 'a b' src/x.rs",max_output_tokens:2500}));
text(await tools.exec_command({ cmd : 'cat "q.txt"' }));
const cmdline = "not a command"; // identifier boundary
text(await tools.exec_command({cmd:"printf \"x\\ny\""}));"#;
        let commands = extract_exec_commands(code);
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0], "pwd; rg -n 'a b' src/x.rs");
        assert_eq!(commands[1], "cat \"q.txt\"");
        assert_eq!(commands[2], "printf \"x\\ny\"");
    }

    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        assert_eq!(absolute_path("a/../b.rs", Some("/w/p")), "/w/p/b.rs");
        assert_eq!(absolute_path("/x/./y", Some("/w")), "/x/y");
        assert_eq!(absolute_path("f.txt", None), "/f.txt");
    }
}
