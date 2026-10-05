//! Query the original prepared generator, including buried cells and explicit known air.
use super::*;
use ilium_ambient::voxel_landscape::{chunks::ColumnCache, generation, VoxelLandscapeSettings};
use ilium_execution::{QuotaGroup, QuotaLimits};

fn fixture() -> (generation::PreparedWorld, QuotaGroup) {
    let settings = VoxelLandscapeSettings {
        seed: 42,
        vegetation_percent: 0,
        structures_percent: 0,
        rivers: false,
        ravines: false,
        caves: false,
        ..Default::default()
    };
    let world = generation::prepare(
        generation::Region {
            minimum: [-2, -3],
            maximum: [3, 4],
        },
        &settings,
        &mut ColumnCache::new(16),
        || false,
    )
    .unwrap();
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 1,
        input_bytes: 1048576,
        result_bytes: 1048576,
        worker_threads: 1,
        worker_bytes: 1048576,
    });
    (world, quota)
}
fn limits() -> RegionLimits {
    RegionLimits {
        cells: 4096,
        palette: 65,
        work: 1000000,
    }
}

#[test]
fn generated_volume_uses_public_axes_and_original_buried_materials() {
    let (world, quota) = fixture();
    let spec = RegionSpec {
        origin: [-2, 0, -3],
        size: [5, 96, 7],
        max_bytes: 65536,
    };
    let baseline = quota.snapshot().worker_bytes;
    let region = collect_generated_region(&world, &quota, spec, limits(), || false).unwrap();
    assert!(quota.snapshot().worker_bytes > baseline);
    assert_eq!(region.blocks.len(), 5 * 96 * 7);
    for (index, &palette_index) in region.blocks.iter().enumerate() {
        let x = index % 5;
        let y = index / 5 / 7;
        let z = index / 5 % 7;
        let expected = world
            .block([-2 + x as i32, -3 + z as i32, y as i32])
            .map(|material| material.generated_state_name())
            .unwrap_or("ilium:generated/air");
        let state = region.palette[usize::from(palette_index)];
        assert_eq!(
            state.name, expected,
            "original voxel at public ({x},{y},{z})"
        );
        assert!(state.properties.is_empty());
    }
    assert!(region
        .palette
        .iter()
        .any(|state| state.name == "ilium:generated/basalt"));
    assert!(region
        .palette
        .iter()
        .any(|state| state.name == "ilium:generated/air"));
    drop(region);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}

#[test]
fn generated_region_rejects_missing_ground_extent_and_render_halo() {
    let (world, quota) = fixture();
    let baseline = quota.snapshot().worker_bytes;
    for (origin, size) in [
        ([-3, 0, 0], [1, 1, 1]),
        ([2, 0, 0], [2, 1, 1]),
        ([0, 0, -4], [1, 1, 1]),
        ([0, 0, 3], [1, 1, 2]),
    ] {
        let spec = RegionSpec {
            origin,
            size,
            max_bytes: 4096,
        };
        assert!(collect_generated_region(&world, &quota, spec, limits(), || false).is_err());
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }
}

#[test]
fn generated_region_rejects_unrepresentable_height_instead_of_guessing_air() {
    let (world, quota) = fixture();
    for (origin, size) in [([0, -1, 0], [1, 1, 1]), ([0, 32767, 0], [1, 2, 1])] {
        let baseline = quota.snapshot().worker_bytes;
        let spec = RegionSpec {
            origin,
            size,
            max_bytes: 4096,
        };
        assert!(collect_generated_region(&world, &quota, spec, limits(), || false).is_err());
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }
}

#[test]
fn generated_region_cancellation_releases_the_original_projection_debit() {
    let (world, quota) = fixture();
    let baseline = quota.snapshot().worker_bytes;
    let spec = RegionSpec {
        origin: [-2, 0, -3],
        size: [5, 96, 7],
        max_bytes: 65536,
    };
    let mut checks = 0;
    let result = collect_generated_region(&world, &quota, spec, limits(), || {
        checks += 1;
        checks > 8
    });
    assert!(matches!(result, Err(RegionError::Cancelled)));
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
