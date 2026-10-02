//! Saved-only tour selection and presentation-confirmed appearance history.
//! This module supplies no renderer. Final displayed block owners are a required
//! host adapter input, never inferred from loading, projection or model presence.
use super::{
    chunk::{BlockSample, BlockState},
    coverage::Line,
    evidence::{self, Category, Confidence, Kind, MapId, Source, TargetKey, TargetSummary},
    loader::LoadedWindow,
    region,
    surface::{MAX_Y, MIN_Y},
};
use serde::{Deserialize, Serialize};
use std::{cmp::Reverse, sync::Arc, time::Duration};

const MAX_MAPS: usize = 16;
const MAX_CHUNKS: usize = 128;
const MAX_TARGETS: usize = 2304;
const MAX_OWNERS: usize = 8192;
const RECENT_RUNS: u64 = 8;
const MAX_DELTA: Duration = Duration::from_secs(2);
const DIRECTIONS: [[f64; 2]; 8] = [
    [1.0, 0.0],
    [0.0, 1.0],
    [1.0, 1.0],
    [1.0, -1.0],
    [2.0, 1.0],
    [1.0, 2.0],
    [2.0, -1.0],
    [1.0, -2.0],
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("tour cancelled")]
    Cancelled,
    #[error("tour limit: {0}")]
    Limit(&'static str),
    #[error("invalid tour input: {0}")]
    Invalid(&'static str),
    #[error("stale tour request or presentation")]
    Stale,
    #[error("tour is already active")]
    Busy,
    #[error("no active tour")]
    Idle,
    #[error("route endpoint has not been presented")]
    NotComplete,
    #[error("animation clock moved backwards")]
    ClockReversed,
    #[error("tour transition is frozen")]
    Frozen,
}
/// Caller-owned cumulative budget. Charges include conservative membership-test
/// reservations before each non-cancellable Coverage::line_through invocation.
/// Cancellation callbacks must be bounded/nonblocking; an observed true is final.
pub struct Budget<'a> {
    used: u64,
    limit: u64,
    cancelled: &'a dyn Fn() -> bool,
}
impl<'a> Budget<'a> {
    pub fn new(limit: u64, cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            used: 0,
            limit,
            cancelled,
        }
    }
    pub fn used(&self) -> u64 {
        self.used
    }
    pub fn check(&self) -> Result<(), Error> {
        if (self.cancelled)() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    fn charge(&mut self, amount: u64) -> Result<(), Error> {
        self.check()?;
        if amount > self.limit.saturating_sub(self.used) {
            return Err(Error::Limit("work"));
        }
        self.used += amount;
        Ok(())
    }
}

