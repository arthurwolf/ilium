//! The read-only documents both `ilium ctl` and the HTTP API serve.
//!
//! Each function takes a tree and returns the JSON value plus the markdown
//! shape to use, so a caller only picks a [`Format`](crate::Format).

use ilium_core::{NodeId, Tree};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::render::Shape;
use crate::select::{resolve_node, resolve_pane, resolve_project, SelectError};
use crate::view::{node_view, pane_detail, tree_view, ViewOptions};

/// A document and how markdown should lay it out.
pub struct Document {
    pub value: Value,
    pub shape: Shape,
}

fn options(depth: Option<usize>, details: bool) -> ViewOptions {
    ViewOptions {
        depth,
        pane_details: details,
        ..ViewOptions::default()
    }
}

/// The whole session, or a single project when `project` is given.
pub fn tree_document(
    tree: &Tree,
    session_name: &str,
    project: Option<&str>,
    depth: Option<usize>,
    details: bool,
) -> Result<Document, SelectError> {
    let options = options(depth, details);
    let Some(selector) = project else {
        return Ok(Document {
            value: tree_view(tree, session_name, options),
            shape: Shape::Tree,
        });
    };
    project_document(tree, selector, depth, details)
}

/// One project and everything under it.
pub fn project_document(
    tree: &Tree,
    selector: &str,
    depth: Option<usize>,
    details: bool,
) -> Result<Document, SelectError> {
    let id = resolve_project(tree, selector)?;
    node_document_by_id(tree, id, depth, details)
}

/// Any node by id.
pub fn node_document(
    tree: &Tree,
    selector: &str,
    depth: Option<usize>,
    details: bool,
) -> Result<Document, SelectError> {
    let id = resolve_node(tree, selector)?;
    node_document_by_id(tree, id, depth, details)
}

fn node_document_by_id(
    tree: &Tree,
    id: NodeId,
    depth: Option<usize>,
    details: bool,
) -> Result<Document, SelectError> {
    let value = node_view(tree, id, options(depth, details)).ok_or(SelectError::NotFound {
        wanted: "node",
        selector: id.0.to_string(),
    })?;
    Ok(Document {
        value,
        shape: Shape::Node,
    })
}

/// One pane with its full detail object merged into the node view.
pub fn pane_document(tree: &Tree, selector: &str) -> Result<Document, SelectError> {
    let id = resolve_pane(tree, selector)?;
    node_document_by_id(tree, id, Some(0), true)
}

/// Filters for [`find_panes`]; every set field must match.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PaneFilter {
    /// `claude`, `codex`, `antigravity` (matched case-insensitively).
    pub agent: Option<String>,
    /// `working`, `idle`, `done`, `waiting_approval`, ...; comma separated.
    pub state: Option<String>,
    /// `shell`, `agent`, `editor`, `board`, `agent_unavailable`.
    pub kind: Option<String>,
    /// Project selector (id, path, directory name or display name).
    pub project: Option<String>,
    /// Case-insensitive substring of the pane title.
    pub name_contains: Option<String>,
}

fn status_text<'a>(detail: &'a Value, key: &str) -> Option<&'a str> {
    detail.get("status")?.get(key)?.as_str()
}

/// A flat list of panes passing `filter`, each as its node view with details.
pub fn find_panes(tree: &Tree, filter: &PaneFilter) -> Result<Document, SelectError> {
    let project_id = filter
        .project
        .as_deref()
        .map(|selector| resolve_project(tree, selector))
        .transpose()?;
    let states: Vec<String> = filter
        .state
        .iter()
        .flat_map(|states| states.split(','))
        .map(|state| state.trim().to_ascii_lowercase())
        .filter(|state| !state.is_empty())
        .collect();
    let mut matches = Vec::new();
    for pane_id in tree.pane_ids_in_tree_order() {
        let Some(detail) = pane_detail(tree, pane_id) else {
            continue;
        };
        if let Some(project_id) = project_id {
            if detail.get("project_id").and_then(Value::as_u64) != Some(project_id.0) {
                continue;
            }
        }
        if let Some(agent) = &filter.agent {
            let actual = status_text(&detail, "agent").unwrap_or_default();
            if !actual.eq_ignore_ascii_case(agent) {
                continue;
            }
        }
        if !states.is_empty() {
            let actual = status_text(&detail, "state").unwrap_or_default();
            if !states.iter().any(|state| state == actual) {
                continue;
            }
        }
        if let Some(kind) = &filter.kind {
            if !status_text(&detail, "kind").is_some_and(|actual| actual.eq_ignore_ascii_case(kind))
            {
                continue;
            }
        }
        let Some(view) = node_view(tree, pane_id, options(Some(0), true)) else {
            continue;
        };
        if let Some(needle) = &filter.name_contains {
            let name = view.get("name").and_then(Value::as_str).unwrap_or_default();
            if !name.to_lowercase().contains(&needle.to_lowercase()) {
                continue;
            }
        }
        matches.push(view);
    }
    Ok(Document {
        value: json!({"count": matches.len(), "panes": matches}),
        shape: Shape::Generic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneContentKind;
    use std::path::PathBuf;

    fn sample() -> Tree {
        let mut tree = Tree::new();
        let alpha = tree.add_project(PathBuf::from("/tmp/alpha")).unwrap();
        let beta = tree.add_project(PathBuf::from("/tmp/beta")).unwrap();
        tree.add_pane(alpha, "build".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(beta, "Docs".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        tree
    }

    #[test]
    fn tree_and_project_documents_resolve() {
        let tree = sample();
        let whole = tree_document(&tree, "default", None, None, true).unwrap();
        assert_eq!(whole.value["project_count"], 2);
        let one = tree_document(&tree, "default", Some("alpha"), None, true).unwrap();
        assert_eq!(one.value["kind"], "project");
        assert!(project_document(&tree, "missing", None, true).is_err());
    }

    #[test]
    fn find_filters_by_project_and_name() {
        let tree = sample();
        let all = find_panes(&tree, &PaneFilter::default()).unwrap();
        assert_eq!(all.value["count"], 2);
        let filter = PaneFilter {
            project: Some("beta".into()),
            ..PaneFilter::default()
        };
        assert_eq!(find_panes(&tree, &filter).unwrap().value["count"], 1);
        let filter = PaneFilter {
            name_contains: Some("DOC".into()),
            ..PaneFilter::default()
        };
        assert_eq!(find_panes(&tree, &filter).unwrap().value["count"], 1);
        let filter = PaneFilter {
            agent: Some("claude".into()),
            ..PaneFilter::default()
        };
        assert_eq!(find_panes(&tree, &filter).unwrap().value["count"], 0);
    }
}
