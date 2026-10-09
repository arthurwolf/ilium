//! Bounded header-candidate search; only the loader can qualify terrain.
use super::surface::Bounds;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub radius_chunks: u8,
    pub candidates: usize,
    /// Number of output slots reserved for the farthest complete windows.
    /// This keeps large saved worlds from collapsing to spawn-local evidence.
    pub spread_candidates: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            radius_chunks: 5,
            candidates: 8,
            spread_candidates: 0,
            work_units: 1_000_000,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub center: [i32; 2],
    pub requested: BTreeSet<[i32; 2]>,
    pub bounds: Bounds,
}
#[derive(Debug, Default)]
pub struct Search {
    /// Candidates retained from the examined prefix, optionally interleaving
    /// the nearest and farthest complete windows. Selection is not a decode,
    /// a coverage result, or evidence of novelty.
    pub candidates: Vec<Candidate>,
    pub work_used: usize,
    /// Complete header footprints considered after recent/anchor exclusions.
    /// Can exceed the output cap, even when scan_complete is true.
    pub header_complete: usize,
    /// False means an unfinished scan, never proof that other windows are absent.
    pub scan_complete: bool,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid saved candidate search limits")]
    Limits,
    #[error("saved candidate search cancelled")]
    Cancelled,
}

/// Retain a useful progress cadence without reporting every membership check.
const PROGRESS_REPORTS_PER_SEARCH: usize = 128;

