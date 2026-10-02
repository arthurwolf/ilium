//! Original abandoned-camp geometry for the eighteen scoped source presets.
//! Source biome/material vocabulary guides the kit; layouts are authored homage.
use super::surface_structures::{PlacementError, Template, TemplateCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampStyle {
    BambooJungle,
    BirchForest,
    CherryGrove,
    DappledForest,
    FlowerForest,
    Forest,
    Meadow,
    OldGrowthBirchForest,
    OldGrowthPineTaiga,
    OldGrowthSpruceTaiga,
    PaleGarden,
    Savanna,
    SnowyTaiga,
    SparseJungle,
    Swamp,
    Taiga,
    WindsweptForest,
    WoodedBadlands,
}
impl CampStyle {
    pub const ALL: [Self; 18] = [
        Self::BambooJungle,
        Self::BirchForest,
        Self::CherryGrove,
        Self::DappledForest,
        Self::FlowerForest,
        Self::Forest,
        Self::Meadow,
        Self::OldGrowthBirchForest,
        Self::OldGrowthPineTaiga,
        Self::OldGrowthSpruceTaiga,
        Self::PaleGarden,
        Self::Savanna,
        Self::SnowyTaiga,
        Self::SparseJungle,
        Self::Swamp,
        Self::Taiga,
        Self::WindsweptForest,
        Self::WoodedBadlands,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::BambooJungle => "bamboo_jungle",
            Self::BirchForest => "birch_forest",
            Self::CherryGrove => "cherry_grove",
            Self::DappledForest => "dappled_forest",
            Self::FlowerForest => "flower_forest",
            Self::Forest => "forest",
            Self::Meadow => "meadow",
            Self::OldGrowthBirchForest => "old_growth_birch_forest",
            Self::OldGrowthPineTaiga => "old_growth_pine_taiga",
            Self::OldGrowthSpruceTaiga => "old_growth_spruce_taiga",
            Self::PaleGarden => "pale_garden",
            Self::Savanna => "savanna",
            Self::SnowyTaiga => "snowy_taiga",
            Self::SparseJungle => "sparse_jungle",
            Self::Swamp => "swamp",
            Self::Taiga => "taiga",
            Self::WindsweptForest => "windswept_forest",
            Self::WoodedBadlands => "wooded_badlands",
        }
    }
    pub fn biome_id(self) -> String {
        format!("minecraft:{}", self.name())
    }
    pub const fn source_id(self) -> &'static str {
        match self {
            Self::BambooJungle => "minecraft:abandoned_camp_bamboo_jungle",
            Self::BirchForest => "minecraft:abandoned_camp_birch_forest",
            Self::CherryGrove => "minecraft:abandoned_camp_cherry_grove",
            Self::DappledForest => "minecraft:abandoned_camp_dappled_forest",
            Self::FlowerForest => "minecraft:abandoned_camp_flower_forest",
            Self::Forest => "minecraft:abandoned_camp_forest",
            Self::Meadow => "minecraft:abandoned_camp_meadow",
            Self::OldGrowthBirchForest => "minecraft:abandoned_camp_old_growth_birch_forest",
            Self::OldGrowthPineTaiga => "minecraft:abandoned_camp_old_growth_pine_taiga",
            Self::OldGrowthSpruceTaiga => "minecraft:abandoned_camp_old_growth_spruce_taiga",
            Self::PaleGarden => "minecraft:abandoned_camp_pale_garden",
            Self::Savanna => "minecraft:abandoned_camp_savanna",
            Self::SnowyTaiga => "minecraft:abandoned_camp_snowy_taiga",
            Self::SparseJungle => "minecraft:abandoned_camp_sparse_jungle",
            Self::Swamp => "minecraft:abandoned_camp_swamp",
            Self::Taiga => "minecraft:abandoned_camp_taiga",
            Self::WindsweptForest => "minecraft:abandoned_camp_windswept_forest",
            Self::WoodedBadlands => "minecraft:abandoned_camp_wooded_badlands",
        }
    }
    pub fn for_biome(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.biome_id() == id)
    }
    fn wood(self) -> &'static str {
        match self {
            Self::BambooJungle | Self::SparseJungle => "jungle",
            Self::BirchForest | Self::OldGrowthBirchForest | Self::FlowerForest => "birch",
            Self::CherryGrove => "cherry",
            Self::DappledForest => "poplar",
            Self::OldGrowthPineTaiga
            | Self::OldGrowthSpruceTaiga
            | Self::SnowyTaiga
            | Self::Taiga
            | Self::WindsweptForest => "spruce",
            Self::PaleGarden => "pale_oak",
            Self::Savanna => "acacia",
            // WoodedBadlands source uses oak fence but no log posts. The oak
            // structural poles here are an explicit original approximation.
            _ => "oak",
        }
    }
    fn ground_decoration(self) -> &'static str {
        match self {
            Self::BambooJungle => "minecraft:melon",
            Self::BirchForest => "minecraft:lily_of_the_valley",
            Self::CherryGrove => "minecraft:pink_petals",
            Self::DappledForest => "minecraft:leaf_litter",
            Self::FlowerForest => "minecraft:allium",
            Self::Forest => "minecraft:lily_of_the_valley",
            Self::Meadow => "minecraft:cornflower",
            Self::OldGrowthBirchForest => "minecraft:brown_mushroom",
            Self::OldGrowthPineTaiga => "minecraft:red_mushroom",
            Self::OldGrowthSpruceTaiga => "minecraft:fern",
            Self::PaleGarden => "minecraft:closed_eyeblossom",
            Self::Savanna => "minecraft:short_grass",
            Self::SnowyTaiga => "minecraft:snow",
            Self::SparseJungle => "minecraft:vine",
            Self::Swamp => "minecraft:blue_orchid",
            Self::Taiga => "minecraft:sweet_berry_bush",
            Self::WindsweptForest => "minecraft:bush",
            Self::WoodedBadlands => "minecraft:short_dry_grass",
        }
    }
}
pub struct Camp<S> {
    pub template: Template<S>,
    /// Supported floor positions leading to the tents and central fire.
    pub entrances: Vec<[i32; 3]>,
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
}

