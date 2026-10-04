//! Fine-grained Smart Copy region detectors.
//!
//! [`crate::smart_copy`] finds structural regions (blocks, tables, sections).
//! This module finds the small things inside them: URLs, file paths, qualified
//! names, addresses, styled runs and so on. Regions are allowed to nest, so a
//! URL inside a sentence inside a paragraph is selectable at every level;
//! hover picks the smallest region under the pointer.
//!
//! Everything here is a pure function of text or of a frozen `vt100::Screen`.
//! Filesystem existence checks are not performed here: path-shaped tokens
//! carry a [`PathProbe`] that the UI thread resolves with a [`PathContext`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

/// One detected token inside a single line. Offsets are character indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMatch {
    pub start: usize,
    pub end: usize,
    pub kind: &'static str,
    pub label: String,
    pub probe: Option<PathProbe>,
}

/// A path-shaped token whose existence the UI thread may check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathProbe {
    pub path: String,
    /// Keep the candidate even when the path cannot be found on disk.
    pub plausible_unverified: bool,
}

/// A postal address, possibly spanning several rows. Rows are `(row,
/// start_char, end_char)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostalMatch {
    pub rows: Vec<(usize, usize, usize)>,
    pub kind: &'static str,
    pub label: String,
}

const FILE_EXTENSIONS: &str = "rs|toml|lock|md|mdx|txt|json|jsonl|yaml|yml|ts|tsx|js|jsx|mjs|cjs|vue|svelte|py|pyi|go|mod|sum|c|h|cc|cpp|hpp|cxx|hxx|java|kt|kts|swift|rb|php|sh|zsh|bash|fish|ps1|bat|css|scss|sass|less|html|htm|sql|xml|csv|tsv|log|conf|cfg|ini|env|lua|zig|cs|dart|ex|exs|erl|hs|ml|nix|svg|png|jpg|jpeg|gif|webp|pdf|zip|tar|gz|tgz|xz|wasm|proto|graphql|tf|service|timer|desktop|diff|patch|rst|tex|bib|ipynb|lockb|plist|gradle|cmake|make|mk|rlib|so|dll|exe|bin|out|a|o";

const KNOWN_DIRECTORIES: &[&str] = &[
    "src",
    "lib",
    "libs",
    "bin",
    "docs",
    "doc",
    "tests",
    "test",
    "target",
    "crates",
    "vendor",
    "node_modules",
    "assets",
    "scripts",
    "examples",
    "benches",
    "build",
    "dist",
    "pkg",
    "app",
    "apps",
    "packages",
    "include",
    "config",
    "configs",
    "data",
    "etc",
    "usr",
    "var",
    "tmp",
    "home",
    "opt",
    "mnt",
    "media",
    "proc",
    "dev",
    "run",
    "srv",
    "sys",
];

const BARE_FILE_NAMES: &str =
    "Makefile|Dockerfile|Justfile|Procfile|Gemfile|Rakefile|LICENSE|Vagrantfile|CODEOWNERS";

fn regex(pattern: &str) -> Regex {
    // Patterns are compile-time literals exercised by this module's tests.
    Regex::new(pattern).expect("smart copy token pattern is valid")
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?i)\b(?:https?|ftp|sftp|ssh|git|file|wss?)://[^\s<>"'`]+"#));
static GIT_REMOTE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\bgit@[A-Za-z0-9.\-]+:[A-Za-z0-9._\-/]+(?:\.git)?"));
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}")
});
static IPV4: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b(?:(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])(?::[0-9]{1,5})?(?:/[0-9]{1,2})?",
    )
});
static IPV6: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\b[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}\b"));
static HOST_PORT: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b(?:localhost|(?:[A-Za-z0-9](?:[A-Za-z0-9\-]*[A-Za-z0-9])?\.)+[A-Za-z]{2,}):[0-9]{2,5}\b",
    )
});
static LOCALHOST: LazyLock<Regex> = LazyLock::new(|| regex(r"\blocalhost\b"));
static DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r#"(?i)\b(?:[a-z0-9][a-z0-9\-]*\.)+(?:com|net|org|io|dev|app|ai|co|fr|de|eu|us|ca|me|info|xyz|cloud|tech|edu|gov|uk|nl|es|it)\b(?:/[^\s<>"'`)\]]*)?"#,
    )
});
static FILE_LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r"(?:[~./\w@+\-]*/)*[\w@+\-.]+\.(?:{FILE_EXTENSIONS})(?::[0-9]+(?::[0-9]+)?|#L[0-9]+(?:-L?[0-9]+)?|\([0-9]+(?:,\s?[0-9]+)?\))"
    ))
});
static PYTHON_TRACE: LazyLock<Regex> = LazyLock::new(|| regex(r#"File "([^"]+)", line [0-9]+"#));
static ABSOLUTE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    regex(r#"(?:^|[\s"'`(\[<=,:])((?:~|\.{1,2})?/(?:[\w.@+%~\-]+/)*[\w.@+%~\-]+/?)"#)
});
static RELATIVE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    regex(r#"(?:^|[\s"'`(\[<=,:])((?:\.{1,2}/)?[\w@+\-][\w.@+\-]*(?:/[\w.@+%~\-]+)+/?)"#)
});
static BARE_FILE: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r#"(?:^|[\s"'`(\[<=,:/])([\w@+\-][\w.@+\-]*\.(?:{FILE_EXTENSIONS})|(?:{BARE_FILE_NAMES}))\b"#
    ))
});
static DOT_FILE: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?:^|[\s"'`(\[<=,:/])(\.[A-Za-z][\w.\-]*)"#));
static QUALIFIED_NAME: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+"));
static TYPE_NAME: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[A-Z][a-z0-9]+(?:[A-Z][a-z0-9]*)+\b"));
static SNAKE_IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[a-z][a-z0-9]*(?:_[a-z0-9]+)+\b"));
static CAMEL_IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[a-z]+(?:[A-Z][a-z0-9]+)+\b"));
static CONSTANT: LazyLock<Regex> = LazyLock::new(|| regex(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b"));
static FUNCTION_CALL: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*\(\)"));
static FUNCTION_DEFINITION: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b(?:fn|def|function|func)\s+([A-Za-z_][A-Za-z0-9_]*)"));
static HASH: LazyLock<Regex> = LazyLock::new(|| regex(r"\b[0-9a-f]{7,64}\b"));
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
});
static HEX_COLOR: LazyLock<Regex> = LazyLock::new(|| regex(r"#[0-9a-fA-F]{3,8}\b"));
static HEX_NUMBER: LazyLock<Regex> = LazyLock::new(|| regex(r"\b0x[0-9a-fA-F]+\b"));
static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"\bv?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.\-]+)?(?:\+[0-9A-Za-z.\-]+)?")
});
static PACKAGE_SPEC: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b[A-Za-z@][A-Za-z0-9_.\-/]*(?:@|==|>=|<=|~=)v?[0-9]+\.[0-9]+(?:\.[0-9]+)?[0-9A-Za-z.\-+]*",
    )
});
static ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"\b([A-Za-z_][A-Za-z0-9_.\-]*)=("[^"]*"|'[^']*'|[^\s,;]+)"#));
static LONG_FLAG: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?:^|[\s(\[])(--[A-Za-z][A-Za-z0-9\-]*(?:=[^\s]+)?|-[A-Za-z])\b"));
static MARKDOWN_LINK: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\[([^\]\n]{1,200})\]\(([^)\s]+)\)"));
static ISO_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[T ][0-9]{2}:[0-9]{2}(?::[0-9]{2}(?:\.[0-9]+)?)?(?:Z|[+\-][0-9]{2}:?[0-9]{2})?)?\b",
    )
});
static SLASH_DATE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[0-9]{1,2}[/.][0-9]{1,2}[/.][0-9]{2,4}\b"));
static CLOCK_TIME: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[0-9]{1,2}:[0-9]{2}(?::[0-9]{2})?(?:\s?[AaPp][Mm])?\b"));
static MEASUREMENT: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b[0-9]+(?:[.,][0-9]+)?\s?(?:ms|µs|us|ns|s|sec|min|h|d|B|KB|MB|GB|TB|KiB|MiB|GiB|TiB|kB|Hz|kHz|MHz|GHz|fps|px|mm|cm|km|kg|g|%|°C|°F|tokens?)\b|\b[0-9]+(?:[.,][0-9]+)?%",
    )
});
static AMOUNT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"[$€£¥]\s?[0-9][0-9,.]*|\b[0-9][0-9,.]*\s?(?:EUR|USD|GBP|CHF|€|\$|£)"));
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"(?:\+[0-9]{1,3}[ .\-]?)?(?:\([0-9]{1,4}\)[ .\-]?)?[0-9]{1,4}(?:[ .\-][0-9]{2,4}){2,5}")
});
static KEY_VALUE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^\s*(?:[•*\-]\s+)?([A-Za-z][A-Za-z0-9 _./\-]{0,30}):\s+(\S.*?)\s*$"));
static SENTENCE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"[^.!?]*[A-Za-z0-9][^.!?]*[.!?]+(?:\s|$)"));

