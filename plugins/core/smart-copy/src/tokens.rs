//! Fine-grained Smart Copy token rules: the pure helpers of `ilium-client/src/smart_copy_tokens.rs`
//! that need no regular expression and no terminal screen (URL tail trimming, path plausibility,
//! hash plausibility, label clipping). The regex based line scanner and the vt100 style runs stay
//! to be moved once the plugin can read screen text (see tests/API_GAPS.md).

/// File extensions that make a bare relative path look like a file.
pub const FILE_EXTENSIONS: &str = "rs|toml|lock|md|mdx|txt|json|jsonl|yaml|yml|ts|tsx|js|jsx|mjs|cjs|vue|svelte|py|pyi|go|mod|sum|c|h|cc|cpp|hpp|cxx|hxx|java|kt|kts|swift|rb|php|sh|zsh|bash|fish|ps1|bat|css|scss|sass|less|html|htm|sql|xml|csv|tsv|log|conf|cfg|ini|env|lua|zig|cs|dart|ex|exs|erl|hs|ml|nix|svg|png|jpg|jpeg|gif|webp|pdf|zip|tar|gz|tgz|xz|wasm|proto|graphql|tf|service|timer|desktop|diff|patch|rst|tex|bib|ipynb|lockb|plist|gradle|cmake|make|mk|rlib|so|dll|exe|bin|out|a|o";

/// First path segments that make a bare relative path look like a project path.
pub const KNOWN_DIRECTORIES: &[&str] = &[
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

/// Well-known extensionless file names.
pub const BARE_FILE_NAMES: &str =
    "Makefile|Dockerfile|Justfile|Procfile|Gemfile|Rakefile|LICENSE|Vagrantfile|CODEOWNERS";

/// Removes trailing characters of `text[start..end]` that appear in `characters`; returns the new
/// end byte offset.
pub fn trim_trailing(text: &str, start: usize, mut end: usize, characters: &[char]) -> usize {
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

/// A single-line label of at most 36 characters, whitespace collapsed, clipped with an ellipsis.
pub fn short(text: &str) -> String {
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

/// Byte length of `url` after dropping trailing sentence punctuation and unbalanced closers.
pub fn url_end(url: &str) -> usize {
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

/// Whether the last path segment carries a known file extension.
pub fn has_source_extension(path: &str) -> bool {
    let Some(name) = path.rsplit('/').next() else {
        return false;
    };
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    FILE_EXTENSIONS.split('|').any(|known| known == extension)
}

/// Whether a path-shaped token is worth offering even when it cannot be found on disk.
pub fn path_is_plausible(path: &str) -> bool {
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

/// The label of a path token.
pub fn path_kind_label(path: &str) -> String {
    format!("path {}", short(path))
}

/// A hash candidate has a digit and a letter and a git/sha length (7 to 12, 40 or 64).
pub fn is_plausible_hash(candidate: &str) -> bool {
    let has_digit = candidate.chars().any(|c| c.is_ascii_digit());
    let has_letter = candidate.chars().any(|c| c.is_ascii_alphabetic());
    has_digit && has_letter && matches!(candidate.len(), 7..=12 | 40 | 64)
}

/// Number of ASCII digits in `text`.
pub fn digit_count(text: &str) -> usize {
    text.chars().filter(char::is_ascii_digit).count()
}
