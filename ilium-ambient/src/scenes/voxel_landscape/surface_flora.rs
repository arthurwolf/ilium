//! Atomic surface-plant admission, independent of asset I/O and rendering.
//! Callers retain source states/providers; support rules are authored homage.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HabitatCell {
    Air,
    Soil,
    Sand,
    Solid,
    Water,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Soil,
    Sand,
    Solid,
    WaterEdge,
    FloatingWater,
    /// Dry ground cover keeps soil support while also admitting arid sand.
    SoilOrSand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloraState {
    pub resource_id: &'static str,
    pub properties: Vec<(&'static str, &'static str)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloraCell {
    pub position: [i32; 3],
    pub state: FloraState,
}

/// One globally owned plant or patch, prepared against one terrain generation.
/// Offsets use scene Z-up coordinates. A tall pair is one candidate, never two.
pub struct PlantCandidate {
    pub anchor: [i32; 3],
    pub source_prescription: &'static str,
    pub cells: Vec<FloraCell>,
    pub support: Support,
    pub cactus_clearance: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionError {
    InvalidCandidate,
    CoordinateOverflow,
    Budget,
    Collision([i32; 3]),
    Unknown([i32; 3]),
    Unsupported([i32; 3]),
    Obstructed([i32; 3]),
    Cancelled,
}

impl PlantCandidate {
    pub fn single(
        anchor: [i32; 3],
        source_prescription: &'static str,
        state: FloraState,
        support: Support,
    ) -> Self {
        Self {
            anchor,
            source_prescription,
            cells: vec![FloraCell {
                position: [0; 3],
                state,
            }],
            support,
            cactus_clearance: false,
        }
    }

    pub fn tall(
        anchor: [i32; 3],
        source_prescription: &'static str,
        mut lower: FloraState,
        support: Support,
    ) -> Self {
        lower.properties.retain(|(key, _)| *key != "half");
        let mut upper = lower.clone();
        lower.properties.push(("half", "lower"));
        upper.properties.push(("half", "upper"));
        Self {
            anchor,
            source_prescription,
            cells: vec![
                FloraCell {
                    position: [0; 3],
                    state: lower,
                },
                FloraCell {
                    position: [0, 0, 1],
                    state: upper,
                },
            ],
            support,
            cactus_clearance: false,
        }
    }

    pub fn column(
        anchor: [i32; 3],
        source_prescription: &'static str,
        state: FloraState,
        height: u8,
        support: Support,
        cactus_clearance: bool,
    ) -> Self {
        Self {
            anchor,
            source_prescription,
            cells: (0..height)
                .map(|z| FloraCell {
                    position: [0, 0, i32::from(z)],
                    state: state.clone(),
                })
                .collect(),
            support,
            cactus_clearance,
        }
    }
}

/// A worker transaction. Failed candidates never leave partial writes behind.
pub struct FloraPlacement {
    cells: BTreeMap<[i32; 3], FloraCell>,
    reserved_air: BTreeSet<[i32; 3]>,
    limit: usize,
}

fn offset(position: [i32; 3], delta: [i32; 3]) -> Result<[i32; 3], AdmissionError> {
    let mut result = position;
    for axis in 0..3 {
        result[axis] = result[axis]
            .checked_add(delta[axis])
            .ok_or(AdmissionError::CoordinateOverflow)?;
    }
    Ok(result)
}

const CARDINAL: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0]];

fn valid_resource(id: &str) -> bool {
    let Some(path) = id.strip_prefix("minecraft:") else {
        return false;
    };
    id.len() <= 128
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
                })
        })
}

// Match the semantic-state adapter syntax without guessing registry legality.
fn valid_property_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.' | b':')
        })
}

impl FloraPlacement {
    pub fn new(limit: usize) -> Result<Self, AdmissionError> {
        if limit == 0 || limit > 65_536 {
            return Err(AdmissionError::Budget);
        }
        Ok(Self {
            cells: BTreeMap::new(),
            reserved_air: BTreeSet::new(),
            limit,
        })
    }

    pub fn cells(&self) -> impl Iterator<Item = &FloraCell> {
        self.cells.values()
    }

    pub fn admit(
        &mut self,
        candidate: PlantCandidate,
        context: impl Fn([i32; 3]) -> HabitatCell,
    ) -> Result<usize, AdmissionError> {
        self.admit_cancellable(candidate, context, || false)
    }

