//! Local kanban document model and its two user-owned storage adapters.

use std::fs;
use std::path::{Path, PathBuf};

use ilium_core::BoardStorage;

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
#[derive(Debug, Clone)]
pub struct BoardPane {
    pub storage: BoardStorage,
    pub columns: Vec<BoardColumn>,
    pub selected_column: usize,
    pub selected_card: usize,
    pub status_message: Option<String>,
    pub drag_source: Option<(usize, usize)>,
}

impl BoardPane {
    pub fn load(storage: BoardStorage) -> Result<Self, String> {
        let columns = match &storage {
            BoardStorage::Folder { path } => load_folder_board(path)?,
            BoardStorage::MarkdownFile { path } => load_markdown_board(path)?,
        };
        Ok(Self {
            storage,
            columns,
            selected_column: 0,
            selected_card: 0,
            status_message: None,
            drag_source: None,
        })
    }
    pub fn create(storage: BoardStorage) -> Result<Self, String> {
        let board = Self {
            storage,
            columns: vec![
                BoardColumn {
                    title: "To do".to_string(),
                    cards: Vec::new(),
                },
                BoardColumn {
                    title: "Doing".to_string(),
                    cards: Vec::new(),
                },
                BoardColumn {
                    title: "Done".to_string(),
                    cards: Vec::new(),
                },
            ],
            selected_column: 0,
            selected_card: 0,
            status_message: None,
            drag_source: None,
        };
        board.save()?;
        Ok(board)
    }
    pub fn save(&self) -> Result<(), String> {
        match &self.storage {
            BoardStorage::Folder { path } => save_folder_board(path, &self.columns),
            BoardStorage::MarkdownFile { path } => save_markdown_board(path, &self.columns),
        }
    }
    pub fn select_next_column(&mut self) {
        if !self.columns.is_empty() {
            self.selected_column = (self.selected_column + 1) % self.columns.len();
            self.selected_card = self.selected_card.min(
                self.columns[self.selected_column]
                    .cards
                    .len()
                    .saturating_sub(1),
            );
        }
    }
    pub fn select_previous_column(&mut self) {
        if !self.columns.is_empty() {
            self.selected_column =
                (self.selected_column + self.columns.len() - 1) % self.columns.len();
            self.selected_card = self.selected_card.min(
                self.columns[self.selected_column]
                    .cards
                    .len()
                    .saturating_sub(1),
            );
        }
    }
    pub fn select_next_card(&mut self) {
        if let Some(column) = self.columns.get(self.selected_column) {
            if !column.cards.is_empty() {
                self.selected_card = (self.selected_card + 1) % column.cards.len();
            }
        }
    }
    pub fn select_previous_card(&mut self) {
        if let Some(column) = self.columns.get(self.selected_column) {
            if !column.cards.is_empty() {
                self.selected_card =
                    (self.selected_card + column.cards.len() - 1) % column.cards.len();
            }
        }
    }
    pub fn add_card(&mut self, title: String) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A card needs a title".to_string());
        }
        let column = self
            .columns
            .get_mut(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        column.cards.push(BoardCard {
            title: title.to_string(),
            body: String::new(),
        });
        self.selected_card = column.cards.len() - 1;
        self.save()
    }
    pub fn add_column(&mut self, title: String) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A column needs a title".to_string());
        }
        self.columns.push(BoardColumn {
            title: title.to_string(),
            cards: Vec::new(),
        });
        self.selected_column = self.columns.len() - 1;
        self.selected_card = 0;
        self.save()
    }

    pub fn rename_selected_card(&mut self, title: String) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("A card needs a title".to_string());
        }
        let card = self
            .columns
            .get_mut(self.selected_column)
            .and_then(|column| column.cards.get_mut(self.selected_card))
            .ok_or_else(|| "No card selected".to_string())?;
        card.title = title.to_string();
        self.save()
    }

    pub fn rename_selected_column(&mut self, title: String) -> Result<(), String> {
        let title = validate_column_title(title)?;
        let current_title = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?
            .title
            .clone();
        if current_title == title {
            return Ok(());
        }
        if self.columns.iter().any(|column| column.title == title) {
            return Err("A column with that name already exists".to_string());
        }
        if let BoardStorage::Folder { path } = &self.storage {
            let source = path.join(&current_title);
            let destination = path.join(&title);
            if destination.exists() {
                return Err(format!("{} already exists", destination.display()));
            }
            fs::rename(&source, &destination)
                .map_err(|error| format!("Could not rename {}: {error}", source.display()))?;
        }
        self.columns[self.selected_column].title = title;
        self.save()
    }

    pub fn delete_selected_card(&mut self) -> Result<(), String> {
        let column = self
            .columns
            .get_mut(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        if self.selected_card >= column.cards.len() {
            return Err("No card selected".to_string());
        }
        column.cards.remove(self.selected_card);
        self.selected_card = self.selected_card.min(column.cards.len().saturating_sub(1));
        self.save()
    }

    pub fn delete_selected_column(&mut self) -> Result<(), String> {
        if self.columns.len() <= 1 {
            return Err("A board must keep at least one column".to_string());
        }
        let column = self
            .columns
            .get(self.selected_column)
            .ok_or_else(|| "No board column selected".to_string())?;
        if !column.cards.is_empty() {
            return Err("Move or remove this column's cards first".to_string());
        }
        if let BoardStorage::Folder { path } = &self.storage {
            let column_path = path.join(&column.title);
            if column_path.exists() {
                fs::remove_dir(&column_path).map_err(|error| {
                    format!(
                        "Could not remove {} (it may contain non-card files): {error}",
                        column_path.display()
                    )
                })?;
            }
        }
        self.columns.remove(self.selected_column);
        self.selected_column = self
            .selected_column
            .min(self.columns.len().saturating_sub(1));
        self.selected_card = self.columns[self.selected_column]
            .cards
            .len()
            .saturating_sub(1);
        self.save()
    }
    pub fn move_selected_card(&mut self, direction: isize) -> Result<(), String> {
        let destination = (self.selected_column as isize + direction)
            .clamp(0, self.columns.len().saturating_sub(1) as isize)
            as usize;
        if destination == self.selected_column {
            return Ok(());
        }
        let card = self
            .columns
            .get_mut(self.selected_column)
            .and_then(|column| {
                (self.selected_card < column.cards.len())
                    .then(|| column.cards.remove(self.selected_card))
            })
            .ok_or_else(|| "No card selected".to_string())?;
        self.columns[destination].cards.push(card);
        self.selected_column = destination;
        self.selected_card = self.columns[destination].cards.len() - 1;
        self.save()
    }
}