/// Immutable admitted snapshot. Retains the loader Arc, never clones palettes.
/// Construct off the render thread. Targets must come from evidence::analyze on
/// THIS snapshot, with THIS Source; structural validation is not reclassification.
#[derive(Debug)]
pub struct PreparedMap {
    source: Source,
    last_played: i64,
    loaded: Arc<LoadedWindow>,
    targets: Vec<TargetSummary>,
}
impl PreparedMap {
    pub fn new(
        source: Source,
        last_played: i64,
        loaded: Arc<LoadedWindow>,
        mut targets: Vec<TargetSummary>,
        budget: &mut Budget<'_>,
    ) -> Result<Self, Error> {
        budget.check()?;
        if source.generation == 0 || last_played < 0 {
            return Err(Error::Invalid("source/LastPlayed"));
        }
        if loaded.chunks.len() > MAX_CHUNKS
            || targets.len() > MAX_TARGETS
            || targets.capacity() > MAX_TARGETS
        {
            return Err(Error::Limit("prepared snapshot"));
        }
        if loaded.coverage.chunks.len() != loaded.chunks.len() {
            return Err(Error::Invalid("coverage/chunk mismatch"));
        }
        for (&position, chunk) in &loaded.chunks {
            budget.charge(48)?;
            if position != chunk.identity.position
                || !loaded.coverage.chunks.contains(&position)
                || position
                    .iter()
                    .any(|&v| v.checked_mul(16).and_then(|v| v.checked_add(15)).is_none())
                || !(region::MIN_DATA_VERSION..=region::MAX_DATA_VERSION)
                    .contains(&chunk.identity.data_version)
                || !chunk.sections_present
                || !chunk.has_full_coverage(-4, 19)
            {
                return Err(Error::Invalid("unqualified coverage"));
            }
            for (&y, section) in &chunk.sections {
                budget.charge(1)?;
                if !(-4..=19).contains(&y) && section.block_states.is_some() {
                    return Err(Error::Invalid("non-Overworld block domain"));
                }
            }
        }
        // At most 2304 small summaries; bounded sort, no callback or state copies.
        targets.sort_unstable_by_key(|target| target.key);
        if targets.windows(2).any(|pair| pair[0].key == pair[1].key) {
            return Err(Error::Invalid("duplicate target"));
        }
        let result = Self {
            source,
            last_played,
            loaded,
            targets,
        };
        for target in &result.targets {
            result.validate_target(target, budget)?;
        }
        budget.check()?;
        Ok(result)
    }
    pub fn source(&self) -> Source {
        self.source
    }
    pub fn targets(&self) -> &[TargetSummary] {
        &self.targets
    }
    pub fn loaded(&self) -> &LoadedWindow {
        &self.loaded
    }
    pub fn state(&self, position: [i32; 3]) -> Option<&BlockState> {
        if !(MIN_Y..=MAX_Y).contains(&position[1]) {
            return None;
        }
        let chunk = self
            .loaded
            .chunks
            .get(&[position[0].div_euclid(16), position[2].div_euclid(16)])?;
        match chunk.block_at(position) {
            BlockSample::State(state) => Some(state),
            _ => None,
        }
    }
    fn validate_target(
        &self,
        target: &TargetSummary,
        budget: &mut Budget<'_>,
    ) -> Result<(), Error> {
        budget.charge(1)?;
        let support = target.support;
        if target.source != self.source
            || target.key.map != self.source.map
            || target.key.revision != evidence::RULE_REVISION
            || !(1..=256).contains(&support.columns)
            || support.primary_columns == 0
            || support.secondary_columns == 0
            || support.primary_columns > support.columns
            || support.secondary_columns > support.columns
            || !(2..=16).contains(&support.secondary_sectors)
            || support.links > 256
            || support
                .footprint
                .iter()
                .map(|word| word.count_ones())
                .sum::<u32>()
                != u32::from(support.columns)
            || (0..3).any(|axis| support.minimum[axis] > support.maximum[axis])
            || support.minimum[1] < MIN_Y
            || support.maximum[1] > MAX_Y
            || target.key.anchor == target.corroboration
            || [0, 1].into_iter().any(|axis| {
                let ground_axis = if axis == 0 { 0 } else { 2 };
                let origin = i64::from(support.origin[axis]);
                let maximum = origin + 15;
                i64::from(support.minimum[ground_axis]) < origin
                    || i64::from(support.maximum[ground_axis]) > maximum
                    || maximum > i64::from(i32::MAX)
            })
            || [
                target.key.anchor[0].div_euclid(16),
                target.key.anchor[2].div_euclid(16),
            ] != target.key.tile
            || (target.key.category != Category::DwellingLikeConstruction
                && support.origin != target.key.tile.map(|coordinate| coordinate * 16))
            || (target.key.category == Category::DwellingLikeConstruction
                && support
                    .origin
                    .into_iter()
                    .any(|coordinate| coordinate.rem_euclid(8) != 0))
        {
            return Err(Error::Invalid("target summary"));
        }
        for index in 0..256 {
            budget.charge(1)?;
            if !bit(&support.footprint, index) {
                continue;
            }
            for (axis, ground_axis, offset) in [(0, 0, index % 16), (2, 1, index / 16)] {
                let coordinate = i64::from(support.origin[ground_axis]) + offset as i64;
                if coordinate < i64::from(support.minimum[axis])
                    || coordinate > i64::from(support.maximum[axis])
                {
                    return Err(Error::Invalid("target footprint"));
                }
            }
        }
        for position in [Some(target.key.anchor), Some(target.corroboration)]
            .into_iter()
            .chain(target.landmarks)
            .flatten()
        {
            budget.charge(1)?;
            if !supported_position(target, position)
                || self.state(position).is_none_or(BlockState::is_air)
            {
                return Err(Error::Invalid("target witness"));
            }
        }
        Ok(())
    }
}
fn bit(mask: &[u64; 4], index: usize) -> bool {
    mask[index / 64] & (1_u64 << (index % 64)) != 0
}
fn supported_position(target: &TargetSummary, position: [i32; 3]) -> bool {
    let [x, _, z] = position;
    (0..3).all(|axis| {
        (target.support.minimum[axis]..=target.support.maximum[axis]).contains(&position[axis])
    }) && {
        let dx = i64::from(x) - i64::from(target.support.origin[0]);
        let dz = i64::from(z) - i64::from(target.support.origin[1]);
        (0..16).contains(&dx)
            && (0..16).contains(&dz)
            && bit(&target.support.footprint, (dz * 16 + dx) as usize)
    }
}

