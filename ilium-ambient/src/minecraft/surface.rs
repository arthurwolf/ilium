//! Read-only, model-independent saved Overworld observations in Minecraft [x,y,z].
//! The caller explicitly selects a -64..=319 Overworld, never a custom dimension.
//! Borrowed chunks must belong to ONE map and ONE preparation generation.
use super::{
    chunk::{BlockSample, BlockState, DecodedChunk},
    coverage::Coverage,
    region,
};
use std::mem::size_of;

pub const MIN_Y: i32 = -64;
pub const MAX_Y: i32 = 319;
const MAX_CHUNKS: usize = 256;
const MAX_COLUMNS: usize = 65_536;

/// Inclusive block-column bounds; unlike generation::Region, maximum is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub minimum: [i32; 2],
    pub maximum: [i32; 2],
}
impl Bounds {
    pub fn contains(self, position: [i32; 2]) -> bool {
        (0..2).all(|axis| {
            position[axis] >= self.minimum[axis] && position[axis] <= self.maximum[axis]
        })
    }
    fn columns(self) -> Result<usize, Error> {
        let mut count = 1_usize;
        for axis in 0..2 {
            let span = i64::from(self.maximum[axis]) - i64::from(self.minimum[axis]) + 1;
            if !(1..=512).contains(&span) {
                return Err(Error::InvalidBounds);
            }
            count = count
                .checked_mul(span as usize)
                .ok_or(Error::InvalidBounds)?;
        }
        Ok(count)
    }
    fn expanded(self, halo: u16) -> Result<Self, Error> {
        let mut result = self;
        for axis in 0..2 {
            result.minimum[axis] = self.minimum[axis]
                .checked_sub(i32::from(halo))
                .ok_or(Error::InvalidBounds)?;
            result.maximum[axis] = self.maximum[axis]
                .checked_add(i32::from(halo))
                .ok_or(Error::InvalidBounds)?;
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_chunks: usize,
    /// Includes the halo; the independent hard ceiling is 65,536 columns.
    pub max_columns: usize,
    /// Self plus reference-vector capacity; excludes caller-owned decoded data.
    pub max_owned_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_chunks: 64,
            max_columns: 16_384,
            max_owned_bytes: 16_384,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    Chunk,
    SectionList,
    Section,
    BlockStates,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    MissingChunk,
    NotFull,
    MissingSectionList,
    MissingSection(i8),
    MissingBlockStates(i8),
    UnsupportedVersion(i32),
    OutsideDomainSection(i8),
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid surface bounds or coordinate overflow")]
    InvalidBounds,
    #[error("surface resource limit: {0}")]
    Limit(&'static str),
    #[error("surface preparation cancelled")]
    Cancelled,
    #[error("duplicate decoded chunk {0:?}")]
    DuplicateChunk([i32; 2]),
    #[error("unqualified saved chunk {position:?}: {reason:?}")]
    Unqualified {
        position: [i32; 2],
        reason: Rejection,
    },
    #[error("qualified data invariant failed at {0:?}")]
    InconsistentSample([i32; 3]),
}

/// One caller-owned cumulative budget, reusable across construction and scanning.
/// Units: input chunk (<=256 section checks), qualification (<=48 section
/// probes), or block lookup. Sorting is bounded by MAX_CHUNKS. All supplied
/// callbacks, including cancellation, must be bounded and nonblocking.
/// Errors never refund work. No operation resets this budget or reads a clock.
pub struct Work<'a> {
    limit: usize,
    used: usize,
    cancelled: &'a dyn Fn() -> bool,
}
impl<'a> Work<'a> {
    pub fn new(limit: usize, cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            limit,
            used: 0,
            cancelled,
        }
    }
    pub fn used(&self) -> usize {
        self.used
    }
    pub fn checkpoint(&self) -> Result<(), Error> {
        if (self.cancelled)() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    fn step(&mut self) -> Result<(), Error> {
        self.checkpoint()?;
        if self.used >= self.limit {
            return Err(Error::Limit("work units"));
        }
        self.used += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample<'a> {
    State(&'a BlockState),
    Missing(Missing),
    OutsideWindow,
    /// Domain boundaries, NOT synthesized air states or saved-coverage evidence.
    AboveOverworld,
    BelowOverworld,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation<'a> {
    pub position: [i32; 3],
    pub state: &'a BlockState,
    /// Every cell strictly above this one is exact decoded air up to MAX_Y.
    /// False means only "a non-air cell above", NOT opaque/hidden/underground.
    pub only_air_above: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub columns: usize,
    pub samples: usize,
    pub non_air: usize,
}

/// The only owned heap storage is a sorted reference index. No dense states,
/// interned strings, generated columns, materials, biome IDs or mesh are created.
/// Missing/proto chunks remain inspectable, but cannot enter a qualified scan.
#[derive(Debug)]
pub struct SurfaceWindow<'a> {
    core: Bounds,
    bounds: Bounds,
    chunks: Vec<&'a DecodedChunk>,
}
impl<'a> SurfaceWindow<'a> {
    pub fn overworld(
        core: Bounds,
        halo: u16,
        chunks: &'a [DecodedChunk],
        limits: Limits,
        work: &mut Work<'_>,
    ) -> Result<Self, Error> {
        Self::overworld_refs(core, halo, chunks.iter(), limits, work)
    }
    /// Borrow loader.chunks.values().map(Arc::as_ref) without cloning states.
    /// Iterator len/next must be bounded and nonblocking, like cancellation.
    /// ExactSizeIterator is an allocation hint, not a trusted safety assertion:
    /// count-limited pulls and an end probe reject lying or infinite iterators.
    pub fn overworld_refs<I>(
        core: Bounds,
        halo: u16,
        chunks: I,
        limits: Limits,
        work: &mut Work<'_>,
    ) -> Result<Self, Error>
    where
        I: IntoIterator<Item = &'a DecodedChunk>,
        I::IntoIter: ExactSizeIterator,
    {
        work.checkpoint()?;
        core.columns()?;
        let bounds = core.expanded(halo)?;
        let mut chunks = chunks.into_iter();
        let count = chunks.len();
        work.checkpoint()?;
        if count > limits.max_chunks.min(MAX_CHUNKS) {
            return Err(Error::Limit("input chunks"));
        }
        if bounds.columns()? > limits.max_columns.min(MAX_COLUMNS) {
            return Err(Error::Limit("columns including halo"));
        }
        let bytes = Self::storage_bytes(count)?;
        if bytes > limits.max_owned_bytes {
            return Err(Error::Limit("owned bytes"));
        }
        let mut index = Vec::new();
        index
            .try_reserve_exact(count)
            .map_err(|_| Error::Limit("index allocation"))?;
        if Self::storage_bytes(index.capacity())? > limits.max_owned_bytes {
            return Err(Error::Limit("index capacity"));
        }
        for _ in 0..count {
            work.step()?;
            let chunk = chunks.next();
            work.checkpoint()?;
            let chunk = chunk.ok_or(Error::Limit("iterator length mismatch"))?;
            let position = chunk.identity.position;
            let version = chunk.identity.data_version;
            if !(region::MIN_DATA_VERSION..=region::MAX_DATA_VERSION).contains(&version) {
                return Err(Error::Unqualified {
                    position,
                    reason: Rejection::UnsupportedVersion(version),
                });
            }
            // These are block coordinates, not the wider i32 chunk-index domain.
            if position.iter().any(|&value| {
                let origin = i64::from(value) * 16;
                origin < i64::from(i32::MIN) || origin + 15 > i64::from(i32::MAX)
            }) {
                return Err(Error::InvalidBounds);
            }
            // Do not silently hide evidence of a different vertical domain.
            // Light-only extra sections have no block_states and may remain.
            for (&y, section) in &chunk.sections {
                work.checkpoint()?;
                if !(-4..=19).contains(&y) && section.block_states.is_some() {
                    return Err(Error::Unqualified {
                        position,
                        reason: Rejection::OutsideDomainSection(y),
                    });
                }
            }
            index.push(chunk);
        }
        work.checkpoint()?;
        let extra = chunks.next();
        work.checkpoint()?;
        if extra.is_some() {
            return Err(Error::Limit("iterator length mismatch"));
        }
        index.sort_unstable_by_key(|chunk| chunk.identity.position);
        for pair in index.windows(2) {
            work.checkpoint()?;
            if pair[0].identity.position == pair[1].identity.position {
                return Err(Error::DuplicateChunk(pair[0].identity.position));
            }
        }
        work.checkpoint()?;
        Ok(Self {
            core,
            bounds,
            chunks: index,
        })
    }
    fn storage_bytes(capacity: usize) -> Result<usize, Error> {
        capacity
            .checked_mul(size_of::<&DecodedChunk>())
            .and_then(|bytes| bytes.checked_add(size_of::<Self>()))
            .ok_or(Error::Limit("owned bytes overflow"))
    }
    pub fn owned_bytes(&self) -> usize {
        // Construction checked this exact capacity; the vector never grows.
        size_of::<Self>() + self.chunks.capacity() * size_of::<&DecodedChunk>()
    }
    pub fn core(&self) -> Bounds {
        self.core
    }
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
    fn chunk(&self, position: [i32; 2]) -> Option<&'a DecodedChunk> {
        self.chunks
            .binary_search_by_key(&position, |chunk| chunk.identity.position)
            .ok()
            .map(|index| self.chunks[index])
    }
    fn qualified(&self, position: [i32; 2]) -> Result<&'a DecodedChunk, Error> {
        let reject = |reason| Error::Unqualified { position, reason };
        let chunk = self
            .chunk(position)
            .ok_or_else(|| reject(Rejection::MissingChunk))?;
        if !chunk.is_full() {
            return Err(reject(Rejection::NotFull));
        }
        if !chunk.sections_present {
            return Err(reject(Rejection::MissingSectionList));
        }
        // Use the production predicate; diagnose its failure without weakening it.
        if !chunk.has_full_coverage(-4, 19) {
            for y in -4_i8..=19 {
                let section = chunk
                    .sections
                    .get(&y)
                    .ok_or_else(|| reject(Rejection::MissingSection(y)))?;
                if section.block_states.is_none() {
                    return Err(reject(Rejection::MissingBlockStates(y)));
                }
            }
            return Err(Error::InconsistentSample([
                position[0] * 16,
                MIN_Y,
                position[1] * 16,
            ]));
        }
        Ok(chunk)
    }
    /// No status-based fabrication: a protochunk may expose exact states here.
    /// Use require_complete before treating observations as eligible tour content.
    /// Horizontal-window exclusion precedes vertical-domain boundary reporting.
    pub fn sample(&self, position: [i32; 3]) -> Sample<'a> {
        let [x, y, z] = position;
        if !self.bounds.contains([x, z]) {
            return Sample::OutsideWindow;
        }
        if y > MAX_Y {
            return Sample::AboveOverworld;
        }
        if y < MIN_Y {
            return Sample::BelowOverworld;
        }
        let Some(chunk) = self.chunk([x.div_euclid(16), z.div_euclid(16)]) else {
            return Sample::Missing(Missing::Chunk);
        };
        if !chunk.sections_present {
            return Sample::Missing(Missing::SectionList);
        }
        match chunk.block_at(position) {
            BlockSample::State(state) => Sample::State(state),
            BlockSample::MissingSection => Sample::Missing(Missing::Section),
            BlockSample::MissingBlockStates => Sample::Missing(Missing::BlockStates),
            BlockSample::OutsideChunk | BlockSample::OutsideSectionRange => Sample::OutsideWindow,
        }
    }
    /// Checks every chunk touched by core AND halo, before any scan callback.
    pub fn require_complete(&self, work: &mut Work<'_>) -> Result<(), Error> {
        work.checkpoint()?;
        let lower = self.bounds.minimum.map(|value| value.div_euclid(16));
        let upper = self.bounds.maximum.map(|value| value.div_euclid(16));
        for z in lower[1]..=upper[1] {
            for x in lower[0]..=upper[0] {
                work.step()?;
                self.qualified([x, z])?;
            }
        }
        work.checkpoint()
    }
    /// Coverage of complete input chunks, not a claim that the whole map was read
    /// or that all their columns lie inside this preparation window. No model gate.
    /// The returned BTreeSet owns <=256 entries; its node layout is std-owned.
    pub fn qualified_coverage(&self, work: &mut Work<'_>) -> Result<Coverage, Error> {
        work.checkpoint()?;
        let mut coverage = Coverage::default();
        for chunk in &self.chunks {
            work.step()?;
            let position = chunk.identity.position;
            match self.qualified(position) {
                Ok(_) => {
                    coverage.chunks.insert(position);
                }
                Err(Error::Unqualified { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        work.checkpoint()?;
        Ok(coverage)
    }
    /// Highest non-air state CELL by BlockState::is_air, not soil, a model
    /// surface, or camera clearance. None means an explicitly all-air column.
    /// Validates this column's whole chunk, not the complete window/halo.
    pub fn top_at(
        &self,
        ground: [i32; 2],
        work: &mut Work<'_>,
    ) -> Result<Option<Observation<'a>>, Error> {
        work.checkpoint()?;
        if !self.bounds.contains(ground) {
            return Err(Error::InvalidBounds);
        }
        work.step()?;
        let chunk = self.qualified(ground.map(|value| value.div_euclid(16)))?;
        for y in (MIN_Y..=MAX_Y).rev() {
            work.step()?;
            let position = [ground[0], y, ground[1]];
            let BlockSample::State(state) = chunk.block_at(position) else {
                return Err(Error::InconsistentSample(position));
            };
            if !state.is_air() {
                work.checkpoint()?;
                return Ok(Some(Observation {
                    position,
                    state,
                    only_air_above: true,
                }));
            }
        }
        work.checkpoint()?;
        Ok(None)
    }

    /// Select an explicit surface render band from exact highest non-air cells.
    /// This is not soil classification or a claim that deeper cells are hidden.
    /// A shared lower bound retains buildings/floating geometry between columns
    /// of different heights; per-column clipping would cut their walls away.
    pub fn surface_band(&self, depth: u8, work: &mut Work<'_>) -> Result<Option<[i32; 2]>, Error> {
        if depth > 64 {
            return Err(Error::InvalidBounds);
        }
        self.require_complete(work)?;
        let mut heights = None::<[i32; 2]>;
        for z in self.core.minimum[1]..=self.core.maximum[1] {
            for x in self.core.minimum[0]..=self.core.maximum[0] {
                let Some(top) = self.top_at([x, z], work)? else {
                    continue;
                };
                let y = top.position[1];
                match &mut heights {
                    Some(bounds) => {
                        bounds[0] = bounds[0].min(y);
                        bounds[1] = bounds[1].max(y);
                    }
                    None => heights = Some([y, y]),
                }
            }
        }
        work.checkpoint()?;
        Ok(heights.map(|[minimum, maximum]| [(minimum - i32::from(depth)).max(MIN_Y), maximum]))
    }
    /// Streams EVERY non-air cell in core, including covered caves and interiors.
    /// Stable z/x/descending-y order. No neighbor, shape or alpha-based culling.
    /// Collect transactionally: cancellation/budget/callback errors may follow
    /// callbacks, and the caller MUST discard their partial accumulated output.
    pub fn visit_non_air(
        &self,
        work: &mut Work<'_>,
        visit: impl FnMut(Observation<'a>) -> Result<(), Error>,
    ) -> Result<Scan, Error> {
        self.visit_band([MIN_Y, MAX_Y], work, visit)
    }

    /// Exact saved non-air states within an explicitly clipped render volume.
    /// The omitted height range is not inferred air. Unless the upper bound is
    /// MAX_Y, only_air_above is conservatively false. As with visit_non_air,
    /// consumers must discard collected output on any failure or cancellation.
    pub fn visit_band(
        &self,
        heights: [i32; 2],
        work: &mut Work<'_>,
        mut visit: impl FnMut(Observation<'a>) -> Result<(), Error>,
    ) -> Result<Scan, Error> {
        if heights[0] < MIN_Y || heights[1] > MAX_Y || heights[0] > heights[1] {
            return Err(Error::InvalidBounds);
        }
        self.require_complete(work)?;
        let mut scan = Scan::default();
        for z in self.core.minimum[1]..=self.core.maximum[1] {
            for x in self.core.minimum[0]..=self.core.maximum[0] {
                scan.columns += 1;
                let mut only_air_above = heights[1] == MAX_Y;
                for y in (heights[0]..=heights[1]).rev() {
                    work.step()?;
                    scan.samples += 1;
                    let position = [x, y, z];
                    let Sample::State(state) = self.sample(position) else {
                        return Err(Error::InconsistentSample(position));
                    };
                    if state.is_air() {
                        continue;
                    }
                    scan.non_air += 1;
                    visit(Observation {
                        position,
                        state,
                        only_air_above,
                    })?;
                    only_air_above = false;
                }
            }
        }
        work.checkpoint()?;
        Ok(scan)
    }
}