fn shelter<S>(
    w: &mut Writer<'_, S>,
    center: [i32; 2],
    variant: u8,
    wood: &str,
) -> Result<[i32; 3], PlacementError> {
    let half = 2 + i32::from(variant % 3);
    let depth = 4 + i32::from((variant / 3) % 3) * 2;
    let [cx, cy] = center;
    let start = cy - 3;
    let end = start + depth;
    let log = format!("minecraft:{wood}_log");
    // Nine A-frame footprints plus one sloping lean-to. Every shell has a
    // clear two-block entrance; cloth is original geometry, not native stairs.
    for y in start..=end {
        for x in cx - half..=cx + half {
            let z = if variant == 9 {
                2 + (x - cx + half) / 2
            } else {
                2 + half - (x - cx).abs()
            };
            w.block([x, y, z], "minecraft:white_wool", &[])?;
        }
    }
    for y in [start, end] {
        let top = 2 + half;
        for z in 1..=top {
            w.block([cx, y, z], &log, &[("axis", "y")])?;
        }
    }
    // Side/back canvas and sleeping mats retain interior air explicitly.
    for y in start..=end {
        for x in [cx - half, cx + half] {
            w.block([x, y, 1], "minecraft:white_wool", &[])?;
        }
    }
    for x in cx - half..=cx + half {
        if x != cx {
            w.block([x, end, 1], "minecraft:white_wool", &[])?;
        }
    }
    for x in [cx - 1, cx + 1] {
        w.block([x, end - 2, 1], "minecraft:yellow_wool", &[])?;
        w.block([x, end - 1, 1], "minecraft:yellow_wool", &[])?;
    }
    // Ridge support is behind the opening, leaving the approach unobstructed.
    for z in 1..=2 {
        w.air([cx, start, z]);
    }
    if variant == 9 {
        w.air([cx + half, end, 2 + half]);
        w.block([cx + half, end - 1, 1], "minecraft:cobweb", &[])?;
    }
    Ok([cx, start - 1, 0])
}

