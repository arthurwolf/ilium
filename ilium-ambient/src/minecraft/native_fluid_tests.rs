use super::*;
use std::cell::Cell as CountCell;
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
fn saved_flowing_levels_preserve_kind_and_exact_position() {
    let java_position = [-17, -64, 1041];
    for (name, kind) in [
        ("minecraft:water", fluids::Kind::Water),
        ("minecraft:lava", fluids::Kind::Lava),
    ] {
        for level in 0..=15 {
            let cell = classify_state(
                &state(name, &[("level", &level.to_string())]),
                java_position,
            )
            .unwrap()
            .unwrap();
            assert_eq!(cell.java_position, java_position);
            assert_eq!(cell.kind, kind);
            assert_eq!(
                cell.amount,
                if level == 0 || level >= 8 {
                    8
                } else {
                    8 - level
                }
            );
            assert_eq!(cell.falling, level >= 8);
            assert!(!cell.waterlogged);
        }
    }
}

#[test]
fn captured_waterlogged_glow_lichen_keeps_solid_and_source_water() {
    let position = [0, 62, 0];
    let cell = classify_state(
        &state(
            "minecraft:glow_lichen",
            &[("north", "true"), ("waterlogged", "true")],
        ),
        position,
    )
    .unwrap()
    .unwrap();
    assert_eq!(cell.java_position, position);
    assert_eq!(cell.kind, fluids::Kind::Water);
    assert_eq!(cell.amount, 8);
    assert!(cell.waterlogged);
    assert!(!cell.falling);

    // This oak slab name has no captured behavior in this list; it cannot
    // inherit native getFluidState merely because its NBT says waterlogged=true.
    let error = classify_state(
        &state(
            "minecraft:oak_slab",
            &[("type", "bottom"), ("waterlogged", "true")],
        ),
        position,
    )
    .unwrap_err();
    assert!(matches!(error, Error::WaterloggedBehavior(p) if p == position));
    assert!(classify_state(&state("minecraft:stone", &[]), position)
        .unwrap()
        .is_none());
}

#[test]
fn pinned_sprite_members_and_render_layers_are_explicit() {
    let water = native_sprites(fluids::Kind::Water).unwrap();
    assert_eq!(water.still.as_str(), "minecraft:block/water_still");
    assert_eq!(water.flowing.as_str(), "minecraft:block/water_flow");
    assert_eq!(
        water.overlay.as_ref().map(ResourceId::as_str),
        Some("minecraft:block/water_overlay")
    );
    assert_eq!(water.alpha, AlphaMode::NativeBlend);
    let lava = native_sprites(fluids::Kind::Lava).unwrap();
    assert_eq!(lava.still.as_str(), "minecraft:block/lava_still");
    assert_eq!(lava.flowing.as_str(), "minecraft:block/lava_flow");
    assert!(lava.overlay.is_none());
    assert_eq!(lava.alpha, AlphaMode::NativeSolid);
}

#[test]
fn pinned_top_and_side_positions_preserve_corner_and_inset_order() {
    let corners = [0.2_f32, 0.4, 0.6, 0.8];
    let top = native_top_points(corners);
    assert_eq!(top[0], [0.0, 0.0, f64::from(corners[0] - 0.001_f32)]);
    assert_eq!(top[1], [0.0, 1.0, f64::from(corners[1] - 0.001_f32)]);
    assert_eq!(top[2], [1.0, 1.0, f64::from(corners[2] - 0.001_f32)]);
    assert_eq!(top[3], [1.0, 0.0, f64::from(corners[3] - 0.001_f32)]);
    let inset = f64::from(0.001_f32);
    assert_eq!(
        native_side_points(HorizontalFace::North, corners, false, false),
        [
            [0.0, inset, f64::from(corners[0])],
            [1.0, inset, f64::from(corners[3])],
            [1.0, inset, 0.0],
            [0.0, inset, 0.0],
        ]
    );
    assert_eq!(
        native_side_points(HorizontalFace::East, corners, false, true)[3],
        [1.0 - inset, 0.0, inset]
    );
    assert_eq!(
        native_side_points(HorizontalFace::North, corners, true, false)[0][2],
        f64::from(corners[0] - 0.001_f32)
    );
    assert_eq!(
        native_side_points(HorizontalFace::North, corners, false, false)[0][2],
        f64::from(corners[0])
    );
}

