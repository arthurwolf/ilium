//! Fine-grained Smart Copy recognisers: the regular expression line scanner (URLs, paths, names,
//! hashes, values, prose and postal addresses) of the former built-in `smart_copy_tokens.rs`.
//!
//! Everything here is a pure function of text. A plugin has no file system, so path-shaped tokens
//! are kept by plausibility alone (see [`crate::tokens::path_is_plausible`]) instead of being
//! probed on disk.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::tokens::{
    digit_count, is_plausible_hash, path_is_plausible, path_kind_label, short, trim_trailing,
    url_end, BARE_FILE_NAMES, FILE_EXTENSIONS, KNOWN_DIRECTORIES,
};

/// One detected token inside a single line. Offsets are character indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenMatch {
    pub start: usize,
    pub end: usize,
    pub kind: &'static str,
    pub label: String,
}

/// A postal address, possibly spanning several rows. Rows are `(row, start_char, end_char)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostalMatch {
    pub rows: Vec<(usize, usize, usize)>,
    pub kind: &'static str,
    pub label: String,
}

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
        let end = found.start() + url_end(found.as_str());
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
        );
    }
    for found in GIT_REMOTE.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "git-remote",
            format!("git remote {}", short(found.as_str())),
        );
    }
    for found in EMAIL.find_iter(text) {
        let end = trim_trailing(text, found.start(), found.end(), &['.', '-']);
        collector.add(
            found.start(),
            end,
            "email",
            format!("email {}", short(&text[found.start()..end])),
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
        );
        if let Some(colon) = found.as_str().rfind(':') {
            collector.add(
                found.start(),
                found.start() + colon,
                "host",
                format!("host {}", &found.as_str()[..colon]),
            );
            collector.add(
                found.start() + colon + 1,
                found.end(),
                "port",
                format!("port {}", &found.as_str()[colon + 1..]),
            );
        }
    }
    for found in LOCALHOST.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "host",
            "host localhost".to_string(),
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
        );
    }
    for captures in MARKDOWN_LINK.captures_iter(text) {
        if let Some(label) = captures.get(1) {
            collector.add(
                label.start(),
                label.end(),
                "link-text",
                format!("link text {}", short(label.as_str())),
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
        if prefix == "~" || prefix == "." || prefix == ".." || !path_is_plausible(prefix) {
            continue;
        }
        collector.add(
            byte_start,
            byte_start + offset,
            "path",
            path_kind_label(prefix),
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
        collector.add(
            found.start(),
            end,
            "file-location",
            format!("location {}", short(token)),
        );
        collector.add(
            found.start(),
            found.start() + path_end,
            "path",
            path_kind_label(path),
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
        // A plugin cannot look the path up on disk, so an implausible one is dropped.
        if !path_is_plausible(path) {
            return;
        }
        collector.add(group.start(), end, "path", path_kind_label(path));
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
            collector.add(group.start(), end, "path", path_kind_label(name));
        }
    }
    for captures in DOT_FILE.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            let end = trim_trailing(text, group.start(), group.end(), &['.', ',', ';', ':']);
            let name = &text[group.start()..end];
            if name.len() < 3 {
                continue;
            }
            collector.add(group.start(), end, "path", path_kind_label(name));
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
        );
    }
    for captures in FUNCTION_DEFINITION.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            collector.add(
                group.start(),
                group.end(),
                "function",
                format!("function {}", group.as_str()),
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
        );
    }
    for found in CONSTANT.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "constant",
            format!("constant {}", found.as_str()),
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
        );
    }
    for found in CAMEL_IDENTIFIER.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "identifier",
            format!("identifier {}", found.as_str()),
        );
    }
}

fn detect_values(collector: &mut Collector<'_>) {
    let text = collector.text;
    for found in UUID.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "uuid",
            format!("UUID {}", short(found.as_str())),
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
            );
        }
    }
    for found in HEX_NUMBER.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "hex-number",
            format!("hex {}", found.as_str()),
        );
    }
    for found in PACKAGE_SPEC.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "package",
            format!("package {}", short(found.as_str())),
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
        );
        collector.add(
            key.start(),
            key.end(),
            "assignment-key",
            format!("key {}", key.as_str()),
        );
    }
    for captures in LONG_FLAG.captures_iter(text) {
        if let Some(group) = captures.get(1) {
            collector.add(
                group.start(),
                group.end(),
                "flag",
                format!("flag {}", short(group.as_str())),
            );
            if let Some(equals) = group.as_str().find('=') {
                collector.add(
                    group.start(),
                    group.start() + equals,
                    "flag",
                    format!("flag {}", &group.as_str()[..equals]),
                );
                collector.add(
                    group.start() + equals + 1,
                    group.end(),
                    "assigned-value",
                    format!("value {}", short(&group.as_str()[equals + 1..])),
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
            );
        }
    }
    for found in AMOUNT.find_iter(text) {
        collector.add(
            found.start(),
            found.end(),
            "amount",
            format!("amount {}", found.as_str().trim()),
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
            );
            collector.add(
                inner_start,
                inner_end,
                "quoted-text",
                format!("text {}", short(&text[inner_start..inner_end])),
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
                );
                collector.add(
                    value.start(),
                    value.end(),
                    "field-value",
                    format!("value {}", short(value.as_str())),
                );
                collector.add(
                    key.start(),
                    key.end(),
                    "field-name",
                    format!("name {}", short(key.as_str())),
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