static STREET_ENGLISH: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b[0-9]{1,5}[A-Za-z]?(?:-[0-9]{1,5})?\s+(?:[A-Z][A-Za-z'’.\-]*\s+){0,4}(?:Street|St|Avenue|Ave|Road|Rd|Boulevard|Blvd|Lane|Ln|Drive|Dr|Court|Ct|Way|Place|Pl|Square|Sq|Terrace|Highway|Hwy|Parkway|Pkwy|Circle|Cir|Close|Crescent|Gardens|Row|Alley|Mews)\b\.?(?:\s+(?:Apt|Suite|Ste|Unit|Floor|Fl)\.?\s?[A-Za-z0-9\-]+)?",
    )
});
static STREET_FRENCH: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b[0-9]{1,4}(?:\s?(?i:bis|ter|quater))?,?\s+(?i:rue|avenue|av\.|boulevard|bd|chemin|impasse|place|all[ée]e|quai|route|cours|passage|square|cit[ée]|villa|sentier|esplanade)\s+(?:(?:de la|de l'|de|du|des|d'|la|le|les)\s?)*\p{Lu}[\p{L}'’\-]*(?:\s+(?:(?:de|du|des|la|le|les)\s+)?\p{Lu}[\p{L}'’\-]*){0,3}",
    )
});
static STREET_GERMAN: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b\p{Lu}[\p{L}\-]+(?:straße|strasse|str\.|weg|platz|allee|gasse|ring|damm|ufer)\s+[0-9]{1,4}[a-z]?\b",
    )
});
static STREET_ROMANCE: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b(?i:calle|avenida|avda\.?|paseo|plaza|via|viale|piazza|corso)\s+\p{L}[\p{L}'’.\-]*(?:\s+\p{L}[\p{L}'’.\-]*){0,3},?\s+[0-9]{1,4}\b",
    )
});
static PO_BOX: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)\bP\.?\s?O\.?\s?Box\s+[0-9]+\b|\bPostfach\s+[0-9]+\b"));
static POSTAL_US: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"\b\p{Lu}[\p{L}.'\-]+(?:\s+\p{Lu}[\p{L}.'\-]+){0,3},?\s+[A-Z]{2}\s+[0-9]{5}(?:-[0-9]{4})?\b",
    )
});
static POSTAL_UK: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[A-Z]{1,2}[0-9][A-Z0-9]?\s?[0-9][A-Z]{2}\b"));
static POSTAL_CANADA: LazyLock<Regex> =
    LazyLock::new(|| regex(r"\b[A-Z][0-9][A-Z]\s?[0-9][A-Z][0-9]\b"));
static POSTAL_NETHERLANDS: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"\b[0-9]{4}\s?[A-Z]{2}\s+\p{Lu}[\p{L}'’\-]+(?:[ \-]\p{L}[\p{L}'’\-]*){0,3}\b")
});
static POSTAL_FIVE_DIGIT_CITY: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"\b(?:[A-Z]{1,2}[- ])?[0-9]{5}\s+\p{Lu}[\p{L}'’\-]+(?:[ \-]\p{L}[\p{L}'’\-]*){0,3}\b")
});
static POSTAL_FOUR_DIGIT_CITY: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"\b(?:[A-Z]{1,2}[- ])?[0-9]{4}\s+\p{Lu}[\p{L}'’\-]+(?:[ \-]\p{L}[\p{L}'’\-]*){0,3}\b")
});
static COUNTRY: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"^\s*(?i:France|Germany|Deutschland|Spain|Espa[nñ]a|Italy|Italia|Belgium|Belgique|Switzerland|Suisse|Schweiz|Netherlands|Nederland|United Kingdom|UK|USA|United States(?: of America)?|Canada|Portugal|Austria|Ireland|Luxembourg)\s*\.?\s*$",
    )
});

/// Maps byte offsets in `text` to character indices.
fn char_index_table(text: &str) -> Vec<usize> {
    let mut table = vec![0; text.len() + 1];
    for (char_index, (byte_index, character)) in text.char_indices().enumerate() {
        for offset in 0..character.len_utf8() {
            table[byte_index + offset] = char_index;
        }
    }
    table[text.len()] = text.chars().count();
    table
}

struct Collector<'a> {
    text: &'a str,
    table: Vec<usize>,
    matches: Vec<TokenMatch>,
    seen: HashSet<(usize, usize)>,
}

impl<'a> Collector<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            table: char_index_table(text),
            matches: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn chars_of(&self, byte_start: usize, byte_end: usize) -> (usize, usize) {
        (self.table[byte_start], self.table[byte_end])
    }

    /// Adds a token covering `byte_start..byte_end`. The first detector that
    /// claims an exact range wins, so detector order encodes priority.
    fn add(
        &mut self,
        byte_start: usize,
        byte_end: usize,
        kind: &'static str,
        label: String,
        probe: Option<PathProbe>,
    ) -> bool {
        if byte_start >= byte_end || byte_end > self.text.len() {
            return false;
        }
        let (start, end) = self.chars_of(byte_start, byte_end);
        if !self.seen.insert((start, end)) {
            return false;
        }
        self.matches.push(TokenMatch {
            start,
            end,
            kind,
            label,
            probe,
        });
        true
    }

    fn overlaps_kind(&self, byte_start: usize, byte_end: usize, kinds: &[&str]) -> bool {
        let (start, end) = self.chars_of(byte_start, byte_end);
        self.matches
            .iter()
            .any(|token| kinds.contains(&token.kind) && token.start < end && start < token.end)
    }
}

