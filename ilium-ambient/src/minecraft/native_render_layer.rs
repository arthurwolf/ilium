//! Pinned Java 1.19.3 ItemBlockRenderTypes lookup by block identity.
//! The retained table is derived from the complete fdq initializer; the
//! original and corrected derivation remain separately auditable.
use serde::Deserialize;
use std::collections::BTreeMap;

const REGISTRATIONS: &str = include_str!("native_render_layers_1193.jsonl");
const EXPECTED_REGISTRATIONS: usize = 275;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Solid,
    Cutout,
    CutoutMipped,
    Translucent,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("pinned native render-layer table is incomplete or malformed")]
    Evidence,
    #[error("native render layer requires a vanilla block identity")]
    Namespace,
}

#[derive(Deserialize)]
struct Registration {
    #[serde(rename = "type")]
    kind: String,
    field: String,
    mapped_name: String,
    layer: String,
}

pub struct RenderLayers {
    blocks: BTreeMap<String, Layer>,
    fancy_leaves: bool,
}
impl RenderLayers {
    /// `fancy_leaves` is an explicit renderer profile choice. This evidence
    /// does not establish the user's current Minecraft graphics setting.
    pub fn load(fancy_leaves: bool) -> Result<Self, Error> {
        let mut blocks = BTreeMap::new();
        let mut total = 0;
        let mut fluid_count = 0;
        for line in REGISTRATIONS.lines() {
            let registration: Registration =
                serde_json::from_str(line).map_err(|_| Error::Evidence)?;
            if registration.kind != "registration"
                || registration.mapped_name.is_empty()
                || !registration
                    .mapped_name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
            {
                return Err(Error::Evidence);
            }
            let layer = match registration.layer.as_str() {
                "solid" => Layer::Solid,
                "cutout" => Layer::Cutout,
                "cutout_mipped" => Layer::CutoutMipped,
                "translucent" => Layer::Translucent,
                _ => return Err(Error::Evidence),
            };
            if registration.field.starts_with("dtk.") {
                if !matches!(registration.mapped_name.as_str(), "WATER" | "FLOWING_WATER")
                    || layer != Layer::Translucent
                {
                    return Err(Error::Evidence);
                }
                fluid_count += 1;
            } else if registration.field.starts_with("cmu.") {
                let id = format!(
                    "minecraft:{}",
                    registration.mapped_name.to_ascii_lowercase()
                );
                if blocks.insert(id, layer).is_some() {
                    return Err(Error::Evidence);
                }
            } else {
                return Err(Error::Evidence);
            }
            total += 1;
        }
        if total != EXPECTED_REGISTRATIONS || fluid_count != 2 || blocks.len() != 273 {
            return Err(Error::Evidence);
        }
        Ok(Self {
            blocks,
            fancy_leaves,
        })
    }

    /// Unregistered vanilla blocks use native solid default. LeavesBlock
    /// overrides the table according to the explicit fancy-leaves profile.
    pub fn block(&self, id: &str) -> Result<Layer, Error> {
        if !id.starts_with("minecraft:") {
            return Err(Error::Namespace);
        }
        if matches!(
            id,
            "minecraft:oak_leaves"
                | "minecraft:spruce_leaves"
                | "minecraft:birch_leaves"
                | "minecraft:jungle_leaves"
                | "minecraft:acacia_leaves"
                | "minecraft:dark_oak_leaves"
                | "minecraft:mangrove_leaves"
                | "minecraft:azalea_leaves"
                | "minecraft:flowering_azalea_leaves"
        ) {
            return Ok(if self.fancy_leaves {
                Layer::CutoutMipped
            } else {
                Layer::Solid
            });
        }
        Ok(self.blocks.get(id).copied().unwrap_or(Layer::Solid))
    }

    pub fn fluid_water(&self) -> Layer {
        Layer::Translucent
    }
    pub fn fluid_lava(&self) -> Layer {
        Layer::Solid
    }
}

#[cfg(test)]
#[path = "native_render_layer_tests.rs"]
mod tests;
