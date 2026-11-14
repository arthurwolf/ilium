//! Absolute anchor ownership keeps structures whole across moving windows.
//! Ecology chooses morphology; habitat checks keep inland cottages out of seas.
use super::catalog::{
    FeatureCategory, FeatureId, FeatureRecipe, Material, Placement, FEATURE_RECIPES,
};
use super::ecology::Ecology;
use super::noise::hash2;
use super::placement::PlacementKey;
use super::settings::VoxelLandscapeSettings;
use super::terrain::Column;
use super::world::WorldWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    pub id: FeatureId,
    pub anchor: [i32; 3],
    pub rotation: u8,
}

/// Real occupancy and footprint relief determine eligibility, independently
/// of camera/detail settings. Preferred ecology prescriptions bypass the broad
/// catalogue affinity (for example alpine meadow flowers), not habitat checks.
pub fn habitat(
    recipe: &FeatureRecipe,
    x: i32,
    y: i32,
    sample: &mut impl FnMut(i32, i32) -> (Column, Ecology),
) -> Option<i32> {
    habitat_rotated(recipe, x, y, 0, sample)
}
fn habitat_rotated(
    recipe: &FeatureRecipe,
    x: i32,
    y: i32,
    rotation: u8,
    sample: &mut impl FnMut(i32, i32) -> (Column, Ecology),
) -> Option<i32> {
    let (column, ecology) = sample(x, y);
    if column.cave.is_some() || column.ravine {
        return None;
    }
    let (minimum, maximum) = recipe.bounds();
    let corners = [
        [minimum[0], minimum[2]],
        [minimum[0], maximum[2]],
        [maximum[0], minimum[2]],
        [maximum[0], maximum[2]],
    ];
    let mut low = column.height;
    let mut high = column.height;
    let mut wet = column.water_level.is_some();
    let mut dry = column.water_level.is_none();
    for [dx, dy] in corners {
        let (dx, dy) = rotate(i32::from(dx), i32::from(dy), rotation);
        let (neighbor, _) = sample(x + dx, y + dy);
        low = low.min(neighbor.height);
        high = high.max(neighbor.height);
        wet |= neighbor.water_level.is_some();
        dry |= neighbor.water_level.is_none();
    }
    let relief = high - low;
    let valid = match recipe.placement {
        Placement::DryGround => !wet && relief <= 5,
        Placement::FlatGround => !wet && relief <= 3,
        Placement::SnowGround => {
            !wet && matches!(ecology.surface, Material::Snow | Material::Ice) && relief <= 4
        }
        Placement::WaterEdge => wet && dry && relief <= 9,
        Placement::ShallowWater => column
            .water_level
            .is_some_and(|water| water - column.height <= 12),
        Placement::Cliff => !column.river && relief >= 2,
        Placement::Slope => !wet && (1..=10).contains(&relief),
    };
    valid.then(|| {
        if recipe.placement == Placement::ShallowWater {
            i32::from(column.water_level.unwrap_or(column.height)) - 1
        } else {
            i32::from(high)
        }
    })
}

