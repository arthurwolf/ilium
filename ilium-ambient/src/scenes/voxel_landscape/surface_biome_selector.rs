//! Authored continuous-field selector for the 43 canonical surface vocabularies.
//! These bands are an Ilium homage, not extracted Minecraft multi-noise bands.
use super::{
    noise::value2,
    surface_biomes::SurfaceBiome,
    terrain_fields::{Landform, TerrainSample},
};
use SurfaceBiome::*;

pub fn select(seed: u64, position: [i32; 2], sample: TerrainSample) -> SurfaceBiome {
    let climate = sample.climate;
    let variant = (0.5
        + 0.5
            * value2(
                seed ^ 0x0062_696f_6d65_3433,
                i64::from(position[0]),
                i64::from(position[1]),
                256,
            ))
    .clamp(0.0, 1.0);
    let t = climate.temperature;
    let h = climate.humidity;
    if sample.river {
        return if t < 0.26 { FrozenRiver } else { River };
    }
    match sample.landform {
        Landform::Island if h > 0.63 && variant > 0.58 => MushroomFields,
        Landform::Coast | Landform::Island => {
            if t < 0.24 {
                SnowyBeach
            } else if climate.erosion < -0.42 || climate.ridge > 0.84 {
                StonyShore
            } else {
                Beach
            }
        }
        Landform::Mesa => {
            if variant < 0.20 {
                ErodedBadlands
            } else if variant > 0.82 && h > 0.19 {
                WoodedBadlands
            } else {
                Badlands
            }
        }
        Landform::Dunes => {
            if h < 0.29 {
                Desert
            } else if variant > 0.77 {
                SavannaPlateau
            } else {
                Savanna
            }
        }
        Landform::Wetland => {
            if t > 0.69 {
                MangroveSwamp
            } else {
                Swamp
            }
        }
        Landform::Mountain => {
            if t < 0.21 {
                if sample.height > 174 && variant < 0.36 {
                    FrozenPeaks
                } else if sample.height > 155 && variant < 0.68 {
                    JaggedPeaks
                } else if variant < 0.84 {
                    SnowySlopes
                } else {
                    Grove
                }
            } else if t > 0.72 && h < 0.40 {
                WindsweptSavanna
            } else if sample.height > 172 {
                StonyPeaks
            } else if h < 0.28 {
                if variant > 0.58 {
                    WindsweptGravellyHills
                } else {
                    WindsweptHills
                }
            } else if variant > 0.78 {
                CherryGrove
            } else if variant > 0.51 {
                WindsweptForest
            } else {
                Meadow
            }
        }
        Landform::RollingHills | Landform::Lowland => {
            if t < 0.21 {
                if h < 0.29 {
                    if sample.height > 93 && variant > 0.82 {
                        IceSpikes
                    } else {
                        SnowyPlains
                    }
                } else if variant > 0.67 {
                    SnowyTaiga
                } else {
                    Taiga
                }
            } else if t < 0.34 {
                if variant < 0.24 {
                    OldGrowthPineTaiga
                } else if variant < 0.48 {
                    OldGrowthSpruceTaiga
                } else if h > 0.48 {
                    Taiga
                } else {
                    Grove
                }
            } else if t > 0.77 && h > 0.68 {
                if variant < 0.29 {
                    BambooJungle
                } else if variant < 0.72 {
                    Jungle
                } else {
                    SparseJungle
                }
            } else if t > 0.72 && h < 0.42 {
                if variant > 0.65 {
                    SavannaPlateau
                } else {
                    Savanna
                }
            } else if h < 0.32 {
                if variant > 0.82 {
                    SunflowerPlains
                } else {
                    Plains
                }
            } else if h > 0.68 {
                if variant < 0.16 {
                    OldGrowthBirchForest
                } else if variant < 0.35 {
                    BirchForest
                } else if variant < 0.52 {
                    DarkForest
                } else if variant < 0.68 {
                    DappledForest
                } else if variant < 0.84 {
                    PaleGarden
                } else {
                    FlowerForest
                }
            } else if variant < 0.14 {
                BirchForest
            } else if variant < 0.34 {
                Forest
            } else if variant < 0.53 {
                FlowerForest
            } else if variant < 0.68 {
                Meadow
            } else if variant < 0.78 {
                CherryGrove
            } else if variant < 0.89 {
                DappledForest
            } else {
                Plains
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::terrain_fields::TerrainFields;
    use super::*;
    #[test]
    fn continuous_field_selection_is_stable_across_chunk_boundaries_and_order() {
        let fields = TerrainFields::new(31);
        for [x, y] in [[-17, -16], [-16, -16], [-1, 0], [0, 0], [15, 16], [16, 16]] {
            let sample = fields.sample(x, y, true);
            let before = select(31, [x, y], sample);
            let _ = select(31, [y, x], fields.sample(y, x, true));
            assert_eq!(before, select(31, [x, y], sample));
            assert_eq!(SurfaceBiome::from_id(before.id()), Some(before));
        }
    }
}
