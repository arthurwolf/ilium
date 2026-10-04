//! Structural tests use actual kit states, Builder and Prepared. Only the
//! explicitly synthetic fixtures populate the terrain cache; natural tests do not.
use super::*;
use std::cell::Cell;

fn never_cancel() -> bool {
    false
}

fn orient([x, y, z]: [i32; 3], turns: u8) -> [i32; 3] {
    [[x, y, z], [-y, x, z], [-x, -y, z], [y, -x, z]][usize::from(turns)]
}

fn turned_direction(value: &str, turns: u8) -> &str {
    match value {
        "north" => ["north", "east", "south", "west"][usize::from(turns)],
        "east" => ["east", "south", "west", "north"][usize::from(turns)],
        "south" => ["south", "west", "north", "east"][usize::from(turns)],
        "west" => ["west", "north", "east", "south"][usize::from(turns)],
        _ => value,
    }
}

fn require_authored_state(actual: &Option<BlockState>, original: &Option<BlockState>, turns: u8) {
    let Some(original) = original else {
        assert!(actual.is_none(), "authored air became occupied");
        return;
    };
    let actual = actual
        .as_ref()
        .expect("authored occupied geometry was carved");
    assert_eq!(actual.id(), original.id());
    let expected: BTreeMap<_, _> = original
        .properties()
        .iter()
        .map(|(key, value)| {
            let transformed = match (key.as_str(), value.as_str(), turns % 2) {
                ("facing", value, _) => turned_direction(value, turns),
                ("axis", "x", 1) => "z",
                ("axis", "z", 1) => "x",
                _ => value.as_str(),
            };
            (
                turned_direction(key, turns).to_owned(),
                transformed.to_owned(),
            )
        })
        .collect();
    assert_eq!(
        actual.properties(),
        &expected,
        "published state lost rotation or authored properties"
    );
}

fn fixture<'a>(
    style: VillageStyle,
    fields: &'a TerrainFields,
    settings: &'a VoxelLandscapeSettings,
    cancelled: &'a dyn Fn() -> bool,
) -> Builder<'a> {
    let plaza = village_kit::build(style, PieceKind::Center, 7, state).unwrap();
    let slope = village_kit::build(style, PieceKind::Road(RoadShape::Slope), 7, state).unwrap();
    let material = |piece: &village_kit::Piece<BlockState>, at| {
        piece
            .template
            .cells
            .iter()
            .find(|cell| cell.position == at)
            .unwrap()
            .state
            .clone()
            .unwrap()
    };
    let mut ground = BTreeMap::new();
    for x in -64..=64 {
        for y in -64..=64 {
            ground.insert([x, y], Some(100));
        }
    }
    Builder {
        village: VillagePlacement {
            style,
            center: [0, 0, 100],
            writes: BTreeMap::new(),
            markers: Vec::new(),
            piece_sources: Vec::new(),
            pieces: Vec::new(),
            residential_pieces: 0,
            rejected_pieces: 0,
        },
        fields,
        settings,
        cancelled,
        ground,
        foundation: material(&plaza, [0, 0, 0]),
        path: material(&slope, [5, 0, 0]),
        stairs: material(&slope, [5, 5, 0]),
    }
}

fn plaza(builder: &mut Builder<'_>, arm: usize) -> GlobalPort {
    let piece = village_kit::build(builder.village.style, PieceKind::Center, 7, state).unwrap();
    let draft = builder
        .rigid(&piece, PieceKind::Center, [-5, -5, 100], 0)
        .unwrap()
        .unwrap();
    let owner = builder.commit(draft).unwrap().unwrap();
    port([-5, -5, 100], 0, piece.ports[arm], owner).unwrap()
}

fn snapshot(village: &VillagePlacement) -> Vec<String> {
    let mut result = vec![format!(
        "{:?}/{:?}/{}",
        village.style, village.center, village.residential_pieces
    )];
    for (position, write) in &village.writes {
        result.push(format!(
            "{position:?}/{:?}/{}/{:?}",
            write.piece_anchor,
            write.piece_source,
            write
                .state
                .as_ref()
                .map(|value| (value.id().as_str(), value.properties()))
        ));
    }
    for marker in &village.markers {
        result.push(format!("marker:{:?}/{:?}", marker.position, marker.kind));
    }
    for source in &village.piece_sources {
        result.push(format!("source:{source}"));
    }
    for piece in &village.pieces {
        result.push(format!(
            "piece:{:?}/{:?}/{}/{:?}/{:?}/{:?}/{:?}",
            piece.kind,
            piece.anchor,
            piece.rotation,
            piece.bounds,
            piece.parent,
            piece.contact,
            piece.walkable
        ));
    }
    result
}