/// A complete original camp, bounded before terrain admission. Caller rotates
/// exact properties and rejects unsupported terrain as one atomic structure.
pub fn build<S>(
    style: CampStyle,
    seed: u64,
    state: impl Fn(&str, &[(&str, &str)]) -> Result<S, PlacementError>,
) -> Result<Camp<S>, PlacementError> {
    let mut w = Writer {
        cells: BTreeMap::new(),
        factory: &state,
    };
    let soil = match style {
        CampStyle::WoodedBadlands => "minecraft:red_sand",
        CampStyle::OldGrowthPineTaiga | CampStyle::OldGrowthSpruceTaiga => "minecraft:podzol",
        _ => "minecraft:grass_block",
    };
    // Reserve the whole clearing, including omitted shelter air. A later
    // region projection must never admit only the convenient visible half.
    for y in 0..28 {
        for x in 0..28 {
            w.block([x, y, 0], soil, &[])?;
            for z in 1..=7 {
                w.air([x, y, z]);
            }
        }
    }
    let variant = (seed % 10) as u8;
    let mut entrances = vec![
        shelter(&mut w, [5, 5], variant, style.wood())?,
        shelter(&mut w, [20, 5], (variant + 3) % 10, style.wood())?,
    ];
    if !seed.is_multiple_of(4) {
        entrances.push(shelter(&mut w, [5, 20], (variant + 6) % 10, style.wood())?);
    }
    let fire = match (seed / 10) % 4 {
        0 => [16, 16],
        1 => [19, 18],
        2 => [17, 21],
        _ => [22, 22],
    };
    w.block(
        [fire[0], fire[1], 1],
        "minecraft:campfire",
        &[
            ("facing", "south"),
            ("lit", "false"),
            ("signal_fire", "false"),
            ("waterlogged", "false"),
        ],
    )?;
    let log = format!("minecraft:{}_log", style.wood());
    for dx in -2..=2 {
        w.block([fire[0] + dx, fire[1] - 2, 0], "minecraft:gravel", &[])?;
        w.block([fire[0] + dx, fire[1] + 2, 0], "minecraft:gravel", &[])?;
    }
    for dx in -1..=1 {
        w.block([fire[0] + dx, fire[1] + 3, 1], &log, &[("axis", "x")])?;
    }
    w.block(
        [fire[0] + 3, fire[1], 1],
        "minecraft:barrel",
        &[("facing", "up"), ("open", "false")],
    )?;
    w.block(
        [fire[0] + 3, fire[1] + 1, 1],
        "minecraft:chest",
        &[
            ("facing", "west"),
            ("type", "single"),
            ("waterlogged", "false"),
        ],
    )?;
    // Route around the shelters, not through their rear ridge posts.
    for &[x, y, _] in &entrances {
        for px in x.min(13)..=x.max(13) {
            w.block([px, y, 0], "minecraft:dirt_path", &[])?;
        }
        for py in y.min(12)..=y.max(12) {
            w.block([13, py, 0], "minecraft:dirt_path", &[])?;
        }
    }
    for x in 13..=fire[0] {
        w.block([x, 12, 0], "minecraft:dirt_path", &[])?;
    }
    for y in 12..fire[1] {
        w.block([fire[0], y, 0], "minecraft:dirt_path", &[])?;
    }
    // Source-style clearing edges provide more than a wood recolor. These are
    // authored camp decorations, not native biome feature placement attempts.
    for y in [4, 8, 12] {
        let id = style.ground_decoration();
        let props: &[(&str, &str)] = match id {
            "minecraft:pink_petals" => &[("facing", "north"), ("flower_amount", "3")],
            "minecraft:leaf_litter" => &[("facing", "north"), ("segment_amount", "3")],
            "minecraft:sweet_berry_bush" => &[("age", "3")],
            "minecraft:snow" => &[("layers", "2")],
            _ => &[],
        };
        if id == "minecraft:vine" {
            // Vines attach to a supported authored pole, never float alone.
            for z in 1..=3 {
                w.block([27, y, z], &log, &[("axis", "y")])?;
            }
            w.block(
                [26, y, 2],
                id,
                &[
                    ("east", "true"),
                    ("west", "false"),
                    ("north", "false"),
                    ("south", "false"),
                    ("up", "false"),
                ],
            )?;
        } else {
            w.block([26, y, 1], id, props)?;
        }
    }
    entrances.push([fire[0], fire[1] - 1, 0]);
    Ok(Camp {
        template: Template {
            source: format!("original:abandoned_camp/{}/seed/{seed}", style.name()),
            cells: w
                .cells
                .into_iter()
                .map(|(position, state)| TemplateCell { position, state })
                .collect(),
        },
        entrances,
    })
}