fn trim_trailing(text: &str, start: usize, mut end: usize, characters: &[char]) -> usize {
    while end > start {
        let Some(last) = text[start..end].chars().next_back() else {
            break;
        };
        if characters.contains(&last) {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn short(text: &str) -> String {
    const LIMIT: usize = 36;
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() <= LIMIT {
        flattened
    } else {
        let mut clipped = flattened.chars().take(LIMIT - 1).collect::<String>();
        clipped.push('…');
        clipped
    }
}

fn is_balanced_url_end(url: &str) -> usize {
    // Drop trailing sentence punctuation and unbalanced closers.
    let mut end = url.len();
    loop {
        let Some(last) = url[..end].chars().next_back() else {
            return end;
        };
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' | '*' | '`' | '_' => true,
            ')' => url[..end].matches('(').count() < url[..end].matches(')').count(),
            ']' => url[..end].matches('[').count() < url[..end].matches(']').count(),
            '}' => url[..end].matches('{').count() < url[..end].matches('}').count(),
            _ => false,
        };
        if strip {
            end -= last.len_utf8();
        } else {
            return end;
        }
    }
}

fn has_source_extension(path: &str) -> bool {
    let Some(name) = path.rsplit('/').next() else {
        return false;
    };
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    FILE_EXTENSIONS.split('|').any(|known| known == extension)
}

fn path_is_plausible(path: &str) -> bool {
    if path.starts_with("~/") || path.starts_with("./") || path.starts_with("../") {
        return true;
    }
    let segments = path
        .trim_end_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .count();
    if path.starts_with('/') {
        return segments >= 2;
    }
    has_source_extension(path)
        || path
            .split('/')
            .next()
            .is_some_and(|first| KNOWN_DIRECTORIES.contains(&first))
}

fn path_kind_label(path: &str) -> String {
    format!("path {}", short(path))
}

/// Finds every fine-grained token in one line of terminal text.
pub fn scan_line(text: &str) -> Vec<TokenMatch> {
    let mut collector = Collector::new(text);
    detect_locators(&mut collector);
    detect_paths(&mut collector);
    detect_code_names(&mut collector);
    detect_identifiers(&mut collector);
    detect_values(&mut collector);
    detect_prose(&mut collector);
    collector.matches
}

fn detect_locators(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in URL.find_iter(text) {
        let end = found.start() + is_balanced_url_end(found.as_str());
        let after_scheme = text[found.start()..end]
            .split_once("://")
            .map_or("", |(_, rest)| rest);
        if !after_scheme
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        collector.add(
            found.start(),
            end,
            "url",
            format!("URL {}", short(&text[found.start()..end])),
            None,
        );
    }
    for found in GIT_REMOTE.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "git-remote",
            format!("git remote {}", short(found.as_str())),
            None,
        );
    }
    for found in EMAIL.find_iter(text) {
        let end = trim_trailing(text, found.start(), found.end(), &['.', '-']);
        collector.add(
            found.start(),
            end,
            "email",
            format!("email {}", short(&text[found.start()..end])),
            None,
        );
    }
    for found in IPV4.find_iter(text) {
        let preceded_by_dot = text[..found.start()].ends_with('.')
            || text[..found.start()]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_digit());
        let followed_by_dot = text[found.end()..].starts_with('.')
            && text[found.end()..]
                .chars()
                .nth(1)
                .is_some_and(|c| c.is_ascii_digit());
        if preceded_by_dot || followed_by_dot {
            continue;
        }
        collector.add(
            found.start(),
            found.end(),
            "ip-address",
            format!("IP {}", found.as_str()),
            None,
        );
        // Nested: the bare address inside "addr:port" or "addr/prefix".
        let address_end = found
            .as_str()
            .find([':', '/'])
            .map_or(found.end(), |offset| found.start() + offset);
        collector.add(
            found.start(),
            address_end,
            "ip-address",
            format!("IP {}", &text[found.start()..address_end]),
            None,
        );
    }
    for found in IPV6.find_iter(text) {
        let before = text[..found.start()].chars().next_back();
        let after = text[found.end()..].chars().next();
        let isolated = !before.is_some_and(|c| c.is_alphanumeric() || c == ':' || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == ':' || c == '_');
        let hex_digits = found
            .as_str()
            .chars()
            .filter(char::is_ascii_hexdigit)
            .count();
        if isolated && hex_digits >= 2 && found.as_str().parse::<std::net::Ipv6Addr>().is_ok() {
            collector.add(
                found.start(),
                found.end(),
                "ip-address",
                format!("IPv6 {}", short(found.as_str())),
                None,
            );
        }
    }
    let file_ranges = file_location_ranges(text);
    for found in HOST_PORT.find_iter(text) {
        if file_ranges
            .iter()
            .any(|(start, end)| *start < found.end() && found.start() < *end)
        {
            continue;
        }
        if collector.overlaps_kind(found.start(), found.end(), &["ip-address", "email", "url"]) {
            continue;
        }
        collector.add(
            found.start(),
            found.end(),
            "host-port",
            format!("endpoint {}", found.as_str()),
            None,
        );
        if let Some(colon) = found.as_str().rfind(':') {
            collector.add(
                found.start(),
                found.start() + colon,
                "host",
                format!("host {}", &found.as_str()[..colon]),
                None,
            );
            collector.add(
                found.start() + colon + 1,
                found.end(),
                "port",
                format!("port {}", &found.as_str()[colon + 1..]),
                None,
            );
        }
    }
    for found in LOCALHOST.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "host",
            "host localhost".to_string(),
            None,
        );
    }
    for found in DOMAIN.find_iter(text) {
        if collector.overlaps_kind(
            found.start(),
            found.end(),
            &["url", "email", "host-port", "git-remote"],
        ) {
            continue;
        }
        if file_ranges
            .iter()
            .any(|(start, end)| *start < found.end() && found.start() < *end)
        {
            continue;
        }
        let end = trim_trailing(text, found.start(), found.end(), &['.', ',', ';', ':']);
        collector.add(
            found.start(),
            end,
            "domain",
            format!("domain {}", short(&text[found.start()..end])),
            None,
        );
    }
    for captures in MARKDOWN_LINK.captures_iter(text) {
        if let Some(label) = captures.get(1) {
            collector.add(
                label.start(),
                label.end(),
                "link-text",
                format!("link text {}", short(label.as_str())),
                None,
            );
        }
    }
}

fn file_location_ranges(text: &str) -> Vec<(usize, usize)> {
    FILE_LOCATION
        .find_iter(text)
        .map(|found| (found.start(), found.end()))
        .collect()
}

/// Adds each leading directory of `path` as a nested region (`a/b` and `a`
/// inside `a/b/c.rs`). They are kept only when they exist on disk.
fn add_directory_prefixes(collector: &mut Collector<'_>, byte_start: usize, path: &str) {
    let trimmed = path.trim_end_matches('/');
    for (offset, _) in trimmed.match_indices('/') {
        if offset == 0 || trimmed[..offset].ends_with('/') {
            continue;
        }
        let prefix = &trimmed[..offset];
        if prefix == "~" || prefix == "." || prefix == ".." {
            continue;
        }
        collector.add(
            byte_start,
            byte_start + offset,
            "path",
            path_kind_label(prefix),
            Some(PathProbe {
                path: prefix.to_string(),
                plausible_unverified: false,
            }),
        );
    }
}

fn detect_paths(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in FILE_LOCATION.find_iter(text) {
        let end = trim_trailing(text, found.start(), found.end(), &['.', ',', ';']);
        let token = &text[found.start()..end];
        let path_end = token
            .find([':', '#', '('])
            .map_or(token.len(), |offset| offset);
        let path = &token[..path_end];
        let probe = PathProbe {
            path: path.to_string(),
            plausible_unverified: true,
        };
        collector.add(
            found.start(),
            end,
            "file-location",
            format!("location {}", short(token)),
            Some(probe.clone()),
        );
        collector.add(
            found.start(),
            found.start() + path_end,
            "path",
            path_kind_label(path),
            Some(probe),
        );
        add_directory_prefixes(collector, found.start(), path);
        // Nested: the line number itself, and the bare file name.
        if let Some(name_start) = path.rfind('/') {
            let name = &path[name_start + 1..];
            if !name.is_empty() {
                collector.add(
                    found.start() + name_start + 1,
                    found.start() + path_end,
                    "path",
                    path_kind_label(name),
                    Some(PathProbe {
                        path: name.to_string(),
                        plausible_unverified: true,
                    }),
                );
            }
        }
    }
    for captures in PYTHON_TRACE.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            collector.add(
                group.start(),
                group.end(),
                "path",
                path_kind_label(group.as_str()),
                Some(PathProbe {
                    path: group.as_str().to_string(),
                    plausible_unverified: true,
                }),
            );
        }
    }
    let add_path = |collector: &mut Collector<'_>, group: regex::Match<'_>| {
        if collector.overlaps_kind(
            group.start(),
            group.end(),
            &["url", "email", "host-port", "domain"],
        ) {
            return;
        }
        let end = trim_trailing(text, group.start(), group.end(), &['.', ',', ';', ':']);
        let path = &text[group.start()..end];
        if path.len() < 2 || path.trim_matches('/').is_empty() {
            return;
        }
        let probe = PathProbe {
            path: path.to_string(),
            plausible_unverified: path_is_plausible(path),
        };
        collector.add(
            group.start(),
            end,
            "path",
            path_kind_label(path),
            Some(probe),
        );
        add_directory_prefixes(collector, group.start(), path);
    };
    for captures in ABSOLUTE_PATH.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            add_path(collector, group);
        }
    }
    for captures in RELATIVE_PATH.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            // Skip "and/or", "I/O" style words and ratios.
            if !group.as_str().contains('.')
                && !group.as_str().contains('_')
                && !group.as_str().contains('-')
                && !path_is_plausible(group.as_str())
            {
                let parts = group.as_str().split('/').collect::<Vec<_>>();
                if parts
                    .iter()
                    .all(|part| part.chars().all(|c| c.is_ascii_alphabetic()) && part.len() <= 4)
                {
                    continue;
                }
            }
            add_path(collector, group);
        }
    }
    for captures in BARE_FILE.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            if collector.overlaps_kind(group.start(), group.end(), &["url", "email", "host-port"]) {
                continue;
            }
            let end = trim_trailing(text, group.start(), group.end(), &['.', ',', ';', ':']);
            let name = &text[group.start()..end];
            collector.add(
                group.start(),
                end,
                "path",
                path_kind_label(name),
                Some(PathProbe {
                    path: name.to_string(),
                    plausible_unverified: true,
                }),
            );
        }
    }
    for captures in DOT_FILE.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            let end = trim_trailing(text, group.start(), group.end(), &['.', ',', ';', ':']);
            let name = &text[group.start()..end];
            if name.len() < 3 {
                continue;
            }
            collector.add(
                group.start(),
                end,
                "path",
                path_kind_label(name),
                Some(PathProbe {
                    path: name.to_string(),
                    plausible_unverified: false,
                }),
            );
        }
    }
}

fn detect_code_names(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in QUALIFIED_NAME.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "qualified-name",
            format!("name {}", short(found.as_str())),
            None,
        );
        // Nested: every prefix path and the final segment.
        let mut cursor = found.start();
        for segment in found.as_str().split("::") {
            let segment_start = cursor;
            let segment_end = segment_start + segment.len();
            let kind = if segment.chars().next().is_some_and(char::is_uppercase) {
                "type-name"
            } else {
                "identifier"
            };
            collector.add(
                segment_start,
                segment_end,
                kind,
                format!("{kind} {segment}"),
                None,
            );
            cursor = segment_end + 2;
        }
    }
    for found in FUNCTION_CALL.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "function",
            format!("function {}", short(found.as_str())),
            None,
        );
    }
    for captures in FUNCTION_DEFINITION.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            collector.add(
                group.start(),
                group.end(),
                "function",
                format!("function {}", group.as_str()),
                None,
            );
        }
    }
}

fn detect_identifiers(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in TYPE_NAME.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "type-name",
            format!("type {}", found.as_str()),
            None,
        );
    }
    for found in CONSTANT.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "constant",
            format!("constant {}", found.as_str()),
            None,
        );
    }
    for found in SNAKE_IDENTIFIER.find_iter(text) {
        if collector.overlaps_kind(found.start(), found.end(), &["path"]) {
            continue;
        }
        collector.add(
            found.start(),
            found.end(),
            "identifier",
            format!("identifier {}", found.as_str()),
            None,
        );
    }
    for found in CAMEL_IDENTIFIER.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "identifier",
            format!("identifier {}", found.as_str()),
            None,
        );
    }
}

fn is_plausible_hash(candidate: &str) -> bool {
    let has_digit = candidate.chars().any(|c| c.is_ascii_digit());
    let has_letter = candidate.chars().any(|c| c.is_ascii_alphabetic());
    has_digit && has_letter && matches!(candidate.len(), 7..=12 | 40 | 64)
}

fn digit_count(text: &str) -> usize {
    text.chars().filter(char::is_ascii_digit).count()
}

fn detect_values(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in UUID.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "uuid",
            format!("UUID {}", short(found.as_str())),
            None,
        );
    }
    for found in HASH.find_iter(text) {
        if is_plausible_hash(found.as_str())
            && !collector.overlaps_kind(found.start(), found.end(), &["uuid", "path", "url"])
        {
            collector.add(
                found.start(),
                found.end(),
                "hash",
                format!("hash {}", short(found.as_str())),
                None,
            );
        }
    }
    for found in HEX_COLOR.find_iter(text) {
        if matches!(found.as_str().len() - 1, 3 | 4 | 6 | 8) {
            collector.add(
                found.start(),
                found.end(),
                "color",
                format!("color {}", found.as_str()),
                None,
            );
        }
    }
    for found in HEX_NUMBER.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "hex-number",
            format!("hex {}", found.as_str()),
            None,
        );
    }
    for found in PACKAGE_SPEC.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "package",
            format!("package {}", short(found.as_str())),
            None,
        );
    }
    for found in VERSION.find_iter(text) {
        let before = text[..found.start()].chars().next_back();
        let after = text[found.end()..].chars().next();
        let after_second = text[found.end()..].chars().nth(1);
        let embedded = before.is_some_and(|c| c.is_ascii_digit() || c == '.')
            || (after == Some('.') && after_second.is_some_and(|c| c.is_ascii_digit()));
        if !embedded {
            collector.add(
                found.start(),
                found.end(),
                "version",
                format!("version {}", found.as_str()),
                None,
            );
        }
    }
    for captures in ASSIGNMENT.captures_iter(text) {
        let (Some(whole), Some(key), Some(value)) =
            (captures.get(0), captures.get(1), captures.get(2))
        else {
            continue;
        };
        let quoted_value = value.as_str().starts_with('"') || value.as_str().starts_with('\'');
        if !quoted_value && value.as_str().starts_with(['[', '<', '(', '{', '$']) {
            continue;
        }
        let value_trimmed_end = if quoted_value {
            value.end()
        } else {
            trim_trailing(
                text,
                value.start(),
                value.end(),
                &['`', '\'', '"', ')', ']', '}', '.', ',', ';', '*'],
            )
        };
        if value_trimmed_end <= value.start() || key.as_str().len() < 2 {
            continue;
        }
        let whole_end = if quoted_value {
            whole.end()
        } else {
            value_trimmed_end
        };
        let whole = (whole.start(), whole_end);
        let value = (value.start(), value_trimmed_end);
        let upper = key
            .as_str()
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        let kind = if upper {
            "env-assignment"
        } else {
            "assignment"
        };
        collector.add(
            whole.0,
            whole.1,
            kind,
            format!("{kind} {}", short(&text[whole.0..whole.1])),
            None,
        );
        let mut value_start = value.0;
        let mut value_end = value.1;
        let quoted = quoted_value
            && value_end - value_start >= 2
            && text[value_start..value_end]
                .ends_with(text[value_start..].chars().next().unwrap_or('"'));
        if quoted {
            value_start += 1;
            value_end -= 1;
        }
        collector.add(
            value_start,
            value_end,
            "assigned-value",
            format!("value {}", short(&text[value_start..value_end])),
            None,
        );
        collector.add(
            key.start(),
            key.end(),
            "assignment-key",
            format!("key {}", key.as_str()),
            None,
        );
    }
    for captures in LONG_FLAG.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            collector.add(
                group.start(),
                group.end(),
                "flag",
                format!("flag {}", short(group.as_str())),
                None,
            );
            if let Some(equals) = group.as_str().find('=') {
                collector.add(
                    group.start(),
                    group.start() + equals,
                    "flag",
                    format!("flag {}", &group.as_str()[..equals]),
                    None,
                );
                collector.add(
                    group.start() + equals + 1,
                    group.end(),
                    "assigned-value",
                    format!("value {}", short(&group.as_str()[equals + 1..])),
                    None,
                );
            }
        }
    }
    for found in ISO_TIMESTAMP.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "timestamp",
            format!("timestamp {}", found.as_str()),
            None,
        );
    }
    for found in SLASH_DATE.find_iter(text) {
        if !collector.overlaps_kind(
            found.start(),
            found.end(),
            &["version", "ip-address", "path"],
        ) {
            collector.add(
                found.start(),
                found.end(),
                "date",
                format!("date {}", found.as_str()),
                None,
            );
        }
    }
    for found in CLOCK_TIME.find_iter(text) {
        if !collector.overlaps_kind(
            found.start(),
            found.end(),
            &[
                "timestamp",
                "ip-address",
                "host-port",
                "file-location",
                "port",
            ],
        ) {
            collector.add(
                found.start(),
                found.end(),
                "time",
                format!("time {}", found.as_str()),
                None,
            );
        }
    }
    for found in AMOUNT.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "amount",
            format!("amount {}", found.as_str().trim()),
            None,
        );
    }
    for found in MEASUREMENT.find_iter(text) {
        if !collector.overlaps_kind(
            found.start(),
            found.end(),
            &["version", "timestamp", "ip-address", "amount"],
        ) {
            collector.add(
                found.start(),
                found.end(),
                "measurement",
                format!("measurement {}", found.as_str()),
                None,
            );
        }
    }
    for found in PHONE.find_iter(text) {
        let digits = digit_count(found.as_str());
        let starts_like_phone =
            found.as_str().starts_with('+') || found.as_str().starts_with('(') || digits >= 10;
        if (8..=15).contains(&digits)
            && !found.as_str().contains('.')
            && starts_like_phone
            && !collector.overlaps_kind(
                found.start(),
                found.end(),
                &["timestamp", "ip-address", "version", "date", "uuid", "hash"],
            )
        {
            collector.add(
                found.start(),
                found.end(),
                "phone-number",
                format!("phone {}", found.as_str().trim()),
                None,
            );
        }
    }
}

