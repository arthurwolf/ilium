//! Original silhouette recipes using factual species and size prescriptions.
use super::tree_geometry::{GeometryError, TreeCell, TreeGeometry};
use super::tree_profiles::{TreeProfile, TreeResource, TreeShape, TreeSize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Growth {
    Young,
    Mature,
    Old,
}

fn crown(
    geometry: &mut TreeGeometry,
    center: [i16; 3],
    mut radii: [i16; 3],
) -> Result<(), GeometryError> {
    if center[2] < 0 {
        return Err(GeometryError::InvalidCanopy);
    }
    // Young forms shorten trunks. Compress their vertical crowns to retain
    // above-ground foliage; the general geometry kernel still permits roots
    // and other intentional below-anchor geometry.
    radii[2] = radii[2].min(center[2]);
    geometry.canopy(center, radii)
}

fn flat_canopy(
    geometry: &mut TreeGeometry,
    center: [i16; 3],
    radius: i16,
    layers: i16,
) -> Result<(), GeometryError> {
    for layer in 0..layers {
        let layer_radius = (radius - layer).max(1);
        crown(
            geometry,
            [center[0], center[1], center[2] + layer],
            [layer_radius, layer_radius, 0],
        )?;
    }
    Ok(())
}

/// Texture-bank-independent semantic state ready for the shared model resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeVoxelState {
    pub position: [i16; 3],
    pub resource_id: &'static str,
    pub properties: Vec<(&'static str, &'static str)>,
}

