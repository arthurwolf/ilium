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

/// Original selected descriptor chain. Path values are persistence labels only.
#[derive(Clone, Copy)]
pub struct PinnedSource<'a> {
    pub root_label: &'a Path,
    pub root: &'a ilium_platform::animation_files::PinnedDirectory,
    pub child: &'a ilium_platform::animation_files::PinnedDirectory,
    pub region: &'a ilium_platform::animation_files::PinnedDirectory,
}
#[derive(Clone, Copy)]
enum SourceDirectory<'a> {
    Path(&'a Path),
    Pinned(PinnedSource<'a>),
}
impl SourceDirectory<'_> {
    fn verify(self, bound: &BoundMap) -> Result<(), Error> {
        match self {
            Self::Path(root) => {
                if !root.is_absolute() {
                    return Err(Error::Source);
                }
                let canonical_root = ilium_platform::paths::canonicalize(root)?;
                bound.verify(&canonical_root)?;
            }
            Self::Pinned(source) => {
                bound.verify_pinned(source.root_label, source.root, source.child)?;
                if source.child.child("region", false)?.identity() != source.region.identity() {
                    return Err(Error::Source);
                }
            }
        }
        Ok(())
    }
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
    qualify_source(
        base,
        bound,
        SourceDirectory::Path(saves_root),
        request,
        account,
        cancel,
        cancelled,
    )
}

/// Qualify full projected coverage below original selected descriptors only.
pub fn qualify_pinned(
    base: &PreparedMap,
    bound: &BoundMap,
    source: PinnedSource<'_>,
    request: &Request,
    account: &ByteBudget,
    cancel: Cancel<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<QualifiedSource, Error> {
    qualify_source(
        base,
        bound,
        SourceDirectory::Pinned(source),
        request,
        account,
        cancel,
        cancelled,
    )
}

fn qualify_source(
    base: &PreparedMap,
    bound: &BoundMap,
    source: SourceDirectory<'_>,
    request: &Request,
    account: &ByteBudget,
    cancel: Cancel<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<QualifiedSource, Error> {
    cancel.check()?;
    if base.source().map != bound.map
        || request.support_chunks().is_empty()
        || request.support_chunks().len() > source_footprint::MAX_REQUESTED_CHUNKS
        || !request.render_chunks().is_subset(request.support_chunks())
    {
        return Err(Error::Source);
    }
    source.verify(bound)?;
    let mut decoded_reservation =
        account.reserve(loader::MAX_PROJECTED_STORAGE_CHARGE as u64, cancel)?;
    let region_directory = bound.directory.join("region");
    let loaded = match source {
        SourceDirectory::Path(_) => {
            loader::load_projected_window(&region_directory, request.support_chunks(), cancelled)?
        }
        SourceDirectory::Pinned(source) => loader::load_projected_window_pinned(
            source.region,
            request.support_chunks(),
            cancelled,
        )?,
    };
    cancel.check()?;
    source.verify(bound)?;
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
    source.verify(bound)?;
    Ok(QualifiedSource { map })
}
