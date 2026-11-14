//! Line lexer behind the plugin's token provider (core-plugin-editor.md "Be the token provider").
//!
//! Pure and synchronous: a block of lines and an optional carried state go in, character-column
//! tokens and the state to carry into the next block come out. The state is one byte, the depth of
//! an open block comment. Columns count Unicode scalar values, never bytes, so a token of a line
//! with multi-byte characters still lands on the right cells. Languages are described by a table
//! ([`LANGUAGES`]); a new language is a new row, not a new branch.

use ilium_plugin_sdk::generated::exports::token_provider::{Token, TokenKind};

/// Most open block comments a lexer state can carry; deeper nesting is clamped.
const MAX_COMMENT_DEPTH: u8 = 16;

/// How one language is lexed.
struct Language {
    /// The canonical name first, then aliases an `editor-opened` buffer may carry.
    names: &'static [&'static str],
    /// File extensions (lower case, no dot) and whole file names the language claims.
    extensions: &'static [&'static str],
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    /// Opening and closing marker of a block comment.
    block_comment: Option<(&'static str, &'static str)>,
    /// Block comments of the language nest (Rust).
    nested_comments: bool,
    /// Characters that open and close a string literal on one line.
    string_delimiters: &'static [char],
    /// Identifiers may start with `$` (JavaScript family).
    dollar_identifiers: bool,
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const JAVASCRIPT_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "constructor",
    "continue",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "keyof",
    "let",
    "namespace",
    "new",
    "null",
    "number",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "string",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield",
];

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "namespace",
    "new",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "typedef",
    "union",
    "unsigned",
    "using",
    "void",
    "volatile",
    "while",
];

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "throws",
    "try",
    "void",
    "while",
];

const SHELL_KEYWORDS: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
    "local", "return", "then", "until", "while",
];

const TOML_KEYWORDS: &[&str] = &["false", "true"];

const MAKE_KEYWORDS: &[&str] = &[
    "define", "else", "endef", "endif", "export", "ifdef", "ifeq", "ifndef", "ifneq", "include",
    "override",
];

const DOCKER_KEYWORDS: &[&str] = &[
    "ADD",
    "ARG",
    "CMD",
    "COPY",
    "ENTRYPOINT",
    "ENV",
    "EXPOSE",
    "FROM",
    "HEALTHCHECK",
    "LABEL",
    "RUN",
    "SHELL",
    "STOPSIGNAL",
    "USER",
    "VOLUME",
    "WORKDIR",
];

const SLASH_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

/// Every language the lexer knows. The first row whose `names` or `extensions` match wins.
const LANGUAGES: &[Language] = &[
    Language {
        names: &["rust", "rs"],
        extensions: &["rs"],
        keywords: RUST_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: true,
        string_delimiters: &['"'],
        dollar_identifiers: false,
    },
    Language {
        names: &["javascript", "js", "jsx"],
        extensions: &["js", "jsx", "mjs", "cjs"],
        keywords: JAVASCRIPT_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: false,
        string_delimiters: &['"', '\'', '`'],
        dollar_identifiers: true,
    },
    Language {
        names: &["typescript", "ts", "tsx"],
        extensions: &["ts", "tsx", "mts", "cts"],
        keywords: TYPESCRIPT_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: false,
        string_delimiters: &['"', '\'', '`'],
        dollar_identifiers: true,
    },
    Language {
        names: &["python", "py"],
        extensions: &["py", "pyi"],
        keywords: PYTHON_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
    Language {
        names: &["go"],
        extensions: &["go"],
        keywords: GO_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: false,
        string_delimiters: &['"', '`'],
        dollar_identifiers: false,
    },
    Language {
        names: &["c", "cpp", "c++", "h"],
        extensions: &["c", "h", "cc", "cpp", "cxx", "hpp", "hh"],
        keywords: C_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: false,
        string_delimiters: &['"'],
        dollar_identifiers: false,
    },
    Language {
        names: &["java"],
        extensions: &["java"],
        keywords: JAVA_KEYWORDS,
        line_comments: &["//"],
        block_comment: SLASH_BLOCK,
        nested_comments: false,
        string_delimiters: &['"'],
        dollar_identifiers: false,
    },
    Language {
        names: &["shell", "sh", "bash", "zsh"],
        extensions: &["sh", "bash", "zsh"],
        keywords: SHELL_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
    Language {
        names: &["toml"],
        extensions: &["toml"],
        keywords: TOML_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
    Language {
        names: &["yaml", "yml"],
        extensions: &["yaml", "yml"],
        keywords: TOML_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
    Language {
        names: &["json"],
        extensions: &["json", "jsonc"],
        keywords: &["false", "null", "true"],
        line_comments: &[],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"'],
        dollar_identifiers: false,
    },
    Language {
        names: &["makefile", "make"],
        extensions: &["makefile", "gnumakefile", "mk"],
        keywords: MAKE_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
    Language {
        names: &["dockerfile", "docker"],
        extensions: &["dockerfile"],
        keywords: DOCKER_KEYWORDS,
        line_comments: &["#"],
        block_comment: None,
        nested_comments: false,
        string_delimiters: &['"', '\''],
        dollar_identifiers: false,
    },
];

/// Name of the Markdown language; its lines are lexed by [`markdown_line`], not by a table row.
const MARKDOWN: &str = "markdown";

/// The language a name from an `editor-opened` buffer stands for, or `None` when there is no
/// lexer for it.
pub fn canonical_language(name: &str) -> Option<&'static str> {
    let lowered = name.to_ascii_lowercase();
    if matches!(lowered.as_str(), "markdown" | "md") {
        return Some(MARKDOWN);
    }
    language_by_name(&lowered).and_then(|language| language.names.first().copied())
}

/// The language a file path stands for: a whole well-known file name first (`Makefile`,
/// `Dockerfile`), then the extension. `None` when nothing matches.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let file_name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if let Some(language) = language_by_extension(&file_name) {
        return language.names.first().copied();
    }
    let (stem, extension) = file_name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    if matches!(extension, "md" | "markdown" | "mdx") {
        return Some(MARKDOWN);
    }
    language_by_extension(extension).and_then(|language| language.names.first().copied())
}

fn language_by_name(name: &str) -> Option<&'static Language> {
    LANGUAGES
        .iter()
        .find(|language| language.names.contains(&name))
}

fn language_by_extension(extension: &str) -> Option<&'static Language> {
    LANGUAGES
        .iter()
        .find(|language| language.extensions.contains(&extension))
}

