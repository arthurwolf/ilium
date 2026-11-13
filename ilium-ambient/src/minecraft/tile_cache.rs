//! Prepared-data cache for the tiles of one built projected route.
//!
//! Building a route's tiles (chunk cells, model binding, face assembly) is the
//! slow part of starting the saved-world animation. The result is plain geometry
//! plus references into the texture bank, so it is stored as data and rebuilt
//! as a `NativeTile` list. Texture handles are only valid for the bank they were
//! made against: the route key includes the bank identity, and every restored
//! quad is re-validated against the freshly imported bank. Nothing read from
//! disk is trusted beyond that: owners are recomputed and budget is re-reserved.
use super::native_binding::{NativeTile, SharedNative};
use super::tours::PreparedMap;
use crate::voxel_landscape::{
    assets::{
        block_state::BlockState,
        budget::Cancel,
        identity::{Digest256, ResourceId},
        models::DefinitionOrigin,
    },
    surface_fluid::{FluidFace, FluidMesh},
    surface_mesh::{BoundModel, BoundQuad, FaceOwner, MeshRegion, PreparedMesh},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Serialize, Deserialize)]
struct CachedModel {
    state: BlockState,
    quads: Vec<BoundQuad>,
    opaque_boundaries: [bool; 6],
    origins: Vec<DefinitionOrigin>,
    medium: Option<ResourceId>,
    medium_boundaries: [bool; 6],
}

/// A mesh face is its position, an index into the route's model table and the
/// quad within that model.
type CachedFace = ([i32; 3], u32, u16);

#[derive(Serialize, Deserialize)]
struct CachedFluidFace {
    position: [i32; 3],
    quad: BoundQuad,
}

#[derive(Serialize, Deserialize)]
struct CachedTile {
    model_epoch: Digest256,
    mesh_region: MeshRegion,
    mesh_faces: Vec<CachedFace>,
    fluid: Option<(MeshRegion, Vec<CachedFluidFace>)>,
}

#[derive(Serialize, Deserialize)]
pub struct CachedRoute {
    bank_epoch: Digest256,
    models: Vec<CachedModel>,
    tiles: Vec<CachedTile>,
}

/// Snapshot the tiles of a finished route. Models shared between faces (and
/// tiles) are stored once.
pub fn capture(tiles: &[NativeTile], bank_epoch: Digest256) -> CachedRoute {
    let mut model_index = HashMap::<*const BoundModel, u32>::new();
    let mut models = Vec::new();
    let mut cached_tiles = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let mut mesh_faces = Vec::with_capacity(tile.mesh.faces.len());
        for face in &tile.mesh.faces {
            let key = Arc::as_ptr(&face.model);
            let index = *model_index.entry(key).or_insert_with(|| {
                models.push(CachedModel {
                    state: face.model.state.clone(),
                    quads: face.model.quads.clone(),
                    opaque_boundaries: face.model.opaque_boundaries,
                    origins: face.model.origins.clone(),
                    medium: face.model.medium.clone(),
                    medium_boundaries: face.model.medium_boundaries,
                });
                (models.len() - 1) as u32
            });
            mesh_faces.push((face.position(), index, face.quad_index));
        }
        // An empty fluid mesh is equivalent to none for every consumer.
        let fluid = tile
            .fluid
            .as_ref()
            .filter(|fluid| !fluid.faces.is_empty())
            .map(|fluid| {
                (
                    fluid.region,
                    fluid
                        .faces
                        .iter()
                        .map(|face| CachedFluidFace {
                            position: face.position,
                            quad: face.quad.clone(),
                        })
                        .collect(),
                )
            });
        cached_tiles.push(CachedTile {
            model_epoch: tile.model_epoch,
            mesh_region: tile.mesh.region,
            mesh_faces,
            fluid,
        });
    }
    CachedRoute {
        bank_epoch,
        models,
        tiles: cached_tiles,
    }
}

/// Rebuild the tiles against the current shared bank. `None` means the entry
/// does not fit this bank or failed validation; the caller rebuilds the route.
pub fn restore(
    cached: CachedRoute,
    shared: &SharedNative,
    map: &Arc<PreparedMap>,
    cancel: Cancel<'_>,
) -> Option<Vec<NativeTile>> {
    let imports = shared.imports();
    let bank = &imports.bank;
    let bank_epoch = bank.identity();
    if cached.bank_epoch != bank_epoch {
        return None;
    }
    let budget = shared.budget();
    let provenance = shared.provenance();
    let mut models = Vec::with_capacity(cached.models.len());
    for model in cached.models {
        let rebuilt = BoundModel::from_cached(
            model.state,
            model.quads,
            model.opaque_boundaries,
            model.origins,
            model.medium,
            model.medium_boundaries,
            bank,
            budget,
            cancel,
        )
        .ok()?;
        models.push(Arc::new(rebuilt));
    }
    let mut tiles = Vec::with_capacity(cached.tiles.len());
    for tile in cached.tiles {
        let mut placed = Vec::with_capacity(tile.mesh_faces.len());
        for (position, index, quad_index) in tile.mesh_faces {
            let model = models.get(usize::try_from(index).ok()?)?;
            placed.push((position, Arc::clone(model), quad_index));
        }
        let mesh =
            PreparedMesh::from_cached(placed, tile.mesh_region, bank_epoch, budget, cancel).ok()?;
        let fluid = match tile.fluid {
            Some((region, faces)) => {
                let faces = faces
                    .into_iter()
                    .map(|face| FluidFace {
                        position: face.position,
                        owner: FaceOwner {
                            position: face.position,
                            part: face.quad.part,
                            face: face.quad.face,
                            layer: face.quad.material.layer,
                        },
                        quad: face.quad,
                    })
                    .collect();
                Some(FluidMesh::from_native_faces(faces, region, bank, budget, cancel).ok()?)
            }
            None => None,
        };
        tiles.push(NativeTile {
            mesh,
            fluid,
            imports: Arc::clone(imports),
            map: Arc::clone(map),
            source_profile: provenance.profile,
            native_archive_sha256: provenance.native_archive_sha256,
            selected_archive_sha256: provenance.selected_archive_sha256,
            bank_epoch,
            model_epoch: tile.model_epoch,
        });
    }
    Some(tiles)
}
