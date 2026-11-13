//! Persistable ordering and visibility for the tree's bottom toolbar.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Prefix that marks a persisted custom-agent entry (`custom:<profile id>`).
const CUSTOM_PREFIX: &str = "custom:";

/// Each available tree-toolbar action appears at most once in the layout.
/// `CustomAgent` carries a custom agent profile's stable id, so custom agents
/// are ordinary, individually orderable toolbar entries beside the built-ins.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    CustomAgent(String),
}

impl TreeToolbarEntry {
    /// The built-in entries; custom agents are added per profile.
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

    /// Stable persisted key.
    pub fn key(&self) -> String {
        match self {
            Self::CustomAgent(profile_id) => format!("{CUSTOM_PREFIX}{profile_id}"),
            other => other.builtin_key().to_owned(),
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        if let Some(profile_id) = key.strip_prefix(CUSTOM_PREFIX) {
            return (!profile_id.is_empty()).then(|| Self::CustomAgent(profile_id.to_owned()));
        }
        Self::ALL.into_iter().find(|entry| entry.builtin_key() == key)
    }

    fn builtin_key(&self) -> &'static str {
        match self {
            Self::CustomAgent(_) => "custom",
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

impl Serialize for TreeToolbarEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for TreeToolbarEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Self::from_key(&key)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown toolbar entry `{key}`")))
    }
}

/// An empty list is intentional: removed entries remain available to add.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeToolbarLayout {
    pub entries: Vec<TreeToolbarEntry>,
    /// Profile ids already offered to this layout. A custom agent absent from
    /// `entries` but present here was removed by the user and stays removed.
    pub known_custom_agents: Vec<String>,
}

impl Default for TreeToolbarLayout {
    fn default() -> Self {
        Self {
            entries: TreeToolbarEntry::ALL.to_vec(),
            known_custom_agents: Vec::new(),
        }
    }
}

impl TreeToolbarLayout {
    /// Trailing entries drawn flush right when they end the layout.
    pub const ANCHORED_SUFFIX: [TreeToolbarEntry; 3] = [
        TreeToolbarEntry::ActivityFilter,
        TreeToolbarEntry::Restructure,
        TreeToolbarEntry::Settings,
    ];

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

    /// Entries that can still be added: unused built-ins, then unused custom
    /// agents in profile order.
    pub fn available(&self, profile_ids: &[String]) -> Vec<TreeToolbarEntry> {
        TreeToolbarEntry::ALL
            .into_iter()
            .chain(profile_ids.iter().cloned().map(TreeToolbarEntry::CustomAgent))
            .filter(|entry| !self.entries.contains(entry))
            .collect()
    }

    /// Drops custom entries whose profile no longer exists and appends every
    /// profile this layout has never seen, so a new custom agent appears in
    /// the toolbar immediately and a removed one stays removed. Returns
    /// whether anything changed.
    pub fn reconcile_custom_agents(&mut self, profile_ids: &[String]) -> bool {
        let before = (self.entries.len(), self.known_custom_agents.len());
        let stale = |id: &String| !profile_ids.contains(id);
        let entries_before = self.entries.clone();
        self.entries.retain(|entry| match entry {
            TreeToolbarEntry::CustomAgent(id) => !stale(id),
            _ => true,
        });
        self.known_custom_agents.retain(|id| !stale(id));
        for id in profile_ids {
            if !self.known_custom_agents.contains(id) {
                self.known_custom_agents.push(id.clone());
                let entry = TreeToolbarEntry::CustomAgent(id.clone());
                if !self.entries.contains(&entry) {
                    // Keep the right-anchored suffix (filter, restructure,
                    // settings) at the right edge.
                    let at = if self.entries.ends_with(&Self::ANCHORED_SUFFIX) {
                        self.entries.len() - Self::ANCHORED_SUFFIX.len()
                    } else {
                        self.entries.len()
                    };
                    self.entries.insert(at, entry);
                }
            }
        }
        before != (self.entries.len(), self.known_custom_agents.len())
            || entries_before != self.entries
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
            ..Default::default()
        }
        .normalized();
        assert_eq!(
            layout.entries,
            vec![TreeToolbarEntry::Settings, TreeToolbarEntry::Search]
        );
        assert!(TreeToolbarLayout {
            entries: Vec::new(),
            ..Default::default()
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
            .available(&[])
            .contains(&TreeToolbarEntry::ActivityFilter));
        layout.entries.push(TreeToolbarEntry::ActivityFilter);
        let last = layout.entries.len() - 1;
        assert!(layout.move_entry(last, 0));
        assert_eq!(layout.entries[0], TreeToolbarEntry::ActivityFilter);
        assert!(!layout.move_entry(0, layout.entries.len()));
    }

    #[test]
    fn custom_agents_join_the_layout_once_and_stay_removed() {
        let mut layout = TreeToolbarLayout::default();
        let ids = vec!["a".to_owned(), "b".to_owned()];
        assert!(layout.reconcile_custom_agents(&ids));
        assert_eq!(
            layout.entries[11..13],
            [
                TreeToolbarEntry::CustomAgent("a".into()),
                TreeToolbarEntry::CustomAgent("b".into())
            ]
        );
        assert!(layout.entries.ends_with(&TreeToolbarLayout::ANCHORED_SUFFIX));
        assert!(!layout.reconcile_custom_agents(&ids));
        layout.entries.retain(|e| *e != TreeToolbarEntry::CustomAgent("a".into()));
        assert!(!layout.reconcile_custom_agents(&ids));
        assert_eq!(layout.available(&ids), vec![TreeToolbarEntry::CustomAgent("a".into())]);
        assert!(layout.reconcile_custom_agents(&["b".to_owned()]));
        assert!(!layout.entries.contains(&TreeToolbarEntry::CustomAgent("a".into())));
    }

    #[test]
    fn custom_entries_round_trip_through_toml() {
        let layout = TreeToolbarLayout {
            entries: vec![
                TreeToolbarEntry::Search,
                TreeToolbarEntry::CustomAgent("my-agent".into()),
            ],
            known_custom_agents: vec!["my-agent".into()],
        };
        let text = toml::to_string(&layout).unwrap();
        assert_eq!(toml::from_str::<TreeToolbarLayout>(&text).unwrap(), layout);
    }

    #[test]
    fn layout_saved_before_custom_agents_existed_still_loads() {
        let layout: TreeToolbarLayout =
            toml::from_str("entries = [\"settings\", \"shell\"]\n").unwrap();
        assert_eq!(layout.entries.len(), 2);
        assert!(layout.known_custom_agents.is_empty());
    }

    #[test]
    fn deleted_profiles_leave_the_layout_and_can_return_when_recreated() {
        let mut layout = TreeToolbarLayout::default();
        let ids = vec!["mine".to_owned()];
        assert!(layout.reconcile_custom_agents(&ids));
        assert!(layout
            .entries
            .contains(&TreeToolbarEntry::CustomAgent("mine".into())));
        assert!(layout.reconcile_custom_agents(&[]));
        assert!(!layout
            .entries
            .contains(&TreeToolbarEntry::CustomAgent("mine".into())));
        assert!(layout.known_custom_agents.is_empty());
        assert!(layout.reconcile_custom_agents(&ids));
        assert!(layout
            .entries
            .contains(&TreeToolbarEntry::CustomAgent("mine".into())));
    }
}
