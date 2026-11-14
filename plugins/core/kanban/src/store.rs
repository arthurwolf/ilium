//! The disk layout of a board as pure data: what a folder board looks like as a listing, and the
//! file operations that bring a folder to the state of the columns.
//!
//! Moved from `ilium-client/src/board.rs` (`load_folder_board`, `save_folder_board`). The plugin
//! reads and lists through the `fs` host import, hands the listing to these functions, and runs
//! the returned operations itself (`io`).

use std::collections::BTreeSet;

use crate::board::{
    parse_folder_card, safe_name, serialize_folder_card, validate_column_title, BoardCard,
    BoardColumn,
};

/// The most cards one board may hold.
pub const MAX_CARDS: usize = 8192;
/// The most columns one board may hold.
pub const MAX_COLUMNS: usize = 1024;
/// The most bytes of titles and bodies one board may hold.
pub const MAX_BYTES: usize = 1024 * 1024;

/// One column directory as listed: its name and the `.md` files in it with their text, in name order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnListing {
    pub name: String,
    pub cards: Vec<CardFile>,
}

/// One card file of a column directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFile {
    pub name: String,
    pub text: String,
}

/// Builds the columns of a folder board from its directory listing (column directories sorted by
/// name, card files sorted by name). Rejects a board over the retained limits.
pub fn columns_from_listing(listing: &[ColumnListing]) -> Result<Vec<BoardColumn>, String> {
    let mut columns = Vec::new();
    let mut retained = 0_usize;
    for column in listing {
        let mut cards = Vec::new();
        for file in &column.cards {
            let stem = file
                .name
                .rsplit_once('.')
                .map_or(file.name.as_str(), |(stem, _)| stem);
            let (title, body) = parse_folder_card(&file.text, stem.replace('-', " "));
            retained = retained.saturating_add(title.len() + body.len() + 48);
            if retained > MAX_BYTES {
                return Err("Board content exceeds retained limit".to_string());
            }
            cards.push(BoardCard { title, body });
        }
        columns.push(BoardColumn {
            title: column.name.clone(),
            cards,
        });
    }
    validate_limits(&columns)?;
    Ok(columns)
}

/// Checks the column, card and byte limits of a board.
pub fn validate_limits(columns: &[BoardColumn]) -> Result<(), String> {
    if columns.len() > MAX_COLUMNS {
        return Err("Board column limit exceeded".to_string());
    }
    let mut bytes = 0_usize;
    let mut cards = 0_usize;
    for column in columns {
        cards += column.cards.len();
        bytes = bytes.saturating_add(column.title.len());
        for card in &column.cards {
            bytes = bytes.saturating_add(card.title.len() + card.body.len());
        }
        if bytes > MAX_BYTES || cards > MAX_CARDS {
            return Err("Board content exceeds retained limit".to_string());
        }
    }
    Ok(())
}

/// The file name of the card at `index` (zero based) of a column: `001-title.md`.
pub fn card_file_name(index: usize, card: &BoardCard) -> String {
    format!("{:03}-{}.md", index + 1, safe_name(&card.title))
}

/// One step that brings a folder board to the state of the columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOp {
    /// Create a directory (its parents exist or are created before it).
    CreateDir(String),
    /// Replace or create a file with this text.
    WriteFile { path: String, text: String },
    /// Remove an obsolete card file.
    RemoveFile(String),
}

/// What a folder board holds now: for each column directory the names of the Markdown files in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderState {
    pub columns: Vec<(String, Vec<String>)>,
}

impl FolderState {
    fn files_of(&self, column: &str) -> &[String] {
        self.columns
            .iter()
            .find(|(name, _)| name == column)
            .map_or(&[][..], |(_, files)| files.as_slice())
    }

    fn has_column(&self, column: &str) -> bool {
        self.columns.iter().any(|(name, _)| name == column)
    }
}

