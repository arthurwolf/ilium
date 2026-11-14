//! Original surface ruin geometry and explicit exposure filtering. Native
//! burial/selection algorithms are not claimed. No buried room or active portal.
use super::surface_landmarks::Landmark;
use super::surface_structures::{PlacementError, Prepared, Template, TemplateCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalStyle {
    Standard,
    Mountain,
    Desert,
    Jungle,
    Swamp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuinKind {
    Portal(PortalStyle),
    TrailTop,
    ExposedFossil,
    MesaMineEntrance,
    BeachedShipwreck,
}
impl RuinKind {
    pub const ALL: [Self; 9] = [
        Self::Portal(PortalStyle::Standard),
        Self::Portal(PortalStyle::Mountain),
        Self::Portal(PortalStyle::Desert),
        Self::Portal(PortalStyle::Jungle),
        Self::Portal(PortalStyle::Swamp),
        Self::TrailTop,
        Self::ExposedFossil,
        Self::MesaMineEntrance,
        Self::BeachedShipwreck,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Portal(PortalStyle::Standard) => "ruined_portal",
            Self::Portal(PortalStyle::Mountain) => "ruined_portal_mountain",
            Self::Portal(PortalStyle::Desert) => "ruined_portal_desert",
            Self::Portal(PortalStyle::Jungle) => "ruined_portal_jungle",
            Self::Portal(PortalStyle::Swamp) => "ruined_portal_swamp",
            Self::TrailTop => "trail_ruins_exposed_top",
            Self::ExposedFossil => "exposed_fossil",
            Self::MesaMineEntrance => "mesa_mineshaft_exposed_entrance",
            Self::BeachedShipwreck => "shipwreck_beached",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    AboveTerrain,
    AboveTerrainAndWater,
}
#[derive(Clone, Copy, Debug)]
pub struct SurfaceSample {
    pub ground: i32,
    pub water: Option<i32>,
}
pub struct Ruin<S> {
    pub landmark: Landmark<S>,
    pub exposure: Exposure,
}
type Factory<'a, S> = &'a dyn Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>;
struct Writer<'a, S> {
    cells: BTreeMap<[i32; 3], Option<S>>,
    factory: Factory<'a, S>,
}
impl<S> Writer<'_, S> {
    fn put(&mut self, p: [i32; 3], id: &str, props: &[(&str, &str)]) -> Result<(), PlacementError> {
        self.cells.insert(p, Some((self.factory)(id, props)?));
        Ok(())
    }
    fn air(&mut self, p: [i32; 3]) {
        self.cells.insert(p, None);
    }
    fn fill(&mut self, min: [i32; 3], max: [i32; 3], id: &str) -> Result<(), PlacementError> {
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    self.put([x, y, z], id, &[])?;
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
    fn log(&mut self, p: [i32; 3], id: &str, axis: &str) -> Result<(), PlacementError> {
        self.put(p, id, &[("axis", axis)])
    }
}
fn portal<S>(w: &mut Writer<'_, S>, style: PortalStyle, seed: u64) -> Result<(), PlacementError> {
    let giant = style == PortalStyle::Mountain || seed & 1 != 0;
    let height = if giant { 11 } else { 8 };
    let right = if giant { 10 } else { 8 };
    for y in 0..=12 {
        for x in 0..=12 {
            let stone = if style == PortalStyle::Desert {
                "minecraft:sandstone"
            } else if style == PortalStyle::Jungle {
                "minecraft:mossy_stone_bricks"
            } else {
                "minecraft:stone_bricks"
            };
            w.put([x, y, 0], stone, &[])?;
            if (x * 17 + y * 7) as u64 % 11 < 3 {
                w.put([x, y, 0], "minecraft:netherrack", &[])?;
            }
            if ((x * 17 + y * 7) as u64).is_multiple_of(29) {
                w.put([x, y, 0], "minecraft:magma_block", &[])?;
            }
        }
    }
    for x in 3..=right {
        w.put([x, 6, 1], "minecraft:obsidian", &[])?;
    }
    w.clear([4, 6, 2], [right - 1, 6, height - 1]);
    for z in 2..=height {
        w.put(
            [3, 6, z],
            if z % 4 == 0 {
                "minecraft:crying_obsidian"
            } else {
                "minecraft:obsidian"
            },
            &[],
        )?;
    }
    for z in 2..height - 2 {
        w.put([right, 6, z], "minecraft:obsidian", &[])?;
    }
    for x in 3..=right - 2 {
        w.put([x, 6, height], "minecraft:obsidian", &[])?;
    }
    // A frame gap stays open: no active purple plane or Nether world content.
    w.fill([1, 10, 1], [1, 10, 3], "minecraft:chiseled_stone_bricks")?;
    w.put([1, 10, 4], "minecraft:gold_block", &[])?;
    if style == PortalStyle::Jungle {
        for z in 2..=5 {
            w.put(
                [3, 5, z],
                "minecraft:vine",
                &[
                    ("east", "false"),
                    ("north", "false"),
                    ("south", "true"),
                    ("up", "false"),
                    ("west", "false"),
                ],
            )?;
        }
        w.fill([9, 9, 1], [11, 11, 2], "minecraft:moss_block")?;
    }
    if style == PortalStyle::Swamp {
        w.put(
            [1, 9, 2],
            "minecraft:vine",
            &[
                ("east", "false"),
                ("north", "false"),
                ("south", "true"),
                ("up", "false"),
                ("west", "false"),
            ],
        )?;
    }
    Ok(())
}
fn trail<S>(w: &mut Writer<'_, S>, seed: u64) -> Result<(), PlacementError> {
    // Only this weathered outdoor tower/top is built; no buried rooms, tunnels
    // or archaeology loot is synthesized below its exposure plane.
    w.fill([0, 0, 0], [6, 6, 0], "minecraft:terracotta")?;
    w.clear([1, 1, 1], [5, 5, 5]);
    for z in 1..=5 {
        for y in 0..=6 {
            for x in 0..=6 {
                if x != 0 && x != 6 && y != 0 && y != 6 {
                    continue;
                }
                if z > 2 && ((x * 7 + y * 11 + z * 3) as u64 ^ seed) % 9 < 3 {
                    continue;
                }
                let id = if z == 2 {
                    "minecraft:orange_terracotta"
                } else if z == 4 {
                    "minecraft:white_terracotta"
                } else {
                    "minecraft:mud_bricks"
                };
                w.put([x, y, z], id, &[])?;
            }
        }
    }
    w.clear([2, 0, 1], [4, 1, 2]);
    for [x, y] in [[0, 0], [6, 0], [0, 6], [6, 6]] {
        w.put([x, y, 6], "minecraft:terracotta", &[])?;
    }
    w.fill([7, 2, 0], [9, 4, 0], "minecraft:gravel")?;
    Ok(())
}
fn fossil<S>(w: &mut Writer<'_, S>) -> Result<(), PlacementError> {
    for y in 0..=14 {
        w.log([0, y, 4], "minecraft:bone_block", "z")?;
    }
    for y in [1, 4, 7, 10, 13] {
        for side in [-1, 1] {
            for x in 1..=4 {
                let z = 5 - x;
                w.log(
                    [side * x, y, z],
                    "minecraft:bone_block",
                    if x == 1 { "x" } else { "y" },
                )?;
            }
        }
    }
    // Hollow skull and lower jaw are part of the same accepted owner.
    for z in 1..=5 {
        for y in -4..=-1 {
            for x in -3..=3 {
                if x == -3 || x == 3 || y == -4 || y == -1 || z == 1 || z == 5 {
                    w.log([x, y, z], "minecraft:bone_block", "y")?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    for x in [-2, 2] {
        w.air([x, -4, 3]);
        w.air([x, -4, 4]);
    }
    Ok(())
}
fn mine<S>(w: &mut Writer<'_, S>) -> Result<(), PlacementError> {
    w.fill([0, 0, 0], [10, 10, 0], "minecraft:stone")?;
    w.clear([2, 0, 1], [8, 10, 5]);
    for y in [1, 5, 9] {
        for x in [2, 8] {
            for z in 1..=5 {
                w.log([x, y, z], "minecraft:dark_oak_log", "y")?;
            }
        }
        for x in 2..=8 {
            w.log([x, y, 5], "minecraft:dark_oak_log", "x")?;
        }
    }
    for y in 0..=10 {
        w.put(
            [5, y, 1],
            "minecraft:rail",
            &[("shape", "north_south"), ("waterlogged", "false")],
        )?;
    }
    w.put([3, 5, 4], "minecraft:cobweb", &[])?;
    Ok(())
}
fn wreck<S>(w: &mut Writer<'_, S>, seed: u64) -> Result<(), PlacementError> {
    for y in 0..=22 {
        let half = if y < 4 {
            y.min(3)
        } else if y > 19 {
            22 - y
        } else {
            3
        };
        for x in 3 - half..=3 + half {
            w.put([x, y, 1], "minecraft:oak_planks", &[])?;
        }
        w.log([3, y, 0], "minecraft:oak_log", "z")?;
        for x in [3 - half, 3 + half] {
            for z in 2..=3 {
                if z == 3 && (x + y) as u64 % 7 == seed % 7 {
                    continue;
                }
                w.put([x, y, z], "minecraft:spruce_planks", &[])?;
            }
        }
        for x in 4 - half..3 + half {
            w.air([x, y, 2]);
            w.air([x, y, 3]);
        }
    }
    w.fill([1, 16, 3], [5, 21, 3], "minecraft:spruce_planks")?;
    for z in 4..=6 {
        for y in 16..=21 {
            for x in 1..=5 {
                if x == 1 || x == 5 || y == 16 || y == 21 {
                    w.put([x, y, z], "minecraft:oak_planks", &[])?;
                } else {
                    w.air([x, y, z]);
                }
            }
        }
    }
    w.clear([3, 16, 4], [3, 17, 5]);
    for x in [1, 5] {
        w.air([x, 18, 5]);
    }
    w.fill([1, 16, 7], [5, 21, 7], "minecraft:spruce_planks")?;
    for z in 2..=11 {
        w.log([3, 10, z], "minecraft:oak_log", "y")?;
    }
    for x in 0..=6 {
        if x == 3 {
            continue;
        }
        w.log([x, 10, 8], "minecraft:oak_log", "x")?;
    }
    for z in 5..=7 {
        for x in 0..=6 {
            if x == 3 || (x + z) as u64 % 4 == seed % 4 {
                continue;
            }
            w.put([x, 10, z], "minecraft:white_wool", &[])?;
        }
    }
    Ok(())
}
pub fn build<S>(
    kind: RuinKind,
    seed: u64,
    factory: impl Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>,
) -> Result<Ruin<S>, PlacementError> {
    let mut w = Writer {
        cells: BTreeMap::new(),
        factory: &factory,
    };
    match kind {
        RuinKind::Portal(style) => portal(&mut w, style, seed)?,
        RuinKind::TrailTop => trail(&mut w, seed)?,
        RuinKind::ExposedFossil => fossil(&mut w)?,
        RuinKind::MesaMineEntrance => mine(&mut w)?,
        RuinKind::BeachedShipwreck => wreck(&mut w, seed)?,
    };
    let mut minimum = [i32::MAX; 3];
    let mut maximum = [i32::MIN; 3];
    for p in w.cells.keys() {
        for a in 0..3 {
            minimum[a] = minimum[a].min(p[a]);
            maximum[a] = maximum[a].max(p[a] + 1);
        }
    }
    let landmark = Landmark {
        template: Template {
            source: format!("original:surface_ruin/{}/seed/{seed}", kind.name()),
            cells: w
                .cells
                .into_iter()
                .map(|(position, state)| TemplateCell { position, state })
                .collect(),
        },
        bounds: [minimum, maximum],
        entrances: vec![],
        occupants: vec![],
    };
    Ok(Ruin {
        landmark,
        // The requested surface scope excludes underwater portions for every
        // family, including beached hulls and fossils near a wet bank.
        exposure: Exposure::AboveTerrainAndWater,
    })
}

/// The caller supplies one immutable actual terrain/water snapshot in scene
/// coordinates. Unknown columns reject all output, including already inspected
/// cells. This explicit surface mask is for exposed ruins, never town clipping.
pub type ExposedCells<'a, S> = Vec<([i32; 3], Option<&'a S>)>;
pub fn exposed_cells<S>(
    prepared: &Prepared<S>,
    exposure: Exposure,
    sample: impl Fn([i32; 2]) -> Option<SurfaceSample>,
    cancelled: impl Fn() -> bool,
) -> Result<ExposedCells<'_, S>, PlacementError> {
    let mut columns = BTreeMap::new();
    let mut cells = Vec::new();
    for (position, state) in prepared.cells() {
        if cancelled() {
            return Err(PlacementError::Cancelled);
        }
        let column = [position[0], position[1]];
        let surface = if let Some(s) = columns.get(&column) {
            *s
        } else {
            let s = sample(column).ok_or(PlacementError::Unknown(position))?;
            columns.insert(column, s);
            s
        };
        if position[2] < surface.ground {
            continue;
        }
        if exposure == Exposure::AboveTerrainAndWater
            && surface.water.is_some_and(|water| position[2] <= water)
        {
            continue;
        }
        cells.push((position, state));
    }
    if cancelled() {
        return Err(PlacementError::Cancelled);
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::super::surface_structures::{Habitat, Template, TemplateCell};
    use super::*;
    use std::collections::BTreeMap;
    type State = (String, BTreeMap<String, String>);
    fn state(id: &str, p: &[(&str, &str)]) -> Result<State, PlacementError> {
        Ok((
            id.into(),
            p.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ))
    }
    #[test]
    fn all_ruins_are_whole_deterministic_templates_without_technical_blocks() {
        for kind in RuinKind::ALL {
            let a = build(kind, 47, state).unwrap();
            let b = build(kind, 47, state).unwrap();
            let ac: Vec<_> = a
                .landmark
                .template
                .cells
                .iter()
                .map(|c| (c.position, &c.state))
                .collect();
            let bc: Vec<_> = b
                .landmark
                .template
                .cells
                .iter()
                .map(|c| (c.position, &c.state))
                .collect();
            assert_eq!(ac, bc);
            assert!(!ac.is_empty());
            assert!(ac.len() < 10_000);
            assert_eq!(
                ac.iter()
                    .map(|(p, _)| p)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                ac.len()
            );
            assert!(ac.iter().all(|(_, s)| s.as_ref().is_none_or(|s| !matches!(
                s.0.as_str(),
                "minecraft:nether_portal" | "minecraft:jigsaw" | "minecraft:structure_block"
            ))));
        }
    }
    #[test]
    fn portal_styles_have_frames_and_swamp_requires_above_water() {
        for style in [
            PortalStyle::Standard,
            PortalStyle::Mountain,
            PortalStyle::Desert,
            PortalStyle::Jungle,
            PortalStyle::Swamp,
        ] {
            let p = build(RuinKind::Portal(style), 0, state).unwrap();
            assert!(p.landmark.template.cells.iter().any(|c| {
                c.state
                    .as_ref()
                    .is_some_and(|s| s.0 == "minecraft:obsidian")
            }));
            assert_eq!(p.exposure, Exposure::AboveTerrainAndWater);
        }
    }
    #[test]
    fn fossil_is_an_exposed_axial_skeleton_and_wreck_has_a_keel_and_mast() {
        let fossil = build(RuinKind::ExposedFossil, 0, state).unwrap();
        assert!(fossil
            .landmark
            .template
            .cells
            .iter()
            .filter_map(|c| c.state.as_ref())
            .all(|s| s.0 == "minecraft:bone_block" && s.1.contains_key("axis")));
        let wreck = build(RuinKind::BeachedShipwreck, 0, state).unwrap();
        assert!(wreck.landmark.bounds[1][1] - wreck.landmark.bounds[0][1] >= 20);
        assert!(wreck.landmark.bounds[1][2] - wreck.landmark.bounds[0][2] >= 10);
        assert!(wreck.landmark.template.cells.iter().any(|c| c
            .state
            .as_ref()
            .is_some_and(
                |s| s.0 == "minecraft:oak_log" && s.1.get("axis").is_some_and(|a| a == "z")
            )));
    }
    #[test]
    fn exposure_uses_actual_heights_and_unknown_or_cancellation_discards_everything() {
        let template = Template {
            source: "original:exposure_fixture".into(),
            cells: (0..5)
                .map(|z| TemplateCell {
                    position: [0, 0, z],
                    state: Some(z),
                })
                .collect(),
        };
        let p = Prepared::prepare(
            &template,
            [0; 3],
            0,
            |s, _| Ok(*s),
            |_| Habitat::Replaceable,
            || false,
        )
        .unwrap();
        let cells = exposed_cells(
            &p,
            Exposure::AboveTerrainAndWater,
            |_| {
                Some(SurfaceSample {
                    ground: 1,
                    water: Some(2),
                })
            },
            || false,
        )
        .unwrap();
        assert_eq!(cells.iter().map(|(p, _)| p[2]).collect::<Vec<_>>(), [3, 4]);
        assert!(exposed_cells(&p, Exposure::AboveTerrain, |_| None, || false).is_err());
        assert!(exposed_cells(
            &p,
            Exposure::AboveTerrain,
            |_| Some(SurfaceSample {
                ground: 0,
                water: None
            }),
            || true
        )
        .is_err());
    }
    #[test]
    fn state_factory_failure_never_returns_a_fragment() {
        for kind in RuinKind::ALL {
            assert!(build::<State>(kind, 0, |_, _| Err(PlacementError::InvalidState)).is_err());
        }
    }
    #[test]
    fn swamp_vine_has_a_solid_south_attachment() {
        let kit = build(RuinKind::Portal(PortalStyle::Swamp), 0, state).unwrap();
        let cells: BTreeMap<_, _> = kit
            .landmark
            .template
            .cells
            .into_iter()
            .map(|c| (c.position, c.state))
            .collect();
        for (p, state) in &cells {
            let Some((id, props)) = state else {
                continue;
            };
            if id != "minecraft:vine" {
                continue;
            }
            assert_eq!(props.get("south").map(String::as_str), Some("true"));
            assert!(cells
                .get(&[p[0], p[1] + 1, p[2]])
                .is_some_and(|s| s.is_some()));
        }
    }
    #[test]
    fn sail_and_yard_preserve_continuous_vertical_mast() {
        let kit = build(RuinKind::BeachedShipwreck, 0, state).unwrap();
        let cells: BTreeMap<_, _> = kit
            .landmark
            .template
            .cells
            .into_iter()
            .map(|c| (c.position, c.state))
            .collect();
        for z in 2..=11 {
            let mast = cells[&[3, 10, z]].as_ref().unwrap();
            assert_eq!(mast.0, "minecraft:oak_log");
            assert_eq!(mast.1.get("axis").map(String::as_str), Some("y"));
        }
    }
    #[test]
    fn every_family_excludes_a_fully_submerged_candidate() {
        for kind in RuinKind::ALL {
            let kit = build(kind, 0, state).unwrap();
            let prepared = Prepared::prepare(
                &kit.landmark.template,
                [0; 3],
                0,
                |s, _| Ok(s.clone()),
                |_| Habitat::Replaceable,
                || false,
            )
            .unwrap();
            let visible = exposed_cells(
                &prepared,
                kit.exposure,
                |_| {
                    Some(SurfaceSample {
                        ground: 0,
                        water: Some(16),
                    })
                },
                || false,
            )
            .unwrap();
            assert!(visible.is_empty(), "{kind:?} underwater feature");
        }
    }
}