/// Bind actual species/providers to the original geometry. Weighted leaves use
/// local positions and anchor entropy, never viewport/traversal/frame order.
pub fn bind_states(
    profile: &TreeProfile,
    geometry: &TreeGeometry,
    entropy: u64,
) -> Result<Vec<TreeVoxelState>, GeometryError> {
    let cells: BTreeMap<_, _> = geometry.cells().collect();
    let root = TreeResource {
        id: "minecraft:mangrove_roots",
        properties: &[("waterlogged", "false")],
        weight: 1,
    };
    let total_weight: u64 = profile
        .crowns
        .iter()
        .map(|resource| u64::from(resource.weight))
        .sum();
    let mut states = Vec::with_capacity(cells.len());
    for (position, cell) in &cells {
        let resource = match cell {
            TreeCell::Log(_) => profile.stem,
            TreeCell::Root if profile.shape == TreeShape::Mangrove => root,
            TreeCell::Root => return Err(GeometryError::InvalidCanopy),
            TreeCell::Leaf => {
                if total_weight == 0 {
                    return Err(GeometryError::InvalidCanopy);
                }
                let mut hash = entropy;
                for coordinate in position {
                    hash ^= (i64::from(*coordinate) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
                    hash = hash.rotate_left(21).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                }
                let mut choice = hash % total_weight;
                let mut selected = None;
                for resource in profile.crowns {
                    if choice < u64::from(resource.weight) {
                        selected = Some(*resource);
                        break;
                    }
                    choice -= u64::from(resource.weight);
                }
                selected.ok_or(GeometryError::InvalidCanopy)?
            }
        };
        let mut properties: BTreeMap<_, _> = resource.properties.iter().copied().collect();
        if let TreeCell::Log(axis) = cell {
            if properties.contains_key("axis") {
                properties.insert("axis", axis.java_value());
            }
        }
        // Giant-fungus outer skin follows actual exposed faces of this homage
        // cap/stem, rather than painting source cap skin onto internal faces.
        if matches!(
            profile.shape,
            TreeShape::RedMushroom | TreeShape::BrownMushroom
        ) {
            for (key, axis, delta) in [
                ("east", 0, 1),
                ("west", 0, -1),
                ("south", 1, 1),
                ("north", 1, -1),
                ("up", 2, 1),
                ("down", 2, -1),
            ] {
                let mut neighbor = *position;
                neighbor[axis] += delta;
                let same = cells.get(&neighbor).is_some_and(|other| {
                    matches!(
                        (cell, other),
                        (TreeCell::Leaf, TreeCell::Leaf) | (TreeCell::Log(_), TreeCell::Log(_))
                    )
                });
                properties.insert(key, if same { "false" } else { "true" });
            }
        }
        states.push(TreeVoxelState {
            position: *position,
            resource_id: resource.id,
            properties: properties.into_iter().collect(),
        });
    }
    Ok(states)
}

pub fn build(
    profile: &TreeProfile,
    growth: Growth,
    entropy: u64,
) -> Result<TreeGeometry, GeometryError> {
    let sample = |minimum: u8, maximum: u8, value: u64| -> Result<i16, GeometryError> {
        if minimum > maximum || maximum > 40 {
            return Err(GeometryError::CoordinateLimit);
        }
        Ok(i16::from(minimum) + (value % (u64::from(maximum - minimum) + 1)) as i16)
    };
    let height = match profile.size {
        TreeSize::SourceTrunk {
            base,
            random_a,
            random_b,
        } => {
            i16::from(base)
                + sample(0, random_a, entropy)?
                + sample(0, random_b, entropy.rotate_right(31))?
        }
        TreeSize::SourceFallenLength { minimum, maximum }
        | TreeSize::AuthoredMushroomHeight { minimum, maximum } => {
            sample(minimum, maximum, entropy)?
        }
    };
    if !(1..=40).contains(&height) {
        return Err(GeometryError::CoordinateLimit);
    }
    // These visual growth forms are authored. They do not claim a game growth
    // simulation or override the profile's factual mature-height distribution.
    let height = match growth {
        Growth::Young => (height / 2).max(1),
        Growth::Mature => height,
        Growth::Old => (height + 3).min(40),
    };
    let spread = if growth == Growth::Young { 2 } else { 3 };
    let acacia_variant = (entropy >> 40) % 3;
    let mut geometry = TreeGeometry::default();
    use TreeShape::*;
    if profile.shape == Fallen {
        geometry.branch([0; 3], [0, 0, 1])?;
        // An intentional fallen-log gap follows the stump. Caller verifies
        // support beneath the whole horizontal log before publishing placement.
        geometry.branch([2, 0, 0], [height + 1, 0, 0])?;
        return Ok(geometry.rotated((entropy >> 24) as u8));
    }
    let width = if matches!(profile.shape, Dense | GiantJungle | GiantSpruce | GiantPine)
        && growth != Growth::Young
    {
        2
    } else {
        1
    };
    let base = if profile.shape == Mangrove { 3 } else { 0 };
    if profile.shape == Forked {
        let bend = (height * 2 / 3).max(1);
        geometry.branch([0, 0, base], [0, 0, bend])?;
        match acacia_variant {
            1 => {
                geometry.branch([0, 0, bend], [2, 0, height + base])?;
                geometry.branch([0, 0, bend], [-2, 0, (height - 1).max(1)])?;
            }
            2 => {
                geometry.branch([0, 0, bend], [1, 0, height + base - 1])?;
                geometry.branch([1, 0, height + base - 1], [2, 0, height + base + 2])?;
            }
            _ => geometry.branch([0, 0, bend], [2, 0, height + base])?,
        }
    } else {
        for x in 0..width {
            for y in 0..width {
                geometry.branch([x, y, base], [x, y, height + base])?;
            }
        }
    }
    match profile.shape {
        Rounded => crown(&mut geometry, [0, 0, height], [spread, spread, 2])?,
        Bush => crown(&mut geometry, [0, 0, height], [2, 2, 1])?,
        Branched | Cherry | Dense | GiantJungle => {
            let reach = if profile.shape == Cherry {
                spread + 1
            } else {
                spread
            };
            for [x, y, dz] in [
                [reach, 1, 0],
                [-reach, 0, -1],
                [0, reach, 1],
                [1, -reach, 0],
            ] {
                let end = [x, y, (height + dz).max(1)];
                geometry.branch([0, 0, (height * 2 / 3).max(1)], end)?;
                crown(&mut geometry, end, [spread, spread, 2])?;
            }
            if profile.shape == GiantJungle {
                for level in [height / 3, height * 2 / 3] {
                    geometry.branch([0, 0, level], [4, -2, level + 1])?;
                    crown(&mut geometry, [4, -2, level + 1], [3, 2, 1])?;
                }
            }
            crown(&mut geometry, [0, 0, height + 1], [spread, spread, 2])?;
        }
        Forked => match acacia_variant {
            1 => {
                flat_canopy(&mut geometry, [2, 0, height + base], 3, 3)?;
                flat_canopy(&mut geometry, [-2, 0, (height - 1).max(1)], 2, 2)?;
            }
            2 => {
                flat_canopy(&mut geometry, [1, 0, height + base - 1], 2, 1)?;
                flat_canopy(&mut geometry, [2, 0, height + base + 2], 2, 2)?;
            }
            _ => flat_canopy(&mut geometry, [2, 0, height + base], 3, 3)?,
        },
        Spruce | Pine | GiantSpruce | GiantPine => {
            let first_layer = match profile.shape {
                Pine => (height - 3).max(1),
                GiantPine => (height - 6).max(1),
                GiantSpruce => (height / 3).max(1),
                _ => (height / 4).max(1),
            };
            let maximum = if width == 2 { 5 } else { 3 };
            for level in first_layer..=height + 1 {
                // Wrap the upper bark in needles before the final leaf tip.
                // Giant forms cover all four trunk columns, not only [0,0].
                let radius = ((height + 2 - level) / 2).min(maximum);
                for x in 0..width {
                    for y in 0..width {
                        crown(&mut geometry, [x, y, level], [radius, radius, 0])?;
                    }
                }
            }
        }
        Mangrove => {
            for end in [[4, 0, 0], [-4, 1, 0], [0, 4, 0], [1, -4, 0]] {
                geometry.root_branch([0, 0, base], end)?;
            }
            for end in [[3, 1, height + base], [-2, 2, height + base - 1]] {
                geometry.branch([0, 0, base + height / 2], end)?;
                crown(&mut geometry, end, [3, 3, 2])?;
            }
            crown(&mut geometry, [0, 0, height + base], [3, 3, 2])?;
        }
        Poplar => {
            for [x, y] in [[3, 0], [-2, 1], [0, -3]] {
                geometry.branch([0, 0, height / 2], [x, y, height - 1])?;
            }
            crown(&mut geometry, [0, 0, height], [spread + 1, spread, 4])?;
        }
        Azalea => {
            geometry.branch([0, 0, height / 2], [2, 1, height])?;
            crown(&mut geometry, [2, 1, height], [3, 3, 2])?;
            crown(&mut geometry, [-1, 0, height - 1], [2, 2, 1])?;
        }
        RedMushroom => crown(&mut geometry, [0, 0, height], [3, 3, 2])?,
        BrownMushroom => crown(&mut geometry, [0, 0, height], [4, 4, 0])?,
        Fallen => return Err(GeometryError::InvalidCanopy),
    }
    Ok(geometry.rotated((entropy >> 24) as u8))
}

#[cfg(test)]
mod tests {
    use super::super::tree_geometry::{LogAxis, TreeCell};
    use super::super::tree_profiles::{profile, TREE_PROFILES};
    use super::*;

    #[test]
    fn every_inventory_configuration_builds_bounded_deterministic_geometry() {
        for recipe in TREE_PROFILES {
            for entropy in [0, 1, 97, u64::MAX] {
                for growth in [Growth::Young, Growth::Mature, Growth::Old] {
                    let shape = build(&recipe, growth, entropy).unwrap();
                    assert!(shape.cells().count() > 0, "{}", recipe.id);
                    assert!(shape.cells().count() <= TreeGeometry::CELL_LIMIT);
                    assert_eq!(shape, build(&recipe, growth, entropy).unwrap());
                }
            }
        }
    }

    #[test]
    fn fallen_tree_has_upright_stump_and_horizontal_separate_log() {
        let shape = build(
            profile("minecraft:fallen_oak_tree").unwrap(),
            Growth::Mature,
            0,
        )
        .unwrap();
        assert!(shape
            .cells()
            .any(|(_, cell)| cell == TreeCell::Log(LogAxis::Vertical)));
        assert!(shape
            .cells()
            .any(|(_, cell)| cell == TreeCell::Log(LogAxis::X)));
        assert!(!shape.cells().any(|(_, cell)| cell == TreeCell::Leaf));
    }

    #[test]
    fn giant_trees_keep_their_two_by_two_trunk() {
        for id in [
            "minecraft:mega_pine",
            "minecraft:mega_spruce",
            "minecraft:mega_jungle_tree",
            "minecraft:dark_oak_leaf_litter",
        ] {
            let shape = build(profile(id).unwrap(), Growth::Mature, 0).unwrap();
            let positions: std::collections::BTreeMap<_, _> = shape.cells().collect();
            for x in 0..2 {
                for y in 0..2 {
                    assert!(
                        matches!(positions.get(&[x, y, 2]), Some(TreeCell::Log(_))),
                        "{id}"
                    );
                }
            }
        }
    }

    #[test]
    fn mushroom_caps_and_conifers_have_different_profiles() {
        let red = build(
            profile("minecraft:huge_red_mushroom").unwrap(),
            Growth::Mature,
            0,
        )
        .unwrap();
        let brown = build(
            profile("minecraft:huge_brown_mushroom").unwrap(),
            Growth::Mature,
            0,
        )
        .unwrap();
        assert_ne!(red, brown);
        let pine = build(profile("minecraft:mega_pine").unwrap(), Growth::Mature, 0).unwrap();
        let spruce = build(profile("minecraft:mega_spruce").unwrap(), Growth::Mature, 0).unwrap();
        let low_leaf = |geometry: &TreeGeometry| {
            geometry
                .cells()
                .filter(|(_, cell)| *cell == TreeCell::Leaf)
                .map(|(p, _)| p[2])
                .min()
                .unwrap()
        };
        assert!(low_leaf(&spruce) < low_leaf(&pine));
    }

    #[test]
    fn oak_and_birch_share_the_common_form_while_acacia_keeps_its_diagonal_trunk() {
        let normalized_leaves = |id: &str| {
            let profile = profile(id).unwrap();
            let geometry = build(profile, Growth::Mature, 97).unwrap();
            let cells: BTreeMap<_, _> = geometry.cells().collect();
            let trunk_top = cells
                .iter()
                .filter(|(_, cell)| matches!(cell, TreeCell::Log(_)))
                .map(|(position, _)| position[2])
                .max()
                .unwrap();
            let leaves = cells
                .iter()
                .filter(|(_, cell)| **cell == TreeCell::Leaf)
                .map(|(position, _)| [position[0], position[1], position[2] - trunk_top])
                .collect::<std::collections::BTreeSet<_>>();
            let crown_height = leaves.iter().map(|position| position[2]).max().unwrap()
                - leaves.iter().map(|position| position[2]).min().unwrap();
            (leaves, crown_height)
        };
        let (oak, oak_crown_height) = normalized_leaves("minecraft:oak");
        let (birch, birch_crown_height) = normalized_leaves("minecraft:birch_bees_0002");
        let acacia_forms = [0, 1_u64 << 40, 2_u64 << 40].map(|entropy| {
            build(
                profile("minecraft:acacia").unwrap(),
                Growth::Mature,
                entropy,
            )
            .unwrap()
        });

        assert_eq!(
            oak, birch,
            "ordinary birch follows the common oak canopy form"
        );
        assert_eq!(oak_crown_height, birch_crown_height);
        assert_ne!(acacia_forms[0], acacia_forms[1]);
        assert_ne!(acacia_forms[1], acacia_forms[2]);
        for acacia in acacia_forms {
            assert!(
                acacia
                    .cells()
                    .any(|(_, cell)| matches!(cell, TreeCell::Log(LogAxis::X | LogAxis::GroundY))),
                "acacia should keep its characteristic diagonal trunk"
            );
            let leaf_levels: std::collections::BTreeSet<_> = acacia
                .cells()
                .filter(|(_, cell)| *cell == TreeCell::Leaf)
                .map(|(position, _)| position[2])
                .collect();
            assert!(
                leaf_levels.len() <= 4,
                "acacia canopy should remain a small number of flat leaf layers"
            );
        }
    }

    #[test]
    fn source_species_are_not_guessed_from_configuration_names() {
        assert_eq!(
            profile("minecraft:jungle_bush").unwrap().crowns[0].id,
            "minecraft:oak_leaves"
        );
        assert_eq!(
            profile("minecraft:pine").unwrap().stem.id,
            "minecraft:spruce_log"
        );
        assert_eq!(
            profile("minecraft:azalea_tree").unwrap().stem.id,
            "minecraft:oak_log"
        );
        assert_eq!(
            profile("minecraft:azalea_tree").unwrap().crowns[1].id,
            "minecraft:flowering_azalea_leaves"
        );
        assert_eq!(
            profile("minecraft:red_poplar_leaf_litter").unwrap().crowns[0].id,
            "minecraft:red_poplar_leaves"
        );
    }

    #[test]
    fn older_growth_changes_geometry_without_changing_source_species() {
        let recipe = profile("minecraft:oak").unwrap();
        let young = build(recipe, Growth::Young, 11).unwrap();
        let old = build(recipe, Growth::Old, 11).unwrap();
        assert!(old.cells().count() > young.cells().count());
        assert!(old.cells().map(|(p, _)| p[2]).max() > young.cells().map(|(p, _)| p[2]).max());
    }

    #[test]
    fn mangrove_roots_and_fallen_end_grain_keep_actual_semantic_states() {
        let mangrove = profile("minecraft:mangrove").unwrap();
        let geometry = build(mangrove, Growth::Mature, 7).unwrap();
        let states = bind_states(mangrove, &geometry, 7).unwrap();
        assert!(states
            .iter()
            .any(|state| state.resource_id == "minecraft:mangrove_roots"));
        assert!(states
            .iter()
            .any(|state| state.resource_id == "minecraft:mangrove_log"));
        let fallen = profile("minecraft:fallen_oak_tree").unwrap();
        let states = bind_states(fallen, &build(fallen, Growth::Mature, 0).unwrap(), 0).unwrap();
        assert!(states
            .iter()
            .any(|state| state.properties.contains(&("axis", "x"))));
        assert!(states
            .iter()
            .any(|state| state.properties.contains(&("axis", "y"))));
    }

    #[test]
    fn azalea_has_both_weighted_leaf_species_and_no_invented_wood() {
        let recipe = profile("minecraft:azalea_tree").unwrap();
        let geometry = build(recipe, Growth::Mature, 8).unwrap();
        let states = bind_states(recipe, &geometry, 8).unwrap();
        let resources: std::collections::HashSet<_> =
            states.iter().map(|state| state.resource_id).collect();
        assert_eq!(
            resources,
            std::collections::HashSet::from([
                "minecraft:oak_log",
                "minecraft:azalea_leaves",
                "minecraft:flowering_azalea_leaves"
            ])
        );
        assert_eq!(states, bind_states(recipe, &geometry, 8).unwrap());
    }

    #[test]
    fn foliage_never_extends_below_the_ground_anchor_for_any_growth_form() {
        for recipe in TREE_PROFILES {
            for entropy in [0, 1, 97, u64::MAX] {
                for growth in [Growth::Young, Growth::Mature, Growth::Old] {
                    let geometry = build(&recipe, growth, entropy).unwrap();
                    assert!(
                        geometry
                            .cells()
                            .all(|(p, cell)| cell != TreeCell::Leaf || p[2] >= 0),
                        "{}/{growth:?}/{entropy}",
                        recipe.id
                    );
                }
            }
        }
    }

    #[test]
    fn conifer_crowns_cover_upper_trunk_columns_below_the_leaf_tip() {
        for id in [
            "minecraft:spruce",
            "minecraft:pine",
            "minecraft:mega_spruce",
            "minecraft:mega_pine",
        ] {
            let geometry = build(profile(id).unwrap(), Growth::Mature, 97).unwrap();
            let cells: BTreeMap<_, _> = geometry.cells().collect();
            let top = cells
                .iter()
                .filter(|(_, cell)| matches!(cell, TreeCell::Log(_)))
                .map(|(p, _)| p[2])
                .max()
                .unwrap();
            for (position, cell) in &cells {
                if position[2] == top && matches!(cell, TreeCell::Log(_)) {
                    let above = [position[0], position[1], top + 1];
                    assert_eq!(
                        cells.get(&above),
                        Some(&TreeCell::Leaf),
                        "{id}/{position:?}"
                    );
                }
            }
            assert!(
                cells
                    .iter()
                    .filter(|(p, cell)| p[2] == top && matches!(cell, TreeCell::Leaf))
                    .count()
                    >= 4,
                "{id}"
            );
        }
    }
}