/// A caller-certified horizontal envelope for the entire explicitly admitted
/// cell-height volume, including pack geometry, orientation and neighbor needs.
/// Clipping excludes saved cells; it never certifies the full Overworld volume.
/// eye_y must clear the domain plus admitted upward model overhang. look_at.y is
/// an observed target height, not the eye height and not a guessed ground plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// Inclusive saved cell heights. Model overhang is additional geometry.
    pub cell_heights: [i32; 2],
    pub viewport_radius: f64,
    pub horizontal_halo: f64,
    pub upward_overhang: f64,
    pub eye_y: f64,
}
impl Envelope {
    fn contains_height(self, height: i32) -> bool {
        (self.cell_heights[0]..=self.cell_heights[1]).contains(&height)
    }
    fn contains_target(self, target: &TargetSummary) -> bool {
        self.contains_height(target.support.minimum[1])
            && self.contains_height(target.support.maximum[1])
            && [Some(target.key.anchor), Some(target.corroboration)]
                .into_iter()
                .chain(target.landmarks)
                .flatten()
                .all(|position| self.contains_height(position[1]))
    }
    pub fn radius(self) -> f64 {
        self.viewport_radius + self.horizontal_halo
    }
    fn validate(self) -> Result<(), Error> {
        if self.cell_heights[0] < MIN_Y
            || self.cell_heights[1] > MAX_Y
            || self.cell_heights[0] > self.cell_heights[1]
            || [
                self.viewport_radius,
                self.horizontal_halo,
                self.upward_overhang,
                self.eye_y,
            ]
            .iter()
            .any(|value| !value.is_finite())
            || self.viewport_radius < 0.0
            || self.horizontal_halo < 0.0
            || self.upward_overhang < 0.0
            || self.radius() > 128.0
            || self.upward_overhang > 128.0
            || self.eye_y <= f64::from(MAX_Y + 1) + self.upward_overhang
            || self.eye_y > 4096.0
        {
            return Err(Error::Invalid("viewport/model envelope"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub envelope: Envelope,
    pub minimum_length: f64,
    pub maximum_length: f64,
    pub max_line_queries: usize,
    pub minimum_confidence: Confidence,
    /// Final surviving pixels per witness, not submitted triangles or raw alpha.
    pub minimum_pixels: u32,
}
impl Policy {
    fn validate(self) -> Result<(), Error> {
        self.envelope.validate()?;
        if !self.minimum_length.is_finite()
            || !self.maximum_length.is_finite()
            || self.minimum_length < 48.0
            || self.maximum_length < self.minimum_length
            || self.maximum_length > 1024.0
            || !(1..=4096).contains(&self.max_line_queries)
            || self.minimum_pixels == 0
        {
            return Err(Error::Invalid("tour policy"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    generation: u64,
    serial: u64,
    run: u64,
    history_revision: u64,
}
impl Ticket {
    pub fn generation(self) -> u64 {
        self.generation
    }
    pub fn run(self) -> u64 {
        self.run
    }
    pub fn intent(self) -> Kind {
        if self.run & 1 == 1 {
            Kind::Biome
        } else {
            Kind::Structure
        }
    }
}
/// Canonical quarter-block endpoints suppress reversal-as-diversity and tiny
/// endpoint jitter. This is a repeat bucket, not the coverage proof geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteKey {
    pub map: MapId,
    pub endpoints: [[i64; 2]; 2],
}
fn route_key(map: MapId, line: Line) -> RouteKey {
    let mut endpoints = [line.start, line.end].map(|point| point.map(|v| (v * 4.0).round() as i64));
    endpoints.sort_unstable();
    RouteKey { map, endpoints }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub key: TargetKey,
    pub source: Source,
    pub confidence: Confidence,
    pub run: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traversal {
    pub route: RouteKey,
    pub run: u64,
}
/// Fixed-capacity serialization: one actual appearance per category, refreshed
/// at most once per run (chosen first when visible in the same frame),
/// retaining its exact target/source, and sixteen displayed completed routes.
/// Completion is scheduling state, never evidence that novelty was achieved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    completed: u64,
    revision: u64,
    seen: [Option<Seen>; 32],
    routes: [Option<Traversal>; 16],
}
impl History {
    pub fn completed(&self) -> u64 {
        self.completed
    }
    pub fn appearances(&self) -> impl Iterator<Item = &Seen> {
        self.seen.iter().flatten()
    }
    pub fn traversals(&self) -> impl Iterator<Item = &Traversal> {
        self.routes.iter().flatten()
    }
    fn next_run(&self) -> Result<u64, Error> {
        self.completed
            .checked_add(1)
            .ok_or(Error::Limit("run counter"))
    }
    fn changed(&mut self) -> Result<(), Error> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::Limit("history revision"))?;
        Ok(())
    }
    fn validate(&self) -> Result<(), Error> {
        let next = self.next_run()?;
        for (index, seen) in self.seen.iter().enumerate() {
            let Some(seen) = seen else { continue };
            if seen.run == 0
                || seen.run > next
                || seen.source.map != seen.key.map
                || seen.source.generation == 0
                || self.seen[..index]
                    .iter()
                    .flatten()
                    .any(|old| old.key.category == seen.key.category)
            {
                return Err(Error::Invalid("appearance history"));
            }
        }
        if self
            .routes
            .iter()
            .flatten()
            .any(|route| route.run == 0 || route.run > self.completed)
        {
            return Err(Error::Invalid("route history"));
        }
        Ok(())
    }
    fn recent(&self, category: Category, run: u64) -> bool {
        self.appearances().any(|seen| {
            seen.key.category == category && run.saturating_sub(seen.run) <= RECENT_RUNS
        })
    }
    fn see(&mut self, target: &TargetSummary, run: u64) -> bool {
        if self
            .appearances()
            .any(|seen| seen.key.category == target.key.category && seen.run == run)
        {
            return false;
        }
        let slot = self
            .seen
            .iter()
            .position(|seen| seen.is_some_and(|seen| seen.key.category == target.key.category))
            .or_else(|| self.seen.iter().position(Option::is_none))
            .map_or(self.seen.len() - 1, |slot| slot);
        for index in (1..=slot).rev() {
            self.seen[index] = self.seen[index - 1];
        }
        self.seen[0] = Some(Seen {
            key: target.key,
            source: target.source,
            confidence: target.confidence,
            run,
        });
        true
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    NovelAppearance,
    RepeatedAppearance,
    OtherKind,
    SavedSurface,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    QueryLimit,
    DesiredKindAbsentFromSuppliedEvidence,
    DesiredKindBelowConfidence,
    DesiredAppearancesRecentlySeen,
    NoNovelNonrepeatingLineInFiniteSearch,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchAudit {
    pub maps: usize,
    pub chunks: usize,
    pub targets: usize,
    pub desired: usize,
    pub confident_desired: usize,
    pub unrecent_desired: usize,
    pub line_queries: usize,
    pub usable_lines: usize,
    pub immediate_repeats: usize,
    pub surface_seeds: usize,
    pub query_limited: bool,
}
#[derive(Clone, Debug)]
pub struct Plan {
    ticket: Ticket,
    map: Arc<PreparedMap>,
    line: Line,
    route: RouteKey,
    target: Option<TargetSummary>,
    focus_y: f64,
    policy: Policy,
    choice: Choice,
    fallback: Option<Fallback>,
    audit: SearchAudit,
}
impl Plan {
    pub fn ticket(&self) -> Ticket {
        self.ticket
    }
    pub fn source(&self) -> Source {
        self.map.source
    }
    pub fn line(&self) -> Line {
        self.line
    }
    pub fn route(&self) -> RouteKey {
        self.route
    }
    pub fn target(&self) -> Option<TargetSummary> {
        self.target
    }
    pub fn choice(&self) -> Choice {
        self.choice
    }
    pub fn fallback(&self) -> Option<Fallback> {
        self.fallback
    }
    pub fn audit(&self) -> SearchAudit {
        self.audit
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    NoPreparedMaps,
    NoQualifiedCoverage,
    QueryLimit,
    NoEligibleNonrepeatingLine,
}
#[derive(Debug)]
pub struct Selection {
    pub plan: Option<Plan>,
    /// Always finite-survey scoped, even when query_limited is false.
    pub audit: SearchAudit,
    pub unavailable: Option<Unavailable>,
}
// Run-seeded tie breaking changes equivalent scenic paths without randomness,
// allocation, wall-clock dependence, or manufacturing new target identities.
fn variety(route: RouteKey, run: u64) -> u64 {
    let mut hash = run.wrapping_mul(0x9e3779b97f4a7c15);
    for coordinate in route.endpoints.into_iter().flatten() {
        hash ^= coordinate as u64;
        hash = hash.rotate_left(27).wrapping_mul(0xbf58476d1ce4e5b9);
    }
    hash ^ (hash >> 31)
}
type Score = (usize, bool, bool, Reverse<i64>, Reverse<u64>, u64, RouteKey);
struct Candidate {
    map: usize,
    line: Line,
    route: RouteKey,
    target: Option<TargetSummary>,
    focus_y: f64,
    score: Score,
}
struct Search<'a, 'b> {
    ticket: Ticket,
    history: &'a History,
    policy: Policy,
    maps: &'a [Arc<PreparedMap>],
    budget: &'a mut Budget<'b>,
    audit: SearchAudit,
    best: Option<Candidate>,
}
impl Search<'_, '_> {
    fn consider(
        &mut self,
        map: usize,
        point: [f64; 2],
        focus_y: f64,
        target: Option<TargetSummary>,
    ) -> Result<bool, Error> {
        for direction_index in 0..DIRECTIONS.len() {
            if self.audit.line_queries == self.policy.max_line_queries {
                self.audit.query_limited = true;
                return Ok(false);
            }
            let offset = ((self.ticket.run - 1) % DIRECTIONS.len() as u64) as usize;
            let direction = DIRECTIONS[(direction_index + offset) % DIRECTIONS.len()];
            let radius = self.policy.envelope.radius();
            // Each extension is <=4 blocks. Reserve an upper bound on all chunk
            // membership probes before the original bounded, non-cancellable call.
            let side = ((2.0 * radius + 4.0) / 16.0).ceil() as u64 + 2;
            let steps = 2 * (self.policy.maximum_length / 8.0).ceil() as u64 + 1;
            self.budget.charge(steps * side * side + 64)?;
            self.audit.line_queries += 1;
            let prepared = &self.maps[map];
            let result = prepared.loaded.coverage.line_through(
                point,
                direction,
                radius,
                self.policy.maximum_length,
            );
            self.budget.check()?;
            let Some(mut line) = result else { continue };
            if line.length() < self.policy.minimum_length {
                continue;
            }
            let route = route_key(prepared.source.map, line);
            if self.history.routes[0].is_some_and(|last| last.route == route) {
                self.audit.immediate_repeats += 1;
                continue;
            }
            self.audit.usable_lines += 1;
            if (self.ticket.run & 1) == (direction_index as u64 & 1) {
                std::mem::swap(&mut line.start, &mut line.end);
            }
            let map_penalty = self
                .history
                .routes
                .iter()
                .position(|old| old.is_some_and(|old| old.route.map == route.map))
                .map_or(0, |index| self.history.routes.len() - index);
            let score = (
                map_penalty,
                self.history.traversals().any(|old| old.route == route),
                target.is_some_and(|target| {
                    self.history.appearances().any(|old| old.key == target.key)
                }),
                Reverse(prepared.last_played),
                Reverse((line.length() * 1024.0).round() as u64),
                variety(route, self.ticket.run),
                route,
            );
            if self.best.as_ref().is_none_or(|old| score < old.score) {
                self.best = Some(Candidate {
                    map,
                    line,
                    route,
                    target,
                    focus_y,
                    score,
                });
            }
        }
        Ok(true)
    }
}
fn fallback(audit: SearchAudit) -> Fallback {
    if audit.query_limited {
        return Fallback::QueryLimit;
    }
    if audit.desired == 0 {
        return Fallback::DesiredKindAbsentFromSuppliedEvidence;
    }
    if audit.confident_desired == 0 {
        return Fallback::DesiredKindBelowConfidence;
    }
    if audit.unrecent_desired == 0 {
        return Fallback::DesiredAppearancesRecentlySeen;
    }
    Fallback::NoNovelNonrepeatingLineInFiniteSearch
}
/// Worker-side finite search. A query cap can return a proved route with an
/// explicit limited-search audit; cancellation or work exhaustion returns Err
/// and publishes NO partial plan. Input map order does not control ranking.
pub fn select(
    ticket: Ticket,
    maps: &[Arc<PreparedMap>],
    history: &History,
    policy: Policy,
    budget: &mut Budget<'_>,
) -> Result<Selection, Error> {
    budget.check()?;
    policy.validate()?;
    history.validate()?;
    if ticket.run != history.next_run()? || ticket.history_revision != history.revision {
        return Err(Error::Stale);
    }
    if maps.len() > MAX_MAPS {
        return Err(Error::Limit("maps"));
    }
    let mut order = [0_usize; MAX_MAPS];
    let mut audit = SearchAudit {
        maps: maps.len(),
        ..SearchAudit::default()
    };
    for (index, map) in maps.iter().enumerate() {
        budget.charge(1)?;
        if map.source.generation != ticket.generation {
            return Err(Error::Stale);
        }
        if maps[..index]
            .iter()
            .any(|old| old.source.map == map.source.map)
        {
            return Err(Error::Invalid("duplicate map identity"));
        }
        order[index] = index;
        audit.chunks += map.loaded.chunks.len();
        audit.targets += map.targets.len();
        for target in &map.targets {
            budget.charge(1)?;
            if !policy.envelope.contains_target(target)
                || target.key.category.kind() != ticket.intent()
            {
                continue;
            }
            audit.desired += 1;
            if target.confidence < policy.minimum_confidence {
                continue;
            }
            audit.confident_desired += 1;
            if !history.recent(target.key.category, ticket.run) {
                audit.unrecent_desired += 1;
            }
        }
    }
    order[..maps.len()]
        .sort_unstable_by_key(|&index| (Reverse(maps[index].last_played), maps[index].source.map));
    let mut search = Search {
        ticket,
        history,
        policy,
        maps,
        budget,
        audit,
        best: None,
    };
    let mut choice = Choice::SavedSurface;
    'phases: for (phase, phase_choice) in [
        Choice::NovelAppearance,
        Choice::RepeatedAppearance,
        Choice::OtherKind,
        Choice::SavedSurface,
    ]
    .into_iter()
    .enumerate()
    {
        choice = phase_choice;
        let slots = maps
            .iter()
            .map(|map| {
                if phase == 3 {
                    map.loaded.chunks.len()
                } else {
                    map.targets.len()
                }
            })
            .max()
            .unwrap_or(0);
        // Round-robin across relatively ranked maps prevents one large map from
        // taking all probes. Run-dependent rotation changes finite search prefixes.
        for slot in 0..slots {
            for &index in &order[..maps.len()] {
                search.budget.charge(1)?;
                let map = &maps[index];
                if phase == 3 {
                    if slot >= map.loaded.chunks.len() {
                        continue;
                    }
                    let offset = ((ticket.run - 1) % map.loaded.chunks.len() as u64) as usize;
                    let Some((&position, chunk)) = map
                        .loaded
                        .chunks
                        .iter()
                        .nth((slot + offset) % map.loaded.chunks.len())
                    else {
                        continue;
                    };
                    let x = position[0] * 16 + 8;
                    let z = position[1] * 16 + 8;
                    for y in (MIN_Y..=MAX_Y).rev() {
                        search.budget.charge(1)?;
                        let BlockSample::State(state) = chunk.block_at([x, y, z]) else {
                            return Err(Error::Invalid("qualified state disappeared"));
                        };
                        if state.is_air() {
                            continue;
                        }
                        // The true saved top is outside this render volume.
                        // Do not replace it with a buried cell inside the band.
                        if !policy.envelope.contains_height(y) {
                            break;
                        }
                        search.audit.surface_seeds += 1;
                        if !search.consider(
                            index,
                            [f64::from(x) + 0.5, f64::from(z) + 0.5],
                            f64::from(y) + 0.5,
                            None,
                        )? {
                            break 'phases;
                        }
                        break;
                    }
                    continue;
                }
                if slot >= map.targets.len() {
                    continue;
                }
                let offset = ((ticket.run - 1) % map.targets.len() as u64) as usize;
                let target = map.targets[(slot + offset) % map.targets.len()];
                if !policy.envelope.contains_target(&target)
                    || target.confidence < policy.minimum_confidence
                {
                    continue;
                }
                let desired = target.key.category.kind() == ticket.intent();
                let recent = history.recent(target.key.category, ticket.run);
                if !matches!(
                    (phase, desired, recent),
                    (0, true, false) | (1, true, true) | (2, false, _)
                ) {
                    continue;
                }
                let [x, y, z] = target.key.anchor;
                if !search.consider(
                    index,
                    [f64::from(x) + 0.5, f64::from(z) + 0.5],
                    f64::from(y) + 0.5,
                    Some(target),
                )? {
                    break 'phases;
                }
            }
        }
        if search.best.is_some() {
            break;
        }
    }
    search.budget.check()?;
    let audit = search.audit;
    let reason = fallback(audit);
    let plan = search.best.map(|candidate| Plan {
        ticket,
        map: Arc::clone(&maps[candidate.map]),
        line: candidate.line,
        route: candidate.route,
        target: candidate.target,
        focus_y: candidate.focus_y,
        policy,
        choice,
        fallback: (choice != Choice::NovelAppearance).then_some(reason),
        audit,
    });
    let unavailable = if plan.is_some() {
        None
    } else if maps.is_empty() {
        Some(Unavailable::NoPreparedMaps)
    } else if audit.chunks == 0 {
        Some(Unavailable::NoQualifiedCoverage)
    } else if audit.query_limited {
        Some(Unavailable::QueryLimit)
    } else {
        Some(Unavailable::NoEligibleNonrepeatingLine)
    };
    Ok(Selection {
        unavailable,
        plan,
        audit,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Frame.time: globally scaled animation time, never Frame.wall or now.
    pub time: Duration,
    /// Blocks per globally scaled animation second; changing it is prospective.
    pub local_speed: f64,
    /// Host explicitly sets this while local OR global animation is frozen,
    /// including when a completed route is waiting for a newly prepared plan.
    pub frozen: bool,
}
impl Clock {
    fn validate(self) -> Result<(), Error> {
        if !self.local_speed.is_finite() || !(0.0..=64.0).contains(&self.local_speed) {
            return Err(Error::Invalid("local speed"));
        }
        Ok(())
    }
    fn frozen(self) -> bool {
        self.frozen || self.local_speed == 0.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Started,
    Advanced,
    Frozen,
    RebasedForwardJump,
    Endpoint,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTag {
    ticket: Ticket,
    sequence: u64,
    source: Source,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub tag: FrameTag,
    /// Minecraft [x,y,z] gaze anchor; renderer axis conversion belongs downstream.
    pub look_at: [f64; 3],
    pub eye_y: f64,
    pub data_radius: f64,
    pub motion: Motion,
}
struct Active {
    plan: Plan,
    distance: f64,
    last_time: Duration,
    last_speed: f64,
    sequence: u64,
    acknowledged: u64,
    any_saved_pixels: bool,
    chosen_visible: bool,
}
impl Active {
    fn view(&self, motion: Motion) -> View {
        let point = self
            .plan
            .line
            .point(self.distance / self.plan.line.length());
        View {
            tag: FrameTag {
                ticket: self.plan.ticket,
                sequence: self.sequence,
                source: self.plan.source(),
            },
            look_at: [point[0], self.plan.focus_y, point[1]],
            eye_y: self.plan.policy.envelope.eye_y,
            data_radius: self.plan.policy.envelope.radius(),
            motion,
        }
    }
}
/// One deduplicated final-display owner. Resolve state from the retained map;
/// pointer equality binds the receipt to its exact decoded palette, not Material.
/// pixels counts surviving contribution after occlusion, alpha, dither and host
/// overlays. This is an adapter contract, NOT an assertion the adapter exists.
#[derive(Clone, Copy, Debug)]
pub struct DisplayedBlock<'a> {
    pub position: [i32; 3],
    pub state: &'a BlockState,
    pub pixels: u32,
    /// False for missing-model/texture diagnostics or generated substitutions.
    /// True requires authentic state-bound pack geometry, including special models.
    pub resolved: bool,
}
pub fn display_order(block: &DisplayedBlock<'_>) -> ([i32; 2], [i32; 3]) {
    (
        [
            block.position[0].div_euclid(16),
            block.position[2].div_euclid(16),
        ],
        block.position,
    )
}
fn visible(
    target: &TargetSummary,
    owners: &[DisplayedBlock<'_>],
    minimum_pixels: u32,
    budget: &mut Budget<'_>,
) -> Result<bool, Error> {
    budget.charge(1)?;
    for position in [Some(target.key.anchor), Some(target.corroboration)]
        .into_iter()
        .chain(target.landmarks)
        .flatten()
    {
        budget.charge(1)?;
        let key = (
            [position[0].div_euclid(16), position[2].div_euclid(16)],
            position,
        );
        let Ok(index) = owners.binary_search_by_key(&key, display_order) else {
            return Ok(false);
        };
        if !owners[index].resolved || owners[index].pixels < minimum_pixels {
            return Ok(false);
        }
    }
    let mut columns = [0_u64; 4];
    let mut sectors = 0_u16;
    let (mut low, mut high) = ([16; 2], [0; 2]);
    let first = target
        .support
        .origin
        .map(|coordinate| coordinate.div_euclid(16));
    let last = target
        .support
        .origin
        .map(|coordinate| (i64::from(coordinate) + 15) as i32)
        .map(|coordinate| coordinate.div_euclid(16));
    for chunk_z in first[1]..=last[1] {
        for chunk_x in first[0]..=last[0] {
            let chunk = [chunk_x, chunk_z];
            let lower = owners.partition_point(|owner| display_order(owner).0 < chunk);
            let upper = owners.partition_point(|owner| display_order(owner).0 <= chunk);
            for owner in &owners[lower..upper] {
                budget.charge(1)?;
                if !owner.resolved
                    || owner.pixels < minimum_pixels
                    || !supported_position(target, owner.position)
                {
                    continue;
                }
                let x =
                    (i64::from(owner.position[0]) - i64::from(target.support.origin[0])) as usize;
                let z =
                    (i64::from(owner.position[2]) - i64::from(target.support.origin[1])) as usize;
                let index = z * 16 + x;
                columns[index / 64] |= 1_u64 << (index % 64);
                sectors |= 1 << (x / 4 + z / 4 * 4);
                low[0] = low[0].min(x);
                low[1] = low[1].min(z);
                high[0] = high[0].max(x);
                high[1] = high[1].max(z);
            }
        }
    }
    let count: u32 = columns.iter().map(|word| word.count_ones()).sum();
    Ok(
        count >= u32::from(target.support.columns).div_ceil(4).max(8)
            && sectors.count_ones() >= 2
            && (0..2).all(|axis| high[axis] >= low[axis] + 3),
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presentation {
    pub chosen_visible: bool,
    pub credited_categories: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Completion {
    pub run: u64,
    pub chosen_visible: bool,
    pub novelty_achieved: bool,
    pub saved_pixels_shown: bool,
}
/// Move retired plans back to the worker/owner for potentially large Arc cleanup.
#[derive(Debug)]
pub struct Finished {
    pub completion: Completion,
    pub retired: Plan,
}
/// No globals, I/O, time reads or worker lifecycle changes. An epoch invalidates
/// pending/active work immediately; the owner must also supersede its worker.
/// Keep a copied History in the worker request; never hold a UI lock while select runs.
pub struct Controller {
    generation: u64,
    serial: u64,
    history: History,
    pending: Option<Ticket>,
    active: Option<Active>,
    last_view: Option<View>,
}
impl Controller {
    pub fn new(generation: u64, history: History) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::Invalid("zero generation"));
        }
        history.validate()?;
        Ok(Self {
            generation,
            serial: 0,
            history,
            pending: None,
            active: None,
            last_view: None,
        })
    }
    pub fn history(&self) -> History {
        self.history
    }
    pub fn view(&self) -> Option<View> {
        self.last_view
    }
    /// Returns the old plan instead of dropping the last snapshot Arc on the UI.
    pub fn invalidate(&mut self, generation: u64) -> Result<Option<Plan>, Error> {
        if generation <= self.generation {
            return Err(Error::Stale);
        }
        self.generation = generation;
        self.pending = None;
        self.last_view = None;
        Ok(self.active.take().map(|active| active.plan))
    }
    pub fn request(&mut self, budget: &Budget<'_>) -> Result<Ticket, Error> {
        budget.check()?;
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        let serial = self
            .serial
            .checked_add(1)
            .ok_or(Error::Limit("request serial"))?;
        let ticket = Ticket {
            generation: self.generation,
            serial,
            run: self.history.next_run()?,
            history_revision: self.history.revision,
        };
        budget.check()?;
        self.serial = serial;
        self.pending = Some(ticket);
        Ok(ticket)
    }
    /// None means frozen: no admission or pose change. Caller retains its Plan
    /// for retry. Route/map transitions are explicit cuts, NEVER interpolated
    /// through unsaved gaps. Failure/cancellation leaves the pending ticket intact.
    pub fn start(
        &mut self,
        plan: &Plan,
        clock: Clock,
        budget: &Budget<'_>,
    ) -> Result<Option<View>, Error> {
        budget.check()?;
        clock.validate()?;
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        if self.pending != Some(plan.ticket)
            || plan.ticket.generation != self.generation
            || plan.ticket.history_revision != self.history.revision
            || plan.ticket.run != self.history.next_run()?
        {
            return Err(Error::Stale);
        }
        if clock.frozen() {
            return Ok(None);
        }
        let active = Active {
            plan: plan.clone(),
            distance: 0.0,
            last_time: clock.time,
            last_speed: clock.local_speed,
            sequence: 1,
            acknowledged: 0,
            any_saved_pixels: false,
            chosen_visible: false,
        };
        let view = active.view(Motion::Started);
        budget.check()?;
        self.active = Some(active);
        self.pending = None;
        self.last_view = Some(view);
        Ok(Some(view))
    }
    /// A >2s jump rebases without movement/debt. Normal elapsed time moves
    /// only this route. Endpoint holds until its exact final frame is acknowledged
    /// and finish is called; no modulo-time skipping through unseen tours.
    pub fn advance(
        &mut self,
        ticket: Ticket,
        clock: Clock,
        budget: &Budget<'_>,
    ) -> Result<View, Error> {
        budget.check()?;
        clock.validate()?;
        let active = self.active.as_ref().ok_or(Error::Idle)?;
        if ticket != active.plan.ticket {
            return Err(Error::Stale);
        }
        let delta = clock
            .time
            .checked_sub(active.last_time)
            .ok_or(Error::ClockReversed)?;
        let sequence = active
            .sequence
            .checked_add(1)
            .ok_or(Error::Limit("frame sequence"))?;
        let (distance, motion) = if clock.frozen() || delta.is_zero() {
            (active.distance, Motion::Frozen)
        } else if delta > MAX_DELTA {
            (active.distance, Motion::RebasedForwardJump)
        } else {
            let distance = (active.distance + delta.as_secs_f64() * active.last_speed)
                .min(active.plan.line.length());
            (
                distance,
                if distance == active.plan.line.length() {
                    Motion::Endpoint
                } else {
                    Motion::Advanced
                },
            )
        };
        budget.check()?;
        let active = self.active.as_mut().ok_or(Error::Idle)?;
        active.distance = distance;
        active.last_time = clock.time;
        active.sequence = sequence;
        active.last_speed = if clock.frozen() {
            0.0
        } else {
            clock.local_speed
        };
        let view = active.view(motion);
        self.last_view = Some(view);
        Ok(view)
    }
    /// Call only AFTER this exact frame was actually presented. Owners must be
    /// strictly sorted by display_order, deduplicated, and from final pixel data.
    /// Incidental targets are checked too, so changing only the selected anchor
    /// does not conceal an appearance already shown. No cross-frame evidence union.
    pub fn presented(
        &mut self,
        tag: FrameTag,
        owners: &[DisplayedBlock<'_>],
        budget: &mut Budget<'_>,
    ) -> Result<Presentation, Error> {
        budget.check()?;
        let active = self.active.as_ref().ok_or(Error::Idle)?;
        let view = active.view(Motion::Frozen);
        if tag != view.tag || active.acknowledged >= tag.sequence {
            return Err(Error::Stale);
        }
        if owners.len() > MAX_OWNERS {
            return Err(Error::Limit("display owners"));
        }
        let map = &active.plan.map;
        let mut any_pixels = false;
        for (index, owner) in owners.iter().enumerate() {
            budget.charge(1)?;
            if index > 0 && display_order(&owners[index - 1]) >= display_order(owner) {
                return Err(Error::Invalid("display owners not unique/sorted"));
            }
            let Some(state) = map.state(owner.position) else {
                return Err(Error::Invalid("display owner not saved"));
            };
            if !active
                .plan
                .policy
                .envelope
                .contains_height(owner.position[1])
                || !std::ptr::eq(state, owner.state)
                || state.is_air()
                || [(0, 0), (2, 2)].into_iter().any(|(axis, look_axis)| {
                    f64::from(owner.position[axis])
                        < (view.look_at[look_axis] - view.data_radius).floor()
                        || f64::from(owner.position[axis])
                            > (view.look_at[look_axis] + view.data_radius).floor()
                })
            {
                return Err(Error::Invalid("display owner snapshot/footprint"));
            }
            any_pixels |= owner.resolved && owner.pixels > 0;
        }
        let mut history = self.history;
        let mut chosen_visible = false;
        let mut credited_categories = 0;
        // Check the chosen key first; keep its exact identity when incidental
        // observations of that category occur in the same presented frame.
        let chosen = active.plan.target;
        for target in chosen.iter().chain(
            map.targets
                .iter()
                .filter(|target| chosen.is_none_or(|chosen| chosen.key != target.key)),
        ) {
            budget.charge(1)?;
            if !active.plan.policy.envelope.contains_target(target)
                || target.confidence < active.plan.policy.minimum_confidence
                || !visible(target, owners, active.plan.policy.minimum_pixels, budget)?
            {
                continue;
            }
            chosen_visible |= chosen.is_some_and(|chosen| chosen.key == target.key);
            credited_categories += usize::from(history.see(target, tag.ticket.run));
        }
        if credited_categories > 0 {
            history.changed()?;
        }
        budget.check()?;
        let active = self.active.as_mut().ok_or(Error::Idle)?;
        active.acknowledged = tag.sequence;
        active.any_saved_pixels |= any_pixels;
        active.chosen_visible |= chosen_visible;
        self.history = history;
        Ok(Presentation {
            chosen_visible,
            credited_categories,
        })
    }
    pub fn finish(
        &mut self,
        ticket: Ticket,
        clock: Clock,
        budget: &Budget<'_>,
    ) -> Result<Finished, Error> {
        budget.check()?;
        clock.validate()?;
        if clock.frozen() {
            return Err(Error::Frozen);
        }
        let active = self.active.as_ref().ok_or(Error::Idle)?;
        if ticket != active.plan.ticket {
            return Err(Error::Stale);
        }
        if clock.time != active.last_time
            || active.distance < active.plan.line.length()
            || active.acknowledged != active.sequence
        {
            return Err(Error::NotComplete);
        }
        let completion = Completion {
            run: ticket.run,
            chosen_visible: active.chosen_visible,
            novelty_achieved: active.chosen_visible
                && active.plan.choice == Choice::NovelAppearance,
            saved_pixels_shown: active.any_saved_pixels,
        };
        let mut history = self.history;
        history.completed = ticket.run;
        if active.any_saved_pixels {
            history.routes.rotate_right(1);
            history.routes[0] = Some(Traversal {
                route: active.plan.route,
                run: ticket.run,
            });
        }
        history.changed()?;
        budget.check()?;
        let retired = self.active.take().ok_or(Error::Idle)?.plan;
        self.history = history;
        Ok(Finished {
            completion,
            retired,
        })
    }
}

#[cfg(test)]
#[path = "tour_tests.rs"]
mod tests;
