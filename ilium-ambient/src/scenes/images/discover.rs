//! Finding images: directories, glob patterns and URL lists. Runs on the
//! worker thread only; it does file system I/O.

use super::settings::expand_home;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];
/// Stop collecting after this many images (memory and startup bound).
pub const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 24;
const MAX_DIRECTORIES: usize = 50_000;
pub const MAX_URLS: usize = 500;

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// Split a `;`-separated list, trimming and dropping empty items.
pub fn split_list(text: &str) -> Vec<&str> {
    text.split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn has_glob_chars(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

/// Split a glob pattern into its longest leading directory without wildcards
/// and the remaining segments.
pub fn glob_split(pattern: &Path) -> (PathBuf, Vec<String>) {
    let mut base = PathBuf::new();
    let mut segments = Vec::new();
    let mut in_pattern = false;
    for component in pattern.components() {
        let text = component.as_os_str().to_string_lossy();
        if !in_pattern && matches!(component, Component::Normal(_)) && has_glob_chars(&text) {
            in_pattern = true;
        }
        if in_pattern {
            segments.push(text.into_owned());
        } else {
            base.push(component);
        }
    }
    if base.as_os_str().is_empty() {
        base = PathBuf::from(".");
    }
    (base, segments)
}

/// The longest leading directory of a glob pattern without wildcards.
pub fn glob_base(pattern: &Path) -> PathBuf {
    glob_split(pattern).0
}

/// Match one path segment against a pattern with `*`, `?` and `[a-z]` / `[!a]`.
pub fn segment_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    matches_from(&pattern, &text)
}

fn matches_from(pattern: &[char], text: &[char]) -> bool {
    let Some((&first, rest)) = pattern.split_first() else {
        return text.is_empty();
    };
    match first {
        '*' => (0..=text.len()).any(|skip| matches_from(rest, &text[skip..])),
        '?' => !text.is_empty() && matches_from(rest, &text[1..]),
        '[' => {
            let Some((matched, after)) = match_class(pattern, text.first().copied()) else {
                // Unterminated class: treat '[' literally.
                return text.first() == Some(&'[') && matches_from(rest, &text[1..]);
            };
            !text.is_empty() && matched && matches_from(after, &text[1..])
        }
        literal => text.first() == Some(&literal) && matches_from(rest, &text[1..]),
    }
}

/// Evaluate the class at the start of `pattern` against `candidate`; returns
/// whether it matched and the pattern after the closing bracket.
fn match_class(pattern: &[char], candidate: Option<char>) -> Option<(bool, &[char])> {
    let mut index = 1;
    let negate = matches!(pattern.get(index), Some('!') | Some('^'));
    if negate {
        index += 1;
    }
    let start = index;
    let mut matched = false;
    loop {
        let current = *pattern.get(index)?;
        if current == ']' && index > start {
            let result = candidate.is_some() && (matched != negate);
            return Some((result, &pattern[index + 1..]));
        }
        if pattern.get(index + 1) == Some(&'-') && pattern.get(index + 2).is_some_and(|c| *c != ']')
        {
            let high = pattern[index + 2];
            if let Some(candidate) = candidate {
                if current <= candidate && candidate <= high {
                    matched = true;
                }
            }
            index += 3;
        } else {
            if candidate == Some(current) {
                matched = true;
            }
            index += 1;
        }
    }
}

/// Result of a discovery run.
#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<PathBuf>,
    pub errors: Vec<String>,
    /// True when `MAX_FILES` cut the scan short.
    pub truncated: bool,
}

struct Scan<'a> {
    stop: &'a AtomicBool,
    files: Vec<PathBuf>,
    directories_seen: usize,
    truncated: bool,
    errors: Vec<String>,
}

