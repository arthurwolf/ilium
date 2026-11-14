//! Original static surface-event vignettes, separate from biome spawn tables.
//! Global candidate ownership and complete groups are authored homage policies;
//! native timers, trading, weather, raids and gameplay are not simulated.
use super::{
    noise::hash2,
    surface_entities::{self, AtlasLayout, ClimateSkin, Model, Species},
};
pub const TRADER_GRID: i32 = 32;
pub struct EventMember {
    pub species: Species,
    pub anchor: [i32; 3],
    pub model: Model,
}
/// One sparse global owner candidate; rarity/jitter are original scene policies.
pub fn trader_candidate(seed: u64, grid: [i32; 2]) -> Option<[i32; 2]> {
    let base = [
        grid[0].checked_mul(TRADER_GRID)?,
        grid[1].checked_mul(TRADER_GRID)?,
    ];
    let entropy = hash2(
        seed ^ 0x7472_6164_6572_5f70,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if !entropy.is_multiple_of(128) {
        return None;
    }
    Some([
        base[0].checked_add(8 + ((entropy >> 16) & 15) as i32)?,
        base[1].checked_add(8 + ((entropy >> 24) & 15) as i32)?,
    ])
}
/// Complete original visitor formation. Missing or steep ground rejects all
/// members; final-world collision admission belongs to the generator.
pub fn trader_party(
    center: [i32; 3],
    climate: ClimateSkin,
    ground: impl Fn([i32; 2]) -> Option<i32>,
) -> Option<Vec<EventMember>> {
    grounded_party(
        center,
        climate,
        ground,
        &[
            (Species::WanderingTrader, [0, 0]),
            (Species::TraderLlama, [-2, 1]),
            (Species::TraderLlama, [2, 1]),
        ],
    )
}
/// Static raid formation; actual village context and final collisions are
/// checked by the generator. This does not simulate raid waves or combat.
pub fn raid_party(
    center: [i32; 3],
    climate: ClimateSkin,
    ground: impl Fn([i32; 2]) -> Option<i32>,
) -> Option<Vec<EventMember>> {
    grounded_party(
        center,
        climate,
        ground,
        &[
            (Species::Ravager, [0, 0]),
            (Species::Pillager, [-3, 2]),
            (Species::Pillager, [3, 2]),
        ],
    )
}
/// A village owns at most one scene; try its four outer approaches in a stable
/// seeded order. Candidate offsets are original scene placement, not spawn rules.
pub fn raid_candidates(seed: u64, village: [i32; 3]) -> Option<[[i32; 2]; 4]> {
    let entropy = hash2(
        seed ^ 0x7261_6964_7061_7274,
        i64::from(village[0]),
        i64::from(village[1]),
    );
    if !entropy.is_multiple_of(4) {
        return None;
    }
    let mut points = [[0; 2]; 4];
    let offsets = [[96, 0], [0, 96], [-96, 0], [0, -96]];
    for (index, point) in points.iter_mut().enumerate() {
        let offset = offsets[(index + ((entropy >> 8) & 3) as usize) % 4];
        *point = [
            village[0].checked_add(offset[0])?,
            village[1].checked_add(offset[1])?,
        ];
    }
    Some(points)
}
fn grounded_party(
    center: [i32; 3],
    climate: ClimateSkin,
    ground: impl Fn([i32; 2]) -> Option<i32>,
    formation: &[(Species, [i32; 2])],
) -> Option<Vec<EventMember>> {
    if ground([center[0], center[1]])? != center[2] {
        return None;
    }
    let mut members = Vec::with_capacity(formation.len());
    for &(species, offset) in formation {
        let xy = [
            center[0].checked_add(offset[0])?,
            center[1].checked_add(offset[1])?,
        ];
        let feet = ground(xy)?;
        if (i64::from(feet) - i64::from(center[2])).abs() > 2 {
            return None;
        }
        members.push(EventMember {
            species,
            anchor: [xy[0], xy[1], feet],
            model: surface_entities::model(species, AtlasLayout::Bedrock, climate),
        });
    }
    Some(members)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn sparse_trader_candidates_have_signed_global_cell_ownership() {
        for seed in [0, 71839, u64::MAX] {
            let mut count = 0;
            for gy in -64..64 {
                for gx in -64..64 {
                    let grid = [gx, gy];
                    if let Some(anchor) = trader_candidate(seed, grid) {
                        count += 1;
                        assert_eq!(anchor.map(|v| v.div_euclid(TRADER_GRID)), grid);
                        assert!(anchor
                            .iter()
                            .all(|v| (8..24).contains(&v.rem_euclid(TRADER_GRID))));
                        assert_eq!(trader_candidate(seed, grid), Some(anchor));
                    }
                }
            }
            assert!(
                (16..256).contains(&count),
                "Visitor distribution has {count} candidates/16384"
            );
        }
        for grid in [[i32::MIN, 0], [i32::MAX, 0], [0, i32::MIN], [0, i32::MAX]] {
            assert!(trader_candidate(71839, grid).is_none());
        }
    }
    #[test]
    fn visitor_party_keeps_a_trader_and_two_llamas_on_their_actual_ground() {
        let floors = BTreeMap::from([([-17, 9], 80), ([-19, 10], 81), ([-15, 10], 79)]);
        let party = trader_party([-17, 9, 80], ClimateSkin::Temperate, |xy| {
            floors.get(&xy).copied()
        })
        .unwrap();
        assert_eq!(party.len(), 3);
        assert_eq!(party[0].species, Species::WanderingTrader);
        assert_eq!(party[0].anchor, [-17, 9, 80]);
        assert_eq!(party[1].species, Species::TraderLlama);
        assert_eq!(party[1].anchor, [-19, 10, 81]);
        assert_eq!(party[2].species, Species::TraderLlama);
        assert_eq!(party[2].anchor, [-15, 10, 79]);
        for member in party {
            assert_eq!(member.model.species, member.species);
            assert!(member.model.bounds().unwrap().0[2] >= 0.0);
        }
    }
    #[test]
    fn missing_steep_or_overflowing_companion_ground_rejects_the_entire_party() {
        let valid = BTreeMap::from([([0, 0], 80), ([-2, 1], 81), ([2, 1], 79)]);
        for missing in [[0, 0], [-2, 1], [2, 1]] {
            assert!(
                trader_party([0, 0, 80], ClimateSkin::Temperate, |xy| if xy == missing {
                    None
                } else {
                    valid.get(&xy).copied()
                })
                .is_none()
            );
        }
        assert!(trader_party([0, 0, 80], ClimateSkin::Temperate, |xy| Some(
            if xy == [2, 1] { 83 } else { 80 }
        ))
        .is_none());
        assert!(trader_party([0, 0, 80], ClimateSkin::Temperate, |_| Some(79)).is_none());
        for center in [[i32::MIN, 0, 80], [i32::MAX, 0, 80], [0, i32::MAX, 80]] {
            assert!(trader_party(center, ClimateSkin::Temperate, |_| Some(80)).is_none());
        }
    }
}

#[cfg(test)]
mod raid_tests {
    use super::*;
    #[test]
    fn village_raid_keeps_a_ravager_and_two_separate_pillagers() {
        let members = raid_party([-20, 10, 80], ClimateSkin::Temperate, |_| Some(80)).unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].species, Species::Ravager);
        assert_eq!(members[1].species, Species::Pillager);
        assert_eq!(members[2].species, Species::Pillager);
        assert_eq!(members[0].anchor, [-20, 10, 80]);
        assert_eq!(members[1].anchor, [-23, 12, 80]);
        assert_eq!(members[2].anchor, [-17, 12, 80]);
        for member in members {
            assert_eq!(member.model.species, member.species);
        }
    }
    #[test]
    fn raid_rejects_late_missing_or_steep_ground_without_partial_members() {
        assert!(
            raid_party([0, 0, 80], ClimateSkin::Temperate, |xy| (xy != [3, 2])
                .then_some(80))
            .is_none()
        );
        assert!(raid_party([0, 0, 80], ClimateSkin::Temperate, |xy| Some(
            if xy == [3, 2] { 83 } else { 80 }
        ))
        .is_none());
        assert!(raid_party([i32::MAX, 0, 80], ClimateSkin::Temperate, |_| Some(80)).is_none());
    }
    #[test]
    fn raid_owner_is_sparse_stable_and_surrounds_the_village() {
        let mut count = 0;
        for x in -64..64 {
            let village = [x * 256, -257, 80];
            if let Some(points) = raid_candidates(71839, village) {
                count += 1;
                assert_eq!(Some(points), raid_candidates(71839, village));
                let unique: std::collections::BTreeSet<_> = points.into_iter().collect();
                assert_eq!(unique.len(), 4);
                for xy in unique {
                    assert_eq!(
                        (i64::from(xy[0]) - i64::from(village[0])).abs()
                            + (i64::from(xy[1]) - i64::from(village[1])).abs(),
                        96
                    );
                }
            }
        }
        assert!((8..64).contains(&count));
        assert!(raid_candidates(71839, [i32::MAX, 0, 80]).is_none());
    }
}