    /// Context belongs to one immutable terrain/occupancy generation. Cancellation
    /// before commit leaves both cells and reserved air unchanged. The commit has
    /// no cancellation point between its two ledger updates.
    pub fn admit_cancellable(
        &mut self,
        candidate: PlantCandidate,
        context: impl Fn([i32; 3]) -> HabitatCell,
        cancelled: impl Fn() -> bool,
    ) -> Result<usize, AdmissionError> {
        if cancelled() {
            return Err(AdmissionError::Cancelled);
        }
        if candidate.source_prescription.is_empty()
            || candidate.cells.is_empty()
            || candidate.cells.len() > 512
        {
            return Err(AdmissionError::InvalidCandidate);
        }
        if self
            .cells
            .len()
            .checked_add(candidate.cells.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(AdmissionError::Budget);
        }
        let mut prepared = BTreeMap::new();
        for mut cell in candidate.cells {
            if cancelled() {
                return Err(AdmissionError::Cancelled);
            }
            if cell.position.iter().any(|n| n.unsigned_abs() > 32)
                || !valid_resource(cell.state.resource_id)
            {
                return Err(AdmissionError::InvalidCandidate);
            }
            cell.position = offset(candidate.anchor, cell.position)?;
            if self.reserved_air.contains(&cell.position) {
                return Err(AdmissionError::Obstructed(cell.position));
            }
            if cell.state.properties.len() > 32 {
                return Err(AdmissionError::InvalidCandidate);
            }
            cell.state.properties.sort_unstable();
            if cell
                .state
                .properties
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0)
                || cell
                    .state
                    .properties
                    .iter()
                    .any(|(key, value)| !valid_property_atom(key) || !valid_property_atom(value))
            {
                return Err(AdmissionError::InvalidCandidate);
            }
            if prepared.contains_key(&cell.position) || self.cells.contains_key(&cell.position) {
                return Err(AdmissionError::Collision(cell.position));
            }
            match context(cell.position) {
                HabitatCell::Air => {}
                HabitatCell::Unknown => return Err(AdmissionError::Unknown(cell.position)),
                _ => return Err(AdmissionError::Obstructed(cell.position)),
            }
            prepared.insert(cell.position, cell);
        }
        // Generated cactus flowers are complete, same-owner crown candidates,
        // not free-standing flowers or attachments borrowing another candidate.
        // The ordinary same-candidate support shortcut alone would also accept
        // a flower above an arbitrary plant, or a cactus above that plant.
        let contains_flower = prepared
            .values()
            .any(|cell| cell.state.resource_id == "minecraft:cactus_flower");
        if candidate.source_prescription == "minecraft:cactus_flower" || contains_flower {
            if candidate.source_prescription != "minecraft:cactus_flower"
                || prepared.len() != 3
                || candidate.support != Support::Sand
                || !candidate.cactus_clearance
            {
                return Err(AdmissionError::InvalidCandidate);
            }
            let crown = prepared.len() - 1;
            for (level, cell) in prepared.values().enumerate() {
                if cancelled() {
                    return Err(AdmissionError::Cancelled);
                }
                let resource = if level == crown {
                    "minecraft:cactus_flower"
                } else {
                    "minecraft:cactus"
                };
                let valid_properties = if level == crown {
                    cell.state.properties.is_empty()
                } else {
                    cell.state.properties.as_slice() == [("age", "0")]
                };
                let height =
                    i32::try_from(level).map_err(|_| AdmissionError::CoordinateOverflow)?;
                if cell.position != offset(candidate.anchor, [0, 0, height])?
                    || cell.state.resource_id != resource
                    || !valid_properties
                {
                    return Err(AdmissionError::InvalidCandidate);
                }
            }
        }
        let positions: BTreeSet<_> = prepared.keys().copied().collect();
        let mut clearance_to_reserve = BTreeSet::new();
        for position in &positions {
            if cancelled() {
                return Err(AdmissionError::Cancelled);
            }
            let below = offset(*position, [0, 0, -1])?;
            if !positions.contains(&below) {
                let support = context(below);
                if support == HabitatCell::Unknown {
                    return Err(AdmissionError::Unknown(below));
                }
                let valid = match candidate.support {
                    Support::Soil => support == HabitatCell::Soil,
                    Support::Sand => support == HabitatCell::Sand,
                    Support::SoilOrSand => matches!(support, HabitatCell::Soil | HabitatCell::Sand),
                    Support::Solid => matches!(
                        support,
                        HabitatCell::Soil | HabitatCell::Sand | HabitatCell::Solid
                    ),
                    Support::FloatingWater => support == HabitatCell::Water,
                    Support::WaterEdge => {
                        let neighbours = CARDINAL.map(|d| offset(below, d));
                        let mut water = false;
                        for neighbour in neighbours {
                            let neighbour = neighbour?;
                            let cell = context(neighbour);
                            if cell == HabitatCell::Unknown {
                                return Err(AdmissionError::Unknown(neighbour));
                            }
                            water |= cell == HabitatCell::Water;
                        }
                        matches!(support, HabitatCell::Soil | HabitatCell::Sand) && water
                    }
                };
                if !valid {
                    return Err(AdmissionError::Unsupported(below));
                }
            }
            if candidate.cactus_clearance {
                for delta in CARDINAL {
                    let neighbour = offset(*position, delta)?;
                    if self.cells.contains_key(&neighbour) || positions.contains(&neighbour) {
                        return Err(AdmissionError::Obstructed(neighbour));
                    }
                    match context(neighbour) {
                        HabitatCell::Air => {}
                        HabitatCell::Unknown => return Err(AdmissionError::Unknown(neighbour)),
                        _ => return Err(AdmissionError::Obstructed(neighbour)),
                    }
                    clearance_to_reserve.insert(neighbour);
                }
            }
        }
        if cancelled() {
            return Err(AdmissionError::Cancelled);
        }
        let count = prepared.len();
        self.cells.extend(prepared);
        self.reserved_air.extend(clearance_to_reserve);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(id: &'static str) -> FloraState {
        FloraState {
            resource_id: id,
            properties: vec![],
        }
    }
    fn soil(p: [i32; 3]) -> HabitatCell {
        if p[2] == 0 {
            HabitatCell::Soil
        } else {
            HabitatCell::Air
        }
    }

