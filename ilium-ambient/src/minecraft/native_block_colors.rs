//! Java 1.19.3 `BlockColors.createDefault` dispatch for an exact saved state.
//!
//! This module chooses a native provider. A `Biome` result still needs the
//! seeded biome lookup and source-backed color calculation before it is paintable.
//! The render overload passes `tint_index` to a handler, but all eleven captured
//! vanilla handlers ignore it. Model tint-index validation remains in the binder.
use super::{
    chunk::BlockState,
    native_tint::{ColorKind, FoliageConstant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockColor {
    /// Native render overload returns -1 when it has no registered handler.
    NoHandler,
    Fixed([u8; 3]),
    Biome {
        kind: ColorKind,
        java_position: [i32; 3],
    },
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("a non-vanilla block needs an independently verified color provider")]
    UnknownNamespace,
    #[error("native color handler requires a canonical saved property: {0}")]
    Property(&'static str),
    #[error("upper double-plant tint position overflows the saved coordinate")]
    Position,
}

fn decimal_property(state: &BlockState, name: &'static str, maximum: u8) -> Result<u8, Error> {
    let value = state.properties.get(name).ok_or(Error::Property(name))?;
    if value.is_empty()
        || value.len() > 2
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Property(name));
    }
    let parsed = value.parse::<u8>().map_err(|_| Error::Property(name))?;
    if parsed > maximum {
        return Err(Error::Property(name));
    }
    Ok(parsed)
}

fn redstone_rgb(power: u8) -> [u8; 3] {
    let f = f32::from(power) / 15.0_f32;
    let red = f * 0.6_f32 + if f > 0.0 { 0.4_f32 } else { 0.3_f32 };
    let green = (f * f * 0.7_f32 - 0.5_f32).clamp(0.0, 1.0);
    let blue = (f * f * 0.6_f32 - 0.7_f32).clamp(0.0, 1.0);
    // Native Mth.color casts each Vec3 component back to float before packing.
    [red, green, blue].map(|channel| (channel * 255.0_f32) as u8)
}

/// `tint_index` is deliberately ignored here by all captured vanilla handlers.
/// Returning `NoHandler` does not bless a custom resource or unknown model tint.
pub fn dispatch(
    state: &BlockState,
    java_position: [i32; 3],
    _tint_index: u16,
) -> Result<BlockColor, Error> {
    if !state.name.starts_with("minecraft:") {
        return Err(Error::UnknownNamespace);
    }
    let biome = |kind, java_position| BlockColor::Biome {
        kind,
        java_position,
    };
    Ok(match state.name.as_str() {
        "minecraft:large_fern" | "minecraft:tall_grass" => {
            let position = match state.properties.get("half").map(String::as_str) {
                Some("lower") => java_position,
                Some("upper") => [
                    java_position[0],
                    java_position[1].checked_sub(1).ok_or(Error::Position)?,
                    java_position[2],
                ],
                _ => return Err(Error::Property("half")),
            };
            biome(ColorKind::Grass, position)
        }
        "minecraft:grass_block"
        | "minecraft:fern"
        | "minecraft:grass"
        | "minecraft:potted_fern"
        | "minecraft:sugar_cane" => biome(ColorKind::Grass, java_position),
        "minecraft:spruce_leaves" => BlockColor::Fixed(FoliageConstant::Evergreen.rgb()),
        "minecraft:birch_leaves" => BlockColor::Fixed(FoliageConstant::Birch.rgb()),
        "minecraft:oak_leaves"
        | "minecraft:jungle_leaves"
        | "minecraft:acacia_leaves"
        | "minecraft:dark_oak_leaves"
        | "minecraft:mangrove_leaves"
        | "minecraft:vine" => biome(ColorKind::Foliage, java_position),
        "minecraft:water" | "minecraft:bubble_column" | "minecraft:water_cauldron" => {
            biome(ColorKind::Water, java_position)
        }
        "minecraft:redstone_wire" => {
            BlockColor::Fixed(redstone_rgb(decimal_property(state, "power", 15)?))
        }
        "minecraft:attached_melon_stem" | "minecraft:attached_pumpkin_stem" => {
            BlockColor::Fixed([0xe0, 0xc7, 0x1c])
        }
        "minecraft:melon_stem" | "minecraft:pumpkin_stem" => {
            let age = decimal_property(state, "age", 7)?;
            BlockColor::Fixed([age * 32, 255 - age * 8, age * 4])
        }
        "minecraft:lily_pad" => BlockColor::Fixed([0x20, 0x80, 0x30]),
        _ => BlockColor::NoHandler,
    })
}

#[cfg(test)]
#[path = "native_block_colors_tests.rs"]
mod tests;
