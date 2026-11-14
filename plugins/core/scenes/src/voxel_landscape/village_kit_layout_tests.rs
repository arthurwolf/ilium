use super::super::{
    assets::{block_state::BlockState, identity::ResourceId},
    surface_structures::{self, Habitat, Prepared},
};
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Cells = BTreeMap<[i32; 3], Option<BlockState>>;
const CARDINAL: [&str; 4] = ["north", "east", "south", "west"];

fn state(id: &str, props: &[(&str, &str)]) -> Result<BlockState, PlacementError> {
    BlockState::new(
        ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
        props
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    )
    .map_err(|_| PlacementError::InvalidState)
}
fn turn([x, y, z]: [i32; 3], turns: u8) -> [i32; 3] {
    match turns {
        1 => [-y, x, z],
        2 => [-x, -y, z],
        3 => [y, -x, z],
        _ => [x, y, z],
    }
}
fn word(value: &str, turns: u8) -> String {
    CARDINAL.iter().position(|item| *item == value).map_or_else(
        || value.to_owned(),
        |index| CARDINAL[(index + usize::from(turns)) % 4].into(),
    )
}
fn expected_props(value: &BlockState, turns: u8) -> BTreeMap<String, String> {
    value
        .properties()
        .iter()
        .map(|(key, value)| {
            let rotated = match (key.as_str(), value.as_str(), turns % 2) {
                ("facing", _, _) => word(value, turns),
                ("axis", "x", 1) => "z".into(),
                ("axis", "z", 1) => "x".into(),
                _ => value.clone(),
            };
            (word(key, turns), rotated)
        })
        .collect()
}
fn prepare(
    piece: &Piece<BlockState>,
    turns: u8,
    blocked: Option<[i32; 3]>,
) -> Result<Prepared<BlockState>, PlacementError> {
    Prepared::prepare(
        &piece.template,
        [0; 3],
        turns,
        |value, rotation| {
            BlockState::new(
                value.id().clone(),
                surface_structures::rotate_properties(value.properties(), rotation)?,
            )
            .map_err(|_| PlacementError::InvalidState)
        },
        |position| {
            if Some(position) == blocked {
                Habitat::Protected
            } else {
                Habitat::Replaceable
            }
        },
        || false,
    )
}
fn cells(piece: &Piece<BlockState>, turns: u8) -> Cells {
    prepare(piece, turns, None)
        .unwrap()
        .cells()
        .map(|(position, value)| (position, value.cloned()))
        .collect()
}
fn clear(value: &Option<BlockState>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let id = value.id().as_str();
    id.ends_with("_door")
        || id.ends_with("_carpet")
        || id == "minecraft:poppy"
        || (id.ends_with("_fence_gate")
            && value
                .properties()
                .get("open")
                .is_some_and(|value| value == "true"))
}
fn route(cells: &Cells, floors: BTreeSet<[i32; 3]>) {
    let Some(first) = floors.first().copied() else {
        return;
    };
    for &[x, y, z] in &floors {
        assert!(cells[&[x, y, z]]
            .as_ref()
            .is_some_and(|value| value.id().as_str() != "minecraft:water"));
        for dz in 1..=2 {
            assert!(clear(cells.get(&[x, y, z + dz]).expect("uncarved route")));
        }
    }
    let mut reached = BTreeSet::from([first]);
    let mut queue = VecDeque::from([first]);
    while let Some([x, y, z]) = queue.pop_front() {
        for [dx, dy] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            for dz in -1..=1 {
                let next = [x + dx, y + dy, z + dz];
                if floors.contains(&next) && reached.insert(next) {
                    queue.push_back(next);
                }
            }
        }
    }
    assert_eq!(reached, floors, "disconnected authored floor route");
}
fn kinds() -> Vec<PieceKind> {
    HomeForm::ALL
        .into_iter()
        .map(PieceKind::Home)
        .chain(Profession::ALL.into_iter().map(PieceKind::Workplace))
        .chain(FarmForm::ALL.into_iter().map(PieceKind::Farm))
        .chain([PieceKind::Pen, PieceKind::Center, PieceKind::Lamp])
        .chain(RoadShape::ALL.into_iter().map(PieceKind::Road))
        .collect()
}

