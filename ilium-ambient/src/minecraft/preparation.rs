//! Worker-side ownership handoff. Exact decoded chunks remain retained; borrowed
//! surface witnesses become small owned summaries, never cloned block catalogs.
use super::{evidence, loader, surface, windows};

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
    pub window: Option<PreparedWindow>,
    pub rejected: Vec<CandidateRejection>,
}

/// Sequential worker-only retry. The selector defaults to eight candidates;
/// this adapter enforces its hard sixteen-output ceiling. Each attempt has the
/// supplied loader/storage/work limits; this is not a scene-wide memory cap.
pub fn load_candidates(
    directory: &std::path::Path,
    source: evidence::Source,
    candidates: &[windows::Candidate],
    loading: loader::Limits,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<CandidateSelection, Error> {
    if cancelled() {
        return Err(loader::Error::Cancelled.into());
    }
    if source.generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    load_candidates_with(candidates, cancelled, |candidate| {
        load_candidate(directory, source, candidate, loading, limits, cancelled)
    })
}

fn load_candidates_with(
    candidates: &[windows::Candidate],
    cancelled: &dyn Fn() -> bool,
    mut prepare: impl FnMut(&windows::Candidate) -> Result<PreparedWindow, Error>,
) -> Result<CandidateSelection, Error> {
    if cancelled() {
        return Err(loader::Error::Cancelled.into());
    }
    if candidates.len() > 16 {
        return Err(Error::CandidateLimit);
    }
    let mut selection = CandidateSelection {
        selected_index: None,
        window: None,
        rejected: Vec::with_capacity(candidates.len()),
    };
    for (index, candidate) in candidates.iter().enumerate() {
        if cancelled() {
            return Err(loader::Error::Cancelled.into());
        }
        let result = prepare(candidate);
        // Cancellation can arrive after the last inner checkpoint. Never
        // publish that output or continue another expensive decode attempt.
        if cancelled() {
            return Err(loader::Error::Cancelled.into());
        }
        match result {
            Ok(window) => {
                selection.selected_index = Some(index);
                selection.window = Some(window);
                return Ok(selection);
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
    Ok(selection)
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
    let loaded = loader::load_window(directory, requested, loading, cancelled)?;
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
