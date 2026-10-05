//! Bounded borrowed native-cell projection for the public region contract.
//! Trusted callers supply the original source reader and quota admission.
//! This module opens no source path and creates no quota bank or synthetic air.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockStateView<'a> {
    pub name: &'a str,
    pub properties: &'a BTreeMap<String, String>,
}
#[derive(Clone, Copy, Debug)]
pub struct RegionSpec {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub max_bytes: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct RegionLimits {
    pub cells: usize,
    pub palette: usize,
    pub work: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub enum RegionError {
    Unavailable,
    Extent,
    Bytes,
    Palette,
    Work,
    Admission,
    Allocation,
    Cancelled,
    MissingCell([i32; 3]),
}
/// Validated request shape only; the borrowed ID conveys no source authority.
#[derive(Debug)]
pub struct ParsedRegionRequest<'a> {
    pub world_id: &'a str,
    pub spec: RegionSpec,
}

/// Validate the SDK's exact request before source lookup or response allocation.
pub fn parse_region_request<'a>(
    fields: &'a serde_json::Map<String, serde_json::Value>,
    limits: RegionLimits,
    receiving_bytes: usize,
) -> Result<ParsedRegionRequest<'a>, RegionError> {
    if fields.len() != 8 {
        return Err(RegionError::Extent);
    }
    let world = fields
        .get("world")
        .and_then(serde_json::Value::as_object)
        .filter(|world| world.len() == 2)
        .ok_or(RegionError::Unavailable)?;
    if world.get("kind").and_then(serde_json::Value::as_str) != Some("worlds") {
        return Err(RegionError::Unavailable);
    }
    let world_id = world
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or(RegionError::Unavailable)?;
    let coordinate = |name: &str| {
        fields
            .get(name)
            .and_then(serde_json::Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(RegionError::Extent)
    };
    let extent = |name: &str| {
        fields
            .get(name)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or(RegionError::Extent)
    };
    let origin = [coordinate("x")?, coordinate("y")?, coordinate("z")?];
    let size = [extent("width")?, extent("height")?, extent("depth")?];
    let cells = size.iter().try_fold(1_usize, |count, &extent| {
        count
            .checked_mul(extent as usize)
            .ok_or(RegionError::Extent)
    })?;
    if cells > limits.cells {
        return Err(RegionError::Extent);
    }
    for (start, extent) in origin.into_iter().zip(size) {
        if i64::from(start) + i64::from(extent) - 1 > i64::from(i32::MAX) {
            return Err(RegionError::Extent);
        }
    }
    let max_bytes = fields
        .get("max_bytes")
        .and_then(serde_json::Value::as_u64)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .filter(|bytes| *bytes != 0 && *bytes <= receiving_bytes)
        .ok_or(RegionError::Bytes)?;
    // The collector requires this existing floor before actual palette strings.
    let minimum_bytes = cells
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(256))
        .ok_or(RegionError::Bytes)?;
    if minimum_bytes > max_bytes {
        return Err(RegionError::Bytes);
    }
    Ok(ParsedRegionRequest {
        world_id,
        spec: RegionSpec {
            origin,
            size,
            max_bytes,
        },
    })
}

#[derive(Debug)]
pub struct BorrowedRegion<'a, G> {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub blocks: Vec<u16>,
    pub palette: Vec<BlockStateView<'a>>,
    pub response_bytes: usize,
    /// Keep the caller-supplied original admission through serialization.
    _admission: G,
}

