//! Original surface exteriors. These are authored homage, not copied native
//! structure algorithms. No basement, pyramid trap room or igloo shaft is built.
use super::surface_structures::{PlacementError, Template, TemplateCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandmarkKind {
    DesertWell,
    DesertPyramid,
    JunglePyramid,
    SwampHut,
    IglooTop,
    PillagerOutpost,
    WoodlandMansion,
}
impl LandmarkKind {
    pub const ALL: [Self; 7] = [
        Self::DesertWell,
        Self::DesertPyramid,
        Self::JunglePyramid,
        Self::SwampHut,
        Self::IglooTop,
        Self::PillagerOutpost,
        Self::WoodlandMansion,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::DesertWell => "desert_well",
            Self::DesertPyramid => "desert_pyramid",
            Self::JunglePyramid => "jungle_pyramid",
            Self::SwampHut => "swamp_hut",
            Self::IglooTop => "igloo_top",
            Self::PillagerOutpost => "pillager_outpost",
            Self::WoodlandMansion => "woodland_mansion",
        }
    }
}
pub struct Landmark<S> {
    pub template: Template<S>,
    pub bounds: [[i32; 3]; 2],
    /// Local supported floor positions. Caller must validate terrain approaches.
    pub entrances: Vec<[i32; 3]>,
    /// Candidate species/feet positions, not admitted or rendered entities.
    pub occupants: Vec<(&'static str, [i32; 3])>,
}
type Factory<'a, S> = &'a dyn Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>;
struct Writer<'a, S> {
    cells: BTreeMap<[i32; 3], Option<S>>,
    factory: Factory<'a, S>,
}
impl<S> Writer<'_, S> {
    fn block(
        &mut self,
        p: [i32; 3],
        id: &str,
        props: &[(&str, &str)],
    ) -> Result<(), PlacementError> {
        self.cells.insert(p, Some((self.factory)(id, props)?));
        Ok(())
    }
    fn air(&mut self, p: [i32; 3]) {
        self.cells.insert(p, None);
    }
    fn box_fill(&mut self, min: [i32; 3], max: [i32; 3], id: &str) -> Result<(), PlacementError> {
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    self.block([x, y, z], id, &[])?;
                }
            }
        }
        Ok(())
    }
    fn clear(&mut self, min: [i32; 3], max: [i32; 3]) {
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    self.air([x, y, z]);
                }
            }
        }
    }
    fn stairs(&mut self, p: [i32; 3], id: &str, facing: &str) -> Result<(), PlacementError> {
        self.block(
            p,
            id,
            &[
                ("facing", facing),
                ("half", "bottom"),
                ("shape", "straight"),
                ("waterlogged", "false"),
            ],
        )
    }
    fn log(&mut self, p: [i32; 3], id: &str, axis: &str) -> Result<(), PlacementError> {
        self.block(p, id, &[("axis", axis)])
    }
}
fn well<S>(w: &mut Writer<'_, S>) -> Result<Vec<[i32; 3]>, PlacementError> {
    w.box_fill([0, 0, 0], [4, 4, 0], "minecraft:sandstone")?;
    w.clear([0, 0, 1], [4, 4, 4]);
    w.block([2, 2, 1], "minecraft:water", &[("level", "0")])?;
    for [x, y] in [[1, 2], [3, 2], [2, 1], [2, 3]] {
        w.block([x, y, 1], "minecraft:water", &[("level", "0")])?;
    }
    for [x, y] in [[0, 0], [4, 0], [0, 4], [4, 4]] {
        w.box_fill([x, y, 1], [x, y, 4], "minecraft:sandstone")?;
    }
    for y in 0..5 {
        for x in 0..5 {
            w.block(
                [x, y, 5],
                "minecraft:sandstone_slab",
                &[("type", "bottom"), ("waterlogged", "false")],
            )?;
        }
    }
    // Open supported approaches; source sand/underfloor admission is external.
    Ok(vec![[2, 0, 0], [0, 2, 0], [4, 2, 0], [2, 4, 0]])
}
fn pyramid<S>(
    w: &mut Writer<'_, S>,
    jungle: bool,
    seed: u64,
) -> Result<Vec<[i32; 3]>, PlacementError> {
    let stone = if jungle {
        "minecraft:cobblestone"
    } else {
        "minecraft:sandstone"
    };
    w.box_fill([0, 0, 0], [20, 20, 0], stone)?;
    for z in 1..=9 {
        let inset = z;
        for y in inset..=20 - inset {
            for x in inset..=20 - inset {
                let edge = x == inset || x == 20 - inset || y == inset || y == 20 - inset;
                if edge {
                    let moss = jungle && ((x * 19 + y * 31 + z * 11) as u64 ^ seed) % 5 < 2;
                    w.block(
                        [x, y, z],
                        if moss {
                            "minecraft:mossy_cobblestone"
                        } else {
                            stone
                        },
                        &[],
                    )?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    // The final perimeter surrounds an air cell; cap it to close the roof.
    w.block([10, 10, 10], stone, &[])?;
    if jungle {
        for x in 8..=12 {
            for step in 0..4 {
                w.stairs([x, step, step], "minecraft:cobblestone_stairs", "south")?;
            }
        }
        w.clear([9, 3, 4], [11, 5, 6]);
        return Ok(vec![[10, 3, 3]]);
    }
    // Twin facade towers and terracotta glyphs give the original exterior its
    // distinct silhouette. No underground pressure plate or treasure chamber.
    for center in [3, 17] {
        w.box_fill(
            [center - 2, 0, 1],
            [center + 2, 4, 10],
            "minecraft:smooth_sandstone",
        )?;
        w.clear([center - 1, 1, 1], [center + 1, 3, 8]);
        w.box_fill(
            [center - 2, 0, 10],
            [center + 2, 4, 10],
            "minecraft:cut_sandstone",
        )?;
        for dz in -2i32..=2 {
            for dx in -2i32..=2 {
                if dx.abs() + dz.abs() == 2 {
                    w.block([center + dx, 0, 6 + dz], "minecraft:orange_terracotta", &[])?;
                }
            }
        }
        w.block([center, 0, 6], "minecraft:orange_terracotta", &[])?;
        w.block([center, 0, 5], "minecraft:blue_terracotta", &[])?;
    }
    w.box_fill([6, 0, 1], [14, 2, 4], "minecraft:smooth_sandstone")?;
    w.clear([9, 0, 1], [11, 5, 3]);
    Ok(vec![[10, 0, 0]])
}
fn hut<S>(w: &mut Writer<'_, S>) -> Result<Vec<[i32; 3]>, PlacementError> {
    for [x, y] in [[0, 0], [6, 0], [0, 8], [6, 8]] {
        for z in 0..=7 {
            w.log([x, y, z], "minecraft:oak_log", "y")?;
        }
    }
    w.box_fill([0, 0, 4], [6, 8, 4], "minecraft:spruce_planks")?;
    for z in 5..=7 {
        for y in 0..=8 {
            for x in 0..=6 {
                if x == 0 || x == 6 || y == 0 || y == 8 {
                    w.block([x, y, z], "minecraft:spruce_planks", &[])?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    w.clear([3, 0, 5], [3, 1, 6]);
    for x in [0, 6] {
        w.air([x, 4, 6]);
    }
    w.box_fill([-1, -1, 8], [7, 9, 8], "minecraft:spruce_planks")?;
    for y in -1..=9 {
        w.stairs([-1, y, 8], "minecraft:spruce_stairs", "east")?;
        w.stairs([7, y, 8], "minecraft:spruce_stairs", "west")?;
    }
    for x in 0..=6 {
        w.stairs([x, -1, 8], "minecraft:spruce_stairs", "south")?;
        w.stairs([x, 9, 8], "minecraft:spruce_stairs", "north")?;
    }
    w.box_fill([0, 0, 9], [6, 8, 9], "minecraft:spruce_planks")?;
    w.block([1, 6, 5], "minecraft:crafting_table", &[])?;
    w.block([5, 6, 5], "minecraft:cauldron", &[])?;
    for step in 0..4 {
        w.stairs([3, step - 4, step], "minecraft:spruce_stairs", "south")?;
        w.clear([3, step - 4, step + 1], [3, step - 4, step + 2]);
    }
    Ok(vec![[3, -4, 0], [3, 0, 4]])
}
fn igloo<S>(w: &mut Writer<'_, S>) -> Result<Vec<[i32; 3]>, PlacementError> {
    // Integer spherical shell with an open tunnel; no subfloor ladder shaft.
    for y in -4i32..=4 {
        for x in -4i32..=4 {
            if x * x + y * y <= 16 {
                w.block([x, y, 0], "minecraft:snow_block", &[])?;
            }
            for z in 1i32..=4 {
                let radius = x * x + y * y + (z - 1) * (z - 1);
                if radius > 20 {
                    continue;
                }
                if radius >= 11 {
                    w.block([x, y, z], "minecraft:snow_block", &[])?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    w.box_fill([-1, -6, 0], [1, -3, 0], "minecraft:snow_block")?;
    for y in -6..=-3 {
        w.block([-1, y, 1], "minecraft:snow_block", &[])?;
        w.block([1, y, 1], "minecraft:snow_block", &[])?;
        w.block([0, y, 3], "minecraft:snow_block", &[])?;
    }
    w.clear([0, -6, 1], [0, -2, 2]);
    w.block([-3, 0, 2], "minecraft:ice", &[])?;
    w.block(
        [2, 1, 1],
        "minecraft:furnace",
        &[("facing", "west"), ("lit", "false")],
    )?;
    for (y, part) in [(0, "foot"), (1, "head")] {
        w.block(
            [-2, y, 1],
            "minecraft:red_bed",
            &[("facing", "south"), ("occupied", "false"), ("part", part)],
        )?;
    }
    Ok(vec![[0, -6, 0]])
}
fn outpost<S>(w: &mut Writer<'_, S>) -> Result<Vec<[i32; 3]>, PlacementError> {
    w.box_fill([0, 0, 0], [8, 8, 0], "minecraft:cobblestone")?;
    for z in 1..=14 {
        for [x, y] in [[1, 1], [7, 1], [1, 7], [7, 7]] {
            w.log([x, y, z], "minecraft:dark_oak_log", "y")?;
        }
    }
    for z in [5, 10, 14] {
        w.box_fill([1, 1, z], [7, 7, z], "minecraft:dark_oak_planks")?;
    }
    for z in 1..=4 {
        for y in 1..=7 {
            for x in 1..=7 {
                if x == 1 || x == 7 || y == 1 || y == 7 {
                    w.block([x, y, z], "minecraft:cobblestone", &[])?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    for z in 6..=13 {
        if z == 10 {
            continue;
        }
        w.clear([2, 2, z], [6, 6, z]);
    }
    // Full-height ladder makes upper decks reachable. Holes are deliberate air.
    for z in 1..=13 {
        w.block(
            [2, 2, z],
            "minecraft:ladder",
            &[("facing", "east"), ("waterlogged", "false")],
        )?;
        w.block([1, 2, z], "minecraft:dark_oak_planks", &[])?;
    }
    w.box_fill([-1, -1, 10], [9, 9, 10], "minecraft:dark_oak_planks")?;
    // Reopen ladder after laying balcony, so the interior route remains intact.
    w.block(
        [2, 2, 10],
        "minecraft:ladder",
        &[("facing", "east"), ("waterlogged", "false")],
    )?;
    for y in -1..=9 {
        for x in -1..=9 {
            if x != -1 && x != 9 && y != -1 && y != 9 {
                continue;
            }
            let n = if y == 9 && x != -1 && x != 9 {
                "false"
            } else if x == -1 || x == 9 {
                if y == -1 {
                    "false"
                } else {
                    "true"
                }
            } else {
                "false"
            };
            let s = if x == -1 || x == 9 {
                if y == 9 {
                    "false"
                } else {
                    "true"
                }
            } else {
                "false"
            };
            let e = if y == -1 || y == 9 {
                if x == 9 {
                    "false"
                } else {
                    "true"
                }
            } else {
                "false"
            };
            let west = if y == -1 || y == 9 {
                if x == -1 {
                    "false"
                } else {
                    "true"
                }
            } else {
                "false"
            };
            w.block(
                [x, y, 11],
                "minecraft:dark_oak_fence",
                &[
                    ("north", n),
                    ("east", e),
                    ("south", s),
                    ("west", west),
                    ("waterlogged", "false"),
                ],
            )?;
        }
    }
    for z in 15..=18 {
        let inset = z - 15;
        w.box_fill(
            [inset, inset, z],
            [8 - inset, 8 - inset, z],
            "minecraft:dark_oak_planks",
        )?;
    }
    w.clear([4, 1, 1], [4, 2, 3]);
    // A supported porch reaches the tower opening.
    w.box_fill([3, 0, 0], [5, 1, 0], "minecraft:cobblestone")?;
    w.clear([4, 0, 1], [4, 1, 2]);
    Ok(vec![[4, 0, 0]])
}
fn mansion<S>(w: &mut Writer<'_, S>) -> Result<Vec<[i32; 3]>, PlacementError> {
    let inside = |x: i32, y: i32| {
        (0..32).contains(&x) && (0..28).contains(&y)
            || (32..44).contains(&x) && (8..28).contains(&y)
    };
    // A single rigid footprint, with an offset eastern wing. The generous air
    // reservation prevents trees from intruding into windows, porches or roof.
    for y in -5..30 {
        for x in -2..46 {
            w.block([x, y, 0], "minecraft:grass_block", &[("snowy", "false")])?;
            for z in 1..=28 {
                w.air([x, y, z]);
            }
        }
    }
    for y in 0..28 {
        for x in 0..44 {
            if !inside(x, y) {
                continue;
            }
            for z in [0, 7, 14] {
                w.block(
                    [x, y, z],
                    if z == 0 {
                        "minecraft:cobblestone"
                    } else {
                        "minecraft:dark_oak_planks"
                    },
                    &[],
                )?;
            }
            let side_x = !inside(x - 1, y) || !inside(x + 1, y);
            let side_y = !inside(x, y - 1) || !inside(x, y + 1);
            if !side_x && !side_y {
                continue;
            }
            for z in 1..=21 {
                let window = (2..=4).contains(&(z % 7))
                    && (side_x && (2..=4).contains(&(y % 6))
                        || side_y && (2..=4).contains(&(x % 6)));
                let post = side_x && y % 6 == 0 || side_y && x % 6 == 0;
                let id = if z % 7 == 0 {
                    "minecraft:cobblestone"
                } else if window && !post {
                    "minecraft:glass"
                } else if post {
                    "minecraft:dark_oak_log"
                } else {
                    "minecraft:dark_oak_planks"
                };
                let props: &[(&str, &str)] = if id.ends_with("_log") {
                    &[("axis", "y")]
                } else {
                    &[]
                };
                w.block([x, y, z], id, props)?;
            }
        }
    }
    // Main roof and the perpendicular wing roof are complete sloping shells.
    // Gable faces and the higher wing's join close the otherwise open attic.
    for x in 0..32 {
        let height = 22 + x.min(31 - x) / 3;
        for y in 0..28 {
            w.block([x, y, height], "minecraft:dark_oak_planks", &[])?;
        }
        for y in [0, 27] {
            for z in 22..height {
                w.block([x, y, z], "minecraft:dark_oak_planks", &[])?;
            }
        }
    }
    for y in 8..28 {
        let height = 22 + (y - 8).min(27 - y) / 2;
        for x in 32..44 {
            w.block([x, y, height], "minecraft:dark_oak_planks", &[])?;
        }
        for x in [32, 43] {
            for z in 22..height {
                w.block([x, y, z], "minecraft:dark_oak_planks", &[])?;
            }
        }
    }
    // Broad stone porch and paired doors are connected to the same ground
    // plane. No basement or scattered source-room fragments are synthesized.
    w.box_fill([12, -4, 0], [19, -1, 0], "minecraft:cobblestone")?;
    for (x, hinge) in [(15, "left"), (16, "right")] {
        w.clear([x, 0, 1], [x, 1, 4]);
        for (z, half) in [(1, "lower"), (2, "upper")] {
            w.block(
                [x, 0, z],
                "minecraft:dark_oak_door",
                &[
                    ("facing", "north"),
                    ("half", half),
                    ("hinge", hinge),
                    ("open", "false"),
                    ("powered", "false"),
                ],
            )?;
        }
    }
    Ok(vec![[15, -1, 0], [16, -1, 0]])
}

pub fn build<S>(
    kind: LandmarkKind,
    seed: u64,
    state: impl Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>,
) -> Result<Landmark<S>, PlacementError> {
    let mut writer = Writer {
        cells: BTreeMap::new(),
        factory: &state,
    };
    let entrances = match kind {
        LandmarkKind::DesertWell => well(&mut writer)?,
        LandmarkKind::DesertPyramid => pyramid(&mut writer, false, seed)?,
        LandmarkKind::JunglePyramid => pyramid(&mut writer, true, seed)?,
        LandmarkKind::SwampHut => hut(&mut writer)?,
        LandmarkKind::IglooTop => igloo(&mut writer)?,
        LandmarkKind::PillagerOutpost => outpost(&mut writer)?,
        LandmarkKind::WoodlandMansion => mansion(&mut writer)?,
    };
    let occupants = match kind {
        LandmarkKind::SwampHut => {
            vec![("minecraft:witch", [3, 5, 5]), ("minecraft:cat", [4, 5, 5])]
        }
        LandmarkKind::PillagerOutpost => vec![("minecraft:pillager", [5, 5, 11])],
        // Original static display positions, not native room-marker/spawn
        // translation: porch guards and a mage beside an upper-storey window.
        LandmarkKind::WoodlandMansion => vec![
            ("minecraft:vindicator", [13, -2, 1]),
            ("minecraft:vindicator", [18, -2, 1]),
            ("minecraft:evoker", [3, 2, 15]),
        ],
        _ => vec![],
    };
    let mut minimum = [i32::MAX; 3];
    let mut maximum = [i32::MIN; 3];
    for p in writer.cells.keys() {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(p[axis]);
            maximum[axis] = maximum[axis].max(p[axis] + 1);
        }
    }
    Ok(Landmark {
        template: Template {
            source: format!("original:surface_landmark/{}/seed/{seed}", kind.name()),
            cells: writer
                .cells
                .into_iter()
                .map(|(position, state)| TemplateCell { position, state })
                .collect(),
        },
        bounds: [minimum, maximum],
        entrances,
        occupants,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    type State = (String, BTreeMap<String, String>);
    fn state(id: &str, props: &[(&str, &str)]) -> Result<State, PlacementError> {
        Ok((
            id.into(),
            props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }
    #[test]
    fn mansion_is_one_complete_three_storey_winged_building_with_closed_roofs() {
        let kit = build(LandmarkKind::WoodlandMansion, 17, state).unwrap();
        let cells: BTreeMap<_, _> = kit
            .template
            .cells
            .iter()
            .map(|c| (c.position, &c.state))
            .collect();
        assert!(cells.len() < super::super::surface_structures::MAX_CELLS);
        for z in [0, 7, 14] {
            assert!(cells[&[20, 12, z]].is_some());
            assert!(cells[&[38, 18, z]].is_some());
        }
        assert!(cells[&[15, 12, 27]].is_some());
        assert!(cells[&[38, 17, 26]].is_some());
        for z in [3, 10, 17] {
            assert_eq!(cells[&[3, 0, z]].as_ref().unwrap().0, "minecraft:glass");
            assert_eq!(cells[&[0, 3, z]].as_ref().unwrap().0, "minecraft:glass");
        }
        for z in [3, 10, 17] {
            assert!(cells
                .values()
                .filter_map(|v| v.as_ref())
                .any(|s| s.0 == "minecraft:glass"));
            assert!(cells[&[20, 12, z]].is_none());
        }
        assert_eq!(kit.entrances, vec![[15, -1, 0], [16, -1, 0]]);
    }
    #[test]
    fn mansion_occupants_have_supported_clear_aboveground_positions() {
        let kit = build(LandmarkKind::WoodlandMansion, 17, state).unwrap();
        assert_eq!(kit.occupants.len(), 3);
        assert!(kit
            .occupants
            .iter()
            .any(|(id, _)| *id == "minecraft:vindicator"));
        assert!(kit
            .occupants
            .iter()
            .any(|(id, _)| *id == "minecraft:evoker"));
        let cells: BTreeMap<_, _> = kit
            .template
            .cells
            .iter()
            .map(|cell| (cell.position, &cell.state))
            .collect();
        for (_, [x, y, z]) in &kit.occupants {
            assert!(*z > 0);
            assert!(cells[&[*x, *y, *z - 1]].is_some());
            for height in [*z, *z + 1] {
                assert!(cells[&[*x, *y, height]].is_none());
            }
        }
    }
    #[test]
    fn every_exterior_has_unique_cells_supported_access_and_no_buried_room() {
        for kind in LandmarkKind::ALL {
            let kit = build(kind, 47, state).unwrap();
            let cells: BTreeMap<_, _> = kit
                .template
                .cells
                .iter()
                .map(|c| (c.position, &c.state))
                .collect();
            assert_eq!(cells.len(), kit.template.cells.len());
            assert!(
                kit.template.cells.len()
                    < if kind == LandmarkKind::WoodlandMansion {
                        super::super::surface_structures::MAX_CELLS
                    } else {
                        10_000
                    }
            );
            assert!(cells.keys().all(|p| p[2] >= 0));
            for p in kit.entrances {
                assert!(
                    cells.get(&p).is_some_and(|s| s.is_some()),
                    "{kind:?} missing floor"
                );
                for dz in [1, 2] {
                    assert!(
                        cells
                            .get(&[p[0], p[1], p[2] + dz])
                            .is_some_and(|s| s.is_none()),
                        "{kind:?} blocked access"
                    );
                }
            }
        }
    }
    #[test]
    fn well_has_covered_water_and_pyramid_has_two_patterned_towers() {
        let well = build(LandmarkKind::DesertWell, 0, state).unwrap();
        assert!(well
            .template
            .cells
            .iter()
            .any(|c| c.state.as_ref().is_some_and(|s| s.0 == "minecraft:water")));
        assert!(well
            .template
            .cells
            .iter()
            .any(|c| c.position == [2, 2, 5] && c.state.is_some()));
        let pyramid = build(LandmarkKind::DesertPyramid, 0, state).unwrap();
        for x in [3, 17] {
            assert!(pyramid.template.cells.iter().any(|c| {
                c.position == [x, 0, 6]
                    && c.state
                        .as_ref()
                        .is_some_and(|s| s.0 == "minecraft:orange_terracotta")
            }));
        }
        assert!(pyramid.template.cells.iter().any(|c| {
            c.state
                .as_ref()
                .is_some_and(|s| s.0 == "minecraft:blue_terracotta")
        }));
    }
    #[test]
    fn both_pyramid_shells_have_closed_peaks() {
        for kind in [LandmarkKind::DesertPyramid, LandmarkKind::JunglePyramid] {
            let kit = build(kind, 0, state).unwrap();
            assert!(kit
                .template
                .cells
                .iter()
                .any(|c| c.position == [10, 10, 10] && c.state.is_some()));
        }
    }
    #[test]
    fn stilt_hut_has_full_stilts_and_continuous_external_steps() {
        let hut = build(LandmarkKind::SwampHut, 0, state).unwrap();
        let cells: BTreeMap<_, _> = hut
            .template
            .cells
            .iter()
            .map(|c| (c.position, &c.state))
            .collect();
        for corner in [[0, 0], [6, 0], [0, 8], [6, 8]] {
            for z in 0..4 {
                assert!(cells[&[corner[0], corner[1], z]]
                    .as_ref()
                    .unwrap()
                    .0
                    .ends_with("_log"));
            }
        }
        for step in 0..4 {
            assert!(cells[&[3, step - 4, step]]
                .as_ref()
                .unwrap()
                .0
                .ends_with("_stairs"));
        }
    }
    #[test]
    fn factory_failure_rejects_whole_kit_and_identical_seed_is_repeatable() {
        for kind in LandmarkKind::ALL {
            assert!(build::<State>(kind, 0, |_, _| Err(PlacementError::InvalidState)).is_err());
            let a = build(kind, 11, state).unwrap();
            let b = build(kind, 11, state).unwrap();
            let a: Vec<_> = a
                .template
                .cells
                .into_iter()
                .map(|c| (c.position, c.state))
                .collect();
            let b: Vec<_> = b
                .template
                .cells
                .into_iter()
                .map(|c| (c.position, c.state))
                .collect();
            assert_eq!(a, b);
        }
    }
}