#[test]
fn native_face_culling_uses_same_fluid_and_state_shape_not_texture_alpha() {
    let full = Shape::block();
    let empty = Shape::empty();
    let no_occlusion = BlockOcclusion {
        can_occlude: false,
        shape: &empty,
    };
    let full_occlusion = BlockOcclusion {
        can_occlude: true,
        shape: &full,
    };
    let water = Cell {
        java_position: [0, 64, 0],
        kind: fluids::Kind::Water,
        amount: 8,
        falling: false,
        waterlogged: false,
    };
    assert!(!native_face_visible(
        fluids::Kind::Water,
        Direction::Up,
        8.0 / 9.0,
        no_occlusion,
        full_occlusion,
        Some(water),
    )
    .unwrap());
    assert!(!native_face_visible(
        fluids::Kind::Water,
        Direction::Up,
        1.0,
        no_occlusion,
        full_occlusion,
        None,
    )
    .unwrap());
    assert!(!native_face_visible(
        fluids::Kind::Water,
        Direction::East,
        8.0 / 9.0,
        no_occlusion,
        full_occlusion,
        None,
    )
    .unwrap());
    assert!(native_face_visible(
        fluids::Kind::Water,
        Direction::East,
        8.0 / 9.0,
        no_occlusion,
        no_occlusion,
        None,
    )
    .unwrap());
    assert!(!native_face_visible(
        fluids::Kind::Water,
        Direction::East,
        1.0,
        full_occlusion,
        no_occlusion,
        None,
    )
    .unwrap());
}

#[test]
fn malformed_liquid_does_not_create_renderable_water() {
    let position = [1, 70, 2];
    for properties in [vec![], vec![("level", "16")], vec![("level", "00")]] {
        assert!(matches!(
            classify_state(&state("minecraft:water", &properties), position),
            Err(Error::InvalidState { position: found, .. }) if found == position
        ));
    }
}

#[test]
fn pinned_native_height_and_conditional_weighted_corner_match_source_order() {
    let position = [0, 63, 0];
    let source = classify_state(&state("minecraft:water", &[("level", "0")]), position)
        .unwrap()
        .unwrap();
    assert_eq!(
        native_height(fluids::Kind::Water, Some(source), false, false),
        8.0_f32 / 9.0
    );
    assert_eq!(
        native_height(fluids::Kind::Water, Some(source), true, false),
        1.0
    );
    assert_eq!(
        native_height(fluids::Kind::Lava, Some(source), false, false),
        0.0
    );
    assert_eq!(
        native_height(fluids::Kind::Lava, Some(source), false, true),
        -1.0
    );
    let diagonal_calls = CountCell::new(0);
    let skipped = native_corner_height(8.0 / 9.0, 0.0, -1.0, || {
        diagonal_calls.set(diagonal_calls.get() + 1);
        1.0
    })
    .unwrap();
    assert_eq!(diagonal_calls.get(), 0);
    assert_eq!(skipped, (8.0_f32 / 9.0 * 10.0) / 11.0);
    let corner = native_corner_height(8.0 / 9.0, 0.5, -1.0, || {
        diagonal_calls.set(diagonal_calls.get() + 1);
        0.9
    })
    .unwrap();
    assert_eq!(diagonal_calls.get(), 1);
    assert_eq!(
        corner,
        (((0.9_f32 * 10.0) + ((8.0_f32 / 9.0) * 10.0)) + 0.5) / 21.0
    );
    assert_eq!(
        native_corner_height(0.2, 1.0, 0.2, || panic!("no diagonal")).unwrap(),
        1.0
    );
    let samples = [[0.2, 0.3, 0.4], [0.5, 8.0 / 9.0, 0.6], [0.7, 0.8, 0.9]];
    let corners = native_corners(samples).unwrap();
    assert_eq!(
        corners[0],
        native_corner_height(samples[1][1], 0.3, 0.5, || 0.2).unwrap()
    );
    assert_eq!(
        corners[1],
        native_corner_height(samples[1][1], 0.8, 0.5, || 0.7).unwrap()
    );
    assert_eq!(
        corners[2],
        native_corner_height(samples[1][1], 0.8, 0.6, || 0.9).unwrap()
    );
    assert_eq!(
        corners[3],
        native_corner_height(samples[1][1], 0.3, 0.6, || 0.4).unwrap()
    );
    assert_eq!(
        native_corners([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0; 3]]).unwrap(),
        [1.0; 4]
    );
}

#[test]
fn pinned_stitched_atlas_uv_uses_atlas_size_and_java_float_steps() {
    let sprite = AtlasSprite::new([512, 256], [16, 32], [64, 32]).unwrap();
    assert_eq!(sprite.uv([8.0, 8.0]), [0.140625, 0.1875]);
    assert_eq!(
        sprite.uv([1.0 + 2.0_f64.powi(-30), 8.0]),
        sprite.uv([1.0, 8.0])
    );
    // 4 / max(512, 256), never 4 / frame width (16).
    assert_eq!(sprite.shrink_ratio(), 1.0 / 128.0);
    let vertices = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let reduced = sprite.shrink_top(vertices);
    assert_eq!(reduced[0], [1.0 / 256.0, 1.0 / 256.0]);
    assert_eq!(reduced[2], [1.0 - 1.0 / 256.0, 1.0 - 1.0 / 256.0]);
    assert!(matches!(
        AtlasSprite::new([16, 16], [17, 1], [0, 0]),
        Err(Error::Atlas)
    ));
    assert!(matches!(
        AtlasSprite::new([16, 16], [1, 1], [16, 0]),
        Err(Error::Atlas)
    ));
}