#[test]
fn every_rotated_piece_preserves_complete_states_clearance_and_protected_claims() {
    let mut cases = 0;
    for style in VillageStyle::ALL {
        for kind in kinds() {
            let piece = build(style, kind, 97, state).unwrap();
            for turns in 0..4 {
                let cells = cells(&piece, turns);
                assert_eq!(cells.len(), piece.template.cells.len());
                for cell in &piece.template.cells {
                    let actual = &cells[&turn(cell.position, turns)];
                    match (&cell.state, actual) {
                        (Some(before), Some(after)) => {
                            assert_eq!(before.id().as_str(), after.id().as_str());
                            assert_eq!(expected_props(before, turns), *after.properties());
                        }
                        (None, None) => {}
                        _ => panic!("lost original cell {style:?}/{kind:?}/turn{turns}"),
                    }
                }
                route(
                    &cells,
                    piece
                        .walkable
                        .iter()
                        .map(|position| turn(*position, turns))
                        .collect(),
                );
                let mut markers = BTreeSet::new();
                for marker in &piece.markers {
                    let [x, y, z] = turn(marker.position, turns);
                    assert!(markers.insert([x, y, z]));
                    assert!(cells[&[x, y, z - 1]].is_some());
                    for dz in 0..2 {
                        assert!(clear(&cells[&[x, y, z + dz]]));
                    }
                }
                let top = cells
                    .iter()
                    .filter(|(_, value)| value.is_some())
                    .max_by_key(|(position, _)| position[2])
                    .map(|(position, _)| *position);
                let air = cells
                    .iter()
                    .find(|(_, value)| value.is_none())
                    .map(|(position, _)| *position);
                for blocked in [top, air].into_iter().flatten() {
                    assert!(
                        matches!(prepare(&piece, turns, Some(blocked)), Err(PlacementError::Protected(position)) if position == blocked)
                    );
                }
                for (&[x, y, z], value) in &cells {
                    let Some(value) = value else {
                        continue;
                    };
                    let id = value.id().as_str();
                    let props = value.properties();
                    let (partner, property) = if id.ends_with("_bed") {
                        assert_eq!(props["occupied"], "false");
                        let direction = CARDINAL
                            .iter()
                            .position(|direction| *direction == props["facing"])
                            .unwrap();
                        let [dx, dy] = [[0, -1], [1, 0], [0, 1], [-1, 0]][direction];
                        let step = if props["part"] == "foot" { 1 } else { -1 };
                        ([x + dx * step, y + dy * step, z], "part")
                    } else if id.ends_with("_door") {
                        for key in ["open", "powered"] {
                            assert_eq!(props[key], "false");
                        }
                        assert_eq!(props["hinge"], "left");
                        (
                            [x, y, z + if props["half"] == "lower" { 1 } else { -1 }],
                            "half",
                        )
                    } else {
                        continue;
                    };
                    let partner = cells[&partner].as_ref().expect("orphan bed/door");
                    assert_eq!(partner.id().as_str(), id);
                    assert_eq!(partner.properties()["facing"], props["facing"]);
                    assert_ne!(partner.properties()[property], props[property]);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 560);
}

#[test]
fn original_missing_porch_air_and_route_are_forced_red() {
    for style in VillageStyle::ALL {
        for kind in kinds()
            .into_iter()
            .filter(|kind| matches!(kind, PieceKind::Home(_) | PieceKind::Workplace(_)))
        {
            let piece = build(style, kind, 97, state).unwrap();
            let cells = cells(&piece, 0);
            let port = piece.ports[0];
            let [x, y, z] = port.position;
            assert_eq!(port.role, PortRole::Entrance);
            assert!(cells[&port.position]
                .as_ref()
                .unwrap()
                .id()
                .as_str()
                .ends_with("_stairs"));
            for dz in 1..=2 {
                assert!(
                    matches!(cells.get(&[x, y, z + dz]), Some(None)),
                    "porch retains terrain {style:?}/{kind:?}"
                );
            }
            assert!(piece.walkable.contains(&port.position));
            assert!(piece
                .walkable
                .contains(&[x - port.front[0], y - port.front[1], z]));
            let door = cells[&[x, y + 1, z + 1]].as_ref().unwrap();
            assert_eq!(door.properties()["open"], "false");
            assert_eq!(
                cells[&port.position].as_ref().unwrap().properties()["facing"],
                "south"
            );
        }
    }
}

#[test]
fn all_thirteen_defaults_generate_exactly_31_states_in_260_workplaces() {
    type WorkstationDefault<'a> = (Profession, &'a str, &'a [(&'a str, &'a str)]);
    let defaults: [WorkstationDefault<'_>; 13] = [
        (
            Profession::Armorer,
            "minecraft:blast_furnace",
            &[("facing", "north"), ("lit", "false")],
        ),
        (
            Profession::Butcher,
            "minecraft:smoker",
            &[("facing", "north"), ("lit", "false")],
        ),
        (Profession::Cartographer, "minecraft:cartography_table", &[]),
        (
            Profession::Cleric,
            "minecraft:brewing_stand",
            &[
                ("has_bottle_0", "false"),
                ("has_bottle_1", "false"),
                ("has_bottle_2", "false"),
            ],
        ),
        (Profession::Farmer, "minecraft:composter", &[("level", "0")]),
        (
            Profession::Fisherman,
            "minecraft:barrel",
            &[("facing", "up"), ("open", "false")],
        ),
        (Profession::Fletcher, "minecraft:fletching_table", &[]),
        (Profession::Leatherworker, "minecraft:cauldron", &[]),
        (
            Profession::Librarian,
            "minecraft:lectern",
            &[
                ("facing", "north"),
                ("has_book", "false"),
                ("powered", "false"),
            ],
        ),
        (
            Profession::Mason,
            "minecraft:stonecutter",
            &[("facing", "north")],
        ),
        (
            Profession::Shepherd,
            "minecraft:loom",
            &[("facing", "north")],
        ),
        (Profession::Toolsmith, "minecraft:smithing_table", &[]),
        (
            Profession::Weaponsmith,
            "minecraft:grindstone",
            &[("face", "floor"), ("facing", "north")],
        ),
    ];
    let mut corpus = BTreeSet::new();
    let mut workplaces = 0;
    for style in VillageStyle::ALL {
        for (job, id, props) in defaults {
            assert_eq!(job.workstation(), id);
            let piece = build(style, PieceKind::Workplace(job), 97, state).unwrap();
            assert!(piece
                .markers
                .iter()
                .any(|marker| marker.kind == MarkerKind::Resident(Some(job))));
            for turns in 0..4 {
                let cells = cells(&piece, turns);
                let actual: Vec<_> = cells
                    .values()
                    .filter_map(Option::as_ref)
                    .filter(|value| value.id().as_str() == id)
                    .collect();
                assert!(!actual.is_empty(), "missing {job:?}/{style:?}/{turns}");
                for value in actual {
                    assert_eq!(
                        *value.properties(),
                        expected_props(&state(id, props).unwrap(), turns)
                    );
                    corpus.insert((id.to_owned(), value.properties().clone()));
                }
                workplaces += 1;
            }
        }
    }
    assert_eq!(workplaces, 260);
    assert_eq!(corpus.len(), 31);
    eprintln!(
        "VILLAGE_KIT workplaces={workplaces} distinct_workstation_states={}",
        corpus.len()
    );
}

#[test]
fn farms_retain_full_dirt_support_irrigation_and_original_mature_crops() {
    for style in VillageStyle::ALL {
        let allowed: &[&str] = match style {
            VillageStyle::Plains => &["wheat", "carrots", "potatoes", "beetroots"],
            VillageStyle::Desert => &["wheat", "beetroots", "melon_stem"],
            VillageStyle::Savanna => &["wheat", "melon_stem"],
            VillageStyle::Taiga => &["wheat", "pumpkin_stem", "potatoes"],
            VillageStyle::Snowy => &["wheat", "carrots", "potatoes"],
        };
        for form in FarmForm::ALL {
            for seed in [0, 1, 97, 71839] {
                let piece = build(style, PieceKind::Farm(form), seed, state).unwrap();
                let cells = cells(&piece, 0);
                let length = if form == FarmForm::Compact { 7 } else { 13 };
                let composter = cells[&[1, 0, 1]].as_ref().unwrap();
                assert_eq!(composter.id().as_str(), "minecraft:composter");
                assert_eq!(composter.properties()["level"], "0");
                for x in 0..9 {
                    for y in 0..length {
                        assert_eq!(
                            cells[&[x, y, -1]].as_ref().unwrap().id().as_str(),
                            "minecraft:dirt"
                        );
                        if x == 0 || x == 8 || y == 0 || y == length - 1 {
                            continue;
                        }
                        let ground = cells[&[x, y, 0]].as_ref().unwrap();
                        if x == 4 {
                            assert_eq!(ground.id().as_str(), "minecraft:water");
                            assert_eq!(ground.properties()["level"], "0");
                            continue;
                        }
                        assert_eq!(ground.id().as_str(), "minecraft:farmland");
                        assert_eq!(ground.properties()["moisture"], "7");
                        let crop = cells[&[x, y, 1]].as_ref().unwrap();
                        let name = crop.id().as_str().strip_prefix("minecraft:").unwrap();
                        assert!(
                            allowed.contains(&name),
                            "changed style crop {style:?}/{name}"
                        );
                        assert_eq!(
                            crop.properties()["age"],
                            if name == "beetroots" { "3" } else { "7" }
                        );
                    }
                }
            }
        }
    }
}
