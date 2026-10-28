//! Mirroring a registry closure into a target store (PKG-817).

use std::collections::BTreeMap;

use crate::error::{ErrorCode, PluginError};
use crate::registry::Registry;
use crate::resolve::Resolver;

/// The store a mirror writes to: `id@version` to package bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorTarget {
    packages: BTreeMap<String, Vec<u8>>,
}

impl MirrorTarget {
    /// A store that holds nothing.
    pub fn empty() -> MirrorTarget {
        MirrorTarget {
            packages: BTreeMap::new(),
        }
    }

    /// Whether the store holds nothing.
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Whether the store holds `id@version`.
    pub fn contains(&self, id: &str, version: &str) -> bool {
        self.packages.contains_key(&format!("{id}@{version}"))
    }
}

/// Mirrors the allow-listed ids and their dependency closure from `source`; an incomplete closure
/// is `mirror.closure-incomplete` and nothing is written (PKG-817).
pub fn mirror(
    source: &Registry,
    allow: &[&str],
    target: &mut MirrorTarget,
) -> Result<(), PluginError> {
    let resolver = Resolver::new(vec![source.clone()]);
    let mut closure: BTreeMap<String, String> = BTreeMap::new();
    for id in allow {
        let resolution = resolver.resolve(id, "*").map_err(|error| {
            PluginError::coded(
                "mirror.closure-incomplete",
                ErrorCode::NotFound,
                format!("the closure of `{id}` is incomplete: {}", error.message),
            )
        })?;
        for (plugin, version, _) in resolution.entries() {
            closure.insert(plugin.clone(), version.clone());
        }
    }
    let mut staged: Vec<(String, Vec<u8>)> = Vec::new();
    for (id, version) in &closure {
        let bytes = source
            .serving_registry(id)
            .and_then(|holder| holder.download(id, version))
            .ok_or_else(|| {
                PluginError::coded(
                    "mirror.closure-incomplete",
                    ErrorCode::NotFound,
                    format!("`{id}@{version}` is not served"),
                )
            })?;
        staged.push((format!("{id}@{version}"), bytes));
    }
    target.packages.extend(staged);
    Ok(())
}