    #[test]
    fn tall_pair_is_atomic_when_upper_cell_is_missing_or_occupied() {
        for obstruction in [HabitatCell::Unknown, HabitatCell::Solid] {
            let mut placement = FloraPlacement::new(16).unwrap();
            let plant = PlantCandidate::tall(
                [4, 7, 1],
                "sunflower",
                state("minecraft:sunflower"),
                Support::Soil,
            );
            assert!(placement
                .admit(plant, |p| if p == [4, 7, 2] {
                    obstruction
                } else {
                    soil(p)
                })
                .is_err());
            assert_eq!(placement.cells().count(), 0);
        }
        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(
            placement.admit(
                PlantCandidate::tall(
                    [4, 7, 1],
                    "sunflower",
                    state("minecraft:sunflower"),
                    Support::Soil
                ),
                soil
            ),
            Ok(2)
        );
        assert_eq!(
            placement.cells().next().unwrap().state.properties,
            [("half", "lower")]
        );
    }

    #[test]
    fn reed_column_requires_water_at_the_ground_level_and_publishes_whole() {
        let mut placement = FloraPlacement::new(16).unwrap();
        let plant = || {
            PlantCandidate::column(
                [0, 0, 1],
                "patch_sugar_cane",
                state("minecraft:sugar_cane"),
                4,
                Support::WaterEdge,
                false,
            )
        };
        assert!(placement.admit(plant(), soil).is_err());
        assert_eq!(placement.cells().count(), 0);
        assert_eq!(
            placement.admit(plant(), |p| if p == [1, 0, 0] {
                HabitatCell::Water
            } else {
                soil(p)
            }),
            Ok(4)
        );
    }

