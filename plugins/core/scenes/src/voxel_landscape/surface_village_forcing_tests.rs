//! Shared original/current forcing tests: register this file on the original
//! source and on the proposal. A historical RED run must execute assertions.
use super::*;
use std::collections::BTreeSet;

fn require_settlement(village: &VillagePlacement, label: &str) {
    let forms: BTreeSet<_> = village
        .piece_sources
        .iter()
        .filter_map(|source| source.split('/').find(|part| part.starts_with("Home(")))
        .collect();
    let workplaces = village
        .piece_sources
        .iter()
        .filter(|source| source.contains("/Workplace("))
        .count();
    let farms = village
        .piece_sources
        .iter()
        .filter(|source| source.contains("/Farm("))
        .count();
    eprintln!("{label}: center={:?}; residential={}; forms={forms:?}; workplaces={workplaces}; farms={farms}; rejected={}; sources={:?}",
        village.center, village.residential_pieces, village.rejected_pieces, village.piece_sources);
    assert!(
        village.residential_pieces >= 6,
        "{label}: fewer than six complete residences"
    );
    assert!(forms.len() >= 2, "{label}: fewer than two home forms");
    assert!(workplaces > 0, "{label}: no whole workplace");
    assert!(farms > 0, "{label}: no whole farm");
}

macro_rules! natural_witness {
    ($direct:ident, $grid_test:ident, $style:expr, $center:expr, $seed:expr, $grid:expr) => {
        #[test]
        fn $direct() {
            let settings = VoxelLandscapeSettings {
                seed: 71839,
                rivers: true,
                ..Default::default()
            };
            let fields = TerrainFields::new(71839);
            let village = assemble($style, $center, $seed, &fields, &settings, || false).unwrap();
            assert_eq!(village.center, $center);
            require_settlement(&village, stringify!($direct));
        }
        #[test]
        fn $grid_test() {
            let settings = VoxelLandscapeSettings {
                seed: 71839,
                rivers: true,
                ..Default::default()
            };
            let fields = TerrainFields::new(71839);
            let village = candidate($grid, &fields, &settings, || false)
                .unwrap()
                .expect("original natural grid disappeared");
            assert_eq!(village.style, $style);
            require_settlement(&village, stringify!($grid_test));
        }
    };
}

natural_witness!(
    original_desert_center,
    original_desert_grid,
    VillageStyle::Desert,
    [-896, -11600, 67],
    13402024533940074428,
    [-4, -46]
);
natural_witness!(
    original_plains_center,
    original_plains_grid,
    VillageStyle::Plains,
    [11440, -12144, 104],
    5726667076090912111,
    [44, -48]
);
natural_witness!(
    original_savanna_center,
    original_savanna_grid,
    VillageStyle::Savanna,
    [416, -12160, 80],
    1573488352327643686,
    [1, -48]
);
natural_witness!(
    original_snowy_center,
    original_snowy_grid,
    VillageStyle::Snowy,
    [-11344, -12128, 99],
    13750931656163572791,
    [-45, -48]
);
natural_witness!(
    original_taiga_center,
    original_taiga_grid,
    VillageStyle::Taiga,
    [2480, -12224, 98],
    2630010982133005319,
    [9, -48]
);

#[test]
fn same_ordinary_terrain_requires_all_five_styles() {
    let settings = VoxelLandscapeSettings {
        seed: 42,
        rivers: false,
        ..Default::default()
    };
    let fields = TerrainFields::new(42);
    assert_eq!(fields.sample(0, 32, false).height, 104);
    for style in VillageStyle::ALL {
        let village = assemble(style, [0, 32, 104], 7, &fields, &settings, || false).unwrap();
        require_settlement(&village, style.name());
        assert!(
            village
                .piece_sources
                .iter()
                .any(|source| source.contains("/Pen/")),
            "{}: no pen",
            style.name()
        );
        assert_eq!(
            village
                .piece_sources
                .iter()
                .filter(|source| source.contains("/Center/"))
                .count(),
            1
        );
    }
}

#[test]
fn authored_entrances_include_porch_door_and_explicit_headroom() {
    for style in VillageStyle::ALL {
        for form in HomeForm::ALL {
            let piece = village_kit::build(style, PieceKind::Home(form), 7, state).unwrap();
            let entrance = piece.ports[0];
            let doorway = [entrance.position[0], 0, 0];
            assert!(
                piece.walkable.contains(&entrance.position),
                "{style:?}/{form:?}: missing porch route"
            );
            assert!(
                piece.walkable.contains(&doorway),
                "{style:?}/{form:?}: missing doorway route"
            );
            for z in [1, 2] {
                let cell = piece
                    .template
                    .cells
                    .iter()
                    .find(|cell| cell.position == [entrance.position[0], -1, z])
                    .expect("porch must explicitly carve both headroom cells");
                assert!(cell.state.is_none());
            }
        }
    }
}

#[test]
fn frozen_snowy_roof_contacts_still_reject_complete_footprints() {
    for (
        accepted_kind,
        accepted_seed,
        accepted_anchor,
        accepted_turns,
        rejected_kind,
        rejected_seed,
        rejected_anchor,
        rejected_turns,
        collision,
        incoming_id,
    ) in [
        (
            PieceKind::Workplace(Profession::Butcher),
            985594904429933968,
            [-11337, -12136, 99],
            3,
            PieceKind::Workplace(Profession::Cleric),
            1651997444482847777,
            [-11330, -12135, 99],
            2,
            [-11330, -12135, 104],
            "minecraft:stripped_spruce_log",
        ),
        (
            PieceKind::Home(HomeForm::Cottage),
            15221988196581632000,
            [-11351, -12120, 99],
            1,
            PieceKind::Home(HomeForm::Longhouse),
            4735857864417500514,
            [-11359, -12121, 99],
            0,
            [-11358, -12121, 104],
            "minecraft:snow_block",
        ),
    ] {
        let accepted =
            village_kit::build(VillageStyle::Snowy, accepted_kind, accepted_seed, state).unwrap();
        let rejected =
            village_kit::build(VillageStyle::Snowy, rejected_kind, rejected_seed, state).unwrap();
        let existing = Prepared::prepare(
            &accepted.template,
            accepted_anchor,
            accepted_turns,
            rotated_state,
            |_| Habitat::Replaceable,
            || false,
        )
        .unwrap();
        let separate = Prepared::prepare(
            &rejected.template,
            rejected_anchor,
            rejected_turns,
            rotated_state,
            |_| Habitat::Replaceable,
            || false,
        )
        .unwrap();
        let roof = existing
            .cells()
            .find(|(at, _)| *at == collision)
            .unwrap()
            .1
            .unwrap();
        let incoming = separate
            .cells()
            .find(|(at, _)| *at == collision)
            .unwrap()
            .1
            .unwrap();
        assert_eq!(collision[2], 104);
        assert_eq!(roof.id().as_str(), "minecraft:spruce_stairs");
        assert_eq!(incoming.id().as_str(), incoming_id);
        let occupied: BTreeSet<_> = existing.cells().map(|(position, _)| position).collect();
        assert!(
            matches!(Prepared::prepare(&rejected.template, rejected_anchor, rejected_turns,
            rotated_state, |position| if occupied.contains(&position) { Habitat::Protected } else { Habitat::Replaceable },
            || false), Err(PlacementError::Protected(position)) if position == collision)
        );
    }
}
