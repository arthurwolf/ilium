//! Worker-only partition of the complete projected source into one saved chunk.
//! The source request proves the entire continuous route's potential viewport;
//! this partition contains every exact non-air cell that may paint from its
//! chunk, plus a one-cell non-air model-culling halo. The halo is available to
//! the binder but only the 16x16 core emits faces. No global height band or
//! buried-block guess is used; fluid/biome support remains in the source map.
use super::{render_cells::RenderCells, source_footprint::Request, surface, tours::PreparedMap};
use crate::voxel_landscape::assets::{
    budget::{ByteBudget, Cancel, Reservation},
    error::AssetError,
};
use std::{mem::size_of, sync::Arc};

const MAX_TILE_CELLS: usize = 18 * 18 * 384;
const TILE_CHARGE: u64 = 2 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved projected source or chunk is not fully qualified")]
    Source,
    #[error("saved projected tile is empty")]
    Empty,
    #[error("saved projected tile work/allocation limit")]
    Limit,
    #[error(transparent)]
    Asset(#[from] AssetError),
}

pub struct PreparedTile {
    pub cells: RenderCells,
    _reservation: Reservation,
}

/// Exact cells only. Every support chunk must already be decoded and checked
/// by `PreparedMap::new`; a missing state is an error, never assumed air. The
/// fixed 2 MiB reservation covers the vector's worst admissible capacity and
/// retained RenderCells struct for this tile in the shared Scene account.
pub fn prepare(
    map: Arc<PreparedMap>,
    request: &Request,
    chunk: [i32; 2],
    budget: &ByteBudget,
    cancel: Cancel<'_>,
) -> Result<PreparedTile, Error> {
    cancel.check()?;
    if !request.render_chunks().contains(&chunk)
        || !map
            .loaded()
            .coverage
            .chunks
            .is_superset(request.support_chunks())
        || map.loaded().chunks.len() > super::source_footprint::MAX_REQUESTED_CHUNKS
    {
        return Err(Error::Source);
    }
    let lower = [
        chunk[0].checked_mul(16).ok_or(Error::Source)?,
        chunk[1].checked_mul(16).ok_or(Error::Source)?,
    ];
    let upper = [
        lower[0].checked_add(15).ok_or(Error::Source)?,
        lower[1].checked_add(15).ok_or(Error::Source)?,
    ];
    let halo_minimum = [
        lower[0].checked_sub(1).ok_or(Error::Source)?,
        lower[1].checked_sub(1).ok_or(Error::Source)?,
    ];
    let halo_maximum = [
        upper[0].checked_add(1).ok_or(Error::Source)?,
        upper[1].checked_add(1).ok_or(Error::Source)?,
    ];
    let core = surface::Bounds {
        minimum: lower,
        maximum: upper,
    };
    let reservation = budget.reserve(TILE_CHARGE, cancel)?;
    let mut positions = Vec::<[i32; 3]>::new();
    positions
        .try_reserve_exact(MAX_TILE_CELLS)
        .map_err(|_| Error::Limit)?;
    let storage_charge = positions
        .capacity()
        .checked_mul(size_of::<[i32; 3]>())
        .and_then(|bytes| bytes.checked_add(size_of::<RenderCells>()))
        .ok_or(Error::Limit)?;
    if storage_charge as u64 > TILE_CHARGE {
        return Err(Error::Limit);
    }
    let mut heights = [i32::MAX, i32::MIN];
    let mut work_used = 0usize;
    for z in halo_minimum[1]..=halo_maximum[1] {
        for x in halo_minimum[0]..=halo_maximum[0] {
            cancel.check()?;
            let mut band = [i32::MAX, i32::MIN];
            for neighbor_z in z - 1..=z + 1 {
                for neighbor_x in x - 1..=x + 1 {
                    if !(lower[0]..=upper[0]).contains(&neighbor_x)
                        || !(lower[1]..=upper[1]).contains(&neighbor_z)
                    {
                        continue;
                    }
                    if let Some([minimum, maximum]) = request.column_band([neighbor_x, neighbor_z])
                    {
                        band[0] = band[0].min(minimum);
                        band[1] = band[1].max(maximum);
                    }
                }
            }
            if band[0] > band[1] {
                continue;
            }
            let minimum_y = (band[0] - 1).max(-64);
            let maximum_y = (band[1] + 1).min(319);
            for y in (minimum_y..=maximum_y).rev() {
                cancel.check()?;
                work_used = work_used
                    .checked_add(1)
                    .filter(|used| *used <= MAX_TILE_CELLS)
                    .ok_or(Error::Limit)?;
                let position = [x, y, z];
                let state = map.state(position).ok_or(Error::Source)?;
                if state.is_air() {
                    continue;
                }
                positions.push(position);
                heights[0] = heights[0].min(y);
                heights[1] = heights[1].max(y);
            }
        }
    }
    if positions.is_empty() {
        return Err(Error::Empty);
    }
    cancel.check()?;
    Ok(PreparedTile {
        cells: RenderCells {
            map,
            core,
            heights,
            positions,
            work_used,
            storage_charge,
        },
        _reservation: reservation,
    })
}
