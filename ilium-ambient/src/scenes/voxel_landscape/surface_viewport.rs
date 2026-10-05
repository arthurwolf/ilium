//! Screen-derived world coverage for the generated surface. A request is
//! rejected when its finite source budget cannot cover the entire screen;
//! shrinking the region would publish clipped geometry as a complete frame.
use super::{
    assets::error::{AssetError, Result},
    surface_generation::Region,
};

const ISOMETRIC_X: f64 = 0.866_025_403_784_438_6;
/// The half-open generated-source Z envelope. Every emitted generated block,
/// fluid and model silhouette is checked before it can enter a published world.
/// The exhaustive authored-family regression below ties this planner bound to
/// the terrain cap, all tree profiles, exterior kits and fauna geometry.
pub const SOURCE_Z_MIN: f64 = 0.0;
pub const SOURCE_Z_MAX: f64 = 320.0;
/// Core side used when finer viewport tiles exceed the preferred tile count or
/// cannot fit the hard per-frame cap. Finer cores tighten projected culling at
/// the cost of repeated halo generation.
pub const CORE_SIDE: i32 = 96;
const FINE_CORE_SIDES: [i32; 2] = [48, 64];
pub const GENERATION_HALO: i32 = 16;
// Streaming binds one tile at a time; this is a finite work bound, not a
// retained-world/mesh allocation bound. The 25% 720x480 witness selects 169.
pub const MAX_VIEWPORT_TILES: usize = 256;
const MAX_RAW_TILES: usize = 1024;
const PROJECTION_MARGIN: f64 = 24.0;
/// Zoom scale at which the preferred viewport budget reaches the hard cap.
const REFERENCE_SCALE: f64 = 2.8;

/// Invert both isometric axes at both source-height extremes. Including both
/// heights is necessary because a high, distant crown can enter the same pixel
/// as a near low riverbank. One cell of model extent and one cell of rounding
/// tolerance are included before aligning to the generation grid.
pub fn region(camera: [f64; 3], scale: f32, size: [usize; 2]) -> Result<Region> {
    if camera.iter().any(|value| !value.is_finite())
        || !scale.is_finite()
        || !(0.01..=1024.0).contains(&scale)
        || size.contains(&0)
    {
        return Err(AssetError::InvalidMetadata(
            "invalid generated-surface viewport".into(),
        ));
    }
    let scale = f64::from(scale);
    let horizontal = size[0] as f64 / (4.0 * ISOMETRIC_X * scale);
    let vertical = size[1] as f64 / (2.0 * scale);
    let reach = horizontal + vertical;
    let lower = SOURCE_Z_MIN - camera[2] - reach - 2.0;
    let upper = SOURCE_Z_MAX - camera[2] + reach + 2.0;
    let mut minimum = [0; 2];
    let mut maximum = [0; 2];
    for axis in 0..2 {
        let low = ((camera[axis] + lower).floor() / 16.0).floor() * 16.0;
        let high = ((camera[axis] + upper).ceil() / 16.0).ceil() * 16.0;
        if low < f64::from(i32::MIN + 256)
            || high > f64::from(i32::MAX - 256)
            || high - low > 2048.0
        {
            return Err(AssetError::Limit {
                resource: "generated viewport axis",
                requested: (high - low).max(0.0) as u64,
                limit: 2048,
            });
        }
        minimum[axis] = low as i32;
        maximum[axis] = high as i32;
    }
    let planned = Region { minimum, maximum };
    visible_tiles(planned, scale as f32, size)?;
    Ok(planned)
}