pub fn place(
    window: &mut WorldWindow,
    recipe: &FeatureRecipe,
    anchor: [i32; 3],
    rotation: u8,
    priority: u8,
    sample: &mut impl FnMut(i32, i32) -> (Column, Ecology),
) -> Instance {
    let key = PlacementKey {
        priority,
        x: anchor[0],
        y: anchor[1],
        feature: recipe.id.0,
    };
    let (minimum, maximum) = recipe.bounds();
    // Buildings receive solid, bounded foundations under the complete rotated
    // footprint. Natural trees/outcrops retain terrain instead of broad platforms.
    if matches!(
        recipe.category,
        FeatureCategory::Settlement
            | FeatureCategory::Agriculture
            | FeatureCategory::Infrastructure
    ) && recipe.placement == Placement::FlatGround
    {
        let mut foundation = Vec::new();
        for x in minimum[0]..=maximum[0] {
            for y in minimum[2]..=maximum[2] {
                let (dx, dy) = rotate(i32::from(x), i32::from(y), rotation);
                let (column, _) = sample(anchor[0] + dx, anchor[1] + dy);
                for z in i32::from(column.height)..anchor[2] {
                    foundation.push(([dx, dy, z - anchor[2]], Some(Material::Cobblestone)));
                }
            }
        }
        window.overlay.place(anchor, 0, key, foundation);
    }
    let mut blocks = Vec::new();
    recipe.visit_blocks(|x, z, y, material| {
        blocks.push(([i32::from(x), i32::from(y), i32::from(z)], material))
    });
    window.overlay.place(anchor, rotation, key, blocks);
    Instance {
        id: recipe.id,
        anchor,
        rotation,
    }
}
fn rotate(x: i32, y: i32, rotation: u8) -> (i32, i32) {
    match rotation % 4 {
        1 => (-y, x),
        2 => (-x, -y),
        3 => (y, -x),
        _ => (x, y),
    }
}

fn footprint(recipe: &FeatureRecipe, rotation: u8) -> ([i32; 2], [i32; 2]) {
    let (minimum, maximum) = recipe.bounds();
    let mut low = [i32::MAX; 2];
    let mut high = [i32::MIN; 2];
    for [x, y] in [
        [minimum[0], minimum[2]],
        [minimum[0], maximum[2]],
        [maximum[0], minimum[2]],
        [maximum[0], maximum[2]],
    ] {
        let (x, y) = rotate(i32::from(x), i32::from(y), rotation);
        low[0] = low[0].min(x);
        low[1] = low[1].min(y);
        high[0] = high[0].max(x);
        high[1] = high[1].max(y);
    }
    (low, high)
}