impl Scan<'_> {
    fn should_stop(&self) -> bool {
        self.truncated
            || self.stop.load(Ordering::Relaxed)
            || self.directories_seen > MAX_DIRECTORIES
    }

    fn add_file(&mut self, path: PathBuf) {
        if self.files.len() >= MAX_FILES {
            self.truncated = true;
        } else {
            self.files.push(path);
        }
    }

    /// Sorted, non-hidden children of `dir` as (path, is_dir).
    fn children(&mut self, dir: &Path, allow_hidden: bool) -> Vec<(PathBuf, bool)> {
        self.directories_seen += 1;
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                self.errors
                    .push(format!("Cannot read {}: {error}", dir.display()));
                return Vec::new();
            }
        };
        let mut children: Vec<(PathBuf, bool)> = entries
            .filter_map(Result::ok)
            .filter(|entry| allow_hidden || !entry.file_name().to_string_lossy().starts_with('.'))
            .map(|entry| {
                let path = entry.path();
                // `Path::is_dir` follows symlinks; depth and directory
                // budgets bound any symlink cycle.
                let is_dir = path.is_dir();
                (path, is_dir)
            })
            .collect();
        children.sort();
        children
    }

    fn walk_plain(&mut self, dir: &Path, recursive: bool, depth: usize) {
        if self.should_stop() || depth > MAX_DEPTH {
            return;
        }
        for (path, is_dir) in self.children(dir, false) {
            if self.should_stop() {
                return;
            }
            if is_dir {
                if recursive {
                    self.walk_plain(&path, true, depth + 1);
                }
            } else if is_image_path(&path) && path.is_file() {
                self.add_file(path);
            }
        }
    }

    fn walk_glob(&mut self, dir: &Path, segments: &[String], depth: usize) {
        let Some((segment, tail)) = segments.split_first() else {
            return;
        };
        if self.should_stop() || depth > MAX_DEPTH {
            return;
        }
        let allow_hidden = segment.starts_with('.');
        let children = self.children(dir, allow_hidden || segment == "**");
        if segment == "**" {
            if tail.is_empty() {
                for (path, is_dir) in &children {
                    if !*is_dir && is_image_path(path) {
                        self.add_file(path.clone());
                    }
                }
            } else {
                self.walk_glob(dir, tail, depth);
            }
            for (path, is_dir) in children {
                let hidden = path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'));
                if is_dir && !hidden {
                    self.walk_glob(&path, segments, depth + 1);
                }
            }
            return;
        }
        for (path, is_dir) in children {
            if self.should_stop() {
                return;
            }
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !segment_matches(segment, &name) {
                continue;
            }
            if tail.is_empty() {
                if !is_dir && is_image_path(&path) {
                    self.add_file(path);
                }
            } else if is_dir {
                self.walk_glob(&path, tail, depth + 1);
            }
        }
    }
}

/// Find every image named by a `;`-separated list of directories and globs.
/// The result is sorted and free of duplicates.
pub fn discover_images(spec: &str, recursive: bool, stop: &AtomicBool) -> Discovery {
    let mut scan = Scan {
        stop,
        files: Vec::new(),
        directories_seen: 0,
        truncated: false,
        errors: Vec::new(),
    };
    for item in split_list(spec) {
        if scan.should_stop() {
            break;
        }
        let expanded = expand_home(item);
        if has_glob_chars(item) {
            let (base, segments) = glob_split(&expanded);
            if !base.is_dir() {
                scan.errors
                    .push(format!("Folder not found: {}", base.display()));
            } else if !segments.is_empty() {
                scan.walk_glob(&base, &segments, 0);
            }
        } else if expanded.is_dir() {
            scan.walk_plain(&expanded, recursive, 0);
        } else if expanded.is_file() && is_image_path(&expanded) {
            scan.add_file(expanded);
        } else {
            scan.errors
                .push(format!("Folder not found: {}", expanded.display()));
        }
    }
    scan.files.sort();
    scan.files.dedup();
    Discovery {
        files: scan.files,
        errors: scan.errors,
        truncated: scan.truncated,
    }
}

/// Lines of a URL list file: one https URL per line, `#` starts a comment.
pub fn parse_url_list_text(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| line.starts_with("https://"))
        .take(MAX_URLS)
        .map(str::to_owned)
        .collect()
}

/// True when a URL points at a text list rather than an image.
pub fn is_url_list_file(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".txt") || lower.ends_with(".lst") || lower.ends_with(".list")
}

