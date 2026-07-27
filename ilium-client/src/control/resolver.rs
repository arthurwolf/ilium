//! Deterministic semantic node resolution.

use ilium_core::{NodeId, ROOT_ID};

use crate::app::App;

use super::command::NodeTarget;

pub fn resolve_node(app: &App, target: &NodeTarget) -> Result<NodeId, String> {
    if let Some(id) = target.id {
        let node_id = NodeId(id);
        return app
            .tree
            .get(node_id)
            .map(|_| node_id)
            .ok_or_else(|| format!("No ilium node has id {id}"));
    }

    if let Some(path) = target
        .path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        return resolve_path(app, path);
    }

    if let Some(name) = target
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        return resolve_unique_name(app, name);
    }

    app.active_pane_id()
        .or_else(|| app.selected_node_id())
        .ok_or_else(|| "No active or selected ilium node".to_owned())
}

pub fn resolve_parent(app: &App, target: &NodeTarget) -> Result<NodeId, String> {
    if !target.is_specified() {
        return app
            .active_pane_id()
            .or_else(|| app.selected_node_id())
            .and_then(|id| nearest_ancestor_accepting_children(app, id))
            .or_else(|| app.tree.project_ids().first().copied())
            .ok_or_else(|| "No destination group is available".to_owned());
    }

    let node_id = resolve_node(app, target)?;
    if app
        .tree
        .get(node_id)
        .is_some_and(|node| node.accepts_normal_children())
    {
        return Ok(node_id);
    }
    Err(format!(
        "Node {} cannot contain ordinary ilium items",
        node_id.0
    ))
}

pub fn node_path(app: &App, node_id: NodeId) -> String {
    let mut names = Vec::new();
    let mut current = Some(node_id);
    while let Some(id) = current {
        let Some(node) = app.tree.get(id) else {
            break;
        };
        if id != ROOT_ID {
            names.push(escape_path_component(&node.name));
        }
        current = node.parent;
    }
    names.reverse();
    format!("/{}", names.join("/"))
}

/// Escapes a literal backslash or `/` inside a single node name so it
/// survives being joined into a `/`-separated path string. Node names are
/// unconstrained free text (renames, shell-command titles, LLM-inferred
/// titles) and may themselves contain `/` -- without escaping, a name like
/// `feat/login` would be indistinguishable from two nested path components
/// once joined, and `resolve_path` (the counterpart that un-escapes and
/// splits) would silently resolve the wrong node instead of round-tripping
/// back to this one.
fn escape_path_component(name: &str) -> String {
    name.replace('\\', "\\\\").replace('/', "\\/")
}

/// Walks upward from `id` (inclusive) to the nearest node that can directly
/// own ordinary items, e.g. skipping past a pane's split-view container to
/// the group that contains the split. A single unconditional hop to
/// `parent_of` is not enough here: a split view is itself a container but
/// explicitly does not accept ordinary children (only panes, and only
/// through its own capacity-checked insertion path), so landing on it
/// without re-checking `accepts_normal_children` would hand back an invalid
/// destination for a new pane/group.
fn nearest_ancestor_accepting_children(app: &App, id: NodeId) -> Option<NodeId> {
    let mut current = Some(id);
    while let Some(candidate) = current {
        let node = app.tree.get(candidate)?;
        if node.accepts_normal_children() {
            return Some(candidate);
        }
        current = node.parent;
    }
    None
}