fn require_air_or_openable(value: &Option<BlockState>) {
    let Some(value) = value else {
        return;
    };
    let id = value.id().as_str();
    assert!(
        id.ends_with("_door")
            || id.ends_with("_carpet")
            || id == "minecraft:poppy"
            || (id.ends_with("_fence_gate")
                && value.properties().get("open").map(String::as_str) == Some("true")),
        "route obstructed by {id}"
    );
}

fn require_routes(village: &VillagePlacement) {
    assert_eq!(village.pieces.len(), village.piece_sources.len());
    assert!(village.pieces.len() <= 37 && village.writes.len() <= 65_536);
    let mut floors = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for (index, piece) in village.pieces.iter().enumerate() {
        assert!(
            owners.insert(&village.piece_sources[index]),
            "duplicate complete owner source"
        );
        assert!(!piece.walkable.is_empty());
        assert!(
            village
                .writes
                .values()
                .filter(|write| write.piece_source == village.piece_sources[index])
                .count()
                <= 4096
        );
        for earlier in &village.pieces[..index] {
            assert!(
                (0..2).any(|axis| piece.bounds[1][axis] <= earlier.bounds[0][axis]
                    || earlier.bounds[1][axis] <= piece.bounds[0][axis]),
                "overlapping complete roof/air parcel"
            );
        }
        if index == 0 {
            assert!(piece.parent.is_none() && piece.contact.is_none());
        }
        if index > 0 {
            let parent = piece.parent.expect("disconnected owner");
            assert!(
                parent < index,
                "ownership graph is cyclic or forward referenced"
            );
            let [from, to] = piece.contact.expect("missing physical route contact");
            assert!(
                village.pieces[parent].walkable.contains(&from) && piece.walkable.contains(&to)
            );
            assert_eq!((from[0] - to[0]).abs() + (from[1] - to[1]).abs(), 1);
            assert!((from[2] - to[2]).abs() <= 1);
        }
        for &position in &piece.walkable {
            assert!(floors.insert(position), "floor is owned twice");
            let floor = &village.writes[&position];
            assert_eq!(floor.piece_source, village.piece_sources[index]);
            assert!(floor
                .state
                .as_ref()
                .is_some_and(|value| value.id().as_str() != "minecraft:water"));
            for dz in [1, 2] {
                require_air_or_openable(
                    &village.writes[&[position[0], position[1], position[2] + dz]].state,
                );
            }
        }
    }
    let first = *floors.first().expect("empty route graph");
    let mut reached = BTreeSet::from([first]);
    let mut pending = vec![first];
    while let Some(at) = pending.pop() {
        for [dx, dy] in [[-1, 0], [0, -1], [0, 1], [1, 0]] {
            for dz in -1..=1 {
                let neighbor = [at[0] + dx, at[1] + dy, at[2] + dz];
                if floors.contains(&neighbor) && reached.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
    }
    assert_eq!(
        reached, floors,
        "a purported walkable route does not reach the plaza"
    );
    let mut markers = BTreeSet::new();
    for marker in &village.markers {
        assert!(markers.insert(marker.position));
        assert!(village.writes[&[
            marker.position[0],
            marker.position[1],
            marker.position[2] - 1
        ]]
            .state
            .is_some());
    }
    for (position, write) in &village.writes {
        for (coordinate, center) in position.iter().zip(village.center.iter()).take(2) {
            assert!((i64::from(*coordinate) - i64::from(*center)).abs() <= 64);
        }
        let owner = village
            .piece_sources
            .iter()
            .position(|source| *source == write.piece_source)
            .unwrap();
        assert_eq!(write.piece_anchor, village.pieces[owner].anchor);
        for (axis, coordinate) in position.iter().enumerate().take(2) {
            assert!(
                (village.pieces[owner].bounds[0][axis]..village.pieces[owner].bounds[1][axis])
                    .contains(coordinate)
            );
        }
        let Some(value) = &write.state else {
            continue;
        };
        if !value.id().as_str().ends_with("_door")
            || value.properties().get("half").map(String::as_str) != Some("lower")
        {
            continue;
        }
        let above = village.writes[&[position[0], position[1], position[2] + 1]]
            .state
            .as_ref()
            .unwrap();
        assert_eq!(above.id(), value.id());
        assert_eq!(above.properties()["half"], "upper");
        for key in ["facing", "hinge", "open", "powered"] {
            assert_eq!(above.properties()[key], value.properties()[key]);
        }
        let floor = [position[0], position[1], position[2] - 1];
        assert!(floors.contains(&floor));
        let outward = match value.properties()["facing"].as_str() {
            "north" => [0, -1],
            "east" => [1, 0],
            "south" => [0, 1],
            "west" => [-1, 0],
            _ => panic!("noncardinal door"),
        };
        assert!(
            floors.contains(&[floor[0] + outward[0], floor[1] + outward[1], floor[2]]),
            "door lacks porch route"
        );
    }
}

#[test]
fn natural_owners_routes_doors_and_full_footprints() {
    let fields = TerrainFields::new(71839);
    let settings = VoxelLandscapeSettings::default();
    for (style, center, seed) in [
        (
            VillageStyle::Desert,
            [-896, -11600, 67],
            13402024533940074428,
        ),
        (
            VillageStyle::Plains,
            [11440, -12144, 104],
            5726667076090912111,
        ),
        (
            VillageStyle::Savanna,
            [416, -12160, 80],
            1573488352327643686,
        ),
        (
            VillageStyle::Snowy,
            [-11344, -12128, 99],
            13750931656163572791,
        ),
        (VillageStyle::Taiga, [2480, -12224, 98], 2630010982133005319),
    ] {
        let village = assemble(style, center, seed, &fields, &settings, || false).unwrap();
        require_routes(&village);
        let columns: BTreeSet<_> = village
            .writes
            .keys()
            .map(|position| [position[0], position[1]])
            .collect();
        for [x, y] in columns {
            let ground = fields.sample(x, y, settings.rivers);
            assert!(
                ground
                    .water_level
                    .is_none_or(|water| water <= ground.height),
                "wet written footprint"
            );
        }
        for piece in &village.pieces {
            for &floor in &piece.walkable {
                let ground = i32::from(fields.sample(floor[0], floor[1], settings.rivers).height);
                assert!(
                    (floor[2] - ground).abs() <= 2,
                    "excessive route earthwork at {floor:?}"
                );
                for z in ground..floor[2] {
                    assert!(
                        village.writes[&[floor[0], floor[1], z]].state.is_some(),
                        "floating route"
                    );
                }
            }
        }
    }
}

#[test]
fn all_styles_piece_variants_keep_published_states_air_roofs_and_support() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    let kinds: Vec<_> = HomeForm::ALL
        .into_iter()
        .map(PieceKind::Home)
        .chain(Profession::ALL.into_iter().map(PieceKind::Workplace))
        .chain(FarmForm::ALL.into_iter().map(PieceKind::Farm))
        .chain([PieceKind::Pen, PieceKind::Center])
        .collect();
    for style in VillageStyle::ALL {
        for &kind in &kinds {
            for turns in 0..4 {
                let mut builder = fixture(style, &fields, &settings, &never_cancel);
                for (xy, height) in &mut builder.ground {
                    *height = Some(100 + (xy[0] + xy[1]).rem_euclid(3));
                }
                let piece = village_kit::build(style, kind, 7, state).unwrap();
                let draft = builder
                    .rigid(&piece, kind, [0, 0, 102], turns)
                    .unwrap()
                    .unwrap();
                for cell in &piece.template.cells {
                    let [x, y, z] = orient(cell.position, turns);
                    if z > 0 || cell.state.is_none() {
                        continue;
                    }
                    for support_z in builder.ground[&[x, y]].unwrap()..102 + z {
                        let support = draft.cells[&[x, y, support_z]]
                            .as_ref()
                            .expect("unsupported authored floor");
                        assert!(
                            !support.id().as_str().ends_with("_stairs"),
                            "foundation repeats hollow stair geometry"
                        );
                    }
                }
                builder.commit(draft).unwrap().unwrap();
                for cell in &piece.template.cells {
                    let [x, y, z] = orient(cell.position, turns);
                    require_authored_state(
                        &builder.village.writes[&[x, y, 102 + z]].state,
                        &cell.state,
                        turns,
                    );
                }
                require_routes(&builder.village);
            }
        }
    }
}

#[test]
fn actual_stair_states_face_uphill_in_every_direction() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    for (arm, facing, opposite) in [
        (0, "north", "south"),
        (1, "east", "west"),
        (2, "south", "north"),
        (3, "west", "east"),
    ] {
        for sign in [-1, 1] {
            let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
            let parent = plaza(&mut builder, arm);
            let right = [parent.front[1], -parent.front[0]];
            for distance in 1..=6 {
                for side in -1..=1 {
                    builder.ground.insert(
                        [
                            parent.position[0] + parent.front[0] * distance + right[0] * side,
                            parent.position[1] + parent.front[1] * distance + right[1] * side,
                        ],
                        Some(100 + sign * ((distance - 1) / 2)),
                    );
                }
            }
            let draft = builder
                .path_draft(
                    parent,
                    6,
                    1,
                    Some(100 + 2 * sign),
                    None,
                    "fixture:stairs".into(),
                )
                .unwrap()
                .unwrap();
            let stairs: Vec<_> = draft
                .cells
                .values()
                .flatten()
                .filter(|value| value.id().as_str().ends_with("_stairs"))
                .collect();
            assert_eq!(
                stairs.len(),
                6,
                "two complete three-cell stair rows required"
            );
            for value in stairs {
                assert_eq!(
                    value.properties()["facing"],
                    if sign > 0 { facing } else { opposite }
                );
                assert_eq!(value.properties()["half"], "bottom");
                assert_eq!(value.properties()["shape"], "straight");
                assert_eq!(value.properties()["waterlogged"], "false");
            }
            for at in &draft.walkable {
                assert!(draft.cells[&[at[0], at[1], at[2] + 1]].is_none());
                assert!(draft.cells[&[at[0], at[1], at[2] + 2]].is_none());
                for z in builder.ground[&[at[0], at[1]]].unwrap()..at[2] {
                    assert!(draft.cells[&[at[0], at[1], z]].is_some());
                }
            }
            builder.commit(draft).unwrap().unwrap();
            require_routes(&builder.village);
        }
    }
}