fn load_folder_board(path: &Path) -> Result<Vec<BoardColumn>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut columns = Vec::new();
    for entry in read_sorted(path)? {
        if !entry.is_dir() {
            continue;
        }
        let mut cards = Vec::new();
        for card_path in read_sorted(&entry)? {
            if card_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                let body = fs::read_to_string(&card_path)
                    .map_err(|error| format!("Could not read {}: {error}", card_path.display()))?;
                let title = body
                    .lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        card_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .replace('-', " ")
                    });
                cards.push(BoardCard { title, body });
            }
        }
        columns.push(BoardColumn {
            title: entry
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            cards,
        });
    }
    Ok(columns)
}
fn load_markdown_board(path: &Path) -> Result<Vec<BoardColumn>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let source = fs::read_to_string(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let mut columns = Vec::new();
    let mut current: Option<BoardColumn> = None;
    for line in source.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(column) = current.take() {
                columns.push(column);
            }
            current = Some(BoardColumn {
                title: title.trim().to_string(),
                cards: Vec::new(),
            });
        } else if let Some(title) = line.strip_prefix("- ") {
            if let Some(column) = current.as_mut() {
                column.cards.push(BoardCard {
                    title: title.trim().to_string(),
                    body: String::new(),
                });
            }
        }
    }
    if let Some(column) = current {
        columns.push(column);
    }
    Ok(columns)
}
fn save_folder_board(path: &Path, columns: &[BoardColumn]) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    for column in columns {
        // A column title is a directory name, not a generated identifier:
        // preserving it lets a manually maintained folder board round-trip
        // its visible column names. Reject separators and traversal rather
        // than silently redirecting a board write outside its root.
        validate_column_title(&column.title)?;
        let column_path = path.join(&column.title);
        fs::create_dir_all(&column_path)
            .map_err(|error| format!("Could not create {}: {error}", column_path.display()))?;
        // Cards are the Markdown files in a board column. Clear only those
        // files before rewriting this column: non-Markdown files and unknown
        // sibling directories are user-owned and must never be erased by a
        // board update.
        for existing_card in read_sorted(&column_path)? {
            if existing_card
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                fs::remove_file(&existing_card).map_err(|error| {
                    format!("Could not update {}: {error}", existing_card.display())
                })?;
            }
        }
        for (index, card) in column.cards.iter().enumerate() {
            let card_path =
                column_path.join(format!("{:03}-{}.md", index + 1, safe_name(&card.title)));
            let body = if card.body.trim().is_empty() {
                format!("# {}\n", card.title)
            } else {
                card.body.clone()
            };
            fs::write(&card_path, body)
                .map_err(|error| format!("Could not write {}: {error}", card_path.display()))?;
        }
    }
    Ok(())
}

