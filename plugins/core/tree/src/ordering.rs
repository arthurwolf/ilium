//! Pure ordering policy of the tree (port of `ilium-client/src/tree_ordering.rs`, the six Order By
//! modes plus the cost mode the cost plugin drives).
//!
//! The server owns one durable manual child vector per container. This module derives a
//! presentation order from that vector without mutating it. Only a parent that accepts normal
//! children (a group or a project) is sorted: the child order of a split view also decides the
//! placement on the right panel and stays structural in every mode.

use std::cmp::Ordering;
use std::collections::HashMap;

use ilium_plugin_sdk::types::{ContainerKind, NodeClass, NodeView};

/// The Order By modes, in the order of the `tree_orderings` ids of the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreeOrder {
    /// The stored order, unchanged.
    #[default]
    Manual,
    /// Containers, folders, agents by provider, terminals, editors, boards; names inside a kind.
    Type,
    /// Youngest first.
    AgeAscending,
    /// Oldest first.
    AgeDescending,
    /// Case-insensitive name, A to Z.
    NameAscending,
    /// Case-insensitive name, Z to A.
    NameDescending,
    /// Most expensive first; driven by the cost plugin rather than chosen in the settings.
    CostDescending,
}

impl TreeOrder {
    /// The ids of the `tree_orderings` contribution, in menu order.
    pub const IDS: [&'static str; 6] = [
        "manual",
        "type",
        "age_ascending",
        "age_descending",
        "name_ascending",
        "name_descending",
    ];

    /// The id used by the settings row `ui.tree_order` and the `tree_orderings` contribution.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Type => "type",
            Self::AgeAscending => "age_ascending",
            Self::AgeDescending => "age_descending",
            Self::NameAscending => "name_ascending",
            Self::NameDescending => "name_descending",
            Self::CostDescending => "cost_descending",
        }
    }

    /// Parses an id; an unknown id is `None` so the caller keeps its previous order.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "manual" => Self::Manual,
            "type" => Self::Type,
            "age_ascending" => Self::AgeAscending,
            "age_descending" => Self::AgeDescending,
            "name_ascending" => Self::NameAscending,
            "name_descending" => Self::NameDescending,
            "cost_descending" => Self::CostDescending,
            _ => return None,
        })
    }

    const fn uses_names(self) -> bool {
        matches!(
            self,
            Self::Type | Self::NameAscending | Self::NameDescending
        )
    }
}

/// What a visible node is, as far as the Type order cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A normal group.
    Group,
    /// A project, a split view or a worktree container.
    OtherContainer,
    /// A folder entry.
    Folder,
    /// A terminal pane running a built-in agent: the rank is 3 Claude, 4 Codex, 5 Antigravity,
    /// 6 for any other signature.
    Agent(u8),
    /// A plain terminal pane.
    Terminal,
    /// An editor pane.
    Editor,
    /// A board pane.
    Board,
}

impl EntryKind {
    /// The Type-order rank: groups first, boards last.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Group => 0,
            Self::OtherContainer => 1,
            Self::Folder => 2,
            Self::Agent(rank) => rank,
            Self::Terminal => 7,
            Self::Editor => 8,
            Self::Board => 9,
        }
    }

    /// The rank of an agent signature: Claude 3, Codex 4, Antigravity 5, anything else 6.
    pub fn agent_rank(signature: &str) -> u8 {
        let lowered = signature.to_lowercase();
        if lowered.contains("claude") {
            3
        } else if lowered.contains("codex") {
            4
        } else if lowered.contains("antigravity") || lowered == "agy" {
            5
        } else {
            6
        }
    }
}

/// One child as the ordering sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Node ids are allocated in creation order and never reused: the larger id is the younger.
    pub id: u64,
    /// The display name.
    pub name: String,
    /// The Type-order kind.
    pub kind: EntryKind,
}

