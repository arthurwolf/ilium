//! Versioned, externally inspectable package metadata.
use crate::error::{AnimationError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimationMode {
    Live,
    PreRendered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub scope: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileInventory {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    #[serde(default = "default_heap")]
    pub heap_bytes: u64,
    #[serde(default = "default_frame")]
    pub frame_bytes: u64,
    #[serde(default = "default_render")]
    pub render_ms: u64,
    #[serde(default = "default_prepare")]
    pub preparation_ms: u64,
    #[serde(default = "default_clip")]
    pub clip_seconds: u64,
}
fn default_heap() -> u64 {
    64 * 1024 * 1024
}
fn default_frame() -> u64 {
    8 * 1024 * 1024
}
fn default_render() -> u64 {
    50
}
fn default_prepare() -> u64 {
    10000
}
fn default_clip() -> u64 {
    30
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            heap_bytes: default_heap(),
            frame_bytes: default_frame(),
            render_ms: default_render(),
            preparation_ms: default_prepare(),
            clip_seconds: default_clip(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub modes: Vec<AnimationMode>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default = "default_settings_schema")]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub assets: Vec<FileInventory>,
    pub files: Vec<FileInventory>,
    #[serde(default)]
    pub limits: RuntimeLimits,
    #[serde(default)]
    pub description: String,
}
fn default_settings_schema() -> serde_json::Value {
    serde_json::json!({"type":"object","properties":{}})
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.api_version != 1 {
            return Err(AnimationError::ApiVersion(self.api_version));
        }
        if self.id.is_empty()
            || self.id.len() > 80
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(AnimationError::InvalidPackage(
                "id must contain lowercase ASCII, digits and hyphens".into(),
            ));
        }
        if self.name.chars().any(char::is_control)
            || self
                .description
                .chars()
                .any(|character| character.is_control() && character != '\n')
            || self.version.chars().any(char::is_control)
            || self.name.trim().is_empty()
            || self.name.len() > 160
            || self.version.is_empty()
            || self.version.len() > 80
            || self.description.len() > 4096
        {
            return Err(AnimationError::InvalidPackage(
                "invalid display metadata".into(),
            ));
        }
        if self.modes.is_empty()
            || self.modes.len() > 2
            || (self.modes.len() == 2 && self.modes[0] == self.modes[1])
        {
            return Err(AnimationError::InvalidPackage(
                "modes must be unique and nonempty".into(),
            ));
        }
        if self.entry != "entry.mjs"
            || self.capabilities.len() > 64
            || self.files.len() > 255
            || self.assets.len() > 254
        {
            return Err(AnimationError::InvalidPackage(
                "invalid entry or manifest counts".into(),
            ));
        }
        crate::settings::validate_schema(&self.settings)?;
        let mut capabilities = BTreeSet::new();
        for capability in &self.capabilities {
            capability.validate()?;
            if !capabilities.insert(serde_json::to_string(capability)?) {
                return Err(AnimationError::InvalidPackage(
                    "duplicate capability".into(),
                ));
            }
        }
        for limit in [
            self.limits.heap_bytes,
            self.limits.frame_bytes,
            self.limits.render_ms,
            self.limits.preparation_ms,
            self.limits.clip_seconds,
        ] {
            if limit == 0 {
                return Err(AnimationError::InvalidPackage(
                    "runtime limits must be positive".into(),
                ));
            }
        }
        for file in &self.files {
            if file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(AnimationError::InvalidPackage(
                    "invalid SHA256 inventory".into(),
                ));
            }
        }
        Ok(())
    }
}

impl Capability {
    pub fn validate(&self) -> Result<()> {
        crate::permission_projection::right(self)?;
        if matches!(self.id.as_str(), "network.http" | "network.local") {
            let origins = self
                .scope
                .get("origins")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| AnimationError::InvalidPackage("network origins".into()))?;
            for origin in origins {
                let value = origin
                    .as_str()
                    .ok_or_else(|| AnimationError::InvalidPackage("network origin type".into()))?;
                if crate::network::validate_https_origin(value)? != value {
                    return Err(AnimationError::InvalidPackage(
                        "manifest origins must use their exact canonical spelling".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_are_not_interchangeable() {
        for (id, scope) in [
            ("network.http", serde_json::json!({"origins":[1]})),
            (
                "disk.read",
                serde_json::json!({"origins":["https://example.org"]}),
            ),
            ("made.up", serde_json::json!("session")),
        ] {
            assert!(Capability {
                id: id.into(),
                scope
            }
            .validate()
            .is_err());
        }
    }
    #[test]
    fn origins_are_exact_and_credential_free() {
        for origin in [
            "https://example.org/path",
            "https://user:password@example.org",
            "http://example.org",
            "https://example.org/",
        ] {
            assert!(Capability {
                id: "network.http".into(),
                scope: serde_json::json!({"origins":[origin]})
            }
            .validate()
            .is_err());
        }
        assert!(Capability {
            id: "network.http".into(),
            scope: serde_json::json!({"origins":["https://example.org"]})
        }
        .validate()
        .is_ok());
    }
}
