//! Turning the user's "Source" text into a list of playable inputs.
//!
//! The source is one or several entries separated by semicolons. Each entry is
//! an http(s) URL, a glob pattern, a folder, or a single file. Everything here
//! touches the file system, so discovery runs on the worker thread, never in
//! `Scene::render`. Settings validation only checks the source text.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Extensions that count as video when scanning folders and globs. A file
/// named explicitly is always accepted, whatever its extension.
pub const VIDEO_EXTENSIONS: [&str; 11] = [
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "mpg", "mpeg", "wmv", "flv", "ts",
];

/// Hard bound so a glob such as `/**` cannot exhaust memory or time.
const MAX_INPUTS: usize = 50_000;
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MediaInput {
    File(PathBuf),
    Url(String),
}

impl MediaInput {
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    /// Short human name for the status line.
    pub fn display_name(&self) -> String {
        match self {
            Self::File(path) => path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            Self::Url(url) => {
                let without_scheme = url.split("://").nth(1).unwrap_or(url);
                let without_query = without_scheme.split(['?', '#']).next().unwrap_or("");
                let mut parts = without_query.split('/').filter(|part| !part.is_empty());
                let host = parts.next().unwrap_or(without_query);
                parts.next_back().unwrap_or(host).to_owned()
            }
        }
    }

    /// The value handed to `-i`. Local files get a `file:` prefix so a name
    /// such as `pipe:1` or `-x.mkv` can never be read as a protocol or option.
    pub fn ffmpeg_argument(&self) -> OsString {
        match self {
            Self::File(path) => {
                let mut argument = OsString::from("file:");
                argument.push(path.as_os_str());
                argument
            }
            Self::Url(url) => OsString::from(url),
        }
    }
}

/// Result of scanning a source specification.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Discovery {
    pub inputs: Vec<MediaInput>,
    /// Entries that matched nothing, for the status line.
    pub unmatched: Vec<String>,
    pub limited: bool,
}

pub fn split_entries(source: &str) -> Vec<&str> {
    source
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect()
}

pub fn is_http_url(entry: &str) -> bool {
    let lower = entry.get(..8).unwrap_or(entry).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn has_glob_characters(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

pub fn video_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let lower = extension.to_string_lossy().to_ascii_lowercase();
        VIDEO_EXTENSIONS.contains(&lower.as_str())
    })
}

fn home_directory() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// Expand a leading `~` (there is no shell to do it for us).
fn expand_home(entry: &str, home: Option<&Path>) -> PathBuf {
    if entry == "~" {
        return home.map_or_else(|| PathBuf::from(entry), Path::to_path_buf);
    }
    match (entry.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(entry),
    }
}

/// Literal directory a glob starts from, and the pattern segments below it.
fn split_glob(pattern: &Path) -> (PathBuf, Vec<String>) {
    let mut base = PathBuf::new();
    let mut segments = Vec::new();
    for component in pattern.components() {
        match component {
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                if !segments.is_empty() || has_glob_characters(&text) {
                    segments.push(text.into_owned());
                } else {
                    base.push(name);
                }
            }
            other if segments.is_empty() => base.push(other.as_os_str()),
            _ => {}
        }
    }
    if base.as_os_str().is_empty() {
        base.push(".");
    }
    (base, segments)
}

/// Check source syntax without contacting a filesystem or network. Missing
/// files and folders are reported by the cancellable discovery worker.
pub fn validate_source(source: &str) -> Result<(), String> {
    validate_source_with_home(source, home_directory().as_deref())
}

fn validate_source_with_home(source: &str, _home: Option<&Path>) -> Result<(), String> {
    // Validation runs in the settings input path: never stat a possibly
    // disconnected filesystem here. Discovery owns availability checks.
    for entry in split_entries(source) {
        if is_http_url(entry) {
            let host = entry.split("://").nth(1).unwrap_or("");
            if host.is_empty() || entry.chars().any(char::is_whitespace) {
                return Err(format!("not a valid URL: {entry}"));
            }
        } else if entry.contains("://") {
            return Err(format!(
                "only http:// and https:// URLs are supported: {entry}"
            ));
        }
    }
    Ok(())
}