impl Entry {
    /// An entry from the tree view of a node.
    pub fn from_view(view: &NodeView) -> Self {
        let kind = match view.class {
            NodeClass::Container => match view.container {
                Some(ContainerKind::Group) | None => EntryKind::Group,
                Some(_) => EntryKind::OtherContainer,
            },
            NodeClass::Folder => EntryKind::Folder,
            NodeClass::Pane => match view.content_kind.as_deref() {
                Some("ilium.editor") => EntryKind::Editor,
                Some("ilium.board") => EntryKind::Board,
                _ => match &view.agent {
                    Some(agent) => EntryKind::Agent(EntryKind::agent_rank(&agent.signature)),
                    None => EntryKind::Terminal,
                },
            },
        };
        Self {
            id: view.id.0,
            name: view.title.clone(),
            kind,
        }
    }
}

/// Whether a parent of this container kind is sorted at all: groups and projects accept normal
/// children; a split view keeps its structural order in every mode.
pub const fn accepts_normal_children(container: Option<ContainerKind>) -> bool {
    matches!(
        container,
        Some(ContainerKind::Group | ContainerKind::Project)
    )
}

/// The ids of `children` in the requested presentation order. `sortable` is whether the parent
/// accepts normal children; `cost_ranks` is the spend `CostDescending` sorts by, and a node that
/// is missing from it ranks as free, so it sinks below every agent that has spent something.
/// Ties keep the manual order because the sort is stable.
pub fn ordered(
    children: &[Entry],
    sortable: bool,
    order: TreeOrder,
    cost_ranks: &HashMap<u64, f64>,
) -> Vec<u64> {
    if !sortable || order == TreeOrder::Manual {
        return children.iter().map(|entry| entry.id).collect();
    }
    // Lower-case each name once per sort instead of twice per comparison.
    let lowered: HashMap<u64, String> = if order.uses_names() {
        children
            .iter()
            .map(|entry| (entry.id, entry.name.to_lowercase()))
            .collect()
    } else {
        HashMap::new()
    };
    let mut sorted: Vec<&Entry> = children.iter().collect();
    sorted.sort_by(|left, right| compare(left, right, order, cost_ranks, &lowered));
    sorted.into_iter().map(|entry| entry.id).collect()
}

fn compare(
    left: &Entry,
    right: &Entry,
    order: TreeOrder,
    cost_ranks: &HashMap<u64, f64>,
    lowered: &HashMap<u64, String>,
) -> Ordering {
    match order {
        TreeOrder::Manual => Ordering::Equal,
        TreeOrder::Type => left
            .kind
            .rank()
            .cmp(&right.kind.rank())
            .then_with(|| compare_names(left, right, lowered)),
        TreeOrder::AgeAscending => right.id.cmp(&left.id),
        TreeOrder::AgeDescending => left.id.cmp(&right.id),
        TreeOrder::NameAscending => compare_names(left, right, lowered),
        TreeOrder::NameDescending => compare_names(right, left, lowered),
        TreeOrder::CostDescending => {
            let rank = |id: u64| cost_ranks.get(&id).copied().unwrap_or(0.0);
            rank(right.id).total_cmp(&rank(left.id))
        }
    }
}

/// Case-insensitive display-name order with the raw name and then the id as tie-breakers, so
/// names that differ only by case render deterministically.
fn compare_names(left: &Entry, right: &Entry, lowered: &HashMap<u64, String>) -> Ordering {
    let lower = |entry: &Entry| {
        lowered
            .get(&entry.id)
            .cloned()
            .unwrap_or_else(|| entry.name.to_lowercase())
    };
    lower(left)
        .cmp(&lower(right))
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.id.cmp(&right.id))
}

/// The four-byte sort key of the `order` decoration (page sample "Order the tree"): the first four
/// bytes of the lower-case title, base 256.
pub fn sort_key(title: &str) -> i64 {
    title
        .to_lowercase()
        .bytes()
        .take(4)
        .fold(0i64, |key, byte| key * 256 + i64::from(byte))
}