    #[test]
    fn cactus_rejects_side_obstructions_and_non_sand_support() {
        let plant = || {
            PlantCandidate::column(
                [0, 0, 1],
                "patch_cactus",
                state("minecraft:cactus"),
                3,
                Support::Sand,
                true,
            )
        };
        let mut placement = FloraPlacement::new(16).unwrap();
        assert!(placement.admit(plant(), soil).is_err());
        let sand = |p: [i32; 3]| {
            if p[2] == 0 {
                HabitatCell::Sand
            } else {
                HabitatCell::Air
            }
        };
        assert!(placement
            .admit(plant(), |p| if p == [1, 0, 2] {
                HabitatCell::Solid
            } else {
                sand(p)
            })
            .is_err());
        assert_eq!(placement.cells().count(), 0);
        assert_eq!(placement.admit(plant(), sand), Ok(3));
    }

    #[test]
    fn floating_lily_requires_water_and_collision_or_budget_never_partially_writes() {
        let mut placement = FloraPlacement::new(2).unwrap();
        let lily = || {
            PlantCandidate::single(
                [0, 0, 1],
                "patch_waterlily",
                state("minecraft:lily_pad"),
                Support::FloatingWater,
            )
        };
        assert!(placement.admit(lily(), soil).is_err());
        assert_eq!(
            placement.admit(lily(), |p| if p[2] == 0 {
                HabitatCell::Water
            } else {
                HabitatCell::Air
            }),
            Ok(1)
        );
        assert_eq!(
            placement.admit(lily(), soil),
            Err(AdmissionError::Collision([0, 0, 1]))
        );
        assert_eq!(
            placement.admit(
                PlantCandidate::tall([2, 0, 1], "lilac", state("minecraft:lilac"), Support::Soil),
                soil
            ),
            Err(AdmissionError::Budget)
        );
        assert_eq!(placement.cells().count(), 1);
    }

    #[test]
    fn coordinate_overflow_and_invalid_properties_preserve_previous_state() {
        let mut placement = FloraPlacement::new(16).unwrap();
        let overflow = PlantCandidate::tall(
            [0, 0, i32::MAX],
            "tall_grass",
            state("minecraft:tall_grass"),
            Support::Soil,
        );
        assert_eq!(
            placement.admit(overflow, |_| HabitatCell::Air),
            Err(AdmissionError::CoordinateOverflow)
        );
        let mut duplicate = state("minecraft:poppy");
        duplicate.properties = vec![("age", "0"), ("age", "1")];
        assert_eq!(
            placement.admit(
                PlantCandidate::single([0, 0, 1], "flower_default", duplicate, Support::Soil),
                soil
            ),
            Err(AdmissionError::InvalidCandidate)
        );
        assert_eq!(placement.cells().count(), 0);
    }

    #[test]
    fn malformed_semantic_ids_are_rejected_before_publication() {
        for id in [
            "minecraft:",
            "minecraft:../poppy",
            "minecraft:BAD",
            "minecraft:poppy:other",
            "minecraft:poppy\n",
        ] {
            let mut placement = FloraPlacement::new(16).unwrap();
            assert_eq!(
                placement.admit(
                    PlantCandidate::single([0, 0, 1], "flower_default", state(id), Support::Soil),
                    soil
                ),
                Err(AdmissionError::InvalidCandidate)
            );
            assert_eq!(placement.cells().count(), 0);
        }
    }

    #[test]
    fn later_flora_cannot_invalidate_an_earlier_cactus_clearance() {
        let context = |p: [i32; 3]| {
            if p == [0, 0, 0] {
                HabitatCell::Sand
            } else if p[2] == 0 {
                HabitatCell::Soil
            } else {
                HabitatCell::Air
            }
        };
        let cactus = || {
            PlantCandidate::column(
                [0, 0, 1],
                "patch_cactus",
                state("minecraft:cactus"),
                3,
                Support::Sand,
                true,
            )
        };
        let poppy = || {
            PlantCandidate::single(
                [1, 0, 1],
                "flower_default",
                state("minecraft:poppy"),
                Support::Soil,
            )
        };
        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(placement.admit(cactus(), context), Ok(3));
        assert_eq!(
            placement.admit(poppy(), context),
            Err(AdmissionError::Obstructed([1, 0, 1]))
        );
        assert_eq!(placement.cells().count(), 3);
        let mut reversed = FloraPlacement::new(16).unwrap();
        assert_eq!(reversed.admit(poppy(), context), Ok(1));
        assert_eq!(
            reversed.admit(cactus(), context),
            Err(AdmissionError::Obstructed([1, 0, 1]))
        );
        assert_eq!(reversed.cells().count(), 1);
    }