struct ScanBudget<'a> {
    stop: &'a AtomicBool,
    remaining: usize,
    deadline: Instant,
}
impl ScanBudget<'_> {
    fn exhausted(&self) -> bool {
        self.remaining == 0 || self.stop.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }
    fn visit(&mut self) -> bool {
        if self.exhausted() {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

pub fn discover_cancellable(source: &str, recursive: bool, stop: &AtomicBool) -> Discovery {
    discover_bounded(source, recursive, home_directory().as_deref(), stop)
}

#[cfg(test)]
fn discover_with_home(source: &str, recursive: bool, home: Option<&Path>) -> Discovery {
    discover_bounded(source, recursive, home, &AtomicBool::new(false))
}

fn discover_bounded(
    source: &str,
    recursive: bool,
    home: Option<&Path>,
    stop: &AtomicBool,
) -> Discovery {
    let mut budget = ScanBudget {
        stop,
        remaining: 20_000,
        deadline: Instant::now() + Duration::from_secs(2),
    };
    let mut discovery = Discovery::default();
    let mut seen: HashSet<MediaInput> = HashSet::new();
    for entry in split_entries(source) {
        if budget.exhausted() {
            break;
        }
        let mut found: Vec<MediaInput> = Vec::new();
        if is_http_url(entry) {
            found.push(MediaInput::Url(entry.to_owned()));
        } else if has_glob_characters(entry) {
            let mut matches = expand_glob(&expand_home(entry, home), &mut budget);
            matches.sort_by_key(|path| path.to_string_lossy().to_lowercase());
            for path in matches {
                collect_path(&path, recursive, false, &mut found, &mut budget);
            }
        } else {
            let path = expand_home(entry, home);
            collect_path(&path, recursive, true, &mut found, &mut budget);
        }
        let matched_nothing = found.is_empty();
        for input in found {
            if discovery.inputs.len() >= MAX_INPUTS {
                break;
            }
            if seen.insert(input.clone()) {
                discovery.inputs.push(input);
            }
        }
        if matched_nothing {
            discovery.unmatched.push(entry.to_owned());
        }
    }
    discovery.limited = budget.exhausted() && !stop.load(Ordering::Relaxed);
    discovery
}

/// Add the videos behind `path`: a file (any extension when `explicit`,
/// otherwise only video extensions) or a folder scanned per `recursive`.
fn collect_path(
    path: &Path,
    recursive: bool,
    explicit: bool,
    out: &mut Vec<MediaInput>,
    budget: &mut ScanBudget<'_>,
) {
    if !budget.visit() {
        return;
    }
    if path.is_dir() {
        scan_directory(path, recursive, out, budget);
    } else if path.is_file() && (explicit || video_extension(path)) {
        out.push(MediaInput::File(path.to_path_buf()));
    }
}

fn scan_directory(
    root: &Path,
    recursive: bool,
    out: &mut Vec<MediaInput>,
    budget: &mut ScanBudget<'_>,
) {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        if budget.exhausted() || files.len() >= MAX_INPUTS {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if !budget.visit() {
                break;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if kind.is_dir() {
                if recursive && !hidden && depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if video_extension(&path) && path.is_file() {
                files.push(path);
            }
            if files.len() >= MAX_INPUTS {
                break;
            }
        }
    }
    files.sort_by_key(|path| path.to_string_lossy().to_lowercase());
    out.extend(files.into_iter().map(MediaInput::File));
}

fn expand_glob(pattern: &Path, budget: &mut ScanBudget<'_>) -> Vec<PathBuf> {
    let (base, segments) = split_glob(pattern);
    let mut out = Vec::new();
    walk_glob(&base, &segments, &mut out, 0, budget);
    out
}

fn sorted_children(directory: &Path, budget: &mut ScanBudget<'_>) -> Vec<(String, PathBuf, bool)> {
    if budget.exhausted() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut children = Vec::new();
    for entry in entries.flatten() {
        if !budget.visit() {
            break;
        }
        children.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
            entry.file_type().is_ok_and(|kind| kind.is_dir()),
        ));
    }
    children.sort();
    children
}

fn walk_glob(
    directory: &Path,
    segments: &[String],
    out: &mut Vec<PathBuf>,
    depth: usize,
    budget: &mut ScanBudget<'_>,
) {
    if budget.exhausted() || out.len() >= MAX_INPUTS || depth > MAX_DEPTH {
        return;
    }
    let Some((segment, rest)) = segments.split_first() else {
        out.push(directory.to_path_buf());
        return;
    };
    if segment == "**" {
        // `**` matches zero or more directories; a trailing `**` means "every file".
        if rest.is_empty() {
            walk_glob(directory, &["*".to_owned()], out, depth, budget);
        } else {
            walk_glob(directory, rest, out, depth, budget);
        }
        for (name, path, is_real_directory) in sorted_children(directory, budget) {
            if is_real_directory && !name.starts_with('.') {
                walk_glob(&path, segments, out, depth + 1, budget);
            }
        }
    } else if has_glob_characters(segment) {
        let pattern: Vec<char> = segment.chars().collect();
        for (name, path, _) in sorted_children(directory, budget) {
            if name.starts_with('.') && !segment.starts_with('.') {
                continue;
            }
            let text: Vec<char> = name.chars().collect();
            if !glob_match(&pattern, &text) {
                continue;
            }
            if rest.is_empty() {
                out.push(path);
            } else if path.is_dir() {
                walk_glob(&path, rest, out, depth + 1, budget);
            }
        }
    } else {
        let next = directory.join(segment);
        if rest.is_empty() {
            if next.exists() {
                out.push(next);
            }
        } else if next.is_dir() {
            walk_glob(&next, rest, out, depth + 1, budget);
        }
    }
}

/// Shell-style matcher: `*`, `?`, `[abc]`, `[a-z]`, `[!x]`. Case-sensitive.
pub fn glob_match(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        let step = match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, t));
                p += 1;
                continue;
            }
            Some('?') => Some(p + 1),
            Some('[') => match_class(pattern, p, text[t]),
            Some(literal) if *literal == text[t] => Some(p + 1),
            _ => None,
        };
        if let Some(next) = step {
            p = next;
            t += 1;
        } else if let Some((star, matched)) = backtrack {
            p = star + 1;
            t = matched + 1;
            backtrack = Some((star, matched + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|character| *character == '*')
}

/// Match `character` against the class starting at `pattern[start] == '['`.
/// Returns the pattern index after the class when it matches. An unterminated
/// class is a literal `[`.
fn match_class(pattern: &[char], start: usize, character: char) -> Option<usize> {
    let mut index = start + 1;
    let negated = matches!(pattern.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let first = index;
    let mut matched = false;
    while let Some(&current) = pattern.get(index) {
        if current == ']' && index > first {
            return (matched != negated).then_some(index + 1);
        }
        if pattern.get(index + 1) == Some(&'-') && pattern.get(index + 2).is_some_and(|c| *c != ']')
        {
            let high = pattern[index + 2];
            matched |= (current..=high).contains(&character);
            index += 3;
        } else {
            matched |= current == character;
            index += 1;
        }
    }
    // No closing bracket: '[' is an ordinary character.
    (character == '[').then_some(start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn matches(pattern: &str, text: &str) -> bool {
        glob_match(
            &pattern.chars().collect::<Vec<_>>(),
            &text.chars().collect::<Vec<_>>(),
        )
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    fn names(discovery: &Discovery, root: &Path) -> Vec<String> {
        discovery
            .inputs
            .iter()
            .map(|input| match input {
                MediaInput::File(path) => path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                MediaInput::Url(url) => url.clone(),
            })
            .collect()
    }

    #[test]
    fn glob_matcher_supports_wildcards_and_classes() {
        assert!(matches("*.mp4", "clip.mp4"));
        assert!(!matches("*.mp4", "clip.mkv"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "ac"));
        assert!(matches("[a-c]x", "bx"));
        assert!(!matches("[a-c]x", "dx"));
        assert!(matches("[!a-c]x", "dx"));
        assert!(matches("*", ""));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("a*b*c", "aXXbYY"));
        assert!(matches("[", "["));
        assert!(matches("caf\u{e9}*", "caf\u{e9} au lait.mp4"));
    }

    #[test]
    fn folder_scan_filters_extensions_and_honours_recursion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("b.MKV"));
        touch(&root.join("a.mp4"));
        touch(&root.join("notes.txt"));
        touch(&root.join("sub/deep.webm"));
        touch(&root.join(".hidden/secret.mp4"));
        let flat = discover_with_home(root.to_str().unwrap(), false, None);
        assert_eq!(names(&flat, root), ["a.mp4", "b.MKV"]);
        let deep = discover_with_home(root.to_str().unwrap(), true, None);
        assert_eq!(names(&deep, root), ["a.mp4", "b.MKV", "sub/deep.webm"]);
        assert!(deep.unmatched.is_empty());
    }

    #[test]
    fn every_listed_extension_is_recognised() {
        for extension in VIDEO_EXTENSIONS {
            assert!(video_extension(Path::new(&format!("x.{extension}"))));
        }
        assert!(!video_extension(Path::new("x.png")));
        assert!(!video_extension(Path::new("noextension")));
    }

    #[test]
    fn globs_expand_files_and_double_star_recurses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("one.mp4"));
        touch(&root.join("two.mp4"));
        touch(&root.join("two.txt"));
        touch(&root.join("nested/three.mp4"));
        let shallow = format!("{}/*.mp4", root.display());
        let found = discover_with_home(&shallow, false, None);
        assert_eq!(names(&found, root), ["one.mp4", "two.mp4"]);
        let deep = format!("{}/**/*.mp4", root.display());
        let found = discover_with_home(&deep, false, None);
        assert_eq!(
            names(&found, root),
            ["nested/three.mp4", "one.mp4", "two.mp4"]
        );
        let everything = format!("{}/**", root.display());
        let found = discover_with_home(&everything, false, None);
        assert_eq!(
            names(&found, root),
            ["nested/three.mp4", "one.mp4", "two.mp4"]
        );
    }

    #[test]
    fn semicolon_lists_mix_files_folders_urls_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("movies/a.mp4"));
        touch(&root.join("odd name \u{e9}\u{4e2d}/-dash.mkv"));
        touch(&root.join("clip.bin"));
        let source = format!(
            "{}/movies; {}/movies/a.mp4 ;{}/clip.bin; https://example.com/v/x.mp4 ;{}/missing.mp4; ;{}/odd*/*.mkv",
            root.display(),
            root.display(),
            root.display(),
            root.display(),
            root.display(),
        );
        let found = discover_with_home(&source, false, None);
        assert_eq!(
            names(&found, root),
            [
                "movies/a.mp4",
                "clip.bin",
                "https://example.com/v/x.mp4",
                "odd name \u{e9}\u{4e2d}/-dash.mkv",
            ]
        );
        assert_eq!(found.unmatched.len(), 1);
        assert!(found.unmatched[0].ends_with("missing.mp4"));
    }

    #[test]
    fn home_directory_is_expanded() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Videos/a.mp4"));
        let found = discover_with_home("~/Videos", false, Some(dir.path()));
        assert_eq!(found.inputs.len(), 1);
        assert!(validate_source_with_home("~/Videos", Some(dir.path())).is_ok());
        assert!(validate_source_with_home("~/Nope", Some(dir.path())).is_ok());
    }

    #[test]
    fn validation_reports_what_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.mp4"));
        let ok = |text: &str| validate_source_with_home(text, None).is_ok();
        assert!(ok(""));
        assert!(ok("https://example.com/a.mp4"));
        assert!(ok("http://example.com/live"));
        assert!(ok(dir.path().to_str().unwrap()));
        assert!(ok(&format!("{}/*.mp4", dir.path().display())));
        assert!(!ok("ftp://example.com/a.mp4"));
        assert!(!ok("https://"));
        assert!(ok("/definitely/not/here.mp4"));
        assert!(ok("/definitely/not/here/*.mp4"));
    }

    #[test]
    fn editing_a_source_does_not_require_touching_its_filesystem() {
        // Path availability belongs to the cancellable discovery worker. A
        // disconnected mount or unavailable folder must still be editable.
        assert!(validate_source_with_home("/synthetic-unavailable-mount/video", None).is_ok());
        assert!(validate_source_with_home("/synthetic-unavailable-mount/**/*.mp4", None).is_ok());
    }

    #[test]
    fn folder_discovery_obeys_entry_budget_and_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..256 {
            touch(&directory.path().join(format!("clip-{index}.mp4")));
        }
        let stop = AtomicBool::new(false);
        let mut budget = ScanBudget {
            stop: &stop,
            remaining: 32,
            deadline: Instant::now() + Duration::from_secs(10),
        };
        let mut inputs = Vec::new();
        scan_directory(directory.path(), true, &mut inputs, &mut budget);
        assert_eq!(inputs.len(), 32);
        assert!(budget.exhausted());
        stop.store(true, Ordering::Relaxed);
        let found = discover_cancellable(directory.path().to_str().unwrap(), true, &stop);
        assert!(found.inputs.is_empty());
    }

    #[test]
    fn display_names_and_arguments_are_safe() {
        let file = MediaInput::File(PathBuf::from("/v/-weird name.mkv"));
        assert_eq!(file.display_name(), "-weird name.mkv");
        assert_eq!(
            file.ffmpeg_argument(),
            OsString::from("file:/v/-weird name.mkv")
        );
        let url = MediaInput::Url("https://cdn.example.com/a/b/clip.mp4?token=1".to_owned());
        assert_eq!(url.display_name(), "clip.mp4");
        assert_eq!(
            MediaInput::Url("https://example.com/".to_owned()).display_name(),
            "example.com"
        );
        assert!(url.is_url() && !file.is_url());
    }
}
