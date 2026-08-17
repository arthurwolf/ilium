use std::collections::HashSet;

use crate::tree_item::TreeItem;

/// A flattened item of all visible [`TreeItem`]s.
///
/// Generated via [`TreeState::flatten`](crate::TreeState::flatten).
#[must_use]
pub struct Flattened<'text, Identifier> {
    pub identifier: Vec<Identifier>,
    pub item: &'text TreeItem<'text, Identifier>,
}

impl<Identifier> Flattened<'_, Identifier> {
    /// Zero based depth. Depth 0 means top level with 0 indentation.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.identifier.len() - 1
    }
}

/// Get a flat list of all visible [`TreeItem`]s.
///
/// `current` starts empty: `&[]`
#[must_use]
pub fn flatten<'text, Identifier>(
    open_identifiers: &HashSet<Vec<Identifier>>,
    items: &'text [TreeItem<'text, Identifier>],
    current: &[Identifier],
) -> Vec<Flattened<'text, Identifier>>
where
    Identifier: Clone + PartialEq + Eq + core::hash::Hash,
{
    let mut result = Vec::with_capacity(items.len());
    let mut identifier = Vec::with_capacity(current.len().saturating_add(1));
    identifier.extend_from_slice(current);
    flatten_into(open_identifiers, items, &mut identifier, &mut result);
    result
}

/// Appends one visible subtree into a shared output allocation. The previous
/// recursive implementation allocated a temporary result `Vec` for every
/// expanded container and repeatedly appended those vectors into their
/// parents; one accumulator keeps traversal linear while retaining one owned
/// identifier path per visible row.
fn flatten_into<'text, Identifier>(
    open_identifiers: &HashSet<Vec<Identifier>>,
    items: &'text [TreeItem<'text, Identifier>],
    current: &mut Vec<Identifier>,
    result: &mut Vec<Flattened<'text, Identifier>>,
) where
    Identifier: Clone + PartialEq + Eq + core::hash::Hash,
{
    // Each item in this visible sibling slice contributes at least one row.
    // Reserving at recursive entry lets a wide opened group grow the shared
    // accumulator once instead of following `Vec`'s 1/2/4/... capacity ladder.
    result.reserve(items.len());
    for item in items {
        current.push(item.identifier.clone());
        let is_open = open_identifiers.contains(current);
        result.push(Flattened {
            identifier: current.clone(),
            item,
        });
        if is_open {
            flatten_into(open_identifiers, &item.children, current, result);
        }
        current.pop();
    }
}

#[test]
fn depth_works() {
    let mut open = HashSet::new();
    open.insert(vec!["b"]);
    open.insert(vec!["b", "d"]);
    let depths = flatten(&open, &TreeItem::example(), &[])
        .into_iter()
        .map(|flattened| flattened.depth())
        .collect::<Vec<_>>();
    assert_eq!(depths, [0, 0, 1, 1, 2, 2, 1, 0]);
}

#[cfg(test)]
fn flatten_works(open: &HashSet<Vec<&'static str>>, expected: &[&str]) {
    let items = TreeItem::example();
    let result = flatten(open, &items, &[]);
    let actual = result
        .into_iter()
        .map(|flattened| flattened.identifier.into_iter().next_back().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn flatten_nothing_open_is_top_level() {
    let open = HashSet::new();
    flatten_works(&open, &["a", "b", "h"]);
}

#[test]
fn flatten_wrong_open_is_only_top_level() {
    let mut open = HashSet::new();
    open.insert(vec!["a"]);
    open.insert(vec!["b", "d"]);
    flatten_works(&open, &["a", "b", "h"]);
}

#[test]
fn flatten_one_is_open() {
    let mut open = HashSet::new();
    open.insert(vec!["b"]);
    flatten_works(&open, &["a", "b", "c", "d", "g", "h"]);
}

#[test]
fn flatten_all_open() {
    let mut open = HashSet::new();
    open.insert(vec!["b"]);
    open.insert(vec!["b", "d"]);
    flatten_works(&open, &["a", "b", "c", "d", "e", "f", "g", "h"]);
}