/// Tokenizes a block of lines. `first_line` is the absolute buffer line of `lines[0]`;
/// `state_in` is the state the previous block returned. An unknown language yields no tokens.
pub fn tokenize(
    language: &str,
    first_line: u32,
    lines: &[String],
    state_in: Option<&[u8]>,
) -> (Vec<Token>, Option<Vec<u8>>) {
    let Some(name) = canonical_language(language) else {
        return (Vec::new(), None);
    };
    let mut tokens = Vec::new();
    if name == MARKDOWN {
        for (offset, line) in lines.iter().enumerate() {
            markdown_line(first_line.saturating_add(offset as u32), line, &mut tokens);
        }
        return (tokens, None);
    }
    let Some(spec) = language_by_name(name) else {
        return (Vec::new(), None);
    };
    let mut depth = state_in
        .and_then(|bytes| bytes.first().copied())
        .filter(|_| spec.block_comment.is_some())
        .unwrap_or(0)
        .min(MAX_COMMENT_DEPTH);
    for (offset, line) in lines.iter().enumerate() {
        let line_number = first_line.saturating_add(offset as u32);
        depth = lex_line(spec, line_number, line, depth, &mut tokens);
    }
    let state_out = (depth > 0).then(|| vec![depth]);
    (tokens, state_out)
}

fn push(tokens: &mut Vec<Token>, line: u32, start: usize, end: usize, kind: TokenKind) {
    if end > start {
        tokens.push(Token {
            line,
            start: start as u32,
            length: (end - start) as u32,
            kind,
        });
    }
}

fn starts_with_at(chars: &[char], index: usize, marker: &str) -> bool {
    let mut position = index;
    for expected in marker.chars() {
        if chars.get(position) != Some(&expected) {
            return false;
        }
        position += 1;
    }
    true
}

fn is_identifier_start(character: char, dollar: bool) -> bool {
    character.is_alphabetic() || character == '_' || (dollar && character == '$')
}

fn is_identifier_part(character: char, dollar: bool) -> bool {
    character.is_alphanumeric() || character == '_' || (dollar && character == '$')
}