/// Coordinates and recent centers are chunk [x,z]; anchor is a candidate only.
/// Grid spacing equals window width, so grid windows do not overlap. The anchor
/// is tried independently to avoid missing small saves between grid centers.
/// This finite grid is not an exhaustive search of every possible footprint.
/// Recent centers must be from this map with the same requested window radius.
/// Work counts candidate visits, recent comparisons and membership checks;
/// BTree lookup and sorting costs are additionally bounded by collection caps.
pub fn search(
    allocated: &BTreeSet<[i32; 2]>,
    anchor: [i32; 2],
    recent: &[[i32; 2]],
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Search, Error> {
    search_with_progress(allocated, anchor, recent, limits, cancelled, &mut |_, _| {})
}

/// Search bounded candidate windows and report counted work against the fixed
/// budget. A successful search may use only part of that budget; callers that
/// display phase completion should close out the phase using `work_used`.
pub fn search_with_progress(
    allocated: &BTreeSet<[i32; 2]>,
    anchor: [i32; 2],
    recent: &[[i32; 2]],
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Search, Error> {
    if cancelled() {
        return Err(Error::Cancelled);
    }
    if limits.radius_chunks > 5
        || limits.candidates == 0
        || limits.candidates > 16
        || limits.spread_candidates > limits.candidates
        || limits.work_units == 0
        || limits.work_units > 8_000_000
        || allocated.len() > 262_144
        || recent.len() > 64
    {
        return Err(Error::Limits);
    }
    progress(0, limits.work_units);
    let mut work = Work {
        used: 0,
        limit: limits.work_units,
        report_interval: limits.work_units.div_ceil(PROGRESS_REPORTS_PER_SEARCH),
        last_reported: 0,
        exhausted: false,
        cancelled,
        progress,
    };
    let mut result = Search::default();
    let mut nearest = Vec::new();
    let mut farthest = Vec::new();
    let side = i64::from(limits.radius_chunks) * 2 + 1;
    let first = candidate(allocated, anchor, recent, limits.radius_chunks, &mut work)?;
    let anchor_admitted = first.is_some();
    if let Some(first) = first {
        result.header_complete += 1;
        retain_nearest(&mut nearest, first.clone(), anchor, limits.candidates);
        retain_farthest(&mut farthest, first, anchor, limits.spread_candidates);
    }
    for &center in allocated {
        if !work.step()? {
            break;
        }
        if center == anchor
            || center.iter().any(|c| i64::from(*c).rem_euclid(side) != 0)
            || (anchor_admitted && distance(center, anchor) < side)
        {
            continue;
        }
        if let Some(candidate) =
            candidate(allocated, center, recent, limits.radius_chunks, &mut work)?
        {
            result.header_complete += 1;
            retain_nearest(&mut nearest, candidate.clone(), anchor, limits.candidates);
            retain_farthest(&mut farthest, candidate, anchor, limits.spread_candidates);
        }
        if work.exhausted {
            break;
        }
    }
    if cancelled() {
        return Err(Error::Cancelled);
    }
    result.work_used = work.used;
    result.scan_complete = !work.exhausted;
    result.candidates = interleave(nearest, farthest, limits.candidates);
    work.report_final();
    Ok(result)
}

fn retain_nearest(
    candidates: &mut Vec<Candidate>,
    candidate: Candidate,
    anchor: [i32; 2],
    limit: usize,
) {
    if limit == 0 {
        return;
    }
    candidates.push(candidate);
    candidates.sort_by_key(|candidate| (distance(candidate.center, anchor), candidate.center));
    candidates.truncate(limit);
}

fn retain_farthest(
    candidates: &mut Vec<Candidate>,
    candidate: Candidate,
    anchor: [i32; 2],
    limit: usize,
) {
    if limit == 0 {
        return;
    }
    candidates.push(candidate);
    candidates.sort_by_key(|candidate| {
        (
            std::cmp::Reverse(distance(candidate.center, anchor)),
            candidate.center,
        )
    });
    candidates.truncate(limit);
}

fn interleave(nearest: Vec<Candidate>, farthest: Vec<Candidate>, limit: usize) -> Vec<Candidate> {
    let mut output = Vec::with_capacity(limit);
    let mut nearest = nearest.into_iter();
    let mut farthest = farthest.into_iter();
    while output.len() < limit {
        let mut progressed = false;
        if let Some(candidate) = nearest.next() {
            output.push(candidate);
            progressed = true;
        }
        if output.len() == limit {
            break;
        }
        if let Some(candidate) = farthest.next() {
            if output
                .iter()
                .all(|selected| selected.center != candidate.center)
            {
                output.push(candidate);
            }
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    output
}

fn distance(left: [i32; 2], right: [i32; 2]) -> i64 {
    (i64::from(left[0]) - i64::from(right[0]))
        .abs()
        .max((i64::from(left[1]) - i64::from(right[1])).abs())
}

struct Work<'a> {
    used: usize,
    limit: usize,
    report_interval: usize,
    last_reported: usize,
    exhausted: bool,
    cancelled: &'a dyn Fn() -> bool,
    progress: &'a mut dyn FnMut(usize, usize),
}
impl Work<'_> {
    fn step(&mut self) -> Result<bool, Error> {
        if (self.cancelled)() {
            return Err(Error::Cancelled);
        }
        if self.used == self.limit {
            self.exhausted = true;
            return Ok(false);
        }
        self.used += 1;
        if self.used - self.last_reported >= self.report_interval || self.used == self.limit {
            self.report_progress();
        }
        Ok(true)
    }

    fn report_final(&mut self) {
        if self.used > self.last_reported {
            self.report_progress();
        }
    }

    fn report_progress(&mut self) {
        (self.progress)(self.used, self.limit);
        self.last_reported = self.used;
    }
}

fn candidate(
    allocated: &BTreeSet<[i32; 2]>,
    center: [i32; 2],
    recent: &[[i32; 2]],
    radius: u8,
    work: &mut Work<'_>,
) -> Result<Option<Candidate>, Error> {
    if !work.step()? {
        return Ok(None);
    }
    let side = i64::from(radius) * 2 + 1;
    for &old in recent {
        if !work.step()? || distance(center, old) < side {
            return Ok(None);
        }
    }
    let radius = i32::from(radius);
    let extent = |c: i32| -> Option<(i32, i32, i32, i32)> {
        let low = c.checked_sub(radius)?;
        let high = c.checked_add(radius)?;
        Some((
            low,
            high,
            low.checked_mul(16)?,
            high.checked_mul(16)?.checked_add(15)?,
        ))
    };
    let (Some(x), Some(z)) = (extent(center[0]), extent(center[1])) else {
        return Ok(None);
    };
    let mut requested = BTreeSet::new();
    for cz in z.0..=z.1 {
        for cx in x.0..=x.1 {
            if !work.step()? || !allocated.contains(&[cx, cz]) {
                return Ok(None);
            }
            requested.insert([cx, cz]);
        }
    }
    Ok(Some(Candidate {
        center,
        requested,
        bounds: Bounds {
            minimum: [x.2, z.2],
            maximum: [x.3, z.3],
        },
    }))
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
