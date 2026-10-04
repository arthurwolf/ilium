use super::super::surface_biome_selector::select;
use super::super::surface_biomes::{SurfaceBiome, ALL_SURFACE_BIOMES};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
// Thresholds below are ecological bounds supported by source calculations, not exact Rust execution receipts.
fn assert_mixed_wetland(center: [i32; 2], target: SurfaceBiome, minimum_named_percent: usize) {
    let fields = TerrainFields::new(71839);
    for rivers in [false, true] {
        let mut matching = 0_usize;
        let mut wet = 0_usize;
        let mut dry = 0_usize;
        let mut minimum_height = i16::MAX;
        let mut maximum_height = i16::MIN;
        let mut maximum_step = 0_i16;
        let mut pool_bank_edges = 0_usize;
        let mut boundary_edges = 0_usize;
        let mut invalid_water = 0_usize;
        for z in center[1] - 32..=center[1] + 32 {
            for x in center[0] - 32..=center[0] + 32 {
                let sample = fields.sample(x, z, rivers);
                if select(71839, [x, z], sample) != target {
                    continue;
                }
                matching += 1;
                minimum_height = minimum_height.min(sample.height);
                maximum_height = maximum_height.max(sample.height);
                if let Some(level) = sample.water_level {
                    wet += 1;
                    invalid_water += usize::from(
                        level != SURFACE_SEA_LEVEL || !(1..=3).contains(&(level - sample.height)),
                    );
                } else {
                    dry += 1;
                    invalid_water += usize::from(sample.height < SURFACE_SEA_LEVEL);
                }
                for [dx, dz] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                    let neighbor = fields.sample(x + dx, z + dz, rivers);
                    maximum_step = maximum_step.max((sample.height - neighbor.height).abs());
                    let same_biome = select(71839, [x + dx, z + dz], neighbor) == target;
                    boundary_edges += usize::from(!same_biome);
                    pool_bank_edges += usize::from(
                        same_biome
                            && sample.water_level.is_some() != neighbor.water_level.is_some(),
                    );
                }
            }
        }
        eprintln!("ecology_wetland target={} center={center:?} rivers={rivers} footprint=4225 named={matching} wet={wet} dry={dry} height={minimum_height}..{maximum_height} maximum_step={maximum_step} pool_bank_edges={pool_bank_edges} boundary_edges={boundary_edges}", target.id());
        assert!(
            matching * 100 >= 4225 * minimum_named_percent,
            "retagging lost {} at {center:?}",
            target.id()
        );
        assert_eq!(wet + dry, matching);
        assert!(
            wet * 100 >= matching * 5,
            "named wetland lacks meaningful pools at {center:?}"
        );
        assert!(
            dry * 100 >= matching * 5,
            "named wetland lacks meaningful banks at {center:?}"
        );
        assert!(
            minimum_height >= SURFACE_SEA_LEVEL - 3 && maximum_height <= SURFACE_SEA_LEVEL + 3,
            "high or deep named wetland at {center:?}: {minimum_height}..{maximum_height}"
        );
        assert_eq!(invalid_water, 0, "invalid wetland water at {center:?}");
        assert!(
            maximum_step <= 2,
            "wetland transition cliff at {center:?}: {maximum_step}"
        );
        assert!(
            pool_bank_edges > 0,
            "wet pools and dry banks never meet at {center:?}"
        );
    }
}
// Separate test identities force both original defects even when one fails before the other is evaluated.
#[test]
fn natural_dry_mangrove_has_mixed_low_wetland_red_green() {
    assert_mixed_wetland([7424, -16384], SurfaceBiome::MangroveSwamp, 75);
}
#[test]
fn natural_high_swamp_has_mixed_low_wetland_red_green() {
    assert_mixed_wetland([-3456, -16384], SurfaceBiome::Swamp, 75);
}
#[test]
fn natural_137_cores_and_mangrove_root_witness_keep_banks() {
    for (center, target) in [
        ([11008, -14464], SurfaceBiome::MangroveSwamp),
        ([8960, -16384], SurfaceBiome::Swamp),
        ([-13056, -15616], SurfaceBiome::MangroveSwamp),
    ] {
        assert_mixed_wetland(center, target, 90);
    }
}
#[test]
fn natural_137_swamp_river_toggle_retains_its_base_ecology() {
    let fields = TerrainFields::new(71839);
    let mut overlay = 0_usize;
    let mut retained = 0_usize;
    let mut coastal_or_pool_water = 0_usize;
    for z in -16416..=-16352 {
        for x in 8928..=8992 {
            let disabled = fields.sample(x, z, false);
            let enabled = fields.sample(x, z, true);
            assert_eq!(select(71839, [x, z], disabled), SurfaceBiome::Swamp);
            assert_eq!(disabled.height, disabled.uncarved_height);
            assert!(!disabled.river);
            assert_eq!(enabled.climate, disabled.climate);
            assert_eq!(enabled.landform, disabled.landform);
            assert_eq!(enabled.uncarved_height, disabled.uncarved_height);
            assert!(enabled.height <= disabled.height);
            coastal_or_pool_water += usize::from(disabled.water_level == Some(SURFACE_SEA_LEVEL));
            if enabled.river {
                overlay += 1;
                assert_eq!(select(71839, [x, z], enabled), SurfaceBiome::River);
                assert_eq!(enabled.water_level, Some(SURFACE_SEA_LEVEL));
                assert!(enabled.height < SURFACE_SEA_LEVEL);
                continue;
            }
            retained += 1;
            assert_eq!(select(71839, [x, z], enabled), SurfaceBiome::Swamp);
        }
    }
    eprintln!("ecology_river_core footprint=4225 overlay={overlay} retained_swamp={retained} water_without_rivers={coastal_or_pool_water}");
    assert_eq!(overlay + retained, 4225);
    assert!(overlay > 0 && retained * 100 >= 4225 * 90);
    assert!(coastal_or_pool_water > 0 && coastal_or_pool_water < 4225);
}
// Identify the frozen original's climate eligibility only; do not reproduce the proposed height or selection implementation.
fn was_original_wetland_core(climate: TerrainClimate) -> bool {
    if climate.humidity <= 0.70
        || climate.continentalness < -0.02
        || climate.continentalness >= 0.18
    {
        return false;
    }
    let inland = smoothstep(-0.08, 0.35, climate.continentalness);
    let mountainous = inland * (1.0 - smoothstep(-0.55, 0.38, climate.erosion));
    128.0 * mountainous * climate.ridge.powi(2) <= 32.0
}
#[test]
fn natural_selector_census_retains_43_ids_and_original_wetland_cores() {
    let fields = TerrainFields::new(71839);
    let mut seen_enabled = BTreeMap::<&'static str, usize>::new();
    let mut seen_disabled = BTreeMap::<&'static str, usize>::new();
    let mut original_cores = 0_usize;
    let mut retained_core_river_overlays = 0_usize;
    let mut maximum_wetland_step = 0_i16;
    let mut wetland_boundary_edges = 0_usize;
    let mut center_count = 0_usize;
    for z in (-16384..=16384).step_by(128) {
        for x in (-16384..=16384).step_by(128) {
            center_count += 1;
            let disabled = fields.sample(x, z, false);
            let enabled = fields.sample(x, z, true);
            let off_biome = select(71839, [x, z], disabled);
            let on_biome = select(71839, [x, z], enabled);
            *seen_disabled.entry(off_biome.id()).or_default() += 1;
            *seen_enabled.entry(on_biome.id()).or_default() += 1;
            assert_eq!(disabled.height, disabled.uncarved_height);
            assert!(!disabled.river && enabled.height <= disabled.height);
            assert_eq!(enabled.climate, disabled.climate);
            assert_eq!(enabled.uncarved_height, disabled.uncarved_height);
            if was_original_wetland_core(disabled.climate) {
                original_cores += 1;
                let expected = if disabled.climate.temperature > 0.69 {
                    SurfaceBiome::MangroveSwamp
                } else {
                    SurfaceBiome::Swamp
                };
                assert_eq!(
                    disabled.landform,
                    Landform::Wetland,
                    "lost original wetland core at [{x}, {z}]"
                );
                assert_eq!(
                    off_biome, expected,
                    "changed wetland core identity at [{x}, {z}]"
                );
                assert!(
                    (SURFACE_SEA_LEVEL - 3..=SURFACE_SEA_LEVEL + 2).contains(&disabled.height),
                    "high original core at [{x}, {z}]: {}",
                    disabled.height
                );
                if enabled.river {
                    retained_core_river_overlays += 1;
                    assert!(matches!(
                        on_biome,
                        SurfaceBiome::River | SurfaceBiome::FrozenRiver
                    ));
                } else {
                    assert_eq!(
                        on_biome, expected,
                        "lost river-free original core at [{x}, {z}]"
                    );
                }
            }
            for (rivers, sample, biome) in [(false, disabled, off_biome), (true, enabled, on_biome)]
            {
                if !matches!(biome, SurfaceBiome::Swamp | SurfaceBiome::MangroveSwamp) {
                    continue;
                }
                assert!(
                    (SURFACE_SEA_LEVEL - 3..=SURFACE_SEA_LEVEL + 3).contains(&sample.height),
                    "named wetland out of envelope at [{x}, {z}]: {}",
                    sample.height
                );
                for [dx, dz] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                    let neighbor = fields.sample(x + dx, z + dz, rivers);
                    let step = (sample.height - neighbor.height).abs();
                    maximum_wetland_step = maximum_wetland_step.max(step);
                    wetland_boundary_edges +=
                        usize::from(select(71839, [x + dx, z + dz], neighbor) != biome);
                    assert!(
                        step <= 3,
                        "wetland cliff at [{x}, {z}] offset [{dx}, {dz}] rivers={rivers}: {step}"
                    );
                }
            }
        }
    }
    eprintln!("ecology_census centers={center_count} original_cores={original_cores} core_river_overlays={retained_core_river_overlays} maximum_wetland_step={maximum_wetland_step} wetland_boundary_edges={wetland_boundary_edges} enabled={seen_enabled:?} disabled={seen_disabled:?}");
    assert_eq!(center_count, 66049);
    assert_eq!(original_cores, 3945);
    assert!(retained_core_river_overlays > 0 && wetland_boundary_edges > 0);
    let expected_enabled: BTreeSet<_> = ALL_SURFACE_BIOMES.iter().map(|biome| biome.id()).collect();
    assert_eq!(expected_enabled.len(), 43);
    assert_eq!(
        seen_enabled.keys().copied().collect::<BTreeSet<_>>(),
        expected_enabled
    );
    let expected_disabled: BTreeSet<_> = ALL_SURFACE_BIOMES
        .iter()
        .filter(|biome| !matches!(biome, SurfaceBiome::River | SurfaceBiome::FrozenRiver))
        .map(|biome| biome.id())
        .collect();
    assert_eq!(
        seen_disabled.keys().copied().collect::<BTreeSet<_>>(),
        expected_disabled
    );
}
