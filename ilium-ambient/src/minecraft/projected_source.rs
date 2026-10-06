//! Worker-only qualification of the complete inverse-projected saved source.
//! A header allocation check is insufficient: every requested block palette,
//! biome palette and source identity must survive the ordinary decoder and
//! PreparedMap checks before a route may publish pixels.
use super::{
    history_store::{self, BoundMap},
    loader,
    source_footprint::{self, Request},
    tours::{self, PreparedMap},
};
use crate::voxel_landscape::assets::{
    budget::{ByteBudget, Cancel},
    error::AssetError,
};
use std::{io, path::Path, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("projected saved source identity or chunk request changed")]
    Source,
    #[error("projected source missing {missing} decoded chunks, first {first:?}")]
    Unqualified {
        missing: usize,
        first: Option<[i32; 2]>,
        missing_positions: Vec<[i32; 2]>,
    },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Binding(#[from] history_store::Error),
    #[error(transparent)]
    Loading(#[from] loader::Error),
    #[error(transparent)]
    Tour(#[from] tours::Error),
    #[error(transparent)]
    Asset(#[from] AssetError),
}

pub struct QualifiedSource {
    // The projected reservation is INSIDE PreparedMap. Every map Arc retained
    // by a tile, owner receipt or renderer therefore retains its accounting.
    pub map: Arc<PreparedMap>,
}

/// Use only on an owned blocking preparation worker. `base` is the original
/// evidence/route map; its target inventory is left intact. The separate
/// renderer map keeps the exact Source and empty target list, since it is for
/// state/biome lookup and painted-owner validation, not reclassification.
pub fn qualify(
    base: &PreparedMap,
    bound: &BoundMap,
    saves_root: &Path,
    request: &Request,
    account: &ByteBudget,
    cancel: Cancel<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<QualifiedSource, Error> {
    cancel.check()?;
    if !saves_root.is_absolute()
        || base.source().map != bound.map
        || request.support_chunks().is_empty()
        || request.support_chunks().len() > source_footprint::MAX_REQUESTED_CHUNKS
        || !request.render_chunks().is_subset(request.support_chunks())
    {
        return Err(Error::Source);
    }
    // The binding uses the platform canonical spelling, including Windows.
    let canonical_root = ilium_platform::paths::canonicalize(saves_root)?;
    bound.verify(&canonical_root)?;
    let mut decoded_reservation =
        account.reserve(loader::MAX_PROJECTED_STORAGE_CHARGE as u64, cancel)?;
    let region_directory = bound.directory.join("region");
    let loaded =
        loader::load_projected_window(&region_directory, request.support_chunks(), cancelled)?;
    cancel.check()?;
    bound.verify(&canonical_root)?;
    if &loaded.coverage.chunks != request.support_chunks() {
        let missing_positions = request
            .support_chunks()
            .difference(&loaded.coverage.chunks)
            .copied()
            .collect::<Vec<_>>();
        return Err(Error::Unqualified {
            missing: missing_positions.len(),
            first: missing_positions.first().copied(),
            missing_positions,
        });
    }
    // Decoding holds the full ceiling. Retained map Arcs need only the loader's
    // conservative capacity-based charge, including rejected-read allowance.
    decoded_reservation.shrink_to(loaded.retained_storage_charge as u64)?;
    let mut tour_budget = tours::Budget::new(16_000_000, cancelled);
    let map =
        Arc::new(base.projected_source(Arc::new(loaded), decoded_reservation, &mut tour_budget)?);
    cancel.check()?;
    bound.verify(&canonical_root)?;
    Ok(QualifiedSource { map })
}