fn resolve_unique_name(app: &App, requested_name: &str) -> Result<NodeId, String> {
    let requested_name = requested_name.trim();
    let matches = app
        .tree
        .all_ids()
        .filter(|id| {
            app.tree
                .get(*id)
                .is_some_and(|node| node.name.eq_ignore_ascii_case(requested_name))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [node_id] => Ok(*node_id),
        [] => Err(format!("No ilium node is named {requested_name:?}")),
        _ => Err(format!(
            "The name {requested_name:?} is ambiguous; use one of these paths: {}",
            matches
                .iter()
                .map(|id| node_path(app, *id))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn resolve_path(app: &App, requested_path: &str) -> Result<NodeId, String> {
    let components = split_path_components(requested_path);
    if components.is_empty() {
        return Ok(ROOT_ID);
    }

    let mut current = ROOT_ID;
    for component in components {
        let children = app
            .tree
            .children_of(current)
            .map_err(|_| format!("{} is not a container", node_path(app, current)))?;
        let matches = children
            .iter()
            .copied()
            .filter(|child| {
                app.tree
                    .get(*child)
                    .is_some_and(|node| node.name.eq_ignore_ascii_case(&component))
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [child] => current = *child,
            [] => {
                return Err(format!(
                    "No child named {component:?} exists under {}",
                    node_path(app, current)
                ))
            }
            _ => {
                return Err(format!(
                    "Path component {component:?} is ambiguous under {}",
                    node_path(app, current)
                ))
            }
        }
    }
    Ok(current)
}

/// Splits `requested_path` on `/`, treating a backslash as escaping the
/// character that follows it (`\/` for a literal separator, `\\` for a
/// literal backslash) so a name containing `/` -- as escaped by
/// [`escape_path_component`] -- round-trips back into a single component
/// instead of fragmenting into extra path levels. Each resulting component
/// is trimmed and blank ones are dropped, matching the previous plain-split
/// behavior for paths that don't need escaping at all.
fn split_path_components(requested_path: &str) -> Vec<String> {
    let mut components = Vec::new();
    let mut current = String::new();
    let mut chars = requested_path.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some(escaped @ ('/' | '\\')) => current.push(escaped),
                Some(other) => {
                    current.push('\\');
                    current.push(other);
                }
                None => current.push('\\'),
            },
            '/' => components.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    components.push(current);

    components
        .into_iter()
        .map(|component| component.trim().to_owned())
        .filter(|component| !component.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ilium_core::{PaneContentKind, SplitOrientation};

    use crate::app::RightPanelTarget;

    use super::*;

    #[test]
    fn resolve_parent_walks_past_a_split_view_to_its_enclosing_group() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let project = app.tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        let group = app.tree.add_group(project, "Work").unwrap();
        let pane_a = app
            .tree
            .add_pane(group, "left", PaneContentKind::Terminal)
            .unwrap();
        let pane_b = app
            .tree
            .add_pane(group, "right", PaneContentKind::Terminal)
            .unwrap();
        let split_view = app
            .tree
            .create_split_view(
                group,
                "split",
                SplitOrientation::Vertical,
                &[pane_a, pane_b],
            )
            .unwrap();
        app.right_panel_target = RightPanelTarget::SplitView {
            split_id: split_view,
            active_pane_id: Some(pane_a),
        };

        // A default (unspecified) target must land on the enclosing group --
        // the split view itself does not accept ordinary children, so a
        // single unchecked hop to the pane's immediate parent would have
        // handed back an invalid destination here.
        assert_eq!(resolve_parent(&app, &NodeTarget::default()), Ok(group));
    }

    #[test]
    fn resolve_parent_treats_a_blank_path_exactly_like_an_omitted_target() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let project = app.tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        let group = app.tree.add_group(project, "Work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.right_panel_target = RightPanelTarget::Pane { pane_id: pane };

        let omitted = resolve_parent(&app, &NodeTarget::default());
        let blank_path = resolve_parent(
            &app,
            &NodeTarget {
                path: Some("   ".to_owned()),
                ..NodeTarget::default()
            },
        );

        assert_eq!(omitted, Ok(group));
        assert_eq!(blank_path, Ok(group));
    }

    #[test]
    fn node_path_and_resolve_path_round_trip_a_name_containing_a_slash() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let project = app.tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        let group = app.tree.add_group(project, "feat/login").unwrap();

        let suggested_path = node_path(&app, group);

        assert_eq!(
            resolve_node(
                &app,
                &NodeTarget {
                    path: Some(suggested_path),
                    ..NodeTarget::default()
                }
            ),
            Ok(group)
        );
    }

    #[test]
    fn duplicate_names_require_an_id_or_path() {
        let mut app = App::new("default".to_owned(), PathBuf::from("/tmp/project"));
        let project = app.tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        let first_group = app.tree.add_group(project, "Work").unwrap();
        let second_group = app.tree.add_group(project, "Work").unwrap();

        let error = resolve_node(
            &app,
            &NodeTarget {
                name: Some("work".to_owned()),
                ..NodeTarget::default()
            },
        )
        .unwrap_err();

        assert!(error.contains("ambiguous"));
        assert_eq!(
            resolve_node(
                &app,
                &NodeTarget {
                    id: Some(first_group.0),
                    ..NodeTarget::default()
                }
            ),
            Ok(first_group)
        );
        assert_ne!(first_group, second_group);
    }
}