#[test]
fn roof_only_contact_and_late_protected_air_reject_whole_owners() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    let mut builder = fixture(VillageStyle::Snowy, &fields, &settings, &never_cancel);
    let kind = PieceKind::Home(HomeForm::Courtyard);
    let piece = village_kit::build(VillageStyle::Snowy, kind, 7, state).unwrap();
    let first = builder
        .rigid(&piece, kind, [0, 0, 100], 0)
        .unwrap()
        .unwrap();
    let isolated = builder
        .rigid(&piece, kind, [12, 0, 100], 0)
        .unwrap()
        .unwrap();
    let shared: Vec<_> = first
        .cells
        .keys()
        .filter(|position| isolated.cells.contains_key(*position))
        .collect();
    assert!(!shared.is_empty());
    assert!(
        shared.iter().all(|position| position[2] >= 105),
        "fixture must collide only at roofs"
    );
    builder.commit(first).unwrap().unwrap();
    let before = snapshot(&builder.village);
    assert!(builder
        .rigid(&piece, kind, [12, 0, 100], 0)
        .unwrap()
        .is_none());
    assert_eq!(before, snapshot(&builder.village));

    let mut builder = fixture(VillageStyle::Snowy, &fields, &settings, &never_cancel);
    let draft = builder
        .rigid(&piece, kind, [0, 0, 100], 0)
        .unwrap()
        .unwrap();
    let protected = *draft
        .cells
        .iter()
        .rev()
        .find(|(_, value)| value.is_none())
        .unwrap()
        .0;
    builder.village.writes.insert(
        protected,
        VillageWrite {
            state: None,
            piece_source: "fixture:protected_air".into(),
            piece_anchor: protected,
        },
    );
    let before = snapshot(&builder.village);
    assert!(builder.commit(draft).unwrap().is_none());
    assert_eq!(
        before,
        snapshot(&builder.village),
        "late protected air leaked cells, markers or owner metadata"
    );
}

