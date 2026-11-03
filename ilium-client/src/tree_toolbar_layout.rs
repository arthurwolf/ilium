//! Persistable ordering and visibility for the tree's bottom toolbar.

use serde::{Deserialize, Serialize};

/// Each available tree-toolbar action appears at most once in the layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeToolbarEntry {
    Search,
    Project,
    Shell,
    Claude,
    Codex,
    Antigravity,
    Editor,
    Board,
    Group,
    Split,
    Folder,
    ActivityFilter,
    Restructure,
    Settings,
}

impl TreeToolbarEntry {
    pub const ALL: [Self; 14] = [
        Self::Search,
        Self::Project,
        Self::Shell,
        Self::Claude,
        Self::Codex,
        Self::Antigravity,
        Self::Editor,
        Self::Board,
        Self::Group,
        Self::Split,
        Self::Folder,
        Self::ActivityFilter,
        Self::Restructure,
        Self::Settings,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Project => "project",
            Self::Shell => "shell",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Editor => "editor",
            Self::Board => "board",
            Self::Group => "group",
            Self::Split => "split",
            Self::Folder => "folder",
            Self::ActivityFilter => "activity_filter",
            Self::Restructure => "restructure",
            Self::Settings => "settings",
        }
    }
}

/// An empty list is intentional: removed entries remain available to add.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeToolbarLayout {
    pub entries: Vec<TreeToolbarEntry>,
}

impl Default for TreeToolbarLayout {
    fn default() -> Self {
        Self {
            entries: TreeToolbarEntry::ALL.to_vec(),
        }
    }
}

impl TreeToolbarLayout {
    pub fn normalized(mut self) -> Self {
        let mut unique = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            if !unique.contains(&entry) {
                unique.push(entry);
            }
        }
        self.entries = unique;
        self
    }

    pub fn available(&self) -> impl Iterator<Item = TreeToolbarEntry> + '_ {
        TreeToolbarEntry::ALL
            .into_iter()
            .filter(|entry| !self.entries.contains(entry))
    }

    pub fn move_entry(&mut self, index: usize, destination: usize) -> bool {
        if index >= self.entries.len() || destination >= self.entries.len() || index == destination
        {
            return false;
        }
        let entry = self.entries.remove(index);
        self.entries.insert(destination, entry);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_preserves_order_and_empty_layouts() {
        let layout = TreeToolbarLayout {
            entries: vec![
                TreeToolbarEntry::Settings,
                TreeToolbarEntry::Search,
                TreeToolbarEntry::Settings,
            ],
        }
        .normalized();
        assert_eq!(
            layout.entries,
            vec![TreeToolbarEntry::Settings, TreeToolbarEntry::Search]
        );
        assert!(TreeToolbarLayout {
            entries: Vec::new()
        }
        .normalized()
        .entries
        .is_empty());
    }

    #[test]
    fn removed_entries_can_be_added_and_moved_again() {
        let mut layout = TreeToolbarLayout::default();
        layout
            .entries
            .retain(|entry| *entry != TreeToolbarEntry::ActivityFilter);
        assert!(layout
            .available()
            .any(|entry| entry == TreeToolbarEntry::ActivityFilter));
        layout.entries.push(TreeToolbarEntry::ActivityFilter);
        let last = layout.entries.len() - 1;
        assert!(layout.move_entry(last, 0));
        assert_eq!(layout.entries[0], TreeToolbarEntry::ActivityFilter);
        assert!(!layout.move_entry(0, layout.entries.len()));
    }
}