/// Select only cells whose complete source-height sweep can touch the frame.
/// The rounded region centre reconstructs camera_x-camera_z and
/// camera_y-camera_z within eight cells. A 24-cell XY expansion covers that
/// rounding, cell/model extents and immediate neighbor/cull support, making
/// the chosen tile set stable for every camera that produces this same region.
pub fn visible_tiles(
    region: Region,
    scale: f32,
    size: [usize; 2],
) -> Result<Vec<(Region, Region)>> {
    if !scale.is_finite() || scale <= 0.0 || size.contains(&0) {
        return Err(AssetError::InvalidMetadata(
            "invalid generated viewport tile projection".into(),
        ));
    }
    let scale = f64::from(scale);
    let middle_z = (SOURCE_Z_MIN + SOURCE_Z_MAX) * 0.5;
    let relative_camera = [
        (f64::from(region.minimum[0]) + f64::from(region.maximum[0])) * 0.5 - middle_z,
        (f64::from(region.minimum[1]) + f64::from(region.maximum[1])) * 0.5 - middle_z,
    ];
    let preferred_tile_count = preferred_tile_count(scale);
    let mut last_budget_error = None;
    let mut coarsest_within_hard_budget = None;
    for core_side in FINE_CORE_SIDES.into_iter().chain([CORE_SIDE]) {
        match visible_tiles_with_core_side(region, scale, size, relative_camera, core_side) {
            Ok(selected) if selected.len() <= preferred_tile_count => return Ok(selected),
            Ok(selected) => coarsest_within_hard_budget = Some(selected),
            Err(error @ AssetError::Limit { .. }) => {
                if core_side == CORE_SIDE {
                    return coarsest_within_hard_budget.ok_or(error);
                }
                last_budget_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    if let Some(selected) = coarsest_within_hard_budget {
        return Ok(selected);
    }
    Err(last_budget_error.unwrap_or_else(|| {
        AssetError::InvalidMetadata("no generated viewport core sizes configured".into())
    }))
}

fn preferred_tile_count(scale: f64) -> usize {
    let scale_ratio = scale / REFERENCE_SCALE;
    (MAX_VIEWPORT_TILES as f64 / scale_ratio.powi(2))
        .round()
        .clamp(1.0, MAX_VIEWPORT_TILES as f64) as usize
}

fn visible_tiles_with_core_side(
    region: Region,
    scale: f64,
    size: [usize; 2],
    relative_camera: [f64; 2],
    core_side: i32,
) -> Result<Vec<(Region, Region)>> {
    let mut selected = Vec::new();
    for (core, expanded) in tiles_with_core_side(region, core_side)? {
        let x0 = f64::from(core.minimum[0]) - relative_camera[0] - PROJECTION_MARGIN;
        let x1 = f64::from(core.maximum[0]) - relative_camera[0] + PROJECTION_MARGIN;
        let y0 = f64::from(core.minimum[1]) - relative_camera[1] - PROJECTION_MARGIN;
        let y1 = f64::from(core.maximum[1]) - relative_camera[1] + PROJECTION_MARGIN;
        let screen_left = ISOMETRIC_X * scale * (x0 - y1);
        let screen_right = ISOMETRIC_X * scale * (x1 - y0);
        let screen_top = scale * ((x0 + y0) * 0.5 - SOURCE_Z_MAX);
        let screen_bottom = scale * ((x1 + y1) * 0.5 - SOURCE_Z_MIN);
        if screen_right < -(size[0] as f64) * 0.5
            || screen_left > size[0] as f64 * 0.5
            || screen_bottom < -(size[1] as f64) * 0.5
            || screen_top > size[1] as f64 * 0.5
        {
            continue;
        }
        selected.push((core, expanded));
        if selected.len() > MAX_VIEWPORT_TILES {
            return Err(AssetError::Limit {
                resource: "generated viewport tiles",
                requested: selected.len() as u64,
                limit: MAX_VIEWPORT_TILES as u64,
            });
        }
    }
    Ok(selected)
}

/// Each owned core is prepared with a 16-cell context on each side. The
/// original 128-cell generator ceiling remains intact. Published cells belong
/// to exactly one core; halo cells are never independently published.
#[cfg(test)]
fn tiles(region: Region) -> Result<Vec<(Region, Region)>> {
    tiles_with_core_side(region, CORE_SIDE)
}

fn tiles_with_core_side(region: Region, core_side: i32) -> Result<Vec<(Region, Region)>> {
    if core_side <= 0 || core_side + GENERATION_HALO * 2 > 128 {
        return Err(AssetError::InvalidMetadata(
            "generated viewport tile core exceeds bounded generator size".into(),
        ));
    }
    if (0..2).any(|axis| {
        region.minimum[axis] >= region.maximum[axis]
            || region.minimum[axis] < i32::MIN + 256
            || region.maximum[axis] > i32::MAX - 256
            || i64::from(region.maximum[axis]) - i64::from(region.minimum[axis]) > 2048
    }) {
        return Err(AssetError::InvalidMetadata(
            "generated viewport outside finite domain".into(),
        ));
    }
    let mut result = Vec::new();
    let mut y = region.minimum[1];
    while y < region.maximum[1] {
        let end_y = y.saturating_add(core_side).min(region.maximum[1]);
        let mut x = region.minimum[0];
        while x < region.maximum[0] {
            let end_x = x.saturating_add(core_side).min(region.maximum[0]);
            let core = Region {
                minimum: [x, y],
                maximum: [end_x, end_y],
            };
            let expanded = Region {
                minimum: [x - GENERATION_HALO, y - GENERATION_HALO],
                maximum: [end_x + GENERATION_HALO, end_y + GENERATION_HALO],
            };
            expanded.validate()?;
            result.push((core, expanded));
            if result.len() > MAX_RAW_TILES {
                return Err(AssetError::Limit {
                    resource: "generated viewport tile grid",
                    requested: result.len() as u64,
                    limit: MAX_RAW_TILES as u64,
                });
            }
            x = end_x;
        }
        y = end_y;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::voxel_landscape::surface_entities::{
        model, AtlasLayout, ClimateSkin, ALL_SPECIES,
    };

    #[test]
    fn twenty_five_percent_large_frame_is_admitted_as_bounded_tiles() {
        let viewport = region([64.0, 64.0, 80.0], 0.7, [720, 480]).unwrap();
        let selected = visible_tiles(viewport, 0.7, [720, 480]).unwrap();
        assert_eq!(selected.len(), 169);
        assert!(selected.len() <= MAX_VIEWPORT_TILES);
        assert!(selected.iter().all(|(core, _)| {
            core.maximum[0] - core.minimum[0] == CORE_SIDE
                && core.maximum[1] - core.minimum[1] == CORE_SIDE
        }));
    }

    #[test]
    fn normal_viewport_uses_finer_cores_when_the_tile_budget_allows_them() {
        let viewport = region([11_328.0, -12_112.0, 96.0], 2.8, [480, 320]).unwrap();
        let selected = visible_tiles(viewport, 2.8, [480, 320]).unwrap();

        assert!(!selected.is_empty() && selected.len() <= MAX_VIEWPORT_TILES);
        assert!(selected.iter().all(|(core, _)| {
            core.maximum[0] - core.minimum[0] <= 48 && core.maximum[1] - core.minimum[1] <= 48
        }));
    }

    #[test]
    fn extreme_zoom_uses_the_coarsest_candidate_when_no_tile_target_is_met() {
        let viewport = Region {
            minimum: [0, 0],
            maximum: [128, 128],
        };

        let selected = visible_tiles(viewport, 1024.0, [4096, 4096]).unwrap();

        assert!(!selected.is_empty() && selected.len() <= MAX_VIEWPORT_TILES);
    }

    #[test]
    fn four_hundred_percent_zoom_uses_the_zoom_scaled_tile_budget() {
        let camera = [64.0, 64.0, 80.0];
        let scale = 2.8 * 4.0;
        let size = [160, 96];
        let viewport = region(camera, scale, size).unwrap();
        let selected = visible_tiles(viewport, scale, size).unwrap();

        assert!(!selected.is_empty() && selected.len() <= 16);

        for y in viewport.minimum[1]..viewport.maximum[1] {
            for x in viewport.minimum[0]..viewport.maximum[0] {
                let screen_x = ISOMETRIC_X
                    * f64::from(scale)
                    * (f64::from(x) - camera[0] - f64::from(y) + camera[1]);
                let screen_y_at_minimum = f64::from(scale)
                    * ((f64::from(x) - camera[0] + f64::from(y) - camera[1]) * 0.5
                        - (SOURCE_Z_MIN - camera[2]));
                let screen_y_at_maximum = f64::from(scale)
                    * ((f64::from(x) - camera[0] + f64::from(y) - camera[1]) * 0.5
                        - (SOURCE_Z_MAX - camera[2]));
                let screen_top = screen_y_at_minimum.min(screen_y_at_maximum);
                let screen_bottom = screen_y_at_minimum.max(screen_y_at_maximum);
                if screen_x.abs() <= size[0] as f64 * 0.5
                    && screen_bottom >= -(size[1] as f64) * 0.5
                    && screen_top <= size[1] as f64 * 0.5
                {
                    assert!(selected.iter().any(|(core, _)| core.contains([x, y, 0])));
                    for [dx, dy] in [[-1, 0], [1, 0], [0, -1], [0, 1]] {
                        assert!(
                            selected
                                .iter()
                                .any(|(core, _)| core.contains([x + dx, y + dy, 0])),
                            "visible source column [{x}, {y}] lacks neighbor [{dx}, {dy}]"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_supported_zoom_keeps_large_screen_source_coverage_finite() {
        let camera = [64.0, 64.0, 80.0];
        for size in [[720, 480], [360, 240], [160, 96]] {
            for zoom in 25..=400 {
                let scale = 2.8 * zoom as f32 / 100.0;
                let viewport = region(camera, scale, size).unwrap();
                let selected = visible_tiles(viewport, scale, size).unwrap();
                assert!(
                    !selected.is_empty() && selected.len() <= MAX_VIEWPORT_TILES,
                    "zoom {zoom}, size {size:?}, selected {} tiles",
                    selected.len()
                );
            }
        }
    }

    #[test]
    fn native_360_by_240_frame_needs_more_than_the_old_96_or_128_cells() {
        let viewport = region([64.0, 64.0, 80.0], 2.8, [360, 240]).unwrap();
        for axis in 0..2 {
            assert!(viewport.maximum[axis] - viewport.minimum[axis] > 128);
        }
        assert!(tiles(viewport).unwrap().len() > 1);
    }

    #[test]
    fn projected_corners_at_all_source_heights_stay_inside_the_request() {
        for (size, scale) in [([160, 96], 2.8), ([360, 240], 2.8), ([720, 480], 4.2)] {
            let camera = [-15_024.25, -16_256.75, 87.0];
            let viewport = region(camera, scale, size).unwrap();
            for sx in [0.0, size[0] as f64] {
                for sy in [0.0, size[1] as f64] {
                    for z in [SOURCE_Z_MIN, SOURCE_Z_MAX] {
                        let horizontal =
                            (sx - size[0] as f64 * 0.5) / (2.0 * ISOMETRIC_X * f64::from(scale));
                        let vertical = (sy - size[1] as f64 * 0.5) / f64::from(scale);
                        let dx = horizontal + vertical + z - camera[2];
                        let dy = -horizontal + vertical + z - camera[2];
                        for (axis, coordinate) in
                            [camera[0] + dx, camera[1] + dy].into_iter().enumerate()
                        {
                            assert!(coordinate >= f64::from(viewport.minimum[axis] + 2));
                            assert!(coordinate <= f64::from(viewport.maximum[axis] - 2));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_tile_has_a_bounded_halo_and_exact_unique_core_ownership() {
        let viewport = Region {
            minimum: [-16, -16],
            maximum: [144, 144],
        };
        let tiles = tiles(viewport).unwrap();
        for (core, expanded) in &tiles {
            expanded.validate().unwrap();
            assert_eq!(expanded.minimum[0], core.minimum[0] - GENERATION_HALO);
            assert_eq!(expanded.maximum[1], core.maximum[1] + GENERATION_HALO);
        }
        for y in viewport.minimum[1]..viewport.maximum[1] {
            for x in viewport.minimum[0]..viewport.maximum[0] {
                assert_eq!(
                    tiles
                        .iter()
                        .filter(|(core, _)| core.contains([x, y, 0]))
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn projected_tile_selection_keeps_every_sample_that_can_enter_the_frame() {
        let camera = [-15011.0, -16246.0, 91.0];
        let scale = 2.8_f32;
        let size = [360, 240];
        let viewport = region(camera, scale, size).unwrap();
        let selected = visible_tiles(viewport, scale, size).unwrap();
        let selected_core_side = selected
            .iter()
            .map(|(core, _)| core.maximum[0] - core.minimum[0])
            .max()
            .unwrap();
        assert!(
            selected.len()
                < tiles_with_core_side(viewport, selected_core_side)
                    .unwrap()
                    .len()
        );
        for y in (viewport.minimum[1]..viewport.maximum[1]).step_by(8) {
            for x in (viewport.minimum[0]..viewport.maximum[0]).step_by(8) {
                for z in [0, 80, 160, 240, 320] {
                    let sx = ISOMETRIC_X
                        * f64::from(scale)
                        * (f64::from(x) - camera[0] - f64::from(y) + camera[1]);
                    let sy = f64::from(scale)
                        * ((f64::from(x) - camera[0] + f64::from(y) - camera[1]) * 0.5
                            - (f64::from(z) - camera[2]));
                    if sx.abs() <= size[0] as f64 * 0.5 && sy.abs() <= size[1] as f64 * 0.5 {
                        assert!(
                            selected.iter().any(|(core, _)| core.contains([x, y, z])),
                            "visible sample [{x}, {y}, {z}] omitted"
                        );
                        for [dx, dy] in [[-1, 0], [1, 0], [0, -1], [0, 1]] {
                            assert!(
                                selected
                                    .iter()
                                    .any(|(core, _)| core.contains([x + dx, y + dy, z])),
                                "visible sample [{x}, {y}, {z}] lacks neighbor [{dx}, {dy}]"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_authored_entity_silhouettes_fit_the_projection_margin() {
        for &species in ALL_SPECIES {
            for climate in [ClimateSkin::Temperate, ClimateSkin::Warm, ClimateSkin::Cold] {
                let entity = model(species, AtlasLayout::Bedrock, climate);
                let (minimum, maximum) = entity.bounds().unwrap();
                for axis in 0..2 {
                    assert!(f64::from(minimum[axis]).abs() < PROJECTION_MARGIN - 8.0);
                    assert!(f64::from(maximum[axis]).abs() < PROJECTION_MARGIN - 8.0);
                }
            }
        }
    }

    #[test]
    fn every_authored_family_respects_the_generated_height_envelope() {
        use super::super::{
            surface_camps::{self, CampStyle},
            surface_landmarks::{self, LandmarkKind},
            surface_ruins::{self, RuinKind},
            tree_forms::{self, Growth},
            tree_profiles::TREE_PROFILES,
            village_kit::{
                self, FarmForm, HomeForm, PieceKind, Profession, RoadShape, VillageStyle,
            },
        };
        let check = |label: &str, heights: Vec<i32>| {
            assert!(!heights.is_empty(), "{label}: empty authored family");
            // Structure anchors are sampled from <=240 terrain (landmarks may
            // accept <=256); a local range within -64..64 fits the 0..320
            // published envelope when whole-candidate habitat rejects Z<0.
            assert!(
                heights.iter().all(|&z| (-64..=63).contains(&z)),
                "{label}: local Z outside +/-64: {:?}",
                (heights.iter().min(), heights.iter().max())
            );
        };
        for profile in TREE_PROFILES {
            for growth in [Growth::Young, Growth::Mature, Growth::Old] {
                for entropy in [0, 1, u64::MAX] {
                    let geometry = tree_forms::build(&profile, growth, entropy).unwrap();
                    let heights: Vec<_> = geometry.cells().map(|(p, _)| i32::from(p[2])).collect();
                    assert!(heights.iter().all(|&z| z.abs()
                        <= i32::from(super::super::tree_geometry::TreeGeometry::COORDINATE_LIMIT)));
                    check(profile.id, heights);
                }
            }
        }
        for seed in 0..40 {
            for style in VillageStyle::ALL {
                for kind in HomeForm::ALL
                    .into_iter()
                    .map(PieceKind::Home)
                    .chain(Profession::ALL.into_iter().map(PieceKind::Workplace))
                    .chain(FarmForm::ALL.into_iter().map(PieceKind::Farm))
                    .chain(RoadShape::ALL.into_iter().map(PieceKind::Road))
                    .chain([PieceKind::Pen, PieceKind::Center, PieceKind::Lamp])
                {
                    let piece = village_kit::build(style, kind, seed, |_, _| Ok(())).unwrap();
                    check(
                        &format!("village/{style:?}/{kind:?}"),
                        piece
                            .template
                            .cells
                            .into_iter()
                            .map(|c| c.position[2])
                            .collect(),
                    );
                }
            }
            for kind in LandmarkKind::ALL {
                let kit = surface_landmarks::build(kind, seed, |_, _| Ok(())).unwrap();
                check(
                    &format!("landmark/{kind:?}"),
                    kit.template
                        .cells
                        .into_iter()
                        .map(|c| c.position[2])
                        .collect(),
                );
            }
            for kind in RuinKind::ALL {
                let kit = surface_ruins::build(kind, seed, |_, _| Ok(())).unwrap();
                check(
                    &format!("ruin/{kind:?}"),
                    kit.landmark
                        .template
                        .cells
                        .into_iter()
                        .map(|c| c.position[2])
                        .collect(),
                );
            }
            for style in CampStyle::ALL {
                let kit = surface_camps::build(style, seed, |_, _| Ok(())).unwrap();
                check(
                    &format!("camp/{style:?}"),
                    kit.template
                        .cells
                        .into_iter()
                        .map(|c| c.position[2])
                        .collect(),
                );
            }
        }
        for &species in ALL_SPECIES {
            for climate in [ClimateSkin::Temperate, ClimateSkin::Warm, ClimateSkin::Cold] {
                let entity = model(species, AtlasLayout::Bedrock, climate);
                let (minimum, maximum) = entity.bounds().unwrap();
                assert!(minimum[2].is_finite() && maximum[2].is_finite());
                assert!(
                    minimum[2] >= -64.0 && maximum[2] <= 64.0,
                    "{species:?} Z silhouette"
                );
            }
        }
    }

    #[test]
    fn extreme_zoom_reports_a_finite_limit_instead_of_truncating_world_edges() {
        let error = region([64.0, 64.0, 80.0], 0.7, [1600, 1000]).unwrap_err();
        assert!(matches!(
            error,
            AssetError::Limit {
                resource: "generated viewport axis",
                ..
            }
        ));
    }
}