fn detect_prose(collector: &mut Collector<'_>) {
    let text = collector.text;
    // Quoted strings: double quotes anywhere, single quotes only where an
    // apostrophe cannot be mistaken for one.
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < characters.len() {
        let (byte, character) = characters[index];
        if character != '"' && character != '\'' && character != '“' && character != '‘' {
            index += 1;
            continue;
        }
        let closing = match character {
            '“' => '”',
            '‘' => '’',
            other => other,
        };
        let opens_cleanly = index == 0 || !characters[index - 1].1.is_alphanumeric();
        if !opens_cleanly {
            index += 1;
            continue;
        }
        let close = (index + 1..characters.len()).find(|&candidate| {
            characters[candidate].1 == closing
                && (closing == '"'
                    || closing == '”'
                    || characters
                        .get(candidate + 1)
                        .is_none_or(|next| !next.1.is_alphanumeric()))
        });
        let Some(close) = close else {
            index += 1;
            continue;
        };
        let inner_start = byte + character.len_utf8();
        let inner_end = characters[close].0;
        if inner_end > inner_start && inner_end - inner_start <= 300 {
            let whole_end = inner_end + closing.len_utf8();
            collector.add(
                byte,
                whole_end,
                "quoted",
                format!("quoted {}", short(&text[inner_start..inner_end])),
                None,
            );
            collector.add(
                inner_start,
                inner_end,
                "quoted-text",
                format!("text {}", short(&text[inner_start..inner_end])),
                None,
            );
        }
        index = close + 1;
    }
    if let Some(captures) = KEY_VALUE.captures(text) {
        if let (Some(key), Some(value)) = (captures.get(1), captures.get(2)) {
            if key.as_str().split_whitespace().count() <= 4 && !value.as_str().starts_with("//") {
                collector.add(
                    key.start(),
                    value.end(),
                    "field",
                    format!("field {}", short(key.as_str())),
                    None,
                );
                collector.add(
                    value.start(),
                    value.end(),
                    "field-value",
                    format!("value {}", short(value.as_str())),
                    None,
                );
                collector.add(
                    key.start(),
                    key.end(),
                    "field-name",
                    format!("name {}", short(key.as_str())),
                    None,
                );
            }
        }
    }
    let sentences: Vec<_> = SENTENCE.find_iter(text).collect();
    if sentences.len() >= 2 {
        for sentence in sentences {
            let end = trim_trailing(text, sentence.start(), sentence.end(), &[' ', '\t']);
            let start = sentence.start() + text[sentence.start()..end].len()
                - text[sentence.start()..end].trim_start().len();
            let sentence_text = &text[start..end];
            if sentence_text.chars().count() >= 12
                && sentence_text.chars().next().is_some_and(char::is_uppercase)
            {
                collector.add(
                    start,
                    end,
                    "sentence",
                    format!("sentence {}", short(sentence_text)),
                    None,
                );
            }
        }
    }
}

