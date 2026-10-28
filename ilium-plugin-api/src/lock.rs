//! The lock file: `{"format": 1, "plugins": [{"id", "version", "sha256"}]}`, sorted by id
//! (reference-dependency-resolution.md; PKG-803, PKG-828).

use serde_json::{json, Value};

use crate::error::{ErrorCode, PluginError};
use crate::registry::Registry;
use crate::resolve::Resolver;

/// A parsed lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFile {
    entries: Vec<(String, String, String)>,
}

impl LockFile {
    /// The locked version of `id`.
    pub fn version_of(&self, id: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(entry, _, _)| entry == id)
            .map(|(_, version, _)| version.clone())
    }

    /// The `(id, version, sha256)` entries, sorted by id.
    pub fn entries(&self) -> &[(String, String, String)] {
        &self.entries
    }

    /// The lock as JSON text.
    pub fn to_text(&self) -> String {
        let plugins: Vec<Value> = self
            .entries
            .iter()
            .map(|(id, version, sha256)| json!({ "id": id, "version": version, "sha256": sha256 }))
            .collect();
        json!({ "format": 1, "plugins": plugins }).to_string()
    }
}

fn invalid(message: impl std::fmt::Display) -> PluginError {
    PluginError::coded("lock.invalid", ErrorCode::InvalidArgument, message)
}

/// Parses and validates a lock text; a bad lock is `lock.invalid` (PKG-828).
pub fn parse(text: &str) -> Result<LockFile, PluginError> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| invalid(format!("not JSON: {error}")))?;
    if value.get("format").and_then(Value::as_u64) != Some(1) {
        return Err(invalid("`format` must be 1"));
    }
    let plugins = value
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("`plugins` must be an array"))?;
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for plugin in plugins {
        let field = |name: &str| {
            plugin
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(format!("an entry lacks `{name}`")))
        };
        let id = field("id")?;
        let version = field("version")?;
        let sha256 = field("sha256")?;
        if semver::Version::parse(version).is_err() {
            return Err(invalid(format!("`{version}` is not a version")));
        }
        let is_hex = sha256.len() == 64
            && sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !is_hex {
            return Err(invalid(format!(
                "the sha256 of `{id}` is not 64 lower-case hex characters"
            )));
        }
        if let Some((previous, _, _)) = entries.last() {
            if previous.as_str() >= id {
                return Err(invalid(format!(
                    "entries must be strictly ascending by id (`{id}` after `{previous}`)"
                )));
            }
        }
        entries.push((id.to_owned(), version.to_owned(), sha256.to_owned()));
    }
    Ok(LockFile { entries })
}

/// Builds a lock for `requests` such as `demo.plugin@^1.0.0`; a yanked version is never chosen (PKG-803).
pub fn regenerate(registry: &Registry, requests: &[&str]) -> Result<LockFile, PluginError> {
    let resolver = Resolver::new(vec![registry.clone()]);
    let mut merged: std::collections::BTreeMap<String, (String, String)> = Default::default();
    for request in requests {
        let (id, range) = request.split_once('@').unwrap_or((request, "*"));
        let resolution = resolver.resolve(id, range)?;
        for (plugin, version, sha256) in resolution.entries() {
            merged.insert(plugin.clone(), (version.clone(), sha256.clone()));
        }
    }
    Ok(LockFile {
        entries: merged
            .into_iter()
            .map(|(id, (version, sha256))| (id, version, sha256))
            .collect(),
    })
}