#[cfg(test)]
fn shelter_test_cells(variant: u8) -> Result<Vec<[i32; 3]>, PlacementError> {
    let state = |id: &str, _: &[(&str, &str)]| Ok(id.to_string());
    let mut w = Writer {
        cells: BTreeMap::new(),
        factory: &state,
    };
    shelter(&mut w, [5, 5], variant, "oak")?;
    Ok(w.cells
        .into_iter()
        .filter_map(|(p, s)| s.map(|_| p))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(id: &str, _: &[(&str, &str)]) -> Result<String, PlacementError> {
        Ok(id.into())
    }
    #[test]
    fn every_source_style_has_supported_open_access_and_an_unlit_fire() {
        assert_eq!(CampStyle::ALL.len(), 18);
        for style in CampStyle::ALL {
            let camp = build(style, 19, state).unwrap();
            let cells: BTreeMap<_, _> = camp
                .template
                .cells
                .iter()
                .map(|c| (c.position, c.state.as_deref()))
                .collect();
            assert_eq!(cells.len(), camp.template.cells.len());
            assert!(cells.len() < 10_000);
            assert!(cells.keys().all(|p| p[2] >= 0));
            assert!(cells.values().any(|v| *v == Some("minecraft:campfire")));
            for [x, y, z] in camp.entrances {
                assert!(cells[&[x, y, z]].is_some());
                assert_eq!(cells[&[x, y, z + 1]], None);
                assert_eq!(cells[&[x, y, z + 2]], None);
            }
            assert!(camp.template.source.contains(style.name()));
        }
    }
    #[test]
    fn ten_shelter_variants_have_distinct_geometry_and_factory_failure_is_atomic() {
        let mut shapes = std::collections::BTreeSet::new();
        for variant in 0..10 {
            let cells = shelter_test_cells(variant).unwrap();
            assert!(shapes.insert(cells));
        }
        assert!(build(CampStyle::Forest, 0, |_, _| Err::<String, _>(
            PlacementError::InvalidState
        ))
        .is_err());
    }
    #[test]
    fn fire_state_is_abandoned_and_seed_layouts_change_without_changing_eligibility() {
        for style in CampStyle::ALL {
            let camp = build(style, 23, |id, props| {
                Ok((
                    id.to_string(),
                    props
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect::<BTreeMap<_, _>>(),
                ))
            })
            .unwrap();
            let fire = camp
                .template
                .cells
                .iter()
                .filter_map(|c| c.state.as_ref())
                .find(|s| s.0 == "minecraft:campfire")
                .unwrap();
            assert_eq!(fire.1.get("lit").map(String::as_str), Some("false"));
            assert!(style.biome_id().starts_with("minecraft:"));
            assert_eq!(CampStyle::for_biome(&style.biome_id()), Some(style));
        }
        let a = build(CampStyle::Forest, 0, state).unwrap();
        let b = build(CampStyle::Forest, 1, state).unwrap();
        assert_ne!(
            a.template
                .cells
                .iter()
                .map(|c| (&c.position, &c.state))
                .collect::<Vec<_>>(),
            b.template
                .cells
                .iter()
                .map(|c| (&c.position, &c.state))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn every_tent_has_a_two_block_clear_path_to_the_shared_court() {
        for seed in 0..40 {
            let camp = build(CampStyle::Forest, seed, state).unwrap();
            let cells: BTreeMap<_, _> = camp
                .template
                .cells
                .iter()
                .map(|c| (c.position, &c.state))
                .collect();
            let mut visited = std::collections::BTreeSet::new();
            let mut queue = std::collections::VecDeque::from([[13, 12]]);
            while let Some([x, y]) = queue.pop_front() {
                if !(0..28).contains(&x) || !(0..28).contains(&y) || visited.contains(&[x, y]) {
                    continue;
                }
                if cells[&[x, y, 0]]
                    .as_ref()
                    .is_none_or(|s| s != "minecraft:dirt_path")
                    || cells[&[x, y, 1]].is_some()
                    || cells[&[x, y, 2]].is_some()
                {
                    continue;
                }
                visited.insert([x, y]);
                queue.extend([[x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]]);
            }
            for [x, y, _] in camp.entrances {
                assert!(visited.contains(&[x, y]), "seed {seed}, entrance {x},{y}");
            }
        }
    }
}
