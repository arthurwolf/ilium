//! Bounded world preparation. The presentation path receives an exposed mesh;
//! climate, chunks and feature geometry are prepared by an owned worker.
use super::{
    catalog::Material,
    chunks::ColumnCache,
    ecology, features,
    placement::PlacementKey,
    settings::VoxelLandscapeSettings,
    terrain, wetland,
    world::{Column, VisibleBlock, WorldWindow},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub minimum: [i32; 2],
    pub maximum: [i32; 2],
}
pub struct PreparedWorld {
    pub region: Region,
    pub blocks: Vec<VisibleBlock>,
    pub instances: Vec<features::Instance>,
    pub biomes: Vec<ecology::Biome>,
    pub cached_chunks: usize,
}
/// Classify from the frozen kernel column, then derive presentation occupancy.
/// Both terrain attachment and feature habitat use this one pure result.
pub fn sample_occupied_column(
    seed: u64,
    x: i32,
    y: i32,
    raw: terrain::Column,
) -> (terrain::Column, ecology::Ecology) {
    let ecology = ecology::sample(seed, x, y, raw);
    (wetland::occupy(seed, x, y, raw, ecology), ecology)
}

pub fn prepare(
    region: Region,
    settings: &VoxelLandscapeSettings,
    cache: &mut ColumnCache<terrain::Column>,
    is_cancelled: impl Fn() -> bool,
) -> Option<PreparedWorld> {
    if region.maximum[0] <= region.minimum[0]
        || region.maximum[1] <= region.minimum[1]
        || (0..2).any(|axis| {
            i64::from(region.maximum[axis]) - i64::from(region.minimum[axis]) > 1024
                || region.minimum[axis] < i32::MIN + 128
                || region.maximum[axis] > i32::MAX - 128
        })
    {
        return None;
    }
    let kernel = terrain::Terrain::new(u64::from(settings.seed)).with_features(
        settings.rivers,
        settings.ravines,
        settings.caves,
    );
    let mut sample = |x, y| {
        let raw = cache
            .get_with_chunk(x, y, |cx, cy| kernel.chunk(cx, cy))
            .unwrap_or_else(|| kernel.sample(x, y));
        sample_occupied_column(u64::from(settings.seed), x, y, raw)
    };
    let mut columns = BTreeMap::new();
    let mut occupied_columns = Vec::new();
    let mut frozen = Vec::new();
    let mut needles = Vec::new();
    let mut biomes = Vec::new();
    for y in region.minimum[1] - 1..=region.maximum[1] {
        if is_cancelled() {
            return None;
        }
        for x in region.minimum[0] - 1..=region.maximum[0] {
            let (occupied, ecology) = sample(x, y);
            let mut column = Column::from_terrain(occupied);
            column.surface = ecology.surface;
            column.soil = ecology.soil;
            column.rock = ecology.rock;
            columns.insert([x, y], column);
            occupied_columns.push(([x, y], occupied));
            if !biomes.contains(&ecology.biome) {
                biomes.push(ecology.biome);
            }
            if ecology.frozen_surface {
                if let Some(water) = occupied.water_level {
                    frozen.push(([x, y, i32::from(water) - 1], Some(Material::Ice)));
                }
            }
            if settings.detail >= 2
                && ecology.biome == ecology::Biome::IceSpikes
                && occupied.water_level.is_none()
                && x.rem_euclid(7) == 0
                && y.rem_euclid(7) == 0
            {
                let hash = super::noise::hash2(
                    u64::from(settings.seed) ^ 0x0069_6365,
                    i64::from(x),
                    i64::from(y),
                );
                if ((hash % 100) as i32) < settings.vegetation_percent {
                    let height = 5 + (hash % 12) as i32;
                    for z in 0..height {
                        let radius = if z < height / 3 { 1 } else { 0 };
                        for dx in -radius..=radius {
                            for dy in -radius..=radius {
                                needles.push((
                                    [x + dx, y + dy, i32::from(occupied.height) + z],
                                    Some(if z + 1 == height {
                                        Material::Snow
                                    } else {
                                        Material::Ice
                                    }),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut window = WorldWindow::from_columns(region.minimum, region.maximum, columns);
    for ([x, y], column) in occupied_columns {
        window.attach_terrain(x, y, column);
    }
    let key = PlacementKey {
        priority: 3,
        x: 0,
        y: 0,
        feature: u16::MAX,
    };
    window.overlay.place([0; 3], 0, key, frozen);
    window.overlay.place([0; 3], 0, key, needles);
    let instances = features::populate(
        &mut window,
        region.minimum,
        region.maximum,
        settings,
        &mut sample,
        &is_cancelled,
    );
    if is_cancelled() {
        return None;
    }
    let blocks = window.visible_blocks_checked(&is_cancelled)?;
    if is_cancelled() {
        return None;
    }
    Some(PreparedWorld {
        region,
        blocks,
        instances,
        biomes,
        cached_chunks: cache.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn mangrove_witness_has_actual_water_mud_and_rooted_tree_geometry() {
        let settings = VoxelLandscapeSettings {
            seed: 7,
            detail: 3,
            structures_percent: 0,
            vegetation_percent: 200,
            ..Default::default()
        };
        let [x, y] = [-439632, -174669];
        let prepared = prepare(
            Region {
                minimum: [x - 32, y - 32],
                maximum: [x + 32, y + 32],
            },
            &settings,
            &mut ColumnCache::new(64),
            || false,
        )
        .unwrap();
        let kernel = terrain::Terrain::new(7);
        let target_blocks: Vec<_> = prepared
            .blocks
            .iter()
            .filter(|block| {
                let [bx, by, _] = block.position;
                ecology::sample(7, bx, by, kernel.sample(bx, by)).biome
                    == ecology::Biome::MangroveSwamp
            })
            .collect();
        assert!(target_blocks
            .iter()
            .any(|block| block.material == Material::Water));
        assert!(target_blocks
            .iter()
            .any(|block| block.material == Material::Mud));
        assert!(target_blocks
            .iter()
            .any(|block| block.material == Material::MangroveLog));
        assert!(prepared
            .instances
            .iter()
            .any(|instance| matches!(instance.id.0, 13 | 14)));
    }

    #[test]
    fn centered_mangrove_raster_has_visible_water_mud_and_stilt_roots() {
        use super::super::{catalog::FEATURE_RECIPES, engine::VoxelLandscapeScene, render};

        let settings = VoxelLandscapeSettings {
            seed: 7,
            zoom_percent: 300,
            detail: 3,
            ..Default::default()
        };
        let camera = [-439632.0, -174669.0, 19.0];
        let size = [320, 200];
        let scale = VoxelLandscapeScene::scale(&settings);
        let region = VoxelLandscapeScene::region(camera, scale, size);
        let prepared = prepare(region, &settings, &mut ColumnCache::new(256), || false).unwrap();
        let canvas = render::draw_world(&prepared, camera, scale, size, 7);
        let materials: BTreeMap<_, _> = prepared
            .blocks
            .iter()
            .map(|block| (block.position, block.material))
            .collect();
        let kernel = terrain::Terrain::new(7);
        let target_visible: BTreeSet<_> = canvas
            .block_owners
            .into_iter()
            .flatten()
            .filter(|position| {
                ecology::sample(
                    7,
                    position[0],
                    position[1],
                    kernel.sample(position[0], position[1]),
                )
                .biome
                    == ecology::Biome::MangroveSwamp
            })
            .collect();
        for material in [Material::Water, Material::Mud] {
            assert!(target_visible
                .iter()
                .any(|position| materials.get(position) == Some(&material)));
        }

        // Tie visible log voxels to recipe 13 itself; a halo placement or a
        // different recipe using MangroveLog cannot satisfy this regression.
        let mut stilt_roots = BTreeSet::new();
        for instance in prepared
            .instances
            .iter()
            .filter(|instance| instance.id.0 == 13)
        {
            let [anchor_x, anchor_y, _] = instance.anchor;
            let (occupied_anchor, source_ecology) =
                sample_occupied_column(7, anchor_x, anchor_y, kernel.sample(anchor_x, anchor_y));
            if source_ecology.biome != ecology::Biome::MangroveSwamp
                || occupied_anchor.water_level.is_none()
            {
                continue;
            }
            FEATURE_RECIPES[13].visit_blocks(|local_x, local_z, local_y, material| {
                if material != Some(Material::MangroveLog) || local_z > 2 {
                    return;
                }
                let (dx, dy) = match instance.rotation % 4 {
                    1 => (-i32::from(local_y), i32::from(local_x)),
                    2 => (-i32::from(local_x), -i32::from(local_y)),
                    3 => (i32::from(local_y), -i32::from(local_x)),
                    _ => (i32::from(local_x), i32::from(local_y)),
                };
                stilt_roots.insert([
                    instance.anchor[0] + dx,
                    instance.anchor[1] + dy,
                    instance.anchor[2] + i32::from(local_z),
                ]);
            });
        }
        assert!(target_visible.iter().any(|position| {
            stilt_roots.contains(position)
                && materials.get(position) == Some(&Material::MangroveLog)
        }));
    }

    #[test]
    fn wetland_overlap_and_cache_eviction_preserve_shared_visible_blocks() {
        let settings = VoxelLandscapeSettings {
            seed: 7,
            detail: 3,
            structures_percent: 0,
            vegetation_percent: 200,
            ..Default::default()
        };
        let [x, y] = [-439632, -174669];
        let a = prepare(
            Region {
                minimum: [x - 40, y - 40],
                maximum: [x + 24, y + 24],
            },
            &settings,
            &mut ColumnCache::new(2),
            || false,
        )
        .unwrap();
        let b = prepare(
            Region {
                minimum: [x - 24, y - 24],
                maximum: [x + 40, y + 40],
            },
            &settings,
            &mut ColumnCache::new(64),
            || false,
        )
        .unwrap();
        let recolored = VoxelLandscapeSettings {
            color_mode: 0,
            palette: 3,
            hue_degrees: 360,
            lightness_percent: 5,
            ..settings
        };
        let c = prepare(a.region, &recolored, &mut ColumnCache::new(64), || false).unwrap();
        assert_eq!(a.instances, c.instances);
        assert_eq!(
            a.blocks
                .iter()
                .map(|block| (block.position, block.material, block.faces))
                .collect::<Vec<_>>(),
            c.blocks
                .iter()
                .map(|block| (block.position, block.material, block.faces))
                .collect::<Vec<_>>()
        );
        let shared = |world: PreparedWorld| {
            world
                .blocks
                .into_iter()
                .filter(|block| {
                    (x - 16..x + 16).contains(&block.position[0])
                        && (y - 16..y + 16).contains(&block.position[1])
                })
                .map(|block| (block.position, block.material, block.faces))
                .collect::<Vec<_>>()
        };
        let a = shared(a);
        let b = shared(b);
        assert!(!a.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn preparation_is_cancellable_and_world_identity_survives_cache_eviction() {
        let settings = VoxelLandscapeSettings {
            detail: 0,
            ..Default::default()
        };
        let region = Region {
            minimum: [-16, -16],
            maximum: [16, 16],
        };
        assert!(prepare(region, &settings, &mut ColumnCache::new(2), || true).is_none());
        let a = prepare(region, &settings, &mut ColumnCache::new(2), || false).unwrap();
        let b = prepare(region, &settings, &mut ColumnCache::new(32), || false).unwrap();
        assert_eq!(
            a.blocks
                .iter()
                .map(|b| (b.position, b.material, b.faces))
                .collect::<Vec<_>>(),
            b.blocks
                .iter()
                .map(|b| (b.position, b.material, b.faces))
                .collect::<Vec<_>>()
        );
        assert!(a.cached_chunks <= 2);
        assert!(a.blocks.len() > 1000);
    }
    #[test]
    fn detail_adds_stable_anchors_and_color_controls_do_not_change_world_geometry() {
        let settings = VoxelLandscapeSettings {
            seed: 7,
            detail: 2,
            ..Default::default()
        };
        let (_, _, x, y) = ecology::WITNESSES
            .iter()
            .find(|(biome, _, _, _)| *biome == ecology::Biome::Forest)
            .copied()
            .unwrap();
        let region = Region {
            minimum: [x - 24, y - 24],
            maximum: [x + 24, y + 24],
        };
        let a = prepare(region, &settings, &mut ColumnCache::new(64), || false).unwrap();
        let richer = VoxelLandscapeSettings {
            detail: 3,
            ..settings.clone()
        };
        let b = prepare(region, &richer, &mut ColumnCache::new(64), || false).unwrap();
        assert!(b.instances.len() > a.instances.len());
        assert!(a
            .instances
            .iter()
            .all(|instance| b.instances.contains(instance)));
        let recolored = VoxelLandscapeSettings {
            color_mode: 0,
            palette: 3,
            hue_degrees: 360,
            lightness_percent: 5,
            ..settings
        };
        let c = prepare(region, &recolored, &mut ColumnCache::new(64), || false).unwrap();
        assert_eq!(a.instances, c.instances);
        assert_eq!(
            a.blocks
                .iter()
                .map(|block| (block.position, block.material, block.faces))
                .collect::<Vec<_>>(),
            c.blocks
                .iter()
                .map(|block| (block.position, block.material, block.faces))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn ice_spikes_are_actual_tapered_vertical_geometry() {
        let (_, _, x, y) = ecology::WITNESSES
            .iter()
            .find(|(biome, _, _, _)| *biome == ecology::Biome::IceSpikes)
            .copied()
            .unwrap();
        let settings = VoxelLandscapeSettings {
            seed: 7,
            detail: 2,
            structures_percent: 0,
            vegetation_percent: 200,
            ..Default::default()
        };
        let prepared = prepare(
            Region {
                minimum: [x - 14, y - 14],
                maximum: [x + 14, y + 14],
            },
            &settings,
            &mut ColumnCache::new(32),
            || false,
        )
        .unwrap();
        let kernel = terrain::Terrain::new(7);
        assert!(prepared
            .blocks
            .iter()
            .any(|block| block.material == Material::Ice
                && block.position[2]
                    > i32::from(kernel.sample(block.position[0], block.position[1]).height) + 3));
    }
}
