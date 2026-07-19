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
    if target.id.is_none() && target.path.is_none() && target.name.is_none() {
        return app
            .active_pane_id()
            .or_else(|| app.selected_node_id())
            .and_then(|id| {
                app.tree
                    .get(id)
                    .filter(|node| node.accepts_normal_children())
                    .map(|_| id)
                    .or_else(|| app.tree.parent_of(id))
            })
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
            names.push(node.name.clone());
        }
        current = node.parent;
    }
    names.reverse();
    format!("/{}", names.join("/"))
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
    let components = requested_path
        .split('/')
        .map(str::trim)
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
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
                    .is_some_and(|node| node.name.eq_ignore_ascii_case(component))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

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
