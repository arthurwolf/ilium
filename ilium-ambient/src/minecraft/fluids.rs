//! Saved liquid state decoding; never generates fluid or drops a solid owner.
//! Native Java1.19.3 LiquidBlock/FlowingFluid semantics were verified from the
//! installed game bytecode. Geometry, neighbour samples and biome tint remain
//! the renderer's responsibilities; a waterlogged flag needs its solid model.
use super::chunk::BlockState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Water,
    Lava,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liquid {
    kind: Kind,
    amount: u8,
    falling: bool,
}
impl Liquid {
    pub fn kind(self) -> Kind {
        self.kind
    }
    pub fn amount(self) -> u8 {
        self.amount
    }
    pub fn falling(self) -> bool {
        self.falling
    }
    pub fn is_source(self) -> bool {
        self.amount == 8 && !self.falling
    }
    pub fn own_height(self) -> f32 {
        f32::from(self.amount) / 9.0
    }
    pub fn height(self, same_kind_above: bool) -> f32 {
        if same_kind_above {
            1.0
        } else {
            self.own_height()
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Empty,
    Liquid(Liquid),
    /// Retain the exact solid state. The renderer must verify the native block
    /// behaviour and build co-occupying water geometry, not replace the solid.
    Waterlogged,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("saved liquid requires its exact canonical level in0..=15")]
    Level,
    #[error("saved liquid has unsupported properties")]
    Properties,
    #[error("saved waterlogged property must be true or false")]
    Waterlogged,
}

/// This parser accepts canonical saved values only. Native IntegerProperty
/// also parses some noncanonical spellings; malformed/absent NBT repair is
/// deliberately not guessed here. Unknown extra liquid properties stay errors.
pub fn parse(state: &BlockState) -> Result<State, Error> {
    let kind = match state.name.as_str() {
        "minecraft:water" => Some(Kind::Water),
        "minecraft:lava" => Some(Kind::Lava),
        _ => None,
    };
    // Native Java1.19.3 BubbleColumnBlock.getFluidState returns source water
    // for either drag direction. Its empty JSON model is not an empty fluid.
    if state.name == "minecraft:bubble_column" {
        if state.properties.len() != 1
            || !matches!(
                state.properties.get("drag").map(String::as_str),
                Some("true" | "false")
            )
        {
            return Err(Error::Properties);
        }
        return Ok(State::Liquid(Liquid {
            kind: Kind::Water,
            amount: 8,
            falling: false,
        }));
    }
    if let Some(kind) = kind {
        let text = state.properties.get("level").ok_or(Error::Level)?;
        if state.properties.len() != 1 {
            return Err(Error::Properties);
        }
        if !(1..=2).contains(&text.len())
            || !text.bytes().all(|byte| byte.is_ascii_digit())
            || (text.len() == 2 && text.starts_with('0'))
        {
            return Err(Error::Level);
        }
        let level = text.parse::<u8>().map_err(|_| Error::Level)?;
        if level > 15 {
            return Err(Error::Level);
        }
        return Ok(State::Liquid(Liquid {
            kind,
            amount: if level == 0 || level >= 8 {
                8
            } else {
                8 - level
            },
            falling: level >= 8,
        }));
    }
    match state.properties.get("waterlogged").map(String::as_str) {
        Some("true") => Ok(State::Waterlogged),
        Some("false") | None => Ok(State::Empty),
        Some(_) => Err(Error::Waterlogged),
    }
}

#[cfg(test)]
mod tests {
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
    fn all_native_saved_levels_keep_kind_amount_falling_and_exact_raw_state() {
        for (name, kind) in [
            ("minecraft:water", Kind::Water),
            ("minecraft:lava", Kind::Lava),
        ] {
            for raw_level in 0..=15 {
                let level = raw_level.to_string();
                let raw = state(name, &[("level", &level)]);
                let original = raw.clone();
                let State::Liquid(liquid) = parse(&raw).unwrap() else {
                    panic!("exact native liquid was not decoded");
                };
                assert_eq!(liquid.kind(), kind);
                assert_eq!(
                    liquid.amount(),
                    if raw_level == 0 || raw_level >= 8 {
                        8
                    } else {
                        8 - raw_level
                    }
                );
                assert_eq!(liquid.falling(), raw_level >= 8);
                assert_eq!(liquid.is_source(), raw_level == 0);
                assert_eq!(liquid.own_height(), f32::from(liquid.amount()) / 9.0);
                assert_eq!(liquid.height(false), liquid.own_height());
                assert_eq!(liquid.height(true), 1.0);
                assert_eq!(raw, original);
            }
        }
    }
    #[test]
    fn bubble_column_is_native_source_water_for_both_drag_directions() {
        for drag in ["true", "false"] {
            let raw = state("minecraft:bubble_column", &[("drag", drag)]);
            let original = raw.clone();
            let State::Liquid(liquid) = parse(&raw).unwrap() else {
                panic!("native bubble column has a source water FluidState");
            };
            assert_eq!(liquid.kind(), Kind::Water);
            assert_eq!(liquid.amount(), 8);
            assert!(!liquid.falling());
            assert!(liquid.is_source());
            assert_eq!(raw, original);
        }
        for properties in [
            vec![],
            vec![("drag", "up")],
            vec![("drag", "true"), ("level", "0")],
        ] {
            assert_eq!(
                parse(&state("minecraft:bubble_column", &properties)),
                Err(Error::Properties)
            );
        }
    }
    #[test]
    fn waterlogging_retains_solid_ownership_and_cannot_turn_into_an_empty_cell() {
        let raw = state(
            "minecraft:oak_slab",
            &[("type", "bottom"), ("waterlogged", "true")],
        );
        let original = raw.clone();
        assert_eq!(parse(&raw), Ok(State::Waterlogged));
        assert_eq!(raw, original);
        assert_eq!(
            parse(&state("minecraft:oak_slab", &[("waterlogged", "false")])),
            Ok(State::Empty)
        );
        assert_eq!(parse(&state("minecraft:stone", &[])), Ok(State::Empty));
        assert_eq!(
            parse(&state("example:water", &[("level", "0")])),
            Ok(State::Empty)
        );
    }
    #[test]
    fn absent_invalid_or_extra_liquid_properties_never_default_to_source_water() {
        for name in ["minecraft:water", "minecraft:lava"] {
            assert_eq!(parse(&state(name, &[])), Err(Error::Level));
            for value in ["", "-1", "16", "0.0", "00", "+0", "false", " 0"] {
                assert_eq!(parse(&state(name, &[("level", value)])), Err(Error::Level));
            }
            assert_eq!(
                parse(&state(name, &[("level", "0"), ("waterlogged", "true")])),
                Err(Error::Properties)
            );
        }
        for value in ["", "TRUE", "1", " true"] {
            assert_eq!(
                parse(&state("minecraft:oak_slab", &[("waterlogged", value)])),
                Err(Error::Waterlogged)
            );
        }
    }
}
