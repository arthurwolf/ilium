//! Complete camp terrain admission before projection. The global grid, sparse
//! selection and bounded foundation cuts are original Ilium placement choices.
use super::{
    assets::{
        block_state::BlockState,
        error::{AssetError, Result},
        identity::ResourceId,
    },
    noise::hash2,
    settings::VoxelLandscapeSettings,
    surface_biome_selector,
    surface_camps::{self, CampStyle},
    surface_ruins::SurfaceSample,
    surface_structures::{self, Habitat, PlacementError, Prepared, TemplateCell},
    terrain_fields::TerrainFields,
};
use std::collections::BTreeMap;

pub struct CampPlacement {
    pub style: CampStyle,
    pub prepared: Prepared<BlockState>,
}
fn state(id: &str, props: &[(&str, &str)]) -> std::result::Result<BlockState, PlacementError> {
    BlockState::new(
        ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
        props.iter().map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .map_err(|_| PlacementError::InvalidState)
}
fn error(e: PlacementError) -> AssetError {
    if e == PlacementError::Cancelled {
        AssetError::Cancelled
    } else {
        AssetError::InvalidMetadata(format!("camp placement: {e:?}"))
    }
}
fn global_xy(anchor: [i32; 3], [x, y]: [i32; 2], turns: u8) -> Option<[i32; 2]> {
    let [x, y] = match turns % 4 {
        1 => [-y, x],
        2 => [-x, -y],
        3 => [y, -x],
        _ => [x, y],
    };
    Some([anchor[0].checked_add(x)?, anchor[1].checked_add(y)?])
}
pub fn assemble(
    style: CampStyle,
    anchor: [i32; 3],
    turns: u8,
    seed: u64,
    sample: impl Fn([i32; 2]) -> Option<SurfaceSample>,
    cancelled: impl Fn() -> bool,
) -> Result<Option<CampPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if !(0..=249).contains(&anchor[2]) {
        return Ok(None);
    }
    let mut kit = surface_camps::build(style, seed, state).map_err(error)?;
    let mut columns = BTreeMap::new();
    // Each complete column is checked once. Unknown or wet terrain rejects the
    // entire camp; lower columns receive short supports rather than float.
    let support = state(
        if style == CampStyle::WoodedBadlands {
            "minecraft:red_sandstone"
        } else {
            "minecraft:dirt"
        },
        &[],
    )
    .map_err(error)?;
    for y in 0..28 {
        for x in 0..28 {
            if cancelled() {
                return Err(AssetError::Cancelled);
            }
            let Some(xy) = global_xy(anchor, [x, y], turns) else {
                return Ok(None);
            };
            let Some(ground) = sample(xy) else {
                return Ok(None);
            };
            if (i64::from(ground.ground) - i64::from(anchor[2])).abs() > 2
                || ground.water.is_some_and(|water| water > ground.ground)
            {
                return Ok(None);
            }
            columns.insert(xy, ground);
            for z in ground.ground + 1..anchor[2] {
                kit.template.cells.push(TemplateCell {
                    position: [x, y, z - anchor[2]],
                    state: Some(support.clone()),
                });
            }
        }
    }
    let prepared = Prepared::prepare(
        &kit.template,
        anchor,
        turns,
        |s, n| {
            BlockState::new(
                s.id().clone(),
                surface_structures::rotate_properties(s.properties(), n)?,
            )
            .map_err(|_| PlacementError::InvalidState)
        },
        |p| {
            if !(0..=256).contains(&p[2]) || !columns.contains_key(&[p[0], p[1]]) {
                Habitat::Unknown
            } else {
                Habitat::Replaceable
            }
        },
        &cancelled,
    );
    match prepared {
        Ok(prepared) => Ok(Some(CampPlacement { style, prepared })),
        Err(PlacementError::Cancelled) => Err(AssetError::Cancelled),
        Err(
            PlacementError::Unknown(_)
            | PlacementError::Protected(_)
            | PlacementError::CoordinateOverflow
            | PlacementError::Bounds,
        ) => Ok(None),
        Err(e) => Err(error(e)),
    }
}
pub fn candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<CampPlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let seed = u64::from(settings.seed);
    let hash = hash2(
        seed ^ 0x6361_6d70_3030_3031,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if hash % 100 >= settings.structures_percent.clamp(0, 200) as u64 / 8 {
        return Ok(None);
    }
    let Some(x) = grid[0]
        .checked_mul(128)
        .and_then(|v| v.checked_add(32 + ((hash >> 8) & 31) as i32))
    else {
        return Ok(None);
    };
    let Some(y) = grid[1]
        .checked_mul(128)
        .and_then(|v| v.checked_add(32 + ((hash >> 16) & 31) as i32))
    else {
        return Ok(None);
    };
    let terrain = fields.sample(x, y, settings.rivers);
    let biome = surface_biome_selector::select(seed, [x, y], terrain);
    let Some(style) = CampStyle::for_biome(biome.id()) else {
        return Ok(None);
    };
    assemble(
        style,
        [x, y, i32::from(terrain.height) - 1],
        ((hash >> 24) & 3) as u8,
        hash,
        |[sx, sy]| {
            let t = fields.sample(sx, sy, settings.rivers);
            Some(SurfaceSample {
                ground: i32::from(t.height) - 1,
                water: t.water_level.map(|v| i32::from(v) - 1),
            })
        },
        cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_foundations_and_all_rotations_project_without_owner_drift() {
        for style in CampStyle::ALL {
            for turns in 0..4 {
                let p = assemble(
                    style,
                    [30, -30, 70],
                    turns,
                    19,
                    |_| {
                        Some(SurfaceSample {
                            ground: 68,
                            water: None,
                        })
                    },
                    || false,
                )
                .unwrap()
                .unwrap()
                .prepared;
                assert!(p.cells().any(|(p, s)| p[2] == 69 && s.is_some()));
                let all: BTreeMap<_, _> = p.cells().map(|(p, s)| (p, s.cloned())).collect();
                let split: BTreeMap<_, _> = p
                    .project([-100, -100], [30, 100])
                    .unwrap()
                    .chain(p.project([30, -100], [100, 100]).unwrap())
                    .map(|(p, s)| (p, s.cloned()))
                    .collect();
                assert_eq!(all, split);
                assert_eq!(p.anchor(), [30, -30, 70]);
                assert!(p.source().contains(style.name()));
            }
        }
    }
    #[test]
    fn wet_unknown_sloped_or_cancelled_terrain_never_publishes_a_fragment() {
        for sample in [
            None,
            Some(SurfaceSample {
                ground: 60,
                water: Some(61),
            }),
            Some(SurfaceSample {
                ground: 56,
                water: None,
            }),
        ] {
            assert!(
                assemble(CampStyle::Forest, [0, 0, 60], 0, 0, |_| sample, || false)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(matches!(
            assemble(
                CampStyle::Forest,
                [0, 0, 60],
                0,
                0,
                |_| Some(SurfaceSample {
                    ground: 60,
                    water: None
                }),
                || true
            ),
            Err(AssetError::Cancelled)
        ));
    }
}