/// A settlement is a whole street plan, with six homes/workplaces, two fields,
/// well and square. Buildings and roads have independent deterministic owners.
pub fn village(
    window: &mut WorldWindow,
    center: [i32; 2],
    seed: u64,
    sample: &mut impl FnMut(i32, i32) -> (Column, Ecology),
) -> Vec<Instance> {
    let [x, y] = center;
    let (column, ecology) = sample(x, y);
    if column.water_level.is_some() || column.ravine || column.cave.is_some() {
        return Vec::new();
    }
    let eligible: Vec<_> = FEATURE_RECIPES
        .iter()
        .filter(|recipe| {
            recipe.category == FeatureCategory::Settlement
                && recipe.accepts_biome(ecology.affinity)
                && !matches!(recipe.id.0, 107..=108)
        })
        .collect();
    if eligible.is_empty() {
        return Vec::new();
    }
    let plots = [[-19, -17], [6, -17], [-19, 5], [6, 5], [-19, 23], [6, 23]];
    let homes: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|recipe| {
            ["house", "home", "cottage", "cabin", "igloo", "hut", "inn"]
                .iter()
                .any(|word| recipe.name.contains(word))
        })
        .collect();
    // Plot selection is stable; placement uses the complete rotated footprint.
    // Doorways face the street and even wide roofs leave its five cells clear.
    let mut buildings = Vec::new();
    for (index, [dx, dy]) in plots.into_iter().enumerate() {
        let hash = hash2(
            seed ^ 0x0076_696c_6c61_6765,
            i64::from(x + dx),
            i64::from(y + dy),
        );
        let options = if index < 2 && !homes.is_empty() {
            &homes
        } else {
            &eligible
        };
        let recipe = options[(hash as usize + index) % options.len()];
        let rotation = if dx < 0 { 1 } else { 3 };
        let (minimum, maximum) = footprint(recipe, rotation);
        let anchor_x = if dx < 0 {
            x - 6 - maximum[0]
        } else {
            x + 6 - minimum[0]
        };
        let anchor_y = y - 22 + (index / 2) as i32 * 22 - minimum[1];
        let Some(z) = habitat_rotated(recipe, anchor_x, anchor_y, rotation, sample) else {
            return Vec::new();
        };
        if (z - i32::from(column.height)).abs() > 6 {
            return Vec::new();
        }
        buildings.push((recipe, [anchor_x, anchor_y, z], rotation));
    }
    // Shared facilities occupy two further rows, also outside the central road.
    for (id, left, row) in [
        (108, false, -38),
        (107, true, -38),
        (130, true, -54),
        (131, false, -54),
    ] {
        let recipe = &FEATURE_RECIPES[id];
        let (minimum, maximum) = footprint(recipe, 0);
        let anchor_x = if left {
            x - 6 - maximum[0]
        } else {
            x + 6 - minimum[0]
        };
        let anchor_y = y + row - minimum[1];
        let Some(z) = habitat(recipe, anchor_x, anchor_y, sample) else {
            return Vec::new();
        };
        if (z - i32::from(column.height)).abs() > 6 {
            return Vec::new();
        }
        buildings.push((recipe, [anchor_x, anchor_y, z], 0));
    }
    let mut road_cells = std::collections::BTreeSet::new();
    for dy in -56..=44 {
        for dx in -2..=2 {
            road_cells.insert([x + dx, y + dy]);
        }
    }
    // Each entrance receives its own spur. Fixed cross streets miss entrances
    // when an inn or smithy has a larger footprint than a cottage.
    for (recipe, anchor, rotation) in &buildings {
        let (minimum, maximum) = footprint(recipe, *rotation);
        let left = anchor[0] + maximum[0] < x;
        let edge_x = anchor[0] + if left { maximum[0] + 1 } else { minimum[0] - 1 };
        let mut entrance_y = anchor[1] + (minimum[1] + maximum[1]) / 2;
        let mut entrance_distance = i32::MAX;
        recipe.visit_blocks(|local_x, height, local_y, material| {
            if height != 1 || material.is_some() {
                return;
            }
            let (dx, dy) = rotate(i32::from(local_x), i32::from(local_y), *rotation);
            let distance = (anchor[0] + dx - x).abs();
            if distance < entrance_distance {
                entrance_distance = distance;
                entrance_y = anchor[1] + dy;
            }
        });
        for road_x in x.min(edge_x)..=x.max(edge_x) {
            road_cells.insert([road_x, entrance_y]);
        }
    }
    let mut road = Vec::new();
    for [road_x, road_y] in road_cells {
        let (ground, _) = sample(road_x, road_y);
        if ground.water_level.is_some() || ground.cave.is_some() || ground.ravine {
            return Vec::new();
        }
        road.push((
            [
                road_x - x,
                road_y - y,
                i32::from(ground.height) - i32::from(column.height),
            ],
            Some(Material::Path),
        ));
    }
    window.overlay.place(
        [x, y, i32::from(column.height)],
        0,
        PlacementKey {
            priority: 20,
            x,
            y,
            feature: 107,
        },
        road,
    );
    let mut instances = Vec::new();
    for (recipe, anchor, rotation) in buildings {
        instances.push(place(window, recipe, anchor, rotation, 30, sample));
    }
    instances
}

