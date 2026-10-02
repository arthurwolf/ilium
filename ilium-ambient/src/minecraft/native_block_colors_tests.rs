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
fn every_registered_handler_has_its_native_provider() {
    let place = [17, 65, -9];
    for name in ["large_fern", "tall_grass"] {
        assert_eq!(
            dispatch(
                &state(&format!("minecraft:{name}"), &[("half", "upper")]),
                place,
                3
            ),
            Ok(BlockColor::Biome {
                kind: ColorKind::Grass,
                java_position: [17, 64, -9]
            })
        );
    }
    for name in ["grass_block", "fern", "grass", "potted_fern", "sugar_cane"] {
        assert_eq!(
            dispatch(&state(&format!("minecraft:{name}"), &[]), place, 0),
            Ok(BlockColor::Biome {
                kind: ColorKind::Grass,
                java_position: place
            })
        );
    }
    for name in [
        "oak_leaves",
        "jungle_leaves",
        "acacia_leaves",
        "dark_oak_leaves",
        "mangrove_leaves",
        "vine",
    ] {
        assert_eq!(
            dispatch(&state(&format!("minecraft:{name}"), &[]), place, 0),
            Ok(BlockColor::Biome {
                kind: ColorKind::Foliage,
                java_position: place
            })
        );
    }
    for name in ["water", "bubble_column", "water_cauldron"] {
        assert_eq!(
            dispatch(&state(&format!("minecraft:{name}"), &[]), place, 0),
            Ok(BlockColor::Biome {
                kind: ColorKind::Water,
                java_position: place
            })
        );
    }
    assert_eq!(
        dispatch(&state("minecraft:spruce_leaves", &[]), place, 0),
        Ok(BlockColor::Fixed(FoliageConstant::Evergreen.rgb()))
    );
    assert_eq!(
        dispatch(&state("minecraft:birch_leaves", &[]), place, 0),
        Ok(BlockColor::Fixed(FoliageConstant::Birch.rgb()))
    );
    assert_eq!(
        dispatch(&state("minecraft:lily_pad", &[]), place, 0),
        Ok(BlockColor::Fixed([0x20, 0x80, 0x30]))
    );
}

#[test]
fn property_colors_keep_native_integer_and_float_rules() {
    let place = [0, 64, 0];
    for age in 0..=7 {
        let value = age.to_string();
        let expected = BlockColor::Fixed([age * 32, 255 - age * 8, age * 4]);
        for name in ["melon_stem", "pumpkin_stem"] {
            assert_eq!(
                dispatch(
                    &state(&format!("minecraft:{name}"), &[("age", &value)]),
                    place,
                    0
                ),
                Ok(expected)
            );
        }
    }
    for name in ["attached_melon_stem", "attached_pumpkin_stem"] {
        assert_eq!(
            dispatch(&state(&format!("minecraft:{name}"), &[]), place, 0),
            Ok(BlockColor::Fixed([0xe0, 0xc7, 0x1c]))
        );
    }
    assert_eq!(
        dispatch(
            &state("minecraft:redstone_wire", &[("power", "0")]),
            place,
            0
        ),
        Ok(BlockColor::Fixed([76, 0, 0]))
    );
    assert_eq!(
        dispatch(
            &state("minecraft:redstone_wire", &[("power", "15")]),
            place,
            0
        ),
        Ok(BlockColor::Fixed([255, 50, 0]))
    );
}

#[test]
fn malformed_required_properties_and_unknown_namespaces_do_not_claim_native_color() {
    let place = [0, 64, 0];
    for (name, key, invalid) in [
        ("redstone_wire", "power", "16"),
        ("redstone_wire", "power", "01"),
        ("melon_stem", "age", "8"),
        ("pumpkin_stem", "age", "-1"),
        ("large_fern", "half", "top"),
    ] {
        assert_eq!(
            dispatch(
                &state(&format!("minecraft:{name}"), &[(key, invalid)]),
                place,
                0
            ),
            Err(Error::Property(key))
        );
    }
    assert_eq!(
        dispatch(&state("minecraft:tall_grass", &[]), place, 0),
        Err(Error::Property("half"))
    );
    assert_eq!(
        dispatch(&state("example:grass_block", &[]), place, 0),
        Err(Error::UnknownNamespace)
    );
    assert_eq!(
        dispatch(&state("minecraft:stone", &[]), place, 0),
        Ok(BlockColor::NoHandler)
    );
    assert_eq!(
        dispatch(
            &state("minecraft:tall_grass", &[("half", "upper")]),
            [0, i32::MIN, 0],
            0
        ),
        Err(Error::Position)
    );
}