pub fn collect_region<'a, G>(
    spec: RegionSpec,
    limits: RegionLimits,
    mut cancelled: impl FnMut() -> bool,
    admit: impl FnOnce(usize) -> Result<G, RegionError>,
    mut sample: impl FnMut([i32; 3]) -> Result<Option<BlockStateView<'a>>, RegionError>,
) -> Result<BorrowedRegion<'a, G>, RegionError> {
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    let cells = spec.size.iter().try_fold(1_usize, |count, &extent| {
        count
            .checked_mul(extent as usize)
            .ok_or(RegionError::Extent)
    })?;
    if cells == 0 || cells > limits.cells {
        return Err(RegionError::Extent);
    }
    for (origin, extent) in spec.origin.into_iter().zip(spec.size) {
        if i64::from(origin) + i64::from(extent) - 1 > i64::from(i32::MAX) {
            return Err(RegionError::Extent);
        }
    }
    let palette_capacity = cells.min(limits.palette).min(usize::from(u16::MAX) + 1);
    if palette_capacity == 0 {
        return Err(RegionError::Palette);
    }
    let mut work = limits.work;
    if work == 0 {
        return Err(RegionError::Work);
    }
    // Fixed metadata allowance covers origin/shape and the native64-byte identity.
    // Six bytes per UTF-8 byte below safely covers worst-case JSON string escaping.
    let mut response_bytes = cells
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(256))
        .ok_or(RegionError::Bytes)?;
    if response_bytes > spec.max_bytes {
        return Err(RegionError::Bytes);
    }
    type Lookup<'a> = (BlockStateView<'a>, u16);
    let charge = palette_capacity
        .checked_mul(std::mem::size_of::<BlockStateView<'a>>() + std::mem::size_of::<Lookup<'a>>())
        .and_then(|bytes| bytes.checked_add(cells.checked_mul(2)?))
        .and_then(|bytes| bytes.checked_add(spec.max_bytes))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<BorrowedRegion<'a, G>>()))
        .ok_or(RegionError::Bytes)?;
    // The original caller admission precedes all three vectors and source access.
    let admission = admit(charge)?;
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    let mut blocks = Vec::new();
    let mut palette = Vec::new();
    let mut lookup: Vec<Lookup<'a>> = Vec::new();
    blocks
        .try_reserve_exact(cells)
        .map_err(|_| RegionError::Allocation)?;
    palette
        .try_reserve_exact(palette_capacity)
        .map_err(|_| RegionError::Allocation)?;
    lookup
        .try_reserve_exact(palette_capacity)
        .map_err(|_| RegionError::Allocation)?;
    let mut maximum_key_bytes = 1_usize;
    let width = spec.size[0] as usize;
    let depth = spec.size[2] as usize;
    // One finite scan uses the same X-fast/Z-next/Y-last order as native chunks.
    for cell in 0..cells {
        if cancelled() {
            return Err(RegionError::Cancelled);
        }
        work = work.checked_sub(1).ok_or(RegionError::Work)?;
        // Validated endpoints prove each intermediate coordinate fits i32.
        let position = [cell % width, cell / width / depth, cell / width % depth];
        let position = std::array::from_fn(|axis| {
            (i64::from(spec.origin[axis]) + position[axis] as i64) as i32
        });
        let state = sample(position)?.ok_or(RegionError::MissingCell(position))?;
        let mut key_bytes = state.name.len().checked_add(1).ok_or(RegionError::Bytes)?;
        for (key, value) in state.properties {
            work = work.checked_sub(1).ok_or(RegionError::Work)?;
            key_bytes = key_bytes
                .checked_add(key.len())
                .and_then(|bytes| bytes.checked_add(value.len()))
                .and_then(|bytes| bytes.checked_add(2))
                .ok_or(RegionError::Bytes)?;
        }
        maximum_key_bytes = maximum_key_bytes.max(key_bytes);
        let comparison_bound = (lookup.len() + 1).ilog2() as usize + 1;
        let comparison_work = maximum_key_bytes
            .checked_mul(comparison_bound)
            .ok_or(RegionError::Work)?;
        work = work.checked_sub(comparison_work).ok_or(RegionError::Work)?;
        let index = match lookup.binary_search_by(|entry| entry.0.cmp(&state)) {
            Ok(found) => lookup[found].1,
            Err(insertion) => {
                if palette.len() == palette_capacity {
                    return Err(RegionError::Palette);
                }
                // Account bounded movement of borrowed records, not a hidden tree allocation.
                let movement = (lookup.len() - insertion)
                    .checked_mul(std::mem::size_of::<Lookup<'a>>())
                    .ok_or(RegionError::Work)?;
                work = work.checked_sub(movement).ok_or(RegionError::Work)?;
                let property_overhead = state
                    .properties
                    .len()
                    .checked_mul(24)
                    .ok_or(RegionError::Bytes)?;
                let metadata = key_bytes
                    .checked_mul(6)
                    .and_then(|bytes| bytes.checked_add(property_overhead))
                    .and_then(|bytes| bytes.checked_add(64))
                    .ok_or(RegionError::Bytes)?;
                response_bytes = response_bytes
                    .checked_add(metadata)
                    .ok_or(RegionError::Bytes)?;
                if response_bytes > spec.max_bytes {
                    return Err(RegionError::Bytes);
                }
                let index = u16::try_from(palette.len()).map_err(|_| RegionError::Palette)?;
                palette.push(state);
                lookup.insert(insertion, (state, index));
                index
            }
        };
        blocks.push(index);
    }
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    Ok(BorrowedRegion {
        origin: spec.origin,
        size: spec.size,
        blocks,
        palette,
        response_bytes,
        _admission: admission,
    })
}

/// Project original generated occupancy in public X/east, Y/up, Z/south axes.
/// Ground bounds and representable heights must be checked before interpreting
/// an absent original block as known air; the visible render mesh is not queried.
pub fn collect_generated_region(
    world: &ilium_ambient::voxel_landscape::generation::PreparedWorld,
    quota: &ilium_execution::QuotaGroup,
    spec: RegionSpec,
    limits: RegionLimits,
    mut cancelled: impl FnMut() -> bool,
) -> Result<BorrowedRegion<'static, ilium_execution::StorageAdmission>, RegionError> {
    if cancelled() {
        return Err(RegionError::Cancelled);
    }
    for (public_axis, ground_axis) in [(0, 0), (2, 1)] {
        let end = i64::from(spec.origin[public_axis]) + i64::from(spec.size[public_axis]);
        if spec.size[public_axis] == 0
            || spec.origin[public_axis] < world.region.minimum[ground_axis]
            || end > i64::from(world.region.maximum[ground_axis])
        {
            return Err(RegionError::Extent);
        }
    }
    let height_end = i64::from(spec.origin[1]) + i64::from(spec.size[1]);
    if spec.size[1] == 0 || spec.origin[1] < 0 || height_end > i64::from(i16::MAX) + 1 {
        return Err(RegionError::Extent);
    }
    static PROPERTIES: BTreeMap<String, String> = BTreeMap::new();
    collect_region(
        spec,
        limits,
        cancelled,
        |bytes| {
            quota
                .reserve_external_storage(bytes)
                .map_err(|_| RegionError::Admission)
        },
        |[x, y, z]| {
            let name = world
                .block([x, z, y])
                .map_or("ilium:generated/air", |material| {
                    material.generated_state_name()
                });
            Ok(Some(BlockStateView {
                name,
                properties: &PROPERTIES,
            }))
        },
    )
}

#[cfg(test)]
#[path = "world_region_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "world_region_generated_tests.rs"]
mod generated_tests;

#[cfg(test)]
#[path = "world_region_request_tests.rs"]
mod request_tests;
