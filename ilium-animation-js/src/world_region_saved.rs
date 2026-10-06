//! Project genuine decoded saved chunks under the caller's original quota.
//! The selected native snapshot remains borrowed; no filesystem path is opened.
use crate::world_region::{
    collect_region, BlockStateView, BorrowedRegion, RegionError, RegionLimits, RegionSpec,
};
use ilium_ambient::minecraft::{
    chunk::{BlockSample, DecodedChunk},
    region::{MAX_DATA_VERSION, MIN_DATA_VERSION},
};
use ilium_execution::{QuotaGroup, StorageAdmission};
use std::{collections::BTreeMap, sync::Arc};

pub fn collect_saved_region<'a>(
    chunks: &'a BTreeMap<[i32; 2], Arc<DecodedChunk>>,
    quota: &QuotaGroup,
    spec: RegionSpec,
    limits: RegionLimits,
    cancelled: impl FnMut() -> bool,
) -> Result<BorrowedRegion<'a, StorageAdmission>, RegionError> {
    collect_region(
        spec,
        limits,
        cancelled,
        |bytes| {
            quota
                .reserve_external_storage(bytes)
                .map_err(|_| RegionError::Admission)
        },
        |position| {
            let key = [position[0].div_euclid(16), position[2].div_euclid(16)];
            let Some(chunk) = chunks.get(&key) else {
                return Ok(None);
            };
            if chunk.identity.position != key
                || !(MIN_DATA_VERSION..=MAX_DATA_VERSION).contains(&chunk.identity.data_version)
                || !chunk.is_full()
                || !chunk.sections_present
            {
                return Err(RegionError::Unavailable);
            }
            match chunk.block_at(position) {
                BlockSample::State(state) => Ok(Some(BlockStateView {
                    name: &state.name,
                    properties: &state.properties,
                })),
                _ => Ok(None),
            }
        },
    )
}

#[cfg(test)]
#[path = "world_region_saved_tests.rs"]
mod tests;

/// Encode a complete response while the caller retains its selected native
/// chunk snapshot. The authenticated world owner must supply these borrowed
/// chunks, its original quota root and original request/lifetime stop token.
/// Projection and output admissions overlap until every source read finishes;
/// only the output admission survives. No path, quota root or stop is created.
pub fn encode_saved_region(
    chunks: &BTreeMap<[i32; 2], Arc<DecodedChunk>>,
    quota: &QuotaGroup,
    stop: &ilium_platform::owned_worker::StopToken,
    spec: RegionSpec,
    limits: RegionLimits,
    identity: &str,
    encoding_work: usize,
) -> Result<crate::world_region_encoding::EncodedRegion<StorageAdmission>, RegionError> {
    let source = collect_saved_region(chunks, quota, spec, limits, || stop.is_stopped())?;
    crate::world_region_encoding::encode_region(
        source,
        identity,
        spec.max_bytes,
        encoding_work,
        || stop.is_stopped(),
        |bytes| {
            quota
                .reserve_external_storage(bytes)
                .map_err(|_| RegionError::Admission)
        },
    )
}