#[test]
fn ragged_dry_and_wet_parcels_have_bounded_atomic_fit() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    let kind = PieceKind::Home(HomeForm::Cottage);
    let piece = village_kit::build(VillageStyle::Plains, kind, 7, state).unwrap();
    for blocked in [Some(105), None] {
        let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
        builder.ground.insert([3, 3], blocked);
        let before = snapshot(&builder.village);
        assert!(builder
            .rigid(&piece, kind, [0, 0, 100], 0)
            .unwrap()
            .is_none());
        assert_eq!(before, snapshot(&builder.village));
    }
    for blocked in [Some(110), None] {
        let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
        let parent = plaza(&mut builder, 2);
        builder.ground.insert([0, 8], blocked);
        let before = snapshot(&builder.village);
        assert!(builder
            .path_draft(parent, 6, 1, None, None, "fixture:blocked".into())
            .unwrap()
            .is_none());
        assert_eq!(before, snapshot(&builder.village));
    }
}

#[test]
fn failed_headroom_and_cancellation_never_publish_partial_owner() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    let kind = PieceKind::Home(HomeForm::Cottage);
    let piece = village_kit::build(VillageStyle::Plains, kind, 7, state).unwrap();
    let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
    let mut draft = builder
        .rigid(&piece, kind, [0, 0, 100], 0)
        .unwrap()
        .unwrap();
    draft
        .cells
        .insert([3, -1, 102], Some(state("minecraft:stone", &[]).unwrap()));
    let before = snapshot(&builder.village);
    assert!(builder.commit(draft).is_err());
    assert_eq!(before, snapshot(&builder.village));
    let calls = Cell::new(0_usize);
    let limit = Cell::new(usize::MAX);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() >= limit.get()
    };
    let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &cancelled);
    let parent = plaza(&mut builder, 2);
    let draft = builder
        .path_draft(parent, 6, 1, None, None, "fixture:cancelled_child".into())
        .unwrap()
        .unwrap();
    let cell_count = draft.cells.len();
    calls.set(0);
    builder.commit(draft).unwrap().unwrap();
    let total = calls.get();
    assert!(total > cell_count);
    for cutoff in [1, 2, total / 2, total - 1, total] {
        limit.set(usize::MAX);
        let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &cancelled);
        let parent = plaza(&mut builder, 2);
        let draft = builder
            .path_draft(parent, 6, 1, None, None, "fixture:cancelled_child".into())
            .unwrap()
            .unwrap();
        let before = snapshot(&builder.village);
        calls.set(0);
        limit.set(cutoff);
        assert!(matches!(builder.commit(draft), Err(AssetError::Cancelled)));
        assert_eq!(
            before,
            snapshot(&builder.village),
            "publication at cancellation checkpoint {cutoff}/{total}"
        );
    }
}