fn first_match<'t>(patterns: &[&Regex], text: &'t str) -> Option<regex::Match<'t>> {
    patterns
        .iter()
        .filter_map(|pattern| pattern.find(text))
        .min_by_key(|found| found.start())
}

/// Finds postal addresses in consecutive rows of text. Addresses are
/// recognised heuristically: a street line (number plus a street-type word, or
/// the local-language equivalents), a postal-code-and-city line, a P.O. box, or
/// the combination of those split across one to three consecutive lines or
/// joined by commas on one line.
pub fn scan_postal_addresses(rows: &[String]) -> Vec<PostalMatch> {
    struct RowFacts {
        street: Option<(usize, usize)>,
        postal: Option<(usize, usize)>,
        country: bool,
    }
    let street_patterns = [
        &*STREET_ENGLISH,
        &*STREET_FRENCH,
        &*STREET_GERMAN,
        &*STREET_ROMANCE,
        &*PO_BOX,
    ];
    let postal_patterns = [
        &*POSTAL_US,
        &*POSTAL_NETHERLANDS,
        &*POSTAL_FIVE_DIGIT_CITY,
        &*POSTAL_UK,
        &*POSTAL_CANADA,
    ];
    let facts: Vec<RowFacts> = rows
        .iter()
        .map(|row| {
            let table = char_index_table(row);
            let street = first_match(&street_patterns, row)
                .map(|found| (table[found.start()], table[found.end()]));
            let postal = first_match(&postal_patterns, row)
                .map(|found| (table[found.start()], table[found.end()]));
            RowFacts {
                street,
                postal,
                country: COUNTRY.is_match(row),
            }
        })
        .collect();
    let mut found = Vec::new();
    let mut claimed_rows = HashSet::new();
    for (row, fact) in facts.iter().enumerate() {
        let Some((street_start, street_end)) = fact.street else {
            continue;
        };
        let mut parts = vec![(row, street_start, street_end)];
        let mut kind = "street-address";
        let mut postal_on_same_row = None;
        if let Some((postal_start, postal_end)) = fact.postal {
            if postal_start >= street_end {
                postal_on_same_row = Some(postal_end);
            }
        }
        if postal_on_same_row.is_none() {
            // Four-digit postal codes are only trusted next to a street.
            let same_line_four_digit = POSTAL_FOUR_DIGIT_CITY
                .find(&rows[row])
                .map(|m| {
                    let table = char_index_table(&rows[row]);
                    (table[m.start()], table[m.end()])
                })
                .filter(|(start, _)| *start >= street_end);
            if let Some((_, end)) = same_line_four_digit {
                postal_on_same_row = Some(end);
            }
        }
        let mut next_row = row + 1;
        if let Some(postal_end) = postal_on_same_row {
            parts[0].2 = postal_end;
            kind = "postal-address";
        } else if let Some(next) = rows.get(next_row) {
            let next_fact = &facts[next_row];
            let four_digit = POSTAL_FOUR_DIGIT_CITY.find(next).map(|m| {
                let table = char_index_table(next);
                (table[m.start()], table[m.end()])
            });
            let postal = next_fact.postal.or(four_digit);
            if let Some((postal_start, postal_end)) = postal {
                if next.chars().take(postal_start).all(char::is_whitespace) {
                    parts.push((next_row, postal_start, postal_end));
                    kind = "postal-address";
                    next_row += 1;
                }
            }
        }
        if kind == "postal-address" {
            if let Some(country_fact) = facts.get(next_row) {
                if country_fact.country {
                    let text = &rows[next_row];
                    let start = text.chars().take_while(|c| c.is_whitespace()).count();
                    let end = text.trim_end().chars().count();
                    parts.push((next_row, start, end));
                }
            }
        }
        let label = format!(
            "address {}",
            short(&rows[row].trim().chars().take(40).collect::<String>())
        );
        for part in &parts {
            claimed_rows.insert(part.0);
        }
        found.push(PostalMatch {
            rows: parts,
            kind,
            label,
        });
    }
    for (row, fact) in facts.iter().enumerate() {
        if claimed_rows.contains(&row) {
            continue;
        }
        if let Some((start, end)) = fact.postal {
            found.push(PostalMatch {
                rows: vec![(row, start, end)],
                kind: "postal-code",
                label: format!(
                    "postal code {}",
                    short(
                        &rows[row]
                            .chars()
                            .skip(start)
                            .take(end - start)
                            .collect::<String>()
                    )
                ),
            });
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Styled runs
// ---------------------------------------------------------------------------

/// A run of adjacent cells sharing a visual style that terminal programs use
/// to mark something out: a foreground colour, a background highlight, bold,
/// italic or underline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledRun {
    pub start_column: u16,
    pub end_column: u16,
    pub kind: &'static str,
    pub label: String,
}

fn color_name(color: vt100::Color) -> Option<String> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(
            match index {
                1 | 9 => "red",
                2 | 10 => "green",
                3 | 11 => "yellow",
                4 | 12 => "blue",
                5 | 13 => "magenta",
                6 | 14 => "cyan",
                16..=231 => "colored",
                // Black, white and the grey ramp are plain-text colours.
                _ => return None,
            }
            .to_string(),
        ),
        vt100::Color::Rgb(red, green, blue) => {
            let spread = red.max(green).max(blue) - red.min(green).min(blue);
            if spread < 28 {
                return None;
            }
            let (r, g, b) = (i32::from(red), i32::from(green), i32::from(blue));
            let name = if b > r + 24 && b >= g {
                if g > b - 40 {
                    "cyan"
                } else {
                    "blue"
                }
            } else if g > r + 24 && g >= b {
                "green"
            } else if r > g + 24 && r >= b {
                if g > 140 && b < 90 {
                    "yellow"
                } else {
                    "red"
                }
            } else {
                "colored"
            };
            Some(name.to_string())
        }
    }
}