fn validate_column_title(title: impl AsRef<str>) -> Result<String, String> {
    let title = title.as_ref().trim();
    if title.is_empty()
        || title == "."
        || title == ".."
        || title.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(format!("Invalid board column name: {title}"));
    }
    Ok(title.to_string())
}
fn save_markdown_board(path: &Path, columns: &[BoardColumn]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let mut source = String::from("# Board\n\n");
    for column in columns {
        source.push_str(&format!("## {}\n", column.title));
        for card in &column.cards {
            source.push_str(&format!("- {}\n", card.title));
        }
        source.push('\n');
    }
    fs::write(path, source).map_err(|error| format!("Could not write {}: {error}", path.display()))
}
fn read_sorted(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    Ok(paths)
}
fn safe_name(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ilium-board-{label}-{unique}"))
    }

    #[test]
    fn markdown_storage_round_trips_columns_and_cards() {
        let path = temporary_path("markdown").join("work.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Ship board".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].cards[0].title, "Ship board");
        assert!(path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn folder_storage_creates_a_column_directory_and_markdown_card() {
        let path = temporary_path("folder");
        let storage = BoardStorage::Folder { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Write tests".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        let todo = reloaded
            .columns
            .iter()
            .find(|column| column.title == "To do")
            .unwrap();
        assert_eq!(todo.cards[0].title, "Write tests");
        assert!(path.join("To do").is_dir());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn rename_and_delete_keep_markdown_storage_in_sync() {
        let path = temporary_path("mutations").join("board.md");
        let storage = BoardStorage::MarkdownFile { path: path.clone() };
        let mut board = BoardPane::create(storage.clone()).unwrap();
        board.add_card("Draft".to_string()).unwrap();
        board.rename_selected_card("Publish".to_string()).unwrap();
        board.delete_selected_card().unwrap();
        board.rename_selected_column("Ideas".to_string()).unwrap();
        let reloaded = BoardPane::load(storage).unwrap();
        assert_eq!(reloaded.columns[0].title, "Ideas");
        assert!(reloaded.columns[0].cards.is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn folder_save_preserves_non_markdown_files_in_column() {
        let path = temporary_path("preserve");
        let storage = BoardStorage::Folder { path: path.clone() };
        let mut board = BoardPane::create(storage).unwrap();
        let attachment = path.join("To do").join("notes.txt");
        fs::write(&attachment, "keep me").unwrap();
        board.add_card("Task".to_string()).unwrap();
        assert_eq!(fs::read_to_string(attachment).unwrap(), "keep me");
        let _ = fs::remove_dir_all(path);
    }
}