/// The operations that make the folder at `root` hold exactly the cards of `columns`. Every
/// write comes before every removal, so a refused write never leaves a column half deleted;
/// only obsolete Markdown files are removed, other files stay.
pub fn plan_folder_save(
    root: &str,
    existing: &FolderState,
    columns: &[BoardColumn],
) -> Result<Vec<FolderOp>, String> {
    validate_limits(columns)?;
    let mut writes = Vec::new();
    let mut removes = Vec::new();
    if !root.is_empty() {
        writes.push(FolderOp::CreateDir(root.to_string()));
    }
    for column in columns {
        validate_column_title(&column.title)?;
        let column_path = join(root, &column.title);
        if !existing.has_column(&column.title) {
            writes.push(FolderOp::CreateDir(column_path.clone()));
        }
        let mut desired = BTreeSet::new();
        for (index, card) in column.cards.iter().enumerate() {
            let name = card_file_name(index, card);
            desired.insert(name.clone());
            writes.push(FolderOp::WriteFile {
                path: join(&column_path, &name),
                text: serialize_folder_card(card),
            });
        }
        for name in existing.files_of(&column.title) {
            if !desired.contains(name) {
                removes.push(FolderOp::RemoveFile(join(&column_path, name)));
            }
        }
    }
    writes.extend(removes);
    Ok(writes)
}

/// True for a file name the board treats as a card (`.md`, any case).
pub fn is_card_file(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("md"))
}

/// A project-relative path of `name` under `parent`.
pub fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(title: &str, body: &str) -> BoardCard {
        BoardCard {
            title: title.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn a_folder_listing_becomes_columns_with_fallback_titles_from_file_names() {
        let listing = vec![
            ColumnListing {
                name: "To do".to_string(),
                cards: vec![
                    CardFile {
                        name: "001-write-docs.md".to_string(),
                        text: "# Write docs\n\nsoon\n".to_string(),
                    },
                    CardFile {
                        name: "002-bare-note.md".to_string(),
                        text: "just text\n".to_string(),
                    },
                ],
            },
            ColumnListing {
                name: "Done".to_string(),
                cards: Vec::new(),
            },
        ];
        let columns = columns_from_listing(&listing).unwrap();
        assert_eq!(columns[0].cards[0], card("Write docs", "soon"));
        assert_eq!(columns[0].cards[1], card("002 bare note", "just text"));
        assert!(columns[1].cards.is_empty());
    }

    #[test]
    fn a_board_over_the_card_limit_is_refused() {
        let columns = vec![BoardColumn {
            title: "A".to_string(),
            cards: (0..=MAX_CARDS).map(|_| card("c", "")).collect(),
        }];
        assert!(validate_limits(&columns).is_err());
    }

    #[test]
    fn folder_save_writes_numbered_cards_and_creates_missing_columns() {
        let columns = vec![BoardColumn {
            title: "To do".to_string(),
            cards: vec![card("Write the migration", "notes")],
        }];
        let ops = plan_folder_save("board", &FolderState::default(), &columns).unwrap();
        assert_eq!(
            ops,
            vec![
                FolderOp::CreateDir("board".to_string()),
                FolderOp::CreateDir("board/To do".to_string()),
                FolderOp::WriteFile {
                    path: "board/To do/001-write-the-migration.md".to_string(),
                    text: "# Write the migration\n\nnotes\n".to_string(),
                },
            ]
        );
    }

    #[test]
    fn folder_save_removes_only_obsolete_markdown_files_after_every_write() {
        let existing = FolderState {
            columns: vec![(
                "To do".to_string(),
                vec!["001-old.md".to_string(), "002-keep.md".to_string()],
            )],
        };
        let columns = vec![BoardColumn {
            title: "To do".to_string(),
            cards: vec![card("keep", ""), card("new", "")],
        }];
        let ops = plan_folder_save("b", &existing, &columns).unwrap();
        assert_eq!(
            ops.last(),
            Some(&FolderOp::RemoveFile("b/To do/001-old.md".to_string()))
        );
        let first_remove = ops
            .iter()
            .position(|op| matches!(op, FolderOp::RemoveFile(_)))
            .unwrap();
        assert!(ops[..first_remove]
            .iter()
            .all(|op| !matches!(op, FolderOp::RemoveFile(_))));
        assert!(ops.contains(&FolderOp::WriteFile {
            path: "b/To do/001-keep.md".to_string(),
            text: "# keep\n".to_string(),
        }));
    }

    #[test]
    fn a_column_name_that_would_leave_the_board_folder_is_refused() {
        let columns = vec![BoardColumn {
            title: "../x".to_string(),
            cards: Vec::new(),
        }];
        assert!(plan_folder_save("b", &FolderState::default(), &columns).is_err());
    }

    #[test]
    fn card_files_are_recognised_by_extension_in_any_case() {
        assert!(is_card_file("a.md"));
        assert!(is_card_file("a.MD"));
        assert!(!is_card_file("a.txt"));
        assert!(!is_card_file("md"));
    }
}