/// Short display name of a URL (last path segment or host).
pub fn url_name(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let trimmed = without_query.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if last.is_empty() || trimmed.ends_with("://") {
        url.to_owned()
    } else {
        last.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, b"x").expect("write");
    }

    fn names(discovery: &Discovery, root: &Path) -> Vec<String> {
        discovery
            .files
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn fixture_tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        for file in [
            "a.png",
            "b.JPG",
            "notes.txt",
            "sub/c.jpeg",
            "sub/deeper/d.webp",
            "sub/deeper/e.gif",
            "sub/readme.md",
            "other/f.bmp",
            "other/f.bmp.bak",
            ".hidden/g.png",
            "noext",
        ] {
            touch(&root.path().join(file));
        }
        root
    }

    #[test]
    fn segment_patterns() {
        assert!(segment_matches("*.jpg", "a.jpg"));
        assert!(!segment_matches("*.jpg", "a.png"));
        assert!(segment_matches("img_??.png", "img_01.png"));
        assert!(!segment_matches("img_??.png", "img_1.png"));
        assert!(segment_matches("[a-c]*.png", "bob.png"));
        assert!(!segment_matches("[a-c]*.png", "zed.png"));
        assert!(segment_matches("[!a]x", "bx"));
        assert!(!segment_matches("[!a]x", "ax"));
        assert!(segment_matches("*", ""));
        assert!(segment_matches("a[", "a["), "unterminated class is literal");
    }

    #[test]
    fn plain_directory_recursive_finds_images_and_ignores_the_rest() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let spec = root.path().to_string_lossy().into_owned();
        let found = discover_images(&spec, true, &stop);
        assert_eq!(
            names(&found, root.path()),
            [
                "a.png",
                "b.JPG",
                "other/f.bmp",
                "sub/c.jpeg",
                "sub/deeper/d.webp",
                "sub/deeper/e.gif"
            ]
        );
        assert!(found.errors.is_empty());
    }

    #[test]
    fn non_recursive_stays_in_the_top_directory() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let found = discover_images(&root.path().to_string_lossy(), false, &stop);
        assert_eq!(names(&found, root.path()), ["a.png", "b.JPG"]);
    }

    #[test]
    fn semicolon_lists_merge_and_deduplicate() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let spec = format!(
            "{0}/sub; {0}/other ; {0}/sub/deeper;{0}/a.png",
            root.path().display()
        );
        let found = discover_images(&spec, true, &stop);
        assert_eq!(
            names(&found, root.path()),
            [
                "a.png",
                "other/f.bmp",
                "sub/c.jpeg",
                "sub/deeper/d.webp",
                "sub/deeper/e.gif"
            ]
        );
    }

    #[test]
    fn glob_patterns_select_by_name_and_depth() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let top = format!("{}/*.png", root.path().display());
        assert_eq!(
            names(&discover_images(&top, true, &stop), root.path()),
            ["a.png"]
        );
        let one_level = format!("{}/*/*.*", root.path().display());
        assert_eq!(
            names(&discover_images(&one_level, true, &stop), root.path()),
            ["other/f.bmp", "sub/c.jpeg"],
            "one directory level, hidden directory skipped, non-images ignored"
        );
        let any_depth = format!("{}/**/*.webp", root.path().display());
        assert_eq!(
            names(&discover_images(&any_depth, true, &stop), root.path()),
            ["sub/deeper/d.webp"]
        );
        let everything = format!("{}/sub/**", root.path().display());
        assert_eq!(
            names(&discover_images(&everything, false, &stop), root.path()),
            ["sub/c.jpeg", "sub/deeper/d.webp", "sub/deeper/e.gif"]
        );
        let class = format!("{}/**/[a-c].*", root.path().display());
        assert_eq!(
            names(&discover_images(&class, true, &stop), root.path()),
            ["a.png", "b.JPG", "sub/c.jpeg"]
        );
    }

    #[test]
    fn missing_directories_are_reported_not_fatal() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let spec = format!(
            "{}/nope;{}/other",
            root.path().display(),
            root.path().display()
        );
        let found = discover_images(&spec, true, &stop);
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.errors.len(), 1);
        assert!(found.errors[0].contains("nope"));
    }

    #[test]
    fn a_single_file_entry_is_accepted() {
        let root = fixture_tree();
        let stop = AtomicBool::new(false);
        let found = discover_images(&format!("{}/a.png", root.path().display()), true, &stop);
        assert_eq!(names(&found, root.path()), ["a.png"]);
    }

    #[test]
    fn stop_flag_aborts_the_scan() {
        let root = fixture_tree();
        let stop = AtomicBool::new(true);
        let found = discover_images(&root.path().to_string_lossy(), true, &stop);
        assert!(found.files.is_empty());
    }

    #[test]
    fn url_list_text_parsing() {
        let text =
            "# comment\nhttps://a/1.jpg\n\n  https://b/2.png  \nhttp://insecure/3.png\nftp://x\n";
        assert_eq!(
            parse_url_list_text(text),
            ["https://a/1.jpg", "https://b/2.png"]
        );
        assert!(is_url_list_file("https://x/list.txt?token=1"));
        assert!(!is_url_list_file("https://x/pic.jpg"));
        assert_eq!(url_name("https://x/a/b/pic.jpg?w=100"), "pic.jpg");
        assert_eq!(url_name("https://images.unsplash.com/photo-1"), "photo-1");
    }

    #[test]
    fn glob_base_stops_at_the_first_wildcard() {
        assert_eq!(
            glob_base(Path::new("/a/b/*/c/*.png")),
            PathBuf::from("/a/b")
        );
        assert_eq!(glob_base(Path::new("*.png")), PathBuf::from("."));
        assert_eq!(glob_base(Path::new("/a/b")), PathBuf::from("/a/b"));
    }
}
