use super::*;
use std::collections::BTreeMap;

fn state(name: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        name: name.into(),
        properties: properties
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn actual_blue_head_and_purple_foot_use_distinct_native_model_parts() {
    let head = recipe(&state(
        "minecraft:blue_bed",
        &[("facing", "south"), ("part", "head"), ("occupied", "false")],
    ))
    .unwrap()
    .unwrap();
    let foot = recipe(&state(
        "minecraft:purple_bed",
        &[("facing", "west"), ("part", "foot"), ("occupied", "true")],
    ))
    .unwrap()
    .unwrap();
    assert_eq!(head.kind, BuiltinKind::Bed(BedPart::Head));
    assert_eq!(head.atlas.as_str(), "minecraft:entity/bed/blue");
    assert_eq!(head.cubes[0].uv_origin, [0, 0]);
    assert_eq!(head.cubes[1].uv_origin, [50, 6]);
    assert_eq!(head.cubes[2].uv_origin, [50, 18]);
    assert_eq!(head.cubes[2].origin, [-16.0, 6.0, 0.0]);
    assert_eq!(
        head.transform,
        Transform::Bed {
            facing_degrees: 180.0
        }
    );
    assert_eq!(foot.kind, BuiltinKind::Bed(BedPart::Foot));
    assert_eq!(foot.atlas.as_str(), "minecraft:entity/bed/purple");
    assert_eq!(foot.cubes[0].uv_origin, [0, 22]);
    assert_eq!(foot.cubes[1].uv_origin, [50, 0]);
    assert_eq!(foot.cubes[2].uv_origin, [50, 12]);
    assert_eq!(foot.cubes[2].pose_rotation, [1.5707964, 0.0, 4.712389]);
    assert_eq!(
        foot.transform,
        Transform::Bed {
            facing_degrees: 270.0
        }
    );
}

#[test]
fn all_three_native_chest_factories_keep_distinct_cube_widths_and_atlases() {
    for (part, kind, atlas, x, width, lock_x, lock_width) in [
        (
            "single",
            ChestPart::Single,
            "minecraft:entity/chest/normal",
            1.0,
            14.0,
            7.0,
            2.0,
        ),
        (
            "left",
            ChestPart::Left,
            "minecraft:entity/chest/normal_left",
            0.0,
            15.0,
            0.0,
            1.0,
        ),
        (
            "right",
            ChestPart::Right,
            "minecraft:entity/chest/normal_right",
            1.0,
            15.0,
            15.0,
            1.0,
        ),
    ] {
        let recipe = chest(&state(
            "minecraft:chest",
            &[("facing", "east"), ("type", part), ("waterlogged", "false")],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(recipe.kind, BuiltinKind::Chest(kind));
        assert_eq!(recipe.atlas.as_str(), atlas);
        assert_eq!(recipe.atlas_size, [64, 64]);
        assert_eq!(recipe.cubes[0].uv_origin, [0, 19]);
        assert_eq!(recipe.cubes[0].origin, [x, 0.0, 1.0]);
        assert_eq!(recipe.cubes[0].size, [width, 10.0, 14.0]);
        assert_eq!(recipe.cubes[1].pose_translation, [0.0, 9.0, 1.0]);
        assert_eq!(recipe.cubes[2].origin, [lock_x, -2.0, 14.0]);
        assert_eq!(recipe.cubes[2].size, [lock_width, 4.0, 1.0]);
        assert_eq!(
            recipe.transform,
            Transform::Chest {
                facing_degrees: -270.0,
                openness: 0.0
            }
        );
    }
}

#[test]
fn absent_or_invalid_native_builtin_states_are_not_turned_into_invented_boxes() {
    assert!(recipe(&state("minecraft:stone", &[])).unwrap().is_none());
    assert!(recipe(&state("minecraft:cyan_bed", &[("part", "head")])).is_err());
    assert!(recipe(&state("minecraft:chest", &[("type", "single")])).is_err());
    assert!(recipe(&state(
        "minecraft:chest",
        &[
            ("facing", "north"),
            ("type", "invented"),
            ("waterlogged", "false")
        ],
    ))
    .is_err());
}

#[test]
fn chest_bake_uses_modelpart_face_order_uvs_and_part_owners() {
    let recipe = chest(&state(
        "minecraft:chest",
        &[
            ("facing", "south"),
            ("type", "single"),
            ("waterlogged", "false"),
        ],
    ))
    .unwrap()
    .unwrap();
    let faces = bake_faces(&recipe);
    assert_eq!(faces.len(), 18);
    let east = &faces[0];
    assert_eq!((east.part, east.face), (0, 0));
    assert_eq!(east.points[0], [15.0 / 16.0, 15.0 / 16.0, 0.0]);
    assert_eq!(east.points[1], [15.0 / 16.0, 1.0 / 16.0, 0.0]);
    assert_eq!(east.uv[0], [42.0 / 64.0, 33.0 / 64.0]);
    assert_eq!(east.uv[2], [28.0 / 64.0, 43.0 / 64.0]);
    assert_eq!(east.normal, [1.0, 0.0, 0.0]);
    assert_eq!((faces[6].part, faces[6].face), (1, 0));
    assert_eq!((faces[12].part, faces[12].face), (2, 0));
    assert_eq!(faces[6].points[0][2], 9.0 / 16.0);
}

#[test]
fn bed_head_and_foot_bake_keep_distinct_atlas_regions_and_poses() {
    let head = bed(&state(
        "minecraft:blue_bed",
        &[("facing", "south"), ("part", "head"), ("occupied", "false")],
    ))
    .unwrap()
    .unwrap();
    let foot = bed(&state(
        "minecraft:purple_bed",
        &[("facing", "south"), ("part", "foot"), ("occupied", "false")],
    ))
    .unwrap()
    .unwrap();
    let head_faces = bake_faces(&head);
    let foot_faces = bake_faces(&foot);
    assert_eq!(head_faces.len(), 18);
    assert_eq!(foot_faces.len(), 18);
    assert_ne!(head_faces[0].uv, foot_faces[0].uv);
    assert_ne!(head_faces[6].points, foot_faces[6].points);
    // Both saved-world branches pass `false` to BedRenderer.renderPiece;
    // the Java Z=-1 foot displacement belongs only to the world-null preview.
    assert_eq!(head_faces[0].points[0][2], foot_faces[0].points[0][2]);
    assert!(head_faces
        .iter()
        .chain(foot_faces.iter())
        .flat_map(|face| face.points)
        .flatten()
        .all(f64::is_finite));
}

#[test]
fn saved_world_bed_head_and_foot_main_pose_matches_for_every_facing() {
    for facing in ["north", "south", "east", "west"] {
        let main = |part| {
            bake_faces(
                &bed(&state(
                    "minecraft:red_bed",
                    &[("facing", facing), ("part", part), ("occupied", "false")],
                ))
                .unwrap()
                .unwrap(),
            )
        };
        let head = main("head");
        let foot = main("foot");
        for face in 0..6 {
            assert_eq!(head[face].points, foot[face].points);
        }
    }
}

#[test]
fn bell_body_adds_exact_zero_ringing_parts_to_json_stand() {
    let bell = bell_body(&state(
        "minecraft:bell",
        &[
            ("facing", "north"),
            ("attachment", "floor"),
            ("powered", "false"),
        ],
    ))
    .unwrap()
    .unwrap();
    assert!(recipe(&state(
        "minecraft:bell",
        &[
            ("facing", "north"),
            ("attachment", "floor"),
            ("powered", "false"),
        ],
    ))
    .unwrap()
    .is_none());
    assert_eq!(bell.atlas.as_str(), "minecraft:entity/bell/bell_body");
    assert_eq!(bell.atlas_size, [32, 32]);
    assert_eq!(bell.cubes[0].uv_origin, [0, 0]);
    assert_eq!(bell.cubes[1].uv_origin, [0, 13]);
    let faces = bake_bell_faces(&bell);
    assert_eq!(faces.len(), 12);
    for (part, expected_min, expected_max) in [
        (0, [5.0, 5.0, 6.0], [11.0, 11.0, 13.0]),
        (1, [4.0, 4.0, 4.0], [12.0, 12.0, 6.0]),
    ] {
        let points = faces
            .iter()
            .filter(|face| face.part == part)
            .flat_map(|face| face.points)
            .collect::<Vec<_>>();
        assert_eq!(points.len(), 24);
        for axis in 0..3 {
            assert_eq!(
                points.iter().map(|point| point[axis]).reduce(f64::min),
                Some(expected_min[axis] / 16.0)
            );
            assert_eq!(
                points.iter().map(|point| point[axis]).reduce(f64::max),
                Some(expected_max[axis] / 16.0)
            );
        }
    }
}
