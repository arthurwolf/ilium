//! Shared helpers of the ABI tests: manual and wit/ locations, a small WIT member scanner,
//! hashing, and manifest fixtures.

use std::path::{Path, PathBuf};

use ilium_plugin_api::manifest::{CheckMode, Diagnostic, ValidationContext};
use ilium_plugin_api::{Platform, Tier};
use sha2::{Digest, Sha256};

/// The canonical manual pages: `ILIUM_PLUGIN_MANUAL_DIR`, else `<repo>/docs/plugins-manual/pages`.
pub fn manual_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("ILIUM_PLUGIN_MANUAL_DIR") {
        return PathBuf::from(directory);
    }
    crate_dir().join("../docs/plugins-manual/pages")
}

/// The `ilium-plugin-api` crate directory.
pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The extracted `wit/` tree at the repository root.
pub fn wit_dir() -> PathBuf {
    crate_dir().join("../wit")
}

/// Reads a UTF-8 file, naming the path when it is missing.
pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Reads one manual page by file name, for example `reference-wit.md`.
pub fn manual_page(name: &str) -> String {
    read(&manual_dir().join(name))
}

/// Every manual page as (file name, text), sorted by name.
pub fn manual_pages() -> Vec<(String, String)> {
    let directory = manual_dir();
    let mut pages: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("list {}: {error}", directory.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (name, read(&path))
        })
        .collect();
    pages.sort();
    pages
}

/// Every `.wit` file under wit/ as (relative POSIX path, text), sorted by path.
pub fn wit_files() -> Vec<(String, String)> {
    fn walk(root: &Path, directory: &Path, found: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("list {}: {error}", directory.display()));
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, found);
            } else if path.extension().is_some_and(|extension| extension == "wit") {
                let relative = path
                    .strip_prefix(root)
                    .map(|relative| {
                        relative
                            .components()
                            .map(|part| part.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();
                found.push((relative, read(&path)));
            }
        }
    }
    let root = wit_dir();
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found.sort();
    found
}

/// Lower-case hex sha256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The wit/ tree hash of tools/gen_plugin_api.py: for each file in path order, the relative
/// path, NUL, the bytes, NUL.
pub fn wit_tree_sha256() -> String {
    let mut hasher = Sha256::new();
    for (relative, text) in wit_files() {
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        hasher.update(text.as_bytes());
        hasher.update([0u8]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Removes `//` comments (doc comments included) up to the end of each line.
pub fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One record, enum, variant or flags definition with its member names in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Enclosing `interface` (or `world`), empty in an excerpt without one.
    pub interface: String,
    /// `record`, `enum`, `variant` or `flags`.
    pub kind: String,
    /// The type name, `%` removed.
    pub name: String,
    /// Field or case names in order, `%` removed.
    pub members: Vec<String>,
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-' || character == '%'
}

/// Tokens: words of `[%A-Za-z0-9_-]` and single other non-space characters, with byte offsets.
fn tokens(text: &str) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < characters.len() {
        let (start, character) = characters[index];
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if is_word_char(character) {
            let mut end_index = index;
            while end_index < characters.len() && is_word_char(characters[end_index].1) {
                end_index += 1;
            }
            let end = characters
                .get(end_index)
                .map(|(offset, _)| *offset)
                .unwrap_or(text.len());
            found.push((text[start..end].to_owned(), start, end));
            index = end_index;
        } else {
            found.push((character.to_string(), start, start + character.len_utf8()));
            index += 1;
        }
    }
    found
}

/// Splits a definition body on top-level commas, ignoring commas inside `<>` and `()`.
fn member_names(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for character in body.chars() {
        match character {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            _ => {}
        }
        if character == ',' && depth == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    parts.push(current);
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let name: String = part.chars().take_while(|c| is_word_char(*c)).collect();
            let name = name.trim_start_matches('%').to_owned();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

/// Every record, enum, variant and flags definition of a WIT text, in order. Mirrors the
/// one-off script that wrote golden/case_order.txt.
pub fn definitions(text: &str) -> Vec<Definition> {
    let text = strip_comments(text);
    let tokens = tokens(&text);
    let mut found = Vec::new();
    let mut interface = String::new();
    let mut index = 0;
    while index < tokens.len() {
        let word = tokens[index].0.as_str();
        let opens_block = tokens.get(index + 2).is_some_and(|token| token.0 == "{");
        if (word == "interface" || word == "world") && opens_block {
            interface = tokens[index + 1].0.clone();
            index += 3;
            continue;
        }
        if matches!(word, "record" | "enum" | "variant" | "flags") && opens_block {
            let body_start = tokens[index + 2].2;
            let body_end = text[body_start..]
                .find('}')
                .map(|offset| body_start + offset)
                .unwrap_or(text.len());
            found.push(Definition {
                interface: interface.clone(),
                kind: word.to_owned(),
                name: tokens[index + 1].0.trim_start_matches('%').to_owned(),
                members: member_names(&text[body_start..body_end]),
            });
            while index < tokens.len() && tokens[index].1 < body_end {
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    found
}

/// Every fenced ```wit block of a Markdown page.
pub fn wit_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in page.lines() {
        match current.as_mut() {
            None if line.trim_start() == "```wit" => current = Some(Vec::new()),
            None => {}
            Some(lines) if line.trim_start().starts_with("```") => {
                blocks.push(lines.join("\n"));
                current = None;
            }
            Some(lines) => lines.push(line),
        }
    }
    blocks
}

/// The minimal valid manifest of the ABI tests (server side only).
pub fn minimal_manifest() -> serde_json::Value {
    serde_json::json!({
        "api": "ilium:plugin@0.1.0",
        "id": "example.abi",
        "name": "ABI test",
        "version": "0.1.0",
        "publisher": "Example",
        "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0" }
    })
}

/// A validation context for `ilium plugin check` on Linux with nothing installed.
pub fn context(tier: Tier) -> ValidationContext {
    ValidationContext {
        tier,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: ilium_plugin_api::abi::UNSTABLE_FEATURES.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// The codes of a diagnostic list, in order.
pub fn codes(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}