/// Lexes one line starting inside `depth` open block comments; returns the depth at its end.
fn lex_line(spec: &Language, line: u32, text: &str, depth_in: u8, tokens: &mut Vec<Token>) -> u8 {
    let chars: Vec<char> = text.chars().collect();
    let count = chars.len();
    let mut depth = depth_in;
    let mut index = 0usize;
    // Where the comment being read began on this line (0 when it carried in from above).
    let mut comment_start = 0usize;
    while index < count {
        if depth > 0 {
            let Some((open, close)) = spec.block_comment else {
                depth = 0;
                continue;
            };
            if starts_with_at(&chars, index, close) {
                index += close.chars().count();
                depth -= 1;
                if depth == 0 {
                    push(tokens, line, comment_start, index, TokenKind::Comment);
                }
            } else if spec.nested_comments
                && depth < MAX_COMMENT_DEPTH
                && starts_with_at(&chars, index, open)
            {
                index += open.chars().count();
                depth += 1;
            } else {
                index += 1;
            }
            continue;
        }
        let character = chars[index];
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if spec
            .line_comments
            .iter()
            .any(|marker| starts_with_at(&chars, index, marker))
        {
            push(tokens, line, index, count, TokenKind::Comment);
            index = count;
            continue;
        }
        if let Some((open, _)) = spec.block_comment {
            if starts_with_at(&chars, index, open) {
                comment_start = index;
                index += open.chars().count();
                depth = 1;
                continue;
            }
        }
        if spec.string_delimiters.contains(&character) {
            let start = index;
            index += 1;
            while index < count {
                if chars[index] == '\\' {
                    index += 2;
                } else if chars[index] == character {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            push(tokens, line, start, index.min(count), TokenKind::String);
            index = index.min(count);
            continue;
        }
        let after_identifier = index > 0 && is_identifier_part(chars[index - 1], false);
        if character.is_ascii_digit() && !after_identifier {
            let start = index;
            index += 1;
            while index < count {
                let current = chars[index];
                let decimal_point = current == '.'
                    && chars
                        .get(index + 1)
                        .is_some_and(|next| next.is_ascii_digit());
                if current.is_alphanumeric() || current == '_' || decimal_point {
                    index += 1;
                } else {
                    break;
                }
            }
            push(tokens, line, start, index, TokenKind::Number);
            continue;
        }
        if is_identifier_start(character, spec.dollar_identifiers) {
            let start = index;
            index += 1;
            while index < count && is_identifier_part(chars[index], spec.dollar_identifiers) {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            let kind = if spec.keywords.contains(&word.as_str()) {
                Some(TokenKind::Keyword)
            } else if matches!(chars.get(index), Some('(') | Some('!'))
                && chars.get(index + 1) != Some(&'=')
            {
                Some(TokenKind::Function)
            } else if character.is_uppercase() {
                Some(TokenKind::TypeName)
            } else {
                None
            };
            if let Some(kind) = kind {
                push(tokens, line, start, index, kind);
            }
            continue;
        }
        index += 1;
    }
    if depth > 0 {
        push(tokens, line, comment_start, count, TokenKind::Comment);
    }
    depth
}

/// Markdown has no carried state: a heading line is a keyword run, an inline code span a string.
fn markdown_line(line: u32, text: &str, tokens: &mut Vec<Token>) {
    let chars: Vec<char> = text.chars().collect();
    let count = chars.len();
    let hashes = chars
        .iter()
        .take_while(|&&character| character == '#')
        .count();
    if (1..=6).contains(&hashes) && chars.get(hashes).is_none_or(|next| *next == ' ') {
        push(tokens, line, 0, count, TokenKind::Keyword);
        return;
    }
    let mut index = 0usize;
    while index < count {
        if chars[index] == '`' {
            let start = index;
            index += 1;
            while index < count && chars[index] != '`' {
                index += 1;
            }
            if index < count {
                index += 1;
                push(tokens, line, start, index, TokenKind::String);
            }
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_string()).collect()
    }

    /// REQ: F-020
    #[test]
    fn extension_and_whole_name_pick_the_language() {
        assert_eq!(language_for_path("src/main.rs"), Some("rust"));
        assert_eq!(language_for_path("app.tsx"), Some("typescript"));
        assert_eq!(language_for_path("README.md"), Some(MARKDOWN));
        assert_eq!(language_for_path("sub/Makefile"), Some("makefile"));
        assert_eq!(language_for_path("Dockerfile"), Some("dockerfile"));
        assert_eq!(language_for_path("data.unknownext"), None);
        assert_eq!(language_for_path(".hidden"), None);
    }

    /// REQ: F-020
    #[test]
    fn aliases_resolve_to_the_canonical_name() {
        assert_eq!(canonical_language("RS"), Some("rust"));
        assert_eq!(canonical_language("md"), Some(MARKDOWN));
        assert_eq!(canonical_language("no-such-language"), None);
    }

    /// REQ: F-020
    #[test]
    fn nested_block_comments_carry_their_depth() {
        let (_, state) = tokenize("rust", 0, &lines(&["/* a /* b"]), None);
        assert_eq!(state, Some(vec![2]));
        let (tokens, state) = tokenize("rust", 1, &lines(&["*/ still */ fn"]), state.as_deref());
        assert_eq!(state, None);
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Comment && token.start == 0));
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Keyword && token.start == 12));
    }

    /// REQ: F-020
    #[test]
    fn an_unterminated_string_ends_at_the_line_end() {
        let (tokens, _) = tokenize("rust", 0, &lines(&["let s = \"open"]), None);
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::String && token.start == 8 && token.length == 5));
    }

    /// REQ: F-020
    #[test]
    fn a_trailing_backslash_never_runs_past_the_line() {
        let (tokens, _) = tokenize("rust", 0, &lines(&["\"a\\"]), None);
        for token in &tokens {
            assert!(token.start + token.length <= 3, "{tokens:?}");
        }
    }

    /// REQ: F-020
    #[test]
    fn markdown_headings_and_code_spans_are_tokens() {
        let (tokens, state) = tokenize(MARKDOWN, 0, &lines(&["# Title", "use `code` now"]), None);
        assert_eq!(state, None);
        assert!(tokens.contains(&Token {
            line: 0,
            start: 0,
            length: 7,
            kind: TokenKind::Keyword
        }));
        assert!(tokens.contains(&Token {
            line: 1,
            start: 4,
            length: 6,
            kind: TokenKind::String
        }));
    }
}