#[test]
fn finite_budgets_coordinate_edges_and_public_cancellation() {
    let fields = TerrainFields::new(71839);
    let settings = VoxelLandscapeSettings::default();
    for axis in 0..3 {
        for edge in [i32::MIN, i32::MAX] {
            let mut center = [0, 0, 100];
            center[axis] = edge;
            assert!(matches!(
                assemble(VillageStyle::Plains, center, 7, &fields, &settings, || {
                    false
                }),
                Err(AssetError::InvalidMetadata(_))
            ));
        }
    }
    for (grid, gate) in [
        ([-2147483640, 0], 4),
        ([2147483631, 0], 3),
        ([0, -2147483640], 3),
        ([0, 2147483645], 6),
    ] {
        assert_eq!(
            hash2(
                71839 ^ 0x0076_696c_6c61_6765,
                i64::from(grid[0]),
                i64::from(grid[1])
            ) % 100,
            gate
        );
        assert!(
            gate < 15,
            "fixture must reach checked coordinate multiplication"
        );
        assert!(candidate(grid, &fields, &settings, || false)
            .unwrap()
            .is_none());
    }
    assert!(matches!(
        assemble(
            VillageStyle::Plains,
            [0, 0, 100],
            7,
            &fields,
            &settings,
            || true
        ),
        Err(AssetError::Cancelled)
    ));
    assert!(matches!(
        candidate([-4, -46], &fields, &settings, || true),
        Err(AssetError::Cancelled)
    ));
    let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
    let kind = PieceKind::Home(HomeForm::Cottage);
    let piece = village_kit::build(VillageStyle::Plains, kind, 7, state).unwrap();
    let mut draft = builder
        .rigid(&piece, kind, [0, 0, 100], 0)
        .unwrap()
        .unwrap();
    for x in -32..=32 {
        for y in -32..=32 {
            draft.cells.insert([x, y, 90], None);
        }
    }
    assert!(draft.cells.len() > MAX_PIECE_WRITES);
    let before = snapshot(&builder.village);
    assert!(matches!(
        builder.commit(draft),
        Err(AssetError::InvalidMetadata(_))
    ));
    assert_eq!(before, snapshot(&builder.village));
    assert!(builder.height([i32::MAX, 0]).is_err());
    assert!(builder.height([65, 0]).unwrap().is_none());
    assert_eq!(MAX_RADIUS, 64);
    assert_eq!(MAX_PIECES, 37);
    assert_eq!(MAX_PIECE_WRITES, 4096);
    assert_eq!(MAX_WRITES, 65_536);
}