pub fn populate(
    window: &mut WorldWindow,
    minimum: [i32; 2],
    maximum: [i32; 2],
    settings: &VoxelLandscapeSettings,
    sample: &mut impl FnMut(i32, i32) -> (Column, Ecology),
    is_cancelled: impl Fn() -> bool,
) -> Vec<Instance> {
    let seed = u64::from(settings.seed);
    let mut instances = Vec::new();
    if settings.detail == 0 {
        return instances;
    }
    // Reserve whole settlement clearings before vegetation. This decision uses
    // absolute town owners, including a halo, so moving windows agree exactly.
    let mut clearings = Vec::new();
    if settings.structures_percent > 0 {
        for cy in (minimum[1] - 96).div_euclid(128)..=(maximum[1] + 96).div_euclid(128) {
            if is_cancelled() {
                return instances;
            }
            for cx in (minimum[0] - 96).div_euclid(128)..=(maximum[0] + 96).div_euclid(128) {
                let hash = hash2(seed ^ 0x746f_776e, i64::from(cx), i64::from(cy));
                if (hash % 300) as i32 >= settings.structures_percent {
                    continue;
                }
                let center = [
                    cx * 128 + 32 + ((hash >> 12) % 64) as i32,
                    cy * 128 + 48 + ((hash >> 24) % 40) as i32,
                ];
                let town = village(window, center, seed, sample);
                if !town.is_empty() {
                    clearings.push((
                        [center[0] - 42, center[1] - 72],
                        [center[0] + 42, center[1] + 62],
                    ));
                    instances.extend(town);
                }
            }
        }
    }
    for (spacing, is_vegetation) in [(6, true), (20, false)] {
        if is_vegetation && (settings.detail < 2 || settings.vegetation_percent == 0) {
            continue;
        }
        if !is_vegetation && settings.structures_percent == 0 {
            continue;
        }
        for cy in (minimum[1] - 64).div_euclid(spacing)..=(maximum[1] + 64).div_euclid(spacing) {
            if is_cancelled() {
                return instances;
            }
            for cx in (minimum[0] - 64).div_euclid(spacing)..=(maximum[0] + 64).div_euclid(spacing)
            {
                let hash = hash2(
                    seed ^ if is_vegetation {
                        0x7472_6565
                    } else {
                        0x6665_6174
                    },
                    i64::from(cx),
                    i64::from(cy),
                );
                let x = cx * spacing + (hash % spacing as u64) as i32;
                let y = cy * spacing + ((hash >> 16) % spacing as u64) as i32;
                if clearings
                    .iter()
                    .any(|(low, high)| x >= low[0] && x <= high[0] && y >= low[1] && y <= high[1])
                {
                    continue;
                }
                let (_, ecology) = sample(x, y);
                let density = if is_vegetation {
                    settings.vegetation_percent * i32::from(ecology.vegetation_density) / 100
                } else {
                    settings.structures_percent / 3
                };
                // Higher detail adds anchors instead of moving existing ones.
                let density = if settings.detail >= 3 {
                    density * 3 / 2
                } else {
                    density
                };
                if ((hash >> 32) % 100) as i32 >= density {
                    continue;
                }
                let mut candidates = Vec::new();
                if is_vegetation && hash & 1 == 0 {
                    for id in ecology.preferred_features {
                        if let Some(recipe) = FEATURE_RECIPES.get(usize::from(id.0)) {
                            candidates.push(recipe);
                        }
                    }
                } else {
                    candidates.extend(FEATURE_RECIPES.iter().filter(|recipe| {
                        recipe.accepts_biome(ecology.affinity)
                            && if is_vegetation {
                                matches!(
                                    recipe.category,
                                    FeatureCategory::Trees | FeatureCategory::Vegetation
                                )
                            } else {
                                !matches!(
                                    recipe.category,
                                    FeatureCategory::Trees
                                        | FeatureCategory::Vegetation
                                        | FeatureCategory::Settlement
                                ) || (recipe.category == FeatureCategory::Settlement
                                    && recipe.placement == Placement::WaterEdge)
                            }
                    }));
                }
                if candidates.is_empty() {
                    continue;
                }
                let recipe = candidates[((hash >> 40) as usize) % candidates.len()];
                let rotation = (hash >> 60) as u8;
                let Some(z) = habitat_rotated(recipe, x, y, rotation, sample) else {
                    continue;
                };
                instances.push(place(
                    window,
                    recipe,
                    [x, y, z],
                    rotation,
                    if is_vegetation { 5 } else { 10 },
                    sample,
                ));
            }
        }
    }
    instances
}