    #[test]
    fn published_properties_meet_the_semantic_state_adapter_contract() {
        for properties in [
            vec![("age", "UPPER")],
            vec![("half\n", "lower")],
            vec![("age", "a/b")],
        ] {
            let mut placement = FloraPlacement::new(16).unwrap();
            let mut invalid = state("minecraft:poppy");
            invalid.properties = properties;
            assert_eq!(
                placement.admit(
                    PlantCandidate::single([0, 0, 1], "flower_default", invalid, Support::Soil),
                    soil
                ),
                Err(AdmissionError::InvalidCandidate)
            );
            assert_eq!(placement.cells().count(), 0);
        }
        let properties = (0..33)
            .map(|index| {
                (
                    Box::leak(format!("p{index}").into_boxed_str()) as &'static str,
                    "0",
                )
            })
            .collect();
        let invalid = FloraState {
            resource_id: "minecraft:poppy",
            properties,
        };
        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(
            placement.admit(
                PlantCandidate::single([0, 0, 1], "flower_default", invalid, Support::Soil),
                soil
            ),
            Err(AdmissionError::InvalidCandidate)
        );
        assert_eq!(placement.cells().count(), 0);
    }
    fn sand(position: [i32; 3]) -> HabitatCell {
        if position[2] == 0 {
            HabitatCell::Sand
        } else {
            HabitatCell::Air
        }
    }

    fn cactus_crown(anchor: [i32; 3]) -> PlantCandidate {
        let mut candidate = PlantCandidate::column(
            anchor,
            "minecraft:cactus_flower",
            FloraState {
                resource_id: "minecraft:cactus",
                properties: vec![("age", "0")],
            },
            2,
            Support::Sand,
            true,
        );
        candidate.cells.push(FloraCell {
            position: [0, 0, 2],
            state: state("minecraft:cactus_flower"),
        });
        candidate
    }

    #[test]
    fn both_dry_grasses_admit_one_propertyless_cell_on_soil_or_sand_only() {
        for id in ["minecraft:short_dry_grass", "minecraft:tall_dry_grass"] {
            for support in [HabitatCell::Soil, HabitatCell::Sand] {
                let mut placement = FloraPlacement::new(4).unwrap();
                let candidate =
                    PlantCandidate::single([0, 0, 1], id, state(id), Support::SoilOrSand);
                assert_eq!(
                    placement.admit(candidate, |position| {
                        if position[2] == 0 {
                            support
                        } else {
                            HabitatCell::Air
                        }
                    }),
                    Ok(1),
                    "{id} on {support:?}"
                );
                let cell = placement.cells().next().unwrap();
                assert_eq!(cell.position, [0, 0, 1]);
                assert_eq!(cell.state.resource_id, id);
                assert!(cell.state.properties.is_empty());
            }
            for support in [HabitatCell::Air, HabitatCell::Solid, HabitatCell::Water] {
                let mut placement = FloraPlacement::new(4).unwrap();
                assert_eq!(
                    placement.admit(
                        PlantCandidate::single([0, 0, 1], id, state(id), Support::SoilOrSand),
                        |position| if position[2] == 0 {
                            support
                        } else {
                            HabitatCell::Air
                        }
                    ),
                    Err(AdmissionError::Unsupported([0, 0, 0])),
                    "{id} on {support:?}"
                );
                assert_eq!(placement.cells().count(), 0);
            }
            let mut placement = FloraPlacement::new(4).unwrap();
            assert_eq!(
                placement.admit(
                    PlantCandidate::single([0, 0, 1], id, state(id), Support::SoilOrSand),
                    |position| if position[2] == 0 {
                        HabitatCell::Unknown
                    } else {
                        HabitatCell::Air
                    }
                ),
                Err(AdmissionError::Unknown([0, 0, 0]))
            );
            assert_eq!(placement.cells().count(), 0);
        }
    }

