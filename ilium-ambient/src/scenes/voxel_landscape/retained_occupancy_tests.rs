//! Original generated occupancy must match retained block identities and bounds.
use super::*;

#[test]
fn retained_generated_occupancy_matches_every_original_visible_material() {
    for seed in [7, 42, 71839] {
        let settings = VoxelLandscapeSettings {
            seed,
            detail: 2,
            rivers: true,
            ravines: true,
            caves: true,
            ..Default::default()
        };
        let mut cache = ColumnCache::new(16);
        let prepared = prepare(
            Region {
                minimum: [-4, -4],
                maximum: [5, 5],
            },
            &settings,
            &mut cache,
            || false,
        )
        .expect("bounded original native world");
        assert!(!prepared.blocks.is_empty());
        for original in &prepared.blocks {
            assert_eq!(
                prepared.block(original.position),
                Some(original.material),
                "original generated material at {:?}, seed {seed}",
                original.position,
            );
        }
    }
}

#[test]
fn retained_generated_occupancy_does_not_expose_the_rendering_halo() {
    let settings = VoxelLandscapeSettings {
        seed: 42,
        vegetation_percent: 0,
        structures_percent: 0,
        rivers: false,
        ravines: false,
        caves: false,
        ..Default::default()
    };
    let mut cache = ColumnCache::new(16);
    let prepared = prepare(
        Region {
            minimum: [-2, -3],
            maximum: [3, 4],
        },
        &settings,
        &mut cache,
        || false,
    )
    .expect("bounded original native world");
    for inside in [[-2, -3, 0], [2, 3, 0]] {
        assert_eq!(prepared.block(inside), Some(Material::Basalt));
    }
    for outside in [[-3, 0, 0], [3, 0, 0], [0, -4, 0], [0, 4, 0]] {
        assert_eq!(prepared.block(outside), None);
    }
    assert_eq!(prepared.block([0, 0, -1]), None);
    assert_eq!(prepared.block([0, 0, 1024]), None);
}
