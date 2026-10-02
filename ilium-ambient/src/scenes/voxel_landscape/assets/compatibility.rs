//! Original, explicit geometry compatibility definitions, not a redistributed art pack.
//! A caller selects exact IDs and semantic textures; filename resemblance is never a rule.
use super::{
    budget::{ByteBudget, Cancel, Limits, Reservation},
    error::{AssetError, Result},
    identity::{AssetPath, BlobOrigin, Label, ResourceId, SourceBlob},
    layers::{ResourceKey, ResourceKind},
    metadata::{self, Document},
    models::{DefinitionInput, DefinitionProvider, Direction},
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
struct CompatibilityEntry {
    blob: SourceBlob,
    reason: Label,
    _reservation: Reservation,
}
pub struct DefinitionSet {
    entries: BTreeMap<ResourceKey, CompatibilityEntry>,
    origin: BlobOrigin,
    limits: Limits,
    budget: ByteBudget,
}
impl DefinitionSet {
    pub fn new(origin: BlobOrigin, limits: Limits, budget: ByteBudget) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            entries: BTreeMap::new(),
            origin,
            limits,
            budget,
        })
    }
    pub fn insert(
        &mut self,
        key: ResourceKey,
        value: &Value,
        reason: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        cancel.check()?;
        if !matches!(key.kind, ResourceKind::Model | ResourceKind::Blockstate) {
            return Err(metadata::invalid(
                "compatibility definitions are models/blockstates, not artwork",
            ));
        }
        if self.entries.contains_key(&key) {
            return Err(AssetError::Duplicate(key.id.to_string()));
        }
        if self.entries.len() >= 4096 {
            return Err(metadata::invalid("compatibility definition count"));
        }
        let bytes = serde_json::to_vec(value).map_err(|e| metadata::invalid(&e.to_string()))?;
        if bytes.len() as u64 > self.limits.metadata_bytes {
            return Err(metadata::invalid("compatibility metadata size"));
        }
        let reservation = self.budget.reserve(8192, cancel)?;
        let mut origin = self.origin.clone();
        origin.path = AssetPath::parse(&format!(
            "compatibility/{:?}/{}/{}.json",
            key.kind,
            key.id.parts().0,
            key.id.parts().1
        ))?;
        let blob = SourceBlob::new(bytes, origin, None, &self.limits, &self.budget, cancel)?;
        // Parse immediately to preserve the same duplicate/shape limits as sourced documents.
        let _validated = Document::parse(&blob, &self.limits, &self.budget, cancel)?;
        self.entries.insert(
            key,
            CompatibilityEntry {
                blob,
                reason,
                _reservation: reservation,
            },
        );
        Ok(())
    }
    /// Install only explicitly named ordinary templates. These do not invent all vanilla block definitions.
    pub fn install_geometry_templates(&mut self, cancel: Cancel<'_>) -> Result<()> {
        let reason = Label::new("Original Ilium cube/cuboid/cross compatibility geometry; not source-pack-authored model JSON")?;
        let mut faces = Map::new();
        for face in Direction::ALL {
            faces.insert(
                face.name().into(),
                json!({"texture":format!("#{}",face.name()),"cullface":face.name()}),
            );
        }
        let cube = json!({"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":faces}]});
        self.insert_model(
            "minecraft:block/block",
            &json!({"elements":[]}),
            reason.clone(),
            cancel,
        )?;
        self.insert_model("minecraft:block/cube", &cube, reason.clone(), cancel)?;
        let mut all = Map::new();
        for face in Direction::ALL {
            all.insert(face.name().into(), json!("#all"));
        }
        self.insert_model(
            "minecraft:block/cube_all",
            &json!({"parent":"minecraft:block/cube","textures":all}),
            reason.clone(),
            cancel,
        )?;
        let side = json!({"down":"#bottom","up":"#top","north":"#side","south":"#side","west":"#side","east":"#side"});
        self.insert_model(
            "minecraft:block/cube_bottom_top",
            &json!({"parent":"minecraft:block/cube","textures":side}),
            reason.clone(),
            cancel,
        )?;
        let column = json!({"down":"#end","up":"#end","north":"#side","south":"#side","west":"#side","east":"#side"});
        self.insert_model(
            "minecraft:block/cube_column",
            &json!({"parent":"minecraft:block/cube","textures":column}),
            reason.clone(),
            cancel,
        )?;
        let mut horizontal_faces = Map::new();
        for face in Direction::ALL {
            let texture = if matches!(face, Direction::Up | Direction::Down) {
                "#end"
            } else {
                "#side"
            };
            let mut value = json!({"texture":texture,"cullface":face.name()});
            if face == Direction::Up {
                value["rotation"] = json!(180);
            }
            horizontal_faces.insert(face.name().into(), value);
        }
        self.insert_model(
            "minecraft:block/cube_column_horizontal",
            &json!({"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":horizontal_faces}]}),
            reason.clone(),
            cancel,
        )?;
        let mut leaf_faces = Map::new();
        for face in Direction::ALL {
            leaf_faces.insert(
                face.name().into(),
                json!({"texture":"#all","tintindex":0,"cullface":face.name()}),
            );
        }
        self.insert_model(
            "minecraft:block/leaves",
            &json!({"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":leaf_faces}]}),
            reason.clone(),
            cancel,
        )?;
        for (id, tint) in [
            ("minecraft:block/cross", false),
            ("minecraft:block/tinted_cross", true),
        ] {
            let mut elements = Vec::new();
            for angle in [-45, 45] {
                let mut faces = Map::new();
                for face in ["north", "south"] {
                    let mut value = json!({"texture":"#cross","uv":[0,0,16,16]});
                    if tint {
                        value["tintindex"] = json!(0);
                    }
                    faces.insert(face.into(), value);
                }
                elements.push(json!({"from":[0,0,8],"to":[16,16,8],"shade":false,"rotation":{"origin":[8,8,8],"axis":"y","angle":angle,"rescale":false},"faces":faces}));
            }
            self.insert_model(id, &json!({"elements":elements}), reason.clone(), cancel)?;
        }
        Ok(())
    }
    pub fn insert_model(
        &mut self,
        id: &str,
        value: &Value,
        reason: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        self.insert(
            ResourceKey {
                kind: ResourceKind::Model,
                id: ResourceId::parse(id)?,
            },
            value,
            reason,
            cancel,
        )
    }
    /// An explicit semantic species/state bridge; call separately for each actual selected species.
    pub fn bind_column(
        &mut self,
        block: ResourceId,
        side: ResourceId,
        end: ResourceId,
        reason: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        let model = ResourceId::parse(&format!("{}:block/{}", block.parts().0, block.parts().1))?;
        self.insert(ResourceKey { kind:ResourceKind::Model,id:model.clone() },&json!({"parent":"minecraft:block/cube_column","textures":{"side":side.as_str(),"end":end.as_str()}}),reason.clone(),cancel)?;
        // Some selected blockstates reference this standard companion while
        // providing only texture overrides. Its origin remains compatibility
        // geometry, and selected-pack definitions retain provider precedence.
        let horizontal = ResourceId::parse(&format!("{}_horizontal", model.as_str()))?;
        self.insert_model(horizontal.as_str(), &json!({"parent":"minecraft:block/cube_column_horizontal","textures":{"side":side.as_str(),"end":end.as_str()}}), reason.clone(), cancel)?;
        self.insert(ResourceKey { kind:ResourceKind::Blockstate,id:block },&json!({"variants":{
            "axis=y":{"model":model.as_str()},"axis=x":{"model":model.as_str(),"x":90,"y":90},"axis=z":{"model":model.as_str(),"x":90}}}),reason,cancel)
    }
    /// A normalized shape binding never coerces one flower/species into a generic surrogate.
    pub fn bind_single(
        &mut self,
        block: ResourceId,
        parent: ResourceId,
        variable: &str,
        texture: ResourceId,
        reason: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        if !["all", "cross"].contains(&variable) {
            return Err(metadata::invalid(
                "single-template variable must be all or cross",
            ));
        }
        let model = ResourceId::parse(&format!("{}:block/{}", block.parts().0, block.parts().1))?;
        let mut textures = Map::new();
        textures.insert(variable.into(), json!(texture.as_str()));
        self.insert(
            ResourceKey {
                kind: ResourceKind::Model,
                id: model.clone(),
            },
            &json!({"parent":parent.as_str(),"textures":textures}),
            reason.clone(),
            cancel,
        )?;
        self.insert(
            ResourceKey {
                kind: ResourceKind::Blockstate,
                id: block,
            },
            &json!({"variants":{"":{"model":model.as_str()}}}),
            reason,
            cancel,
        )
    }
    /// Explicit grass face semantics: top tint, dirt bottom, dirt-bearing side and optional tinted side overlay.
    #[expect(
        clippy::too_many_arguments,
        reason = "the four distinct source faces and provenance must remain explicit"
    )]
    pub fn bind_grass(
        &mut self,
        block: ResourceId,
        top: ResourceId,
        dirt: ResourceId,
        side: ResourceId,
        overlay: Option<ResourceId>,
        reason: Label,
        cancel: Cancel<'_>,
    ) -> Result<()> {
        let model = ResourceId::parse(&format!("{}:block/{}", block.parts().0, block.parts().1))?;
        let mut faces = Map::new();
        for face in Direction::ALL {
            let texture = match face {
                Direction::Up => top.as_str(),
                Direction::Down => dirt.as_str(),
                _ => side.as_str(),
            };
            let mut value = json!({"texture":texture,"cullface":face.name()});
            if face == Direction::Up {
                value["tintindex"] = json!(0);
            }
            faces.insert(face.name().into(), value);
        }
        let mut elements = vec![json!({"from":[0,0,0],"to":[16,16,16],"faces":faces})];
        if let Some(overlay) = overlay {
            let mut faces = Map::new();
            for face in [
                Direction::North,
                Direction::South,
                Direction::West,
                Direction::East,
            ] {
                faces.insert(
                    face.name().into(),
                    json!({"texture":overlay.as_str(),"cullface":face.name(),"tintindex":0}),
                );
            }
            elements.push(json!({"from":[0,0,0],"to":[16,16,16],"faces":faces}));
        }
        self.insert(
            ResourceKey {
                kind: ResourceKind::Model,
                id: model.clone(),
            },
            &json!({"elements":elements}),
            reason.clone(),
            cancel,
        )?;
        self.insert(
            ResourceKey {
                kind: ResourceKind::Blockstate,
                id: block,
            },
            &json!({"variants":{"":{"model":model.as_str()}}}),
            reason,
            cancel,
        )
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
impl DefinitionProvider for DefinitionSet {
    fn definition(&self, key: &ResourceKey, cancel: Cancel<'_>) -> Result<Option<DefinitionInput>> {
        cancel.check()?;
        let Some(entry) = self.entries.get(key) else {
            return Ok(None);
        };
        Ok(Some(DefinitionInput {
            document: Document::parse(&entry.blob, &self.limits, &self.budget, cancel)?,
            compatibility: Some(entry.reason.clone()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{identity::OriginKind, models::ModelCompiler, review::fixture_origin};
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn horizontal_column_companion_compiles_with_distinct_real_end_texture() {
        let stop = AtomicBool::new(false);
        let cancel = Cancel::new(&stop);
        let limits = Limits::default();
        let budget = ByteBudget::new(32 * 1024 * 1024).unwrap();
        let mut definitions = DefinitionSet::new(
            fixture_origin(OriginKind::OriginalCompatibilityGeometry),
            limits,
            budget.clone(),
        )
        .unwrap();
        definitions.install_geometry_templates(cancel).unwrap();
        definitions
            .bind_column(
                ResourceId::parse("minecraft:oak_log").unwrap(),
                ResourceId::parse("minecraft:block/oak_log").unwrap(),
                ResourceId::parse("minecraft:block/oak_log_top").unwrap(),
                Label::new("explicit original column geometry").unwrap(),
                cancel,
            )
            .unwrap();
        let mut compiler = ModelCompiler::new(&definitions, limits, budget).unwrap();
        let model = compiler
            .compile_model(
                &ResourceId::parse("minecraft:block/oak_log_horizontal").unwrap(),
                cancel,
            )
            .unwrap();
        assert_eq!(model.quads.len(), 6);
        assert_eq!(
            model
                .quads
                .iter()
                .filter(|quad| quad.texture.as_str() == "minecraft:block/oak_log_top")
                .count(),
            2
        );
        assert_eq!(
            model
                .quads
                .iter()
                .filter(|quad| quad.texture.as_str() == "minecraft:block/oak_log")
                .count(),
            4
        );
    }
}