    #[test]
    fn cactus_flower_requires_the_exact_same_owner_crown_shape_and_states() {
        let mut malformed = Vec::new();
        malformed.push(PlantCandidate::single(
            [0, 0, 1],
            "minecraft:cactus_flower",
            state("minecraft:cactus_flower"),
            Support::Sand,
        ));
        let mut missing_crown = cactus_crown([0, 0, 1]);
        missing_crown.cells.pop();
        malformed.push(missing_crown);
        let mut wrong_source = cactus_crown([0, 0, 1]);
        wrong_source.source_prescription = "minecraft:cactus";
        malformed.push(wrong_source);
        let mut wrong_stem = cactus_crown([0, 0, 1]);
        wrong_stem.cells[0].state.resource_id = "minecraft:dead_bush";
        malformed.push(wrong_stem);
        let mut wrong_age = cactus_crown([0, 0, 1]);
        wrong_age.cells[1].state.properties = vec![("age", "1")];
        malformed.push(wrong_age);
        let mut decorated_flower = cactus_crown([0, 0, 1]);
        decorated_flower.cells[2].state.properties = vec![("age", "0")];
        malformed.push(decorated_flower);
        let mut gap = cactus_crown([0, 0, 1]);
        gap.cells[1].position = [0, 0, 3];
        malformed.push(gap);
        let mut sideways_crown = cactus_crown([0, 0, 1]);
        sideways_crown.cells[2].position = [1, 0, 2];
        malformed.push(sideways_crown);
        let mut missing_clearance = cactus_crown([0, 0, 1]);
        missing_clearance.cactus_clearance = false;
        malformed.push(missing_clearance);
        let mut wrong_support = cactus_crown([0, 0, 1]);
        wrong_support.support = Support::Soil;
        malformed.push(wrong_support);
        let mut extra_stem = cactus_crown([0, 0, 1]);
        extra_stem.cells.push(FloraCell {
            position: [0, 0, 3],
            state: state("minecraft:cactus"),
        });
        malformed.push(extra_stem);
        for candidate in malformed {
            let mut placement = FloraPlacement::new(16).unwrap();
            assert_eq!(
                placement.admit(candidate, sand),
                Err(AdmissionError::InvalidCandidate)
            );
            assert_eq!(placement.cells().count(), 0);
            assert!(placement.reserved_air.is_empty());
        }
        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(placement.admit(cactus_crown([0, 0, 1]), sand), Ok(3));
        for (height, expected) in [
            "minecraft:cactus",
            "minecraft:cactus",
            "minecraft:cactus_flower",
        ]
        .into_iter()
        .enumerate()
        {
            let cell = placement.cells.get(&[0, 0, 1 + height as i32]).unwrap();
            assert_eq!(cell.state.resource_id, expected);
            if height == 2 {
                assert!(cell.state.properties.is_empty());
            } else {
                assert_eq!(cell.state.properties, [("age", "0")]);
            }
        }
        assert_eq!(placement.reserved_air.len(), 12);
    }

    #[test]
    fn cactus_crown_rejects_missing_support_blocked_cells_and_lateral_air_atomically() {
        for support in [
            HabitatCell::Air,
            HabitatCell::Soil,
            HabitatCell::Solid,
            HabitatCell::Water,
        ] {
            let mut placement = FloraPlacement::new(16).unwrap();
            assert_eq!(
                placement.admit(cactus_crown([0, 0, 1]), |position| {
                    if position == [0, 0, 0] {
                        support
                    } else {
                        sand(position)
                    }
                }),
                Err(AdmissionError::Unsupported([0, 0, 0]))
            );
            assert!(placement.cells.is_empty());
            assert!(placement.reserved_air.is_empty());
        }
        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(
            placement.admit(cactus_crown([0, 0, 1]), |position| {
                if position == [0, 0, 0] {
                    HabitatCell::Unknown
                } else {
                    sand(position)
                }
            }),
            Err(AdmissionError::Unknown([0, 0, 0]))
        );
        assert!(placement.cells.is_empty());
        assert!(placement.reserved_air.is_empty());

        for blocked in [[0, 0, 1], [0, 0, 2], [0, 0, 3], [1, 0, 3]] {
            for habitat in [HabitatCell::Solid, HabitatCell::Water, HabitatCell::Unknown] {
                let mut placement = FloraPlacement::new(16).unwrap();
                let expected = if habitat == HabitatCell::Unknown {
                    AdmissionError::Unknown(blocked)
                } else {
                    AdmissionError::Obstructed(blocked)
                };
                assert_eq!(
                    placement.admit(cactus_crown([0, 0, 1]), |position| {
                        if position == blocked {
                            habitat
                        } else {
                            sand(position)
                        }
                    }),
                    Err(expected),
                    "blocked={blocked:?} habitat={habitat:?}"
                );
                assert!(placement.cells.is_empty());
                assert!(placement.reserved_air.is_empty());
                assert_eq!(placement.admit(cactus_crown([0, 0, 1]), sand), Ok(3));
            }
        }
    }