type StyleKey = Option<(&'static str, String)>;

fn style_key_for(cell: &vt100::Cell, category: usize) -> StyleKey {
    match category {
        0 => color_name(cell.fgcolor())
            .map(|name| ("colored", format!("{name:?}-{:?}", cell.fgcolor()))),
        1 => (!cell.inverse())
            .then(|| color_name(cell.bgcolor()))
            .flatten()
            .map(|_| ("highlighted", format!("{:?}", cell.bgcolor()))),
        2 => cell.bold().then(|| ("bold", String::new())),
        3 => cell.italic().then(|| ("italic", String::new())),
        _ => cell.underline().then(|| ("underlined", String::new())),
    }
}

/// Collects the styled runs of one screen row.
pub fn styled_runs(screen: &vt100::Screen, row: u16) -> Vec<StyledRun> {
    let (_, columns) = screen.size();
    let mut runs = Vec::new();
    for category in 0..5 {
        let mut active: Option<(u16, u16, StyleKey, String)> = None;
        let mut pending_gap = 0u16;
        let flush = |active: &mut Option<(u16, u16, StyleKey, String)>,
                     runs: &mut Vec<StyledRun>| {
            let Some((start, end, key, text)) = active.take() else {
                return;
            };
            let Some((kind, detail)) = key else {
                return;
            };
            let trimmed = text.trim();
            if trimmed.chars().count() < 2 || !trimmed.chars().any(char::is_alphanumeric) {
                return;
            }
            let label = if kind == "colored" {
                let name = detail
                    .split('-')
                    .next()
                    .unwrap_or("colored")
                    .trim_matches('"')
                    .to_string();
                format!("{name} text {}", short(trimmed))
            } else {
                format!("{kind} text {}", short(trimmed))
            };
            runs.push(StyledRun {
                start_column: start,
                end_column: end,
                kind,
                label,
            });
        };
        for column in 0..columns {
            let Some(cell) = screen.cell(row, column) else {
                flush(&mut active, &mut runs);
                continue;
            };
            if cell.is_wide_continuation() {
                if let Some(run) = active.as_mut() {
                    run.1 = column;
                }
                continue;
            }
            let contents = cell.contents();
            let is_space = contents.is_empty() || contents.chars().all(char::is_whitespace);
            if is_space {
                pending_gap += 1;
                if pending_gap > 1 {
                    flush(&mut active, &mut runs);
                }
                continue;
            }
            let key = style_key_for(cell, category);
            match (&mut active, key) {
                (Some(run), Some(candidate)) if run.2.as_ref() == Some(&candidate) => {
                    if pending_gap == 1 {
                        run.3.push(' ');
                    }
                    run.1 = column;
                    run.3.push_str(contents);
                }
                (_, Some(candidate)) => {
                    flush(&mut active, &mut runs);
                    active = Some((column, column, Some(candidate), contents.to_string()));
                }
                (_, None) => flush(&mut active, &mut runs),
            }
            pending_gap = 0;
        }
        flush(&mut active, &mut runs);
    }
    runs
}

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
}