#[test]
fn exhausted_owner_and_global_write_budgets_preserve_existing_publication() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    for exhaust_owners in [true, false] {
        let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
        let parent = plaza(&mut builder, 2);
        let draft = builder
            .path_draft(parent, 6, 1, None, None, "fixture:budget_child".into())
            .unwrap()
            .unwrap();
        // Inject occupied capacity after preparing a valid child. Neither
        // fixture intersects it: rejection must come from the requested budget.
        if exhaust_owners {
            while builder.village.pieces.len() < MAX_PIECES {
                builder.village.pieces.push(VillagePiece {
                    kind: PieceKind::Lamp,
                    anchor: [40, 40, 100],
                    rotation: 0,
                    bounds: [[40, 40], [41, 41]],
                    parent: Some(0),
                    contact: None,
                    walkable: Vec::new(),
                });
                builder
                    .village
                    .piece_sources
                    .push(format!("fixture:capacity/{}", builder.village.pieces.len()));
            }
        } else {
            for index in builder.village.writes.len()..MAX_WRITES {
                let at = [60, 60, index as i32];
                builder.village.writes.insert(
                    at,
                    VillageWrite {
                        state: None,
                        piece_source: "fixture:occupied_capacity".into(),
                        piece_anchor: at,
                    },
                );
            }
            assert_eq!(builder.village.writes.len(), MAX_WRITES);
        }
        let before = snapshot(&builder.village);
        assert!(
            matches!(builder.commit(draft), Err(AssetError::InvalidMetadata(reason)) if reason.contains("Budget"))
        );
        assert_eq!(before, snapshot(&builder.village));
    }
}

#[test]
fn actual_sixteen_locality_visits_keep_frozen_hashes_and_cancel() {
    let fields = TerrainFields::new(71839);
    let settings = VoxelLandscapeSettings::default();
    let expected: [(usize, u64, [i32; 2]); 16] = [
        (0, 4690014267269557743, [-2384, 1424]),
        (1, 3451775953021660114, [-2464, 1376]),
        (2, 4152179410116082454, [-2400, 1376]),
        (3, 16580920125738997008, [-2496, 1376]),
        (4, 350176288317251271, [-2384, 1344]),
        (5, 13556357462026790329, [-2480, 1456]),
        (6, 13170582480373004570, [-2464, 1392]),
        (7, 4929618528995179952, [-2496, 1440]),
        (8, 404818040792833934, [-2400, 1360]),
        (9, 9636938490169577227, [-2448, 1360]),
        (10, 6073279794940979870, [-2400, 1392]),
        (11, 8495511136774232718, [-2400, 1360]),
        (12, 9404033408162152061, [-2416, 1456]),
        (13, 1449488701090996538, [-2464, 1456]),
        (14, 1094830847167724765, [-2416, 1392]),
        (15, 14447761341723375890, [-2464, 1376]),
    ];
    let mut visits = Vec::new();
    let village = candidate_with_observer(
        [-10, 5],
        &fields,
        &settings,
        || false,
        |attempt, seed, center| visits.push((attempt, seed, center)),
    )
    .unwrap();
    assert!(village.is_none());
    assert_eq!(visits, expected);
    for &(_, _, [x, y]) in &visits {
        let sample = fields.sample(x, y, true);
        assert!(
            sample
                .water_level
                .is_some_and(|water| water > sample.height),
            "frozen wet refusal changed"
        );
    }
    let count = Cell::new(0);
    let result = candidate_with_observer(
        [-10, 5],
        &fields,
        &settings,
        || count.get() == 5,
        |_, _, _| count.set(count.get() + 1),
    );
    assert!(matches!(result, Err(AssetError::Cancelled)));
    assert_eq!(count.get(), 5);
    for density in [i32::MIN, -1, 0] {
        let settings = VoxelLandscapeSettings {
            structures_percent: density,
            ..settings.clone()
        };
        let mut count = 0;
        assert!(candidate_with_observer(
            [-10, 5],
            &fields,
            &settings,
            || false,
            |_, _, _| count += 1
        )
        .unwrap()
        .is_none());
        assert_eq!(count, 0);
    }
}