    #[test]
    fn cactus_crown_budget_collision_and_reserved_air_leave_both_ledgers_intact() {
        let mut too_small = FloraPlacement::new(2).unwrap();
        assert_eq!(
            too_small.admit(cactus_crown([0, 0, 1]), sand),
            Err(AdmissionError::Budget)
        );
        assert!(too_small.cells.is_empty());
        assert!(too_small.reserved_air.is_empty());

        let mut placement = FloraPlacement::new(16).unwrap();
        assert_eq!(placement.admit(cactus_crown([0, 0, 1]), sand), Ok(3));
        let cells = placement.cells.clone();
        let reserved_air = placement.reserved_air.clone();
        assert_eq!(
            placement.admit(cactus_crown([0, 0, 1]), sand),
            Err(AdmissionError::Collision([0, 0, 1]))
        );
        assert_eq!(
            placement.admit(
                PlantCandidate::single(
                    [1, 0, 1],
                    "minecraft:short_dry_grass",
                    state("minecraft:short_dry_grass"),
                    Support::SoilOrSand,
                ),
                sand,
            ),
            Err(AdmissionError::Obstructed([1, 0, 1]))
        );
        assert_eq!(placement.cells, cells);
        assert_eq!(placement.reserved_air, reserved_air);
    }

    #[test]
    fn cactus_crown_negative_coordinates_overflow_and_cancellation_are_atomic() {
        let mut negative = FloraPlacement::new(16).unwrap();
        assert_eq!(negative.admit(cactus_crown([-4, -8, 1]), sand), Ok(3));
        assert!(negative.cells.contains_key(&[-4, -8, 1]));
        assert!(negative.cells.contains_key(&[-4, -8, 2]));
        assert!(negative.cells.contains_key(&[-4, -8, 3]));
        assert!(negative.reserved_air.contains(&[-5, -8, 3]));

        let mut overflow = FloraPlacement::new(16).unwrap();
        assert_eq!(
            overflow.admit(cactus_crown([i32::MAX, 0, 1]), sand),
            Err(AdmissionError::CoordinateOverflow)
        );
        assert!(overflow.cells.is_empty());
        assert!(overflow.reserved_air.is_empty());
        assert_eq!(
            overflow.admit(cactus_crown([0, 0, i32::MAX]), sand),
            Err(AdmissionError::CoordinateOverflow)
        );
        assert!(overflow.cells.is_empty());

        use std::cell::Cell;
        let calls = Cell::new(0);
        let mut successful = FloraPlacement::new(16).unwrap();
        assert_eq!(
            successful.admit_cancellable(cactus_crown([0, 0, 1]), sand, || {
                calls.set(calls.get() + 1);
                false
            }),
            Ok(3)
        );
        let final_checkpoint = calls.get();
        assert!(final_checkpoint > 3);
        for stop_at in 1..=final_checkpoint {
            let mut placement = FloraPlacement::new(16).unwrap();
            assert_eq!(
                placement.admit(
                    PlantCandidate::single(
                        [-8, 0, 1],
                        "minecraft:short_dry_grass",
                        state("minecraft:short_dry_grass"),
                        Support::SoilOrSand,
                    ),
                    sand,
                ),
                Ok(1)
            );
            let previous_cells = placement.cells.clone();
            let previous_air = placement.reserved_air.clone();
            let attempt_calls = Cell::new(0);
            assert_eq!(
                placement.admit_cancellable(cactus_crown([0, 0, 1]), sand, || {
                    attempt_calls.set(attempt_calls.get() + 1);
                    attempt_calls.get() == stop_at
                }),
                Err(AdmissionError::Cancelled),
                "stop_at={stop_at} final_checkpoint={final_checkpoint}"
            );
            assert_eq!(placement.cells, previous_cells);
            assert_eq!(placement.reserved_air, previous_air);
        }
    }
}