/// Where relative paths on screen may be resolved. Roots are tried in order:
/// the pane's working directory, then each ancestor of it, then the project
/// root. A bare file name is also looked up in a few conventional source
/// directories under each root, because tools print `main.rs` for
/// `src/main.rs`.
#[derive(Debug, Default)]
pub struct PathContext {
    roots: Vec<PathBuf>,
    home: Option<PathBuf>,
    budget: std::cell::Cell<usize>,
    cache: std::cell::RefCell<HashMap<String, Option<PathKind>>>,
}

const MAXIMUM_ANCESTOR_DEPTH: usize = 8;
const MAXIMUM_PATH_LOOKUPS: usize = 800;
const SOURCE_SUBDIRECTORIES: &[&str] = &[
    "src", "tests", "docs", "lib", "app", "crates", "scripts", "bin",
];

impl PathContext {
    pub fn new(pane_cwd: Option<&Path>, project_root: &Path, home: Option<PathBuf>) -> Self {
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut push = |candidate: &Path| {
            if !roots.iter().any(|existing| existing == candidate) {
                roots.push(candidate.to_path_buf());
            }
        };
        if let Some(cwd) = pane_cwd {
            for ancestor in cwd.ancestors().take(MAXIMUM_ANCESTOR_DEPTH) {
                push(ancestor);
            }
        }
        push(project_root);
        Self {
            roots,
            home,
            budget: std::cell::Cell::new(MAXIMUM_PATH_LOOKUPS),
            cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    fn lookup(&self, candidate: &Path) -> Option<PathKind> {
        let key = candidate.to_string_lossy().into_owned();
        if let Some(cached) = self.cache.borrow().get(&key) {
            return *cached;
        }
        let remaining = self.budget.get();
        if remaining == 0 {
            return None;
        }
        self.budget.set(remaining - 1);
        let found = std::fs::metadata(candidate).ok().map(|metadata| {
            if metadata.is_dir() {
                PathKind::Directory
            } else {
                PathKind::File
            }
        });
        self.cache.borrow_mut().insert(key, found);
        found
    }

    /// Resolves a path as printed on screen to something that exists.
    pub fn resolve(&self, raw: &str) -> Option<PathKind> {
        let raw = raw.trim_end_matches('/');
        if raw.is_empty() {
            return None;
        }
        if let Some(rest) = raw.strip_prefix("~/") {
            return self.lookup(&self.home.as_ref()?.join(rest));
        }
        if raw.starts_with('/') {
            return self.lookup(Path::new(raw));
        }
        // Git diffs print `a/src/x.rs` and `b/src/x.rs`.
        let stripped = raw.strip_prefix("a/").or_else(|| raw.strip_prefix("b/"));
        let spellings = std::iter::once(raw).chain(stripped);
        let is_bare = !raw.contains('/');
        for spelling in spellings {
            for root in &self.roots {
                if let Some(kind) = self.lookup(&root.join(spelling)) {
                    return Some(kind);
                }
                if is_bare {
                    for subdirectory in SOURCE_SUBDIRECTORIES {
                        if let Some(kind) = self.lookup(&root.join(subdirectory).join(spelling)) {
                            return Some(kind);
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&'static str, String)> {
        scan_line(text)
            .into_iter()
            .map(|token| {
                (
                    token.kind,
                    text.chars()
                        .skip(token.start)
                        .take(token.end - token.start)
                        .collect(),
                )
            })
            .collect()
    }

    fn has(text: &str, kind: &str, expected: &str) -> bool {
        kinds(text)
            .iter()
            .any(|(found_kind, found)| *found_kind == kind && found == expected)
    }

    #[test]
    fn urls_nest_inside_lines_and_drop_trailing_punctuation() {
        assert!(has(
            "see https://example.test/a_(b)). ok",
            "url",
            "https://example.test/a_(b)"
        ));
        assert!(has(
            "clone git@github.com:org/repo.git now",
            "git-remote",
            "git@github.com:org/repo.git"
        ));
    }

    #[test]
    fn qualified_names_split_into_nested_segments() {
        let text = "use ilium_execution::JobOutcome here";
        assert!(has(text, "qualified-name", "ilium_execution::JobOutcome"));
        assert!(has(text, "type-name", "JobOutcome"));
        assert!(has(text, "identifier", "ilium_execution"));
    }

    #[test]
    fn file_locations_nest_path_and_file_name() {
        let text = "error at ilium-client/src/app.rs:6004:9";
        assert!(has(text, "file-location", "ilium-client/src/app.rs:6004:9"));
        assert!(has(text, "path", "ilium-client/src/app.rs"));
        assert!(has(text, "path", "app.rs"));
        assert!(has("File \"/tmp/x.py\", line 3", "path", "/tmp/x.py"));
        assert!(has(
            "src/lib.rs#L12-L20",
            "file-location",
            "src/lib.rs#L12-L20"
        ));
    }

    #[test]
    fn paths_absolute_relative_home_and_bare() {
        assert!(has(
            "open /usr/local/bin/tool now",
            "path",
            "/usr/local/bin/tool"
        ));
        assert!(has("edit ./src/main.rs.", "path", "./src/main.rs"));
        assert!(has("see ~/dev/ai/ilium", "path", "~/dev/ai/ilium"));
        assert!(has("run Makefile target", "path", "Makefile"));
        assert!(has("open .gitignore", "path", ".gitignore"));
        assert!(!has("and/or maybe", "path", "and/or"));
    }

    #[test]
    fn urls_do_not_produce_path_tokens() {
        assert!(!kinds("go to https://example.test/a/b/c.txt")
            .iter()
            .any(|(kind, _)| *kind == "path"));
    }

    #[test]
    fn endpoints_addresses_and_emails() {
        assert!(has(
            "listening on localhost:4005",
            "host-port",
            "localhost:4005"
        ));
        assert!(has("listening on localhost:4005", "port", "4005"));
        assert!(has("bind 0.0.0.0:3000 ok", "ip-address", "0.0.0.0:3000"));
        assert!(has("bind 0.0.0.0:3000 ok", "ip-address", "0.0.0.0"));
        assert!(has("10.1.2.3/24", "ip-address", "10.1.2.3/24"));
        assert!(has(
            "peer fe80::1ff:fe23:4567:890a up",
            "ip-address",
            "fe80::1ff:fe23:4567:890a"
        ));
        assert!(has(
            "write to wolf.arthur@gmail.com.",
            "email",
            "wolf.arthur@gmail.com"
        ));
        assert!(has(
            "api.example.com:8443/x",
            "host-port",
            "api.example.com:8443"
        ));
        assert!(!has("main.rs:6004", "host-port", "main.rs:6004"));
        assert!(!kinds("rust f64::MAX and :: and ::fd")
            .iter()
            .any(|(kind, _)| *kind == "ip-address"));
        assert!(!kinds("ratio :1800 here")
            .iter()
            .any(|(kind, _)| *kind == "port"));
    }

    #[test]
    fn version_is_not_found_inside_an_ip_address() {
        assert!(!kinds("host 192.168.1.10 up")
            .iter()
            .any(|(kind, _)| *kind == "version"));
        assert!(has("ilium v0.1.0 released", "version", "v0.1.0"));
    }

    #[test]
    fn hashes_uuids_colors_and_assignments() {
        assert!(has("commit e4f9d84 fix", "hash", "e4f9d84"));
        assert!(!has("the value 1234567 here", "hash", "1234567"));
        assert!(has(
            "id 123e4567-e89b-12d3-a456-426614174000",
            "uuid",
            "123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(has("paint #ff8800 now", "color", "#ff8800"));
        assert!(has(
            "RUST_LOG=debug cargo run",
            "env-assignment",
            "RUST_LOG=debug"
        ));
        assert!(has("RUST_LOG=debug cargo run", "assigned-value", "debug"));
        assert!(has("cargo build --release -p ilium", "flag", "--release"));
        assert!(has("cargo build --release -p ilium", "flag", "-p"));
        assert!(has("tokio@1.40.2 selected", "package", "tokio@1.40.2"));
        assert!(!kinds("note=[weekly] and x=<how>")
            .iter()
            .any(|(kind, _)| kind.contains("assign")));
        assert!(has("TMPDIR=/dev/shm`", "env-assignment", "TMPDIR=/dev/shm"));
        assert!(has("TMPDIR=/dev/shm`", "path", "/dev/shm"));
        assert!(has(
            "(https://github.com/a/b)**",
            "url",
            "https://github.com/a/b"
        ));
        assert!(!has("e.g. this", "sentence", "e.g."));
    }

    #[test]
    fn dates_times_measurements_amounts_and_phones() {
        assert!(has(
            "at 2026-10-03 03:31:38 done",
            "timestamp",
            "2026-10-03 03:31:38"
        ));
        assert!(has(
            "took 4m03s total 250 ms and 12%",
            "measurement",
            "250 ms"
        ));
        assert!(has("pay €12.50 today", "amount", "€12.50"));
        assert!(has(
            "call +33 1 42 68 53 00 now",
            "phone-number",
            "+33 1 42 68 53 00"
        ));
        assert!(!has("on 2026-10-03", "phone-number", "2026-10-03"));
    }

    #[test]
    fn code_identifiers_and_functions() {
        assert!(has("call parse_line() twice", "function", "parse_line()"));
        assert!(has("fn detect_regions(x)", "function", "detect_regions"));
        assert!(has(
            "MAXIMUM_CANDIDATES is large",
            "constant",
            "MAXIMUM_CANDIDATES"
        ));
        assert!(has("the camelCaseName var", "identifier", "camelCaseName"));
        assert!(has(
            "a snake_case_name var",
            "identifier",
            "snake_case_name"
        ));
    }

    #[test]
    fn quotes_key_values_and_sentences_nest() {
        assert!(has("say \"hello world\" now", "quoted-text", "hello world"));
        assert!(has("say \"hello world\" now", "quoted", "\"hello world\""));
        assert!(!kinds("it's don't won't")
            .iter()
            .any(|(kind, _)| kind.starts_with("quoted")));
        assert!(has("Status: all green", "field-value", "all green"));
        assert!(has(
            "One thing here. Two things there.",
            "sentence",
            "Two things there."
        ));
        assert!(has("[the docs](https://x.test/d)", "link-text", "the docs"));
    }

    #[test]
    fn postal_addresses_single_multi_line_and_foreign() {
        let rows = |lines: &[&str]| {
            lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
        };
        let found =
            scan_postal_addresses(&rows(&["Visit 221B Baker Street, London NW1 6XE today"]));
        assert!(found
            .iter()
            .any(|m| m.kind == "street-address" || m.kind == "postal-address"));
        let found = scan_postal_addresses(&rows(&["10 rue de la Paix", "75002 Paris", "France"]));
        let address = found
            .iter()
            .find(|m| m.kind == "postal-address")
            .expect("address");
        assert_eq!(address.rows.len(), 3);
        let found =
            scan_postal_addresses(&rows(&["1600 Pennsylvania Avenue", "Washington, DC 20500"]));
        assert!(found
            .iter()
            .any(|m| m.kind == "postal-address" && m.rows.len() == 2));
        let found = scan_postal_addresses(&rows(&["Hauptstraße 5", "10115 Berlin"]));
        assert!(found.iter().any(|m| m.kind == "postal-address"));
        let found = scan_postal_addresses(&rows(&["P.O. Box 1234"]));
        assert!(found.iter().any(|m| m.kind == "street-address"));
        assert!(scan_postal_addresses(&rows(&["no address in this prose"])).is_empty());
    }

    #[test]
    fn styled_runs_group_colour_bold_and_background() {
        let mut parser = vt100::Parser::new(2, 60, 0);
        parser.process(b"plain \x1b[34mcrates/app.rs\x1b[0m and \x1b[1mbold word\x1b[0m \x1b[7;42m\x1b[27m\x1b[42mhl text\x1b[0m\r\n");
        let runs = styled_runs(parser.screen(), 0);
        assert!(runs
            .iter()
            .any(|run| run.kind == "colored" && run.label.starts_with("blue")));
        assert!(runs
            .iter()
            .any(|run| run.kind == "bold" && run.label.contains("bold word")));
        assert!(runs.iter().any(|run| run.kind == "highlighted"));
        let blue = runs
            .iter()
            .find(|run| run.kind == "colored")
            .expect("blue run");
        assert_eq!((blue.start_column, blue.end_column), (6, 18));
    }

    #[test]
    fn grey_and_default_colours_are_not_styled_runs() {
        let mut parser = vt100::Parser::new(1, 40, 0);
        parser.process(
            b"\x1b[90mdim grey text\x1b[0m default text \x1b[38;2;120;120;120mgrey rgb\x1b[0m",
        );
        assert!(styled_runs(parser.screen(), 0)
            .iter()
            .all(|run| run.kind != "colored"));
    }

    #[test]
    fn path_context_resolves_across_ancestors_and_source_directories() {
        let root = std::env::temp_dir().join(format!("ilium-smart-copy-{}", std::process::id()));
        let nested = root.join("crate").join("src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();
        let cwd = root.join("crate");
        let context = PathContext::new(Some(&cwd), &root, None);
        assert_eq!(context.resolve("src/main.rs"), Some(PathKind::File));
        assert_eq!(context.resolve("main.rs"), Some(PathKind::File));
        assert_eq!(context.resolve("README.md"), Some(PathKind::File));
        assert_eq!(context.resolve("a/src/main.rs"), Some(PathKind::File));
        assert_eq!(context.resolve("src"), Some(PathKind::Directory));
        assert_eq!(context.resolve("missing.rs"), None);
        assert_eq!(
            context.resolve(&nested.join("main.rs").to_string_lossy()),
            Some(PathKind::File)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