fn all_street_ports(builder: &mut Builder<'_>) -> LotAssignments {
    let first = plaza(builder, 0);
    let piece = village_kit::build(builder.village.style, PieceKind::Center, 7, state).unwrap();
    let mut queue = VecDeque::new();
    for (arm, local) in piece.ports.iter().enumerate() {
        queue.push_back(StreetTask {
            port: port([-5, -5, 100], 0, *local, first.owner).unwrap(),
            arm,
            segment: 0,
        });
    }
    let mut lots = LotAssignments {
        ports: Vec::new(),
        used_ports: [false; MAX_LOTS],
        placed: [false; MAX_LOTS],
        attempted: [[false; MAX_LOTS]; MAX_LOTS],
    };
    while let Some(task) = queue.pop_front() {
        let [next, left, right] = builder.street(task, 7).unwrap().unwrap();
        lots.ports.extend([left, right]);
        if task.segment < 2 {
            queue.push_back(StreetTask {
                port: next,
                segment: task.segment + 1,
                ..task
            });
        }
    }
    assert_eq!(lots.ports.len(), 24);
    assert_eq!(builder.village.pieces.len(), 13);
    lots
}

#[test]
fn actual_lot_matrix_is_finite_memoized_and_never_duplicates_a_demand() {
    let fields = TerrainFields::new(42);
    let settings = VoxelLandscapeSettings::default();
    let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
    let mut lots = all_street_ports(&mut builder);
    for height in builder.ground.values_mut() {
        *height = None;
    }
    let demands: Vec<_> = (0..24)
        .map(|seed| {
            builder
                .lot_demand(PieceKind::Home(HomeForm::Longhouse), seed)
                .unwrap()
        })
        .collect();
    assert!(demands.iter().all(|demand| demand.variants.len() == 2));
    let before = snapshot(&builder.village);
    for (index, demand) in demands.iter().enumerate() {
        assert!(!lots.place(index, demand, &mut builder).unwrap());
    }
    assert_eq!(
        lots.attempted
            .iter()
            .flatten()
            .filter(|tried| **tried)
            .count(),
        576
    );
    assert!(lots.used_ports.iter().all(|used| !used) && lots.placed.iter().all(|placed| !placed));
    assert_eq!(before, snapshot(&builder.village));
    let rejected = builder.village.rejected_pieces;
    for _ in 0..2 {
        for (index, demand) in demands.iter().enumerate() {
            assert!(!lots.place(index, demand, &mut builder).unwrap());
        }
    }
    assert_eq!(
        builder.village.rejected_pieces, rejected,
        "a failed pair was fitted again"
    );
    assert_eq!(
        lots.attempted
            .iter()
            .flatten()
            .filter(|tried| **tried)
            .count(),
        576
    );
    assert_eq!(before, snapshot(&builder.village));

    let mut builder = fixture(VillageStyle::Plains, &fields, &settings, &never_cancel);
    let mut lots = all_street_ports(&mut builder);
    let demand = builder
        .lot_demand(PieceKind::Home(HomeForm::Cottage), 97)
        .unwrap();
    assert!(lots.place(0, &demand, &mut builder).unwrap());
    let before = snapshot(&builder.village);
    assert!(!lots.place(0, &demand, &mut builder).unwrap());
    assert_eq!(before, snapshot(&builder.village));
    assert_eq!(lots.used_ports.iter().filter(|used| **used).count(), 1);
    assert_eq!(lots.placed.iter().filter(|placed| **placed).count(), 1);
    require_routes(&builder.village);
}

#[test]
fn admitted_grid_density_changes_preserve_source_state_and_marker_identity() {
    let fields = TerrainFields::new(71839);
    let settings = VoxelLandscapeSettings::default();
    let expected = candidate([9, -48], &fields, &settings, || false)
        .unwrap()
        .unwrap();
    for density in [200, i32::MAX] {
        let changed = VoxelLandscapeSettings {
            structures_percent: density,
            ..settings.clone()
        };
        let actual = candidate([9, -48], &fields, &changed, || false)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot(&expected), snapshot(&actual));
    }
}
