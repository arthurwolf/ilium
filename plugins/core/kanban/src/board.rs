//! The board model and its pure text rules: the Markdown board parse and serialize, folder-card
//! parse and serialize, title validation, task-checkbox scan and the plain-text extract.
//!
//! Moved from `ilium-client/src/board.rs`. File I/O, locking, revision checks and rollback stay out
//! of this module: the plugin reads and writes through the `fs` host import (G10) and hands the
//! text to these functions.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardCard {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardColumn {
    pub title: String,
    pub cards: Vec<BoardCard>,
}

/// Byte offsets of every `[ ]`, `[x]` and `[X]` marker in a card title, with its checked state.
pub fn checkbox_occurrences(title: &str) -> Vec<(usize, bool)> {
    title
        .as_bytes()
        .windows(3)
        .enumerate()
        .filter_map(|(index, marker)| match marker {
            b"[ ]" => Some((index, false)),
            b"[x]" | b"[X]" => Some((index, true)),
            _ => None,
        })
        .collect()
}

/// Plain-text rendering of a board's columns and cards. Never written to disk and never parsed
/// back: it is only a content extract for context gathering.
pub fn board_text_extract(columns: &[BoardColumn]) -> String {
    let mut extract = String::new();
    for column in columns {
        extract.push_str("## ");
        extract.push_str(&column.title);
        extract.push('\n');
        for card in &column.cards {
            extract.push_str("- ");
            extract.push_str(&card.title);
            let body = card.body.trim();
            if !body.is_empty() {
                extract.push_str(": ");
                extract.push_str(body);
            }
            extract.push('\n');
        }
    }
    extract
}

/// Parses a folder-board card file: a `# Title` first line, an optional blank line, then the body.
/// Without a usable heading the whole text is the body and `fallback_title` is the title.
pub fn parse_folder_card(source: &str, fallback_title: String) -> (String, String) {
    let mut lines = source.lines();
    let Some(first_line) = lines.next() else {
        return (fallback_title, String::new());
    };
    let Some(title) = first_line
        .strip_prefix("# ")
        .map(str::trim)
        .filter(|title| !title.is_empty())
    else {
        return (fallback_title, source.trim_end().to_string());
    };
    let mut body_lines = lines.collect::<Vec<_>>();
    if body_lines.first().is_some_and(|line| line.is_empty()) {
        body_lines.remove(0);
    }
    (
        title.to_string(),
        body_lines.join("\n").trim_end().to_string(),
    )
}

/// The file text of one folder-board card.
pub fn serialize_folder_card(card: &BoardCard) -> String {
    if card.body.is_empty() {
        format!("# {}\n", card.title)
    } else {
        format!("# {}\n\n{}\n", card.title, card.body.trim_end())
    }
}

/// Parses a Markdown board: headings are columns, list items are cards, indented lines under a
/// card are its body, fenced blocks are body text, and a lone top-level `#` document title is
/// dropped when other columns follow.
pub fn parse_markdown_board(source: &str) -> Vec<BoardColumn> {
    let mut parsed_columns: Vec<(usize, BoardColumn)> = Vec::new();
    let mut current_column: Option<(usize, BoardColumn)> = None;
    let mut is_in_fenced_code_block = false;

    for line in source.lines() {
        let trimmed_line = line.trim_start();
        // Body lines keep everything beyond the canonical two-space prefix so an indented body
        // line round-trips; other indent styles fall back to the fully trimmed line.
        let body_line = line.strip_prefix("  ").unwrap_or(trimmed_line);
        if trimmed_line.starts_with("```") || trimmed_line.starts_with("~~~") {
            append_markdown_card_body_line(&mut current_column, body_line);
            is_in_fenced_code_block = !is_in_fenced_code_block;
            continue;
        }
        if is_in_fenced_code_block {
            append_markdown_card_body_line(&mut current_column, body_line);
            continue;
        }

        // Only a non-indented line can start a heading.
        let is_indented_line = line.len() != trimmed_line.len();
        if !is_indented_line {
            if let Some((level, title)) = markdown_heading(trimmed_line) {
                if let Some(column) = current_column.take() {
                    parsed_columns.push(column);
                }
                current_column = Some((
                    level,
                    BoardColumn {
                        title: title.to_string(),
                        cards: Vec::new(),
                    },
                ));
                continue;
            }
        }

        let Some(title) = markdown_list_item(trimmed_line) else {
            if is_indented_line {
                append_markdown_card_body_line(&mut current_column, body_line);
            } else if trimmed_line.is_empty() {
                append_markdown_card_body_line(&mut current_column, "");
            }
            continue;
        };
        let column = current_column.get_or_insert_with(|| {
            (
                0,
                BoardColumn {
                    title: "Items".to_string(),
                    cards: Vec::new(),
                },
            )
        });
        column.1.cards.push(BoardCard {
            title: title.to_string(),
            body: String::new(),
        });
    }

    if let Some(column) = current_column {
        parsed_columns.push(column);
    }
    if parsed_columns.len() > 1 && parsed_columns[0].0 == 1 && parsed_columns[0].1.cards.is_empty()
    {
        parsed_columns.remove(0);
    }
    parsed_columns
        .into_iter()
        .map(|(_, mut column)| {
            for card in &mut column.cards {
                card.body = card.body.trim_end().to_string();
            }
            column
        })
        .collect()
}

