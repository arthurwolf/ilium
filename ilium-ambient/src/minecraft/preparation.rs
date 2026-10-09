//! Worker-side ownership handoff. Exact decoded chunks remain retained; borrowed
//! surface witnesses become small owned summaries, never cloned block catalogs.
use super::{evidence, loader, surface, windows};

const MAX_WINDOWS_PER_SAVE: usize = 3;

// This value keeps directory authority explicit across the common validation.
#[derive(Clone, Copy)]
enum WindowDirectory<'a> {
    Path(&'a std::path::Path),
    Pinned(&'a ilium_platform::animation_files::PinnedDirectory),
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub surface: surface::Limits,
    pub evidence: evidence::Limits,
    pub work_units: usize,
    /// Added summary vector and this struct, excluding retained loader storage.
    pub summary_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            surface: surface::Limits {
                max_chunks: 128,
                max_columns: 32_768,
                max_owned_bytes: 16_384,
            },
            evidence: evidence::Limits::default(),
            work_units: 16_000_000,
            summary_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub struct PreparedWindow {
    pub source: evidence::Source,
    pub core: surface::Bounds,
    pub bounds: surface::Bounds,
    pub loaded: loader::LoadedWindow,
    pub targets: Vec<evidence::TargetSummary>,
    pub stats: evidence::Stats,
    pub work_used: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Loader(#[from] loader::Error),
    #[error("invalid saved window candidate")]
    InvalidCandidate,
    #[error("saved preparation accepts at most sixteen candidate attempts")]
    CandidateLimit,
    #[error("saved preparation needs a nonzero scene generation")]
    InvalidGeneration,
    #[error(transparent)]
    Surface(#[from] surface::Error),
    #[error(transparent)]
    Evidence(#[from] evidence::Error),
    #[error("saved preparation contains unqualified input chunks")]
    UnqualifiedInput,
    #[error("saved preparation summary storage exceeds limit")]
    SummaryLimit,
    #[error("saved preparation work exceeds the combined window limit")]
    WorkLimit,
    #[error("saved preparation windows overlap or disagree on source identity")]
    OverlappingWindows,
}

/// A failed attempt retains only its typed error, never its decoded chunks.
#[derive(Debug)]
pub struct CandidateRejection {
    pub center: [i32; 2],
    pub error: Error,
}

#[derive(Debug)]
pub struct CandidateSelection {
    /// Index in the supplied finite candidate list. None means this list was
    /// exhausted, not that the save contains no other qualified terrain.
    pub selected_index: Option<usize>,
    /// Number of qualified, disjoint windows merged into `window`.
    pub successful_windows: usize,
    pub window: Option<PreparedWindow>,
    pub rejected: Vec<CandidateRejection>,
}

/// Sequential worker-only retry. At most three disjoint windows are retained
/// from the finite candidate list. Each decode has the supplied loader cap;
/// the aggregate therefore retains at most three 32 MiB-qualified windows per
/// save. This is not a scene-wide memory cap.
pub fn load_candidates(
    directory: &std::path::Path,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<CandidateSelection, Error> {
    load_candidates_with_progress(
        directory,
        source,
        candidates,
        loading,
        limits,
        cancelled,
        &mut |_| {},
    )
}

pub fn load_candidates_with_progress(
    directory: &std::path::Path,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<CandidateSelection, Error> {
    load_candidates_directory(
        WindowDirectory::Path(directory),
        source,
        candidates,
        loading,
        limits,
        cancelled,
        progress,
    )
}

/// Read only through the retained selected region descriptor. The caller owns
/// the original DiskRead operation and finite worker/storage admission.
pub fn load_candidates_pinned(
    directory: &ilium_platform::animation_files::PinnedDirectory,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<CandidateSelection, Error> {
    load_candidates_pinned_with_progress(
        directory,
        source,
        candidates,
        loading,
        limits,
        cancelled,
        &mut |_| {},
    )
}

pub fn load_candidates_pinned_with_progress(
    directory: &ilium_platform::animation_files::PinnedDirectory,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<CandidateSelection, Error> {
    load_candidates_directory(
        WindowDirectory::Pinned(directory),
        source,
        candidates,
        loading,
        limits,
        cancelled,
        progress,
    )
}

fn load_candidates_directory(
    directory: WindowDirectory<'_>,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<CandidateSelection, Error> {
    if cancelled() {
        return Err(loader::Error::Cancelled.into());
    }
    if source.generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    if loading.max_storage_charge == 0 || loading.max_storage_charge > 32 << 20 {
        return Err(Error::Loader(loader::Error::StorageLimit));
    }
    if limits.summary_bytes == 0 || limits.summary_bytes > 2 << 20 {
        return Err(Error::SummaryLimit);
    }
    let maximum_retained_charge = loading
        .max_storage_charge
        .checked_mul(MAX_WINDOWS_PER_SAVE)
        .ok_or(Error::SummaryLimit)?;
    let maximum_work_units = limits
        .work_units
        .checked_mul(MAX_WINDOWS_PER_SAVE)
        .ok_or(Error::WorkLimit)?;
    load_candidates_bounded(
        candidates,
        MAX_WINDOWS_PER_SAVE,
        limits.summary_bytes,
        maximum_retained_charge,
        maximum_work_units,
        cancelled,
        |candidate| {
            load_candidate_directory(
                directory, source, candidate, loading, limits, cancelled, progress,
            )
        },
    )
}

#[cfg(test)]
fn load_candidates_with(
    candidates: &[windows::Candidate],
    cancelled: &dyn Fn() -> bool,
    prepare: impl FnMut(&windows::Candidate) -> Result<PreparedWindow, Error>,
) -> Result<CandidateSelection, Error> {
    load_candidates_bounded(
        candidates,
        MAX_WINDOWS_PER_SAVE,
        Limits::default().summary_bytes,
        MAX_WINDOWS_PER_SAVE * (32 << 20),
        MAX_WINDOWS_PER_SAVE * Limits::default().work_units,
        cancelled,
        prepare,
    )
}

fn load_candidates_bounded(
    candidates: &[windows::Candidate],
    max_successful_windows: usize,
    summary_limit: usize,
    retained_charge_limit: usize,
    work_limit: usize,
    cancelled: &dyn Fn() -> bool,
    mut prepare: impl FnMut(&windows::Candidate) -> Result<PreparedWindow, Error>,
) -> Result<CandidateSelection, Error> {
    if cancelled() {
        return Err(loader::Error::Cancelled.into());
    }
    if candidates.len() > 16 || !(1..=MAX_WINDOWS_PER_SAVE).contains(&max_successful_windows) {
        return Err(Error::CandidateLimit);
    }
    let mut selection = CandidateSelection {
        selected_index: None,
        successful_windows: 0,
        window: None,
        rejected: Vec::with_capacity(candidates.len()),
    };
    let mut accepted: Vec<PreparedWindow> = Vec::with_capacity(max_successful_windows);
    let mut accepted_chunks = std::collections::BTreeSet::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if cancelled() {
            return Err(loader::Error::Cancelled.into());
        }
        if candidate
            .requested
            .iter()
            .any(|position| accepted_chunks.contains(position))
        {
            selection.rejected.push(CandidateRejection {
                center: candidate.center,
                error: Error::OverlappingWindows,
            });
            continue;
        }
        let result = prepare(candidate);
        // Cancellation can arrive after the last inner checkpoint. Never
        // publish that output or continue another expensive decode attempt.
        if cancelled() {
            return Err(loader::Error::Cancelled.into());
        }
        match result {
            Ok(window) => {
                let positions: std::collections::BTreeSet<_> =
                    window.loaded.chunks.keys().copied().collect();
                if window.loaded.chunks.is_empty()
                    || window.source.generation == 0
                    || window.loaded.coverage.chunks != positions
                    || positions.iter().any(|position| {
                        !candidate.requested.contains(position)
                            || accepted_chunks.contains(position)
                    })
                {
                    selection.rejected.push(CandidateRejection {
                        center: candidate.center,
                        error: Error::UnqualifiedInput,
                    });
                    continue;
                }
                if let Some(first) = accepted.first() {
                    if first.source != window.source {
                        selection.rejected.push(CandidateRejection {
                            center: candidate.center,
                            error: Error::OverlappingWindows,
                        });
                        continue;
                    }
                }
                selection.selected_index.get_or_insert(index);
                accepted_chunks.extend(positions);
                accepted.push(window);
                if accepted.len() == max_successful_windows {
                    break;
                }
            }
            Err(
                Error::Loader(loader::Error::Cancelled)
                | Error::Surface(surface::Error::Cancelled)
                | Error::Evidence(evidence::Error::Surface(surface::Error::Cancelled)),
            ) => return Err(loader::Error::Cancelled.into()),
            Err(error) => selection.rejected.push(CandidateRejection {
                center: candidate.center,
                error,
            }),
        }
    }
    selection.successful_windows = accepted.len();
    if !accepted.is_empty() {
        selection.window = Some(merge_windows(
            accepted,
            max_successful_windows,
            summary_limit,
            retained_charge_limit,
            work_limit,
        )?);
    }
    Ok(selection)
}

fn merge_windows(
    mut windows: Vec<PreparedWindow>,
    maximum_windows: usize,
    summary_limit: usize,
    retained_charge_limit: usize,
    work_limit: usize,
) -> Result<PreparedWindow, Error> {
    use std::mem::size_of;

    if windows.is_empty() || windows.len() > maximum_windows {
        return Err(Error::UnqualifiedInput);
    }
    let mut merged = windows.remove(0);
    let mut accepted_chunks: std::collections::BTreeSet<_> =
        merged.loaded.chunks.keys().copied().collect();
    for mut window in windows {
        if window.source != merged.source
            || window
                .loaded
                .chunks
                .keys()
                .any(|position| accepted_chunks.contains(position))
        {
            return Err(Error::OverlappingWindows);
        }

        for axis in 0..2 {
            merged.core.minimum[axis] = merged.core.minimum[axis].min(window.core.minimum[axis]);
            merged.core.maximum[axis] = merged.core.maximum[axis].max(window.core.maximum[axis]);
            merged.bounds.minimum[axis] =
                merged.bounds.minimum[axis].min(window.bounds.minimum[axis]);
            merged.bounds.maximum[axis] =
                merged.bounds.maximum[axis].max(window.bounds.maximum[axis]);
        }

        let target_count = merged
            .targets
            .len()
            .checked_add(window.targets.len())
            .ok_or(Error::SummaryLimit)?;
        let summary_bytes = target_count
            .checked_mul(size_of::<evidence::TargetSummary>())
            .and_then(|bytes| bytes.checked_add(size_of::<PreparedWindow>()))
            .ok_or(Error::SummaryLimit)?;
        if summary_bytes > summary_limit {
            return Err(Error::SummaryLimit);
        }
        merged
            .targets
            .try_reserve_exact(window.targets.len())
            .map_err(|_| Error::SummaryLimit)?;
        if merged
            .targets
            .capacity()
            .checked_mul(size_of::<evidence::TargetSummary>())
            .and_then(|bytes| bytes.checked_add(size_of::<PreparedWindow>()))
            .is_none_or(|bytes| bytes > summary_limit)
        {
            return Err(Error::SummaryLimit);
        }
        merged.targets.append(&mut window.targets);

        merged
            .loaded
            .issues
            .try_reserve(window.loaded.issues.len())
            .map_err(|_| Error::SummaryLimit)?;
        merged.loaded.issues.append(&mut window.loaded.issues);
        merged.loaded.rejected_chunks = merged
            .loaded
            .rejected_chunks
            .checked_add(window.loaded.rejected_chunks)
            .ok_or(Error::SummaryLimit)?;
        merged.loaded.retained_storage_charge = merged
            .loaded
            .retained_storage_charge
            .checked_add(window.loaded.retained_storage_charge)
            .ok_or(Error::SummaryLimit)?;
        merged.work_used = merged
            .work_used
            .checked_add(window.work_used)
            .ok_or(Error::WorkLimit)?;
        if merged.work_used > work_limit {
            return Err(Error::WorkLimit);
        }
        merged.stats = add_stats(merged.stats, window.stats)?;

        for (&position, chunk) in &window.loaded.chunks {
            if position != chunk.identity.position || !accepted_chunks.insert(position) {
                return Err(Error::OverlappingWindows);
            }
            merged.loaded.coverage.chunks.insert(position);
            merged
                .loaded
                .chunks
                .insert(position, std::sync::Arc::clone(chunk));
        }
        if window.loaded.coverage.chunks.iter().any(|position| {
            !window.loaded.chunks.contains_key(position)
                || !merged.loaded.chunks.contains_key(position)
        }) {
            return Err(Error::UnqualifiedInput);
        }
    }
    if merged.loaded.coverage.chunks.len() != merged.loaded.chunks.len() {
        return Err(Error::UnqualifiedInput);
    }
    if merged.loaded.retained_storage_charge > retained_charge_limit {
        return Err(Error::Loader(loader::Error::StorageLimit));
    }
    if merged.work_used > work_limit {
        return Err(Error::WorkLimit);
    }
    Ok(merged)
}

fn add_stats(mut total: evidence::Stats, next: evidence::Stats) -> Result<evidence::Stats, Error> {
    total.tiles = total
        .tiles
        .checked_add(next.tiles)
        .ok_or(Error::WorkLimit)?;
    total.construction_windows = total
        .construction_windows
        .checked_add(next.construction_windows)
        .ok_or(Error::WorkLimit)?;
    total.columns = total
        .columns
        .checked_add(next.columns)
        .ok_or(Error::WorkLimit)?;
    total.edge_columns_not_surveyed = total
        .edge_columns_not_surveyed
        .checked_add(next.edge_columns_not_surveyed)
        .ok_or(Error::WorkLimit)?;
    total.empty_columns = total
        .empty_columns
        .checked_add(next.empty_columns)
        .ok_or(Error::WorkLimit)?;
    total.band_limited_columns = total
        .band_limited_columns
        .checked_add(next.band_limited_columns)
        .ok_or(Error::WorkLimit)?;
    total.unrecognized_columns = total
        .unrecognized_columns
        .checked_add(next.unrecognized_columns)
        .ok_or(Error::WorkLimit)?;
    total.qualifying_components = total
        .qualifying_components
        .checked_add(next.qualifying_components)
        .ok_or(Error::WorkLimit)?;
    total.operations = total
        .operations
        .checked_add(next.operations)
        .ok_or(Error::WorkLimit)?;
    Ok(total)
}

/// Blocking adapter for an owned preparation worker. Header candidates are
/// independently decoded and qualified here; rejected chunks never become air.
/// Caller associates directory and stable map identity; neither is inferred.
pub fn load_candidate(
    directory: &std::path::Path,
    source: evidence::Source,
    candidate: &windows::Candidate,
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedWindow, Error> {
    load_candidate_with_progress(
        directory,
        source,
        candidate,
        loading,
        limits,
        cancelled,
        &mut |_| {},
    )
}

pub fn load_candidate_with_progress(
    directory: &std::path::Path,
    source: evidence::Source,
    candidate: &windows::Candidate,
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<PreparedWindow, Error> {
    load_candidate_directory(
        WindowDirectory::Path(directory),
        source,
        candidate,
        loading,
        limits,
        cancelled,
        progress,
    )
}

/// Read only through the retained selected region descriptor. The caller owns
/// the original DiskRead operation and finite worker/storage admission.
pub fn load_candidate_pinned(
    directory: &ilium_platform::animation_files::PinnedDirectory,
    source: evidence::Source,
    candidate: &windows::Candidate,
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedWindow, Error> {
    load_candidate_pinned_with_progress(
        directory,
        source,
        candidate,
        loading,
        limits,
        cancelled,
        &mut |_| {},
    )
}

pub fn load_candidate_pinned_with_progress(
    directory: &ilium_platform::animation_files::PinnedDirectory,
    source: evidence::Source,
    candidate: &windows::Candidate,
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<PreparedWindow, Error> {
    load_candidate_directory(
        WindowDirectory::Pinned(directory),
        source,
        candidate,
        loading,
        limits,
        cancelled,
        progress,
    )
}

fn load_candidate_directory(
    directory: WindowDirectory<'_>,
    source: evidence::Source,
    candidate: &windows::Candidate,
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(loader::LoadProgress),
) -> Result<PreparedWindow, Error> {
    if cancelled() {
        return Err(loader::Error::Cancelled.into());
    }
    if source.generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    if candidate.requested.is_empty() {
        return Err(Error::InvalidCandidate);
    }
    let requested = &candidate.requested;
    if requested.len() > loading.max_chunks.min(128) {
        return Err(loader::Error::Limit.into());
    }
    let minimum = [
        requested.iter().map(|p| p[0]).min(),
        requested.iter().map(|p| p[1]).min(),
    ];
    let maximum = [
        requested.iter().map(|p| p[0]).max(),
        requested.iter().map(|p| p[1]).max(),
    ];
    let boundary = |values: [Option<i32>; 2], last: bool| -> Option<[i32; 2]> {
        let convert = |value: Option<i32>| {
            value?
                .checked_mul(16)?
                .checked_add(if last { 15 } else { 0 })
        };
        Some([convert(values[0])?, convert(values[1])?])
    };
    if boundary(minimum, false) != Some(candidate.bounds.minimum)
        || boundary(maximum, true) != Some(candidate.bounds.maximum)
    {
        return Err(Error::InvalidCandidate);
    }
    let widths = [0, 1].map(|axis| {
        i64::from(candidate.bounds.maximum[axis]) - i64::from(candidate.bounds.minimum[axis]) + 1
    });
    let side = widths[0] / 16;
    if widths[0] != widths[1]
        || side > 11
        || side % 2 != 1
        || side * side != requested.len() as i64
        || [0, 1].into_iter().any(|axis| {
            i64::from(candidate.bounds.minimum[axis]) / 16 + side / 2
                != i64::from(candidate.center[axis])
        })
    {
        return Err(Error::InvalidCandidate);
    }
    let loaded = match directory {
        WindowDirectory::Path(directory) => {
            loader::load_window_with_progress(directory, requested, loading, cancelled, progress)?
        }
        WindowDirectory::Pinned(directory) => loader::load_window_pinned_with_progress(
            directory, requested, loading, cancelled, progress,
        )?,
    };
    finish_window(loaded, source, candidate.bounds, 0, limits, cancelled)
}

/// Run only in the owned preparation worker. The caller supplies one map's
/// loaded chunks and its stable map identity/current generation. No discovery,
/// map selection, model readiness, rendering or shown-history is inferred.
/// Core and halo must both be complete; any failure discards the entire output.
pub fn finish_window(
    mut loaded: loader::LoadedWindow,
    source: evidence::Source,
    core: surface::Bounds,
    halo: u16,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedWindow, Error> {
    use std::{mem::size_of, sync::Arc};
    let mut work = surface::Work::new(limits.work_units, cancelled);
    work.checkpoint()?;
    if source.generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    if size_of::<PreparedWindow>() > limits.summary_bytes {
        return Err(Error::SummaryLimit);
    }
    // A public LoadedWindow can be constructed outside the loader. Never let
    // map keys disagree with the coordinates the renderer will later sample.
    for (position, chunk) in &loaded.chunks {
        work.checkpoint()?;
        if *position != chunk.identity.position {
            return Err(Error::UnqualifiedInput);
        }
    }
    let (bounds, coverage, targets, stats) = {
        let window = surface::SurfaceWindow::overworld_refs(
            core,
            halo,
            loaded.chunks.values().map(Arc::as_ref),
            surface::Limits {
                max_chunks: limits.surface.max_chunks.min(128),
                ..limits.surface
            },
            &mut work,
        )?;
        window.require_complete(&mut work)?;
        let coverage = window.qualified_coverage(&mut work)?;
        if coverage.chunks.len() != loaded.chunks.len() {
            return Err(Error::UnqualifiedInput);
        }
        let report = evidence::analyze(&window, source, limits.evidence, &mut work)?;
        let bytes = |capacity: usize| {
            capacity
                .checked_mul(size_of::<evidence::TargetSummary>())
                .and_then(|bytes| bytes.checked_add(size_of::<PreparedWindow>()))
        };
        if bytes(report.targets.len()).is_none_or(|bytes| bytes > limits.summary_bytes) {
            return Err(Error::SummaryLimit);
        }
        let mut targets = Vec::new();
        targets
            .try_reserve_exact(report.targets.len())
            .map_err(|_| Error::SummaryLimit)?;
        if bytes(targets.capacity()).is_none_or(|bytes| bytes > limits.summary_bytes) {
            return Err(Error::SummaryLimit);
        }
        for target in &report.targets {
            work.checkpoint()?;
            targets.push(target.summary());
        }
        (window.bounds(), coverage, targets, report.stats)
    };
    work.checkpoint()?;
    // Derive coverage from the retained decoded data, never a caller's claim.
    loaded.coverage = coverage;
    Ok(PreparedWindow {
        source,
        core,
        bounds,
        loaded,
        targets,
        stats,
        work_used: work.used(),
    })
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