#[test]
fn native_backward_top_check_uses_same_y_fluid_and_solid_render() {
    let water = Some(fluids::Kind::Water);
    let same = [[(water, false); 3]; 3];
    assert!(!native_backward_up_face(fluids::Kind::Water, same));
    let mut different_but_solid = same;
    different_but_solid[0][0] = (None, true);
    assert!(!native_backward_up_face(
        fluids::Kind::Water,
        different_but_solid
    ));
    different_but_solid[0][0] = (Some(fluids::Kind::Lava), false);
    assert!(native_backward_up_face(
        fluids::Kind::Water,
        different_but_solid
    ));
}

#[test]
fn native_light_combines_actual_world_lanes_independently() {
    assert_eq!(
        native_world_packed_light(15, 3, 7, false).unwrap(),
        0xF00070
    );
    assert_eq!(native_world_packed_light(0, 0, 0, true).unwrap(), 0xF000F0);
    assert!(matches!(
        native_world_packed_light(16, 0, 0, false),
        Err(Error::Light)
    ));
    let here = native_world_packed_light(15, 3, 3, false).unwrap();
    let above = native_world_packed_light(1, 8, 2, false).unwrap();
    assert_eq!(native_fluid_packed_light(here, above), 0xF00080);
}

#[test]
fn source_top_and_side_uvs_keep_face_order_and_atlas_shrink() {
    let sprite = AtlasSprite::new([512, 256], [16, 32], [64, 32]).unwrap();
    let still = sprite.still_top_uv();
    assert_eq!(
        still[0],
        [0.125 + 0.000_122_070_31, 0.125 + 0.000_488_281_25]
    );
    assert_eq!(
        still[2],
        [0.15625 - 0.000_122_070_31, 0.25 - 0.000_488_281_25]
    );
    let flow = sprite.flowing_top_uv(0.0, 0.25).unwrap();
    assert_eq!(flow[0][0], flow[1][0]);
    assert_eq!(flow[2][0], flow[3][0]);
    assert_eq!(flow[0][1], flow[3][1]);
    assert_eq!(flow[1][1], flow[2][1]);
    assert!(matches!(
        sprite.flowing_top_uv(f32::NAN, 0.0),
        Err(Error::Atlas)
    ));
    let side = sprite.side_uv(HorizontalFace::North, [0.5, 0.2, 0.3, 1.0]);
    assert_eq!(side[0], sprite.uv([0.0, 4.0]));
    assert_eq!(side[1], sprite.uv([8.0, 0.0]));
    assert_eq!(side[2], sprite.uv([8.0, 8.0]));
}

#[test]
fn captured_projected_palette_waterlogging_preserves_source_water_and_dry_state() {
    // Pinned Java 1.19.3 getFluidState implementations return source water.
    for name in [
        "minecraft:big_dripleaf",
        "minecraft:big_dripleaf_stem",
        "minecraft:dark_oak_fence",
        "minecraft:dark_oak_slab",
        "minecraft:dark_oak_stairs",
        "minecraft:dark_oak_trapdoor",
        "minecraft:jungle_fence",
        "minecraft:mossy_stone_brick_slab",
        "minecraft:mossy_stone_brick_stairs",
        "minecraft:pointed_dripstone",
        "minecraft:spruce_slab",
        "minecraft:stone_brick_slab",
        "minecraft:stone_brick_stairs",
    ] {
        let position = [188, -3, -23];
        let cell = classify_state(&state(name, &[("waterlogged", "true")]), position)
            .unwrap()
            .unwrap();
        assert_eq!(cell.java_position, position);
        assert_eq!(cell.kind, fluids::Kind::Water);
        assert_eq!(cell.amount, 8);
        assert!(cell.waterlogged);
        assert!(!cell.falling);
        assert!(
            classify_state(&state(name, &[("waterlogged", "false")]), position)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn captured_small_dripleaf_waterlogging_preserves_both_halves_and_facings() {
    // Pinned SmallDripleafBlock.getFluidState returns source water when wet.
    for half in ["lower", "upper"] {
        for facing in ["north", "south", "east", "west"] {
            let position = [187, -3, -22];
            let wet = state(
                "minecraft:small_dripleaf",
                &[("waterlogged", "true"), ("half", half), ("facing", facing)],
            );
            let cell = classify_state(&wet, position).unwrap().unwrap();
            assert_eq!(cell.java_position, position);
            assert_eq!(cell.kind, fluids::Kind::Water);
            assert_eq!(cell.amount, 8);
            assert!(cell.waterlogged);
            assert!(!cell.falling);
            let dry = state(
                "minecraft:small_dripleaf",
                &[("waterlogged", "false"), ("half", half), ("facing", facing)],
            );
            assert!(classify_state(&dry, position).unwrap().is_none());
        }
    }
}
