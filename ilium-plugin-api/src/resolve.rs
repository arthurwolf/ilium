//! Dependency resolution over registry sources (reference-dependency-resolution.md; PKG-804).

use std::collections::{BTreeMap, VecDeque};

use crate::error::{ErrorCode, PluginError};
use crate::registry::{Registry, VersionInfo};

/// Most distinct plugins one resolution may select (PKG-804).
pub const MAX_GRAPH_PLUGINS: usize = 256;
/// Deepest dependency chain one resolution may follow (PKG-804).
pub const MAX_GRAPH_DEPTH: usize = 32;

/// Resolves a plugin and its requirements without downloading packages.
#[derive(Debug, Clone)]
pub struct Resolver {
    /// The registries searched in order.
    pub sources: Vec<Registry>,
}

/// The plugins a resolution selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    entries: Vec<(String, String, String)>,
}

impl Resolution {
    /// The ids of the selected plugins.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.entries.iter().map(|(id, _, _)| id.clone()).collect()
    }

    /// The `(id, version, sha256)` of every selected plugin, in id order.
    pub fn entries(&self) -> &[(String, String, String)] {
        &self.entries
    }
}

/// The highest live (not yanked, not taken down) version of `id` in `registry` that satisfies `range`.
pub(crate) fn pick_version(registry: &Registry, id: &str, range: &str) -> Option<VersionInfo> {
    registry
        .own_versions(id)
        .into_iter()
        .rev()
        .filter(|info| !info.taken_down && info.yank_reason.is_none())
        .find(|info| info.satisfies(range))
}

impl Resolver {
    /// A resolver over `sources`.
    pub fn new(sources: Vec<Registry>) -> Resolver {
        Resolver { sources }
    }

    fn find(&self, id: &str, range: &str) -> Option<VersionInfo> {
        self.sources.iter().find_map(|source| {
            let holder = source.serving_registry(id)?;
            pick_version(&holder, id, range)
        })
    }

    /// Resolves `id` at `range`; more than 256 plugins or a chain deeper than 32 is
    /// `resolve.graph-too-large`, before any download (PKG-804).
    pub fn resolve(&self, id: &str, range: &str) -> Result<Resolution, PluginError> {
        let mut selected: BTreeMap<String, VersionInfo> = BTreeMap::new();
        let mut queue: VecDeque<(String, String, usize)> = VecDeque::new();
        queue.push_back((id.to_owned(), range.to_owned(), 1));
        while let Some((current, current_range, depth)) = queue.pop_front() {
            if depth > MAX_GRAPH_DEPTH {
                return Err(PluginError::coded(
                    "resolve.graph-too-large",
                    ErrorCode::QuotaExceeded,
                    format!("the dependency chain is deeper than {MAX_GRAPH_DEPTH}"),
                ));
            }
            if selected.contains_key(&current) {
                continue;
            }
            let info = self.find(&current, &current_range).ok_or_else(|| {
                PluginError::coded(
                    "plugin.not-found",
                    ErrorCode::NotFound,
                    format!("no live version of `{current}` satisfies {current_range}"),
                )
            })?;
            if selected.len() + 1 > MAX_GRAPH_PLUGINS {
                return Err(PluginError::coded(
                    "resolve.graph-too-large",
                    ErrorCode::QuotaExceeded,
                    format!("the graph holds more than {MAX_GRAPH_PLUGINS} plugins"),
                ));
            }
            for (required, required_range, optional) in &info.requires {
                if *optional && self.find(required, required_range).is_none() {
                    continue;
                }
                queue.push_back((required.clone(), required_range.clone(), depth + 1));
            }
            selected.insert(current, info);
        }
        Ok(Resolution {
            entries: selected
                .into_iter()
                .map(|(plugin, info)| (plugin, info.version, info.sha256))
                .collect(),
        })
    }
}
