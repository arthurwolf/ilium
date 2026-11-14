//! Generated-world tint policy, separate from exact saved-world supplied colors.
use super::surface_biomes::SurfaceBiome;

fn rgb(value: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((value >> shift) & 255) as f32 / 255.)
}

pub(super) fn color(biome: Option<SurfaceBiome>, block: &str) -> [f32; 3] {
    // These leaf textures carry their own pink, pale or autumn artwork. The
    // shared leaves parent has tint index 0 even when no green provider applies.
    match block {
        "cherry_leaves"
        | "pale_oak_leaves"
        | "azalea_leaves"
        | "flowering_azalea_leaves"
        | "orange_poplar_leaves"
        | "red_poplar_leaves"
        | "yellow_poplar_leaves" => return [1.; 3],
        "birch_leaves" => return rgb(0x80a755),
        "spruce_leaves" => return rgb(0x619961),
        _ => {}
    }
    // Descriptive palette overrides are pinned to shipped Java 26.3 biome data.
    // Remaining climates retain the authored approximation below; neither this
    // nor saved-world supplied tints claim a newly sampled pack colormap here.
    let foliage = block.ends_with("_leaves");
    let palette = match (biome, foliage) {
        (
            Some(
                SurfaceBiome::Badlands
                | SurfaceBiome::ErodedBadlands
                | SurfaceBiome::WoodedBadlands,
            ),
            true,
        ) => Some(0x9e814d),
        (
            Some(
                SurfaceBiome::Badlands
                | SurfaceBiome::ErodedBadlands
                | SurfaceBiome::WoodedBadlands,
            ),
            false,
        ) => Some(0x90814d),
        (Some(SurfaceBiome::CherryGrove), _) => Some(0xb6db61),
        (Some(SurfaceBiome::DappledForest), true) => Some(0xe68e30),
        (Some(SurfaceBiome::DappledForest), false) => Some(0xdf6827),
        (Some(SurfaceBiome::PaleGarden), true) => Some(0x878d76),
        (Some(SurfaceBiome::PaleGarden), false) => Some(0x778272),
        (Some(SurfaceBiome::Swamp), true) => Some(0x6a7039),
        (Some(SurfaceBiome::MangroveSwamp), true) => Some(0x8db127),
        _ => None,
    };
    if let Some(palette) = palette {
        return rgb(palette);
    }
    let (temperature, downfall) = biome
        .map(|biome| {
            let descriptor = biome.descriptor();
            (
                descriptor.gameplay_temperature.clamp(0., 1.),
                descriptor.downfall.clamp(0., 1.),
            )
        })
        .unwrap_or((0.6, 0.5));
    let dry = 1.0 - downfall * temperature;
    let rgb = if block.ends_with("_leaves") {
        [
            0.32 + 0.22 * dry,
            0.62 + 0.22 * downfall,
            0.24 + 0.12 * (1. - temperature),
        ]
    } else {
        [
            0.40 + 0.25 * dry,
            0.67 + 0.18 * downfall,
            0.30 + 0.12 * (1. - temperature),
        ]
    };
    rgb.map(|v| v.clamp(0., 1.))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_palette_overrides_preserve_distinctive_biomes() {
        use SurfaceBiome::*;
        for (biome, grass, foliage) in [
            (Badlands, 0x90814d, 0x9e814d),
            (ErodedBadlands, 0x90814d, 0x9e814d),
            (WoodedBadlands, 0x90814d, 0x9e814d),
            (CherryGrove, 0xb6db61, 0xb6db61),
            (DappledForest, 0xdf6827, 0xe68e30),
            (PaleGarden, 0x778272, 0x878d76),
        ] {
            assert_eq!(
                color(Some(biome), "grass_block"),
                rgb(grass),
                "{biome:?} grass"
            );
            assert_eq!(
                color(Some(biome), "oak_leaves"),
                rgb(foliage),
                "{biome:?} biome foliage"
            );
        }
        assert_eq!(color(Some(Swamp), "oak_leaves"), rgb(0x6a7039));
        assert_eq!(color(Some(MangroveSwamp), "mangrove_leaves"), rgb(0x8db127));
    }
    #[test]
    fn colored_leaf_art_keeps_its_authored_hue_in_every_biome() {
        for &biome in SurfaceBiome::all() {
            for block in [
                "cherry_leaves",
                "pale_oak_leaves",
                "azalea_leaves",
                "flowering_azalea_leaves",
                "orange_poplar_leaves",
                "red_poplar_leaves",
                "yellow_poplar_leaves",
            ] {
                assert_eq!(color(Some(biome), block), [1.; 3], "{biome:?}/{block}");
            }
        }
    }
    #[test]
    fn birch_and_spruce_keep_species_colors_across_climates() {
        for &biome in SurfaceBiome::all() {
            assert_eq!(color(Some(biome), "birch_leaves"), rgb(0x80a755));
            assert_eq!(color(Some(biome), "spruce_leaves"), rgb(0x619961));
        }
    }
    #[test]
    fn every_biome_and_missing_column_have_finite_bounded_tints() {
        for biome in SurfaceBiome::all().iter().copied().map(Some).chain([None]) {
            for block in ["grass_block", "oak_leaves", "fern", "vine"] {
                assert!(color(biome, block)
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            }
        }
    }
}
