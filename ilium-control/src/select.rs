//! Selector resolution: turns what a human or agent typed into a node id.

use ilium_core::{NodeId, NodeKind, Tree};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectError {
    #[error("no {wanted} matches {selector:?}")]
    NotFound {
        wanted: &'static str,
        selector: String,
    },
    #[error("{selector:?} is ambiguous; it matches {candidates}")]
    Ambiguous {
        selector: String,
        /// `id (name)` pairs, comma separated, for the error message.
        candidates: String,
    },
    #[error("node {id} is a {actual}, not a {wanted}")]
    WrongKind {
        id: u64,
        actual: &'static str,
        wanted: &'static str,
    },
}

/// Any node by numeric id.
pub fn resolve_node(tree: &Tree, selector: &str) -> Result<NodeId, SelectError> {
    let id = selector
        .trim()
        .parse::<u64>()
        .map(NodeId)
        .ok()
        .filter(|id| tree.get(*id).is_some());
    id.ok_or_else(|| SelectError::NotFound {
        wanted: "node",
        selector: selector.to_owned(),
    })
}

/// A pane by numeric id.
pub fn resolve_pane(tree: &Tree, selector: &str) -> Result<NodeId, SelectError> {
    let id = resolve_node(tree, selector)?;
    let node = tree.get(id).ok_or_else(|| SelectError::NotFound {
        wanted: "pane",
        selector: selector.to_owned(),
    })?;
    if matches!(node.kind, NodeKind::Pane { .. }) {
        return Ok(id);
    }
    Err(SelectError::WrongKind {
        id: id.0,
        actual: crate::view::node_kind_name(node),
        wanted: "pane",
    })
}

/// A project by node id, absolute path, directory name, or exact display name.
/// An id or path always wins; a bare name must be unambiguous.
pub fn resolve_project(tree: &Tree, selector: &str) -> Result<NodeId, SelectError> {
    let trimmed = selector.trim();
    let projects = tree.project_ids();
    if let Ok(number) = trimmed.parse::<u64>() {
        let id = NodeId(number);
        if projects.contains(&id) {
            return Ok(id);
        }
        if let Some(node) = tree.get(id) {
            return Err(SelectError::WrongKind {
                id: number,
                actual: crate::view::node_kind_name(node),
                wanted: "project",
            });
        }
    }
    let describe = |id: &NodeId| {
        let name = tree.get(*id).map_or("?", |node| node.name.as_str());
        format!("{} ({name})", id.0)
    };
    let by_path: Vec<NodeId> = projects
        .iter()
        .copied()
        .filter(|id| {
            tree.project_path_for(*id)
                .is_some_and(|path| path == std::path::Path::new(trimmed))
        })
        .collect();
    if let [only] = by_path.as_slice() {
        return Ok(*only);
    }
    let by_name: Vec<NodeId> = projects
        .iter()
        .copied()
        .filter(|id| {
            let directory = tree
                .project_path_for(*id)
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str());
            let display = tree.get(*id).map(|node| node.name.as_str());
            directory == Some(trimmed) || display == Some(trimmed)
        })
        .collect();
    match by_name.as_slice() {
        [] => Err(SelectError::NotFound {
            wanted: "project",
            selector: selector.to_owned(),
        }),
        [only] => Ok(*only),
        many => Err(SelectError::Ambiguous {
            selector: selector.to_owned(),
            candidates: many.iter().map(describe).collect::<Vec<_>>().join(", "),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneContentKind;
    use std::path::PathBuf;

    fn two_projects() -> (Tree, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let first = tree.add_project(PathBuf::from("/tmp/a/app")).unwrap();
        let second = tree.add_project(PathBuf::from("/tmp/b/app")).unwrap();
        let group = tree.add_group(first, "g").unwrap();
        let pane = tree
            .add_pane(group, "p".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        (tree, first, second, pane)
    }

    #[test]
    fn project_by_id_path_and_unique_name() {
        let (tree, first, second, _) = two_projects();
        assert_eq!(resolve_project(&tree, &first.0.to_string()), Ok(first));
        assert_eq!(resolve_project(&tree, "/tmp/b/app"), Ok(second));
    }

    #[test]
    fn project_name_shared_by_two_projects_is_ambiguous() {
        let (tree, ..) = two_projects();
        assert!(matches!(
            resolve_project(&tree, "app"),
            Err(SelectError::Ambiguous { .. })
        ));
        assert!(matches!(
            resolve_project(&tree, "missing"),
            Err(SelectError::NotFound { .. })
        ));
    }

    #[test]
    fn pane_selector_rejects_other_kinds() {
        let (tree, first, _, pane) = two_projects();
        assert_eq!(resolve_pane(&tree, &pane.0.to_string()), Ok(pane));
        assert!(matches!(
            resolve_pane(&tree, &first.0.to_string()),
            Err(SelectError::WrongKind { .. })
        ));
        assert!(resolve_pane(&tree, "nope").is_err());
    }
}