fn append_markdown_card_body_line(current_column: &mut Option<(usize, BoardColumn)>, line: &str) {
    let Some(card) = current_column
        .as_mut()
        .and_then(|(_, column)| column.cards.last_mut())
    else {
        return;
    };
    if !card.body.is_empty() {
        card.body.push('\n');
    }
    card.body.push_str(line);
}

/// Parses a non-indented ATX heading and rejects empty marker-only headings.
fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let marker_count = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&marker_count) {
        return None;
    }
    let title = line.get(marker_count..)?.strip_prefix(' ')?.trim();
    // A closing sequence ("## Title ##") only counts when whitespace separates it from the text;
    // a trailing '#' glued to the text is part of the title ("C#").
    let stripped = title.trim_end_matches('#');
    let title = if stripped.len() != title.len()
        && (stripped.is_empty() || stripped.ends_with(char::is_whitespace))
    {
        stripped.trim_end()
    } else {
        title
    };
    (!title.is_empty()).then_some((marker_count, title))
}

/// The visible text of an unordered Markdown list item.
fn markdown_list_item(line: &str) -> Option<&str> {
    let title = ["- ", "* ", "+ "]
        .into_iter()
        .find_map(|marker| line.strip_prefix(marker))?
        .trim();
    (!title.is_empty()).then_some(title)
}

/// The Markdown text of a whole board.
pub fn serialize_markdown_board(columns: &[BoardColumn]) -> String {
    let mut source = String::from("# Board\n\n");
    for column in columns {
        source.push_str(&format!("## {}\n", column.title));
        for card in &column.cards {
            source.push_str(&format!("- {}\n", card.title));
            for body_line in card.body.lines() {
                source.push_str("  ");
                source.push_str(body_line);
                source.push('\n');
            }
        }
        source.push('\n');
    }
    source
}

/// A card title must be one non-empty line: it is stored as one `- ` or `# ` line, and an embedded
/// break would reparse as unrelated cards on the next load.
pub fn validate_card_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A card needs a title".to_string());
    }
    if title.contains(['\n', '\r']) {
        return Err("A card title must be a single line".to_string());
    }
    Ok(title.to_string())
}

/// A column title becomes a directory name and a `## ` line, so separators, dot names and line
/// breaks are rejected.
pub fn validate_column_title(title: impl AsRef<str>) -> Result<String, String> {
    let title = title.as_ref().trim();
    if title.is_empty()
        || title == "."
        || title == ".."
        || title
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\n' | '\r'))
    {
        return Err(format!("Invalid board column name: {title}"));
    }
    Ok(title.to_string())
}

/// A file-name-safe lowercase slug of at most 80 characters; `card` when nothing is left.
pub fn safe_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = sanitized
        .trim_matches('-')
        .chars()
        .take(80)
        .collect::<String>();
    if sanitized.is_empty() {
        "card".to_string()
    } else {
        sanitized
    }
}
