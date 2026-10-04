//! Bounded, globally owned five-style villages with fitted parcels and streets.
//! Authored kit geometry/state identity is retained; this is not native jigsaw RNG.
use super::{
    assets::{
        block_state::BlockState,
        error::{AssetError, Result},
        identity::ResourceId,
    },
    noise::hash2,
    settings::VoxelLandscapeSettings,
    surface_biome_selector,
    surface_biomes::SurfaceBiome,
    surface_structures::{self, Habitat, PlacementError, Prepared, Template, TemplateCell},
    terrain_fields::TerrainFields,
    village_kit::{
        self, FarmForm, HomeForm, MarkerKind, PieceKind, Port, PortRole, Profession, RoadShape,
        VillageStyle,
    },
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_RADIUS: i32 = 64;
pub const MAX_PIECES: usize = 37;
pub const MAX_PIECE_WRITES: usize = 4096;
pub const MAX_WRITES: usize = 65_536;
const MAX_EARTHWORK: i32 = 2;
const LOCALITY_ATTEMPTS: i64 = 16;
const LOT_SETBACKS: [i32; 2] = [5, 6];
const MAX_LOTS: usize = 24;

#[derive(Clone)]
pub struct VillageWrite {
    pub state: Option<BlockState>,
    pub piece_source: String,
    pub piece_anchor: [i32; 3],
}
#[derive(Clone, Copy)]
pub struct VillageMarker {
    pub position: [i32; 3],
    pub kind: MarkerKind,
}
/// One atomically admitted owner, including its foundation and entrance link.
/// `contact` records adjacent parent/child floor cells, not overlapping ports.
pub struct VillagePiece {
    pub kind: PieceKind,
    pub anchor: [i32; 3],
    pub rotation: u8,
    pub bounds: [[i32; 2]; 2],
    pub parent: Option<usize>,
    pub contact: Option<[[i32; 3]; 2]>,
    pub walkable: Vec<[i32; 3]>,
}
pub struct VillagePlacement {
    pub style: VillageStyle,
    pub center: [i32; 3],
    pub writes: BTreeMap<[i32; 3], VillageWrite>,
    pub markers: Vec<VillageMarker>,
    pub piece_sources: Vec<String>,
    pub pieces: Vec<VillagePiece>,
    /// Whole Home/Workplace owners; plaza and farm residents are excluded.
    pub residential_pieces: usize,
    pub rejected_pieces: usize,
}
impl VillagePlacement {
    fn quality(&self) -> (bool, usize, usize, usize) {
        let mut forms = [false; 4];
        let mut workplaces = 0;
        let mut farms = 0;
        for piece in &self.pieces {
            match piece.kind {
                PieceKind::Home(form) => {
                    if let Some(index) = HomeForm::ALL.iter().position(|value| *value == form) {
                        forms[index] = true;
                    }
                }
                PieceKind::Workplace(_) => workplaces += 1,
                PieceKind::Farm(_) => farms += 1,
                _ => {}
            }
        }
        let forms = forms.into_iter().filter(|present| *present).count();
        (
            self.residential_pieces >= 6 && forms >= 2 && workplaces > 0 && farms > 0,
            self.residential_pieces,
            forms,
            workplaces + farms,
        )
    }
}
#[derive(Clone, Copy)]
struct GlobalPort {
    position: [i32; 3],
    front: [i32; 2],
    owner: usize,
}
#[derive(Clone, Copy)]
struct StreetTask {
    port: GlobalPort,
    arm: usize,
    segment: usize,
}
struct LotDemand {
    requested: PieceKind,
    variants: Vec<(PieceKind, village_kit::Piece<BlockState>)>,
}
struct LotAssignments {
    ports: Vec<GlobalPort>,
    used_ports: [bool; MAX_LOTS],
    placed: [bool; MAX_LOTS],
    attempted: [[bool; MAX_LOTS]; MAX_LOTS],
}
impl LotAssignments {
    fn place(
        &mut self,
        index: usize,
        demand: &LotDemand,
        builder: &mut Builder<'_>,
    ) -> Result<bool> {
        if self.placed[index] {
            return Ok(false);
        }
        for port in 0..self.ports.len() {
            builder.check_cancelled()?;
            if self.used_ports[port] || self.attempted[index][port] {
                continue;
            }
            // Claims only grow: a failed demand/port fit cannot improve later.
            self.attempted[index][port] = true;
            if builder.lot_variants(self.ports[port], &demand.variants)? {
                self.placed[index] = true;
                self.used_ports[port] = true;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
struct Draft {
    kind: PieceKind,
    source: String,
    anchor: [i32; 3],
    rotation: u8,
    cells: BTreeMap<[i32; 3], Option<BlockState>>,
    walkable: Vec<[i32; 3]>,
    markers: Vec<VillageMarker>,
    contact: Option<(GlobalPort, [i32; 3])>,
}
#[derive(Clone, Copy)]
struct HeightRange {
    lower: i32,
    upper: i32,
    preferred: i32,
}
struct Builder<'a> {
    village: VillagePlacement,
    fields: &'a TerrainFields,
    settings: &'a VoxelLandscapeSettings,
    cancelled: &'a dyn Fn() -> bool,
    ground: BTreeMap<[i32; 2], Option<i32>>,
    foundation: BlockState,
    path: BlockState,
    stairs: BlockState,
}
fn rotate([x, y, z]: [i32; 3], turns: u8) -> [i32; 3] {
    match turns % 4 {
        1 => [-y, x, z],
        2 => [-x, -y, z],
        3 => [y, -x, z],
        _ => [x, y, z],
    }
}
fn checked_add3(a: [i32; 3], b: [i32; 3]) -> Option<[i32; 3]> {
    Some([
        a[0].checked_add(b[0])?,
        a[1].checked_add(b[1])?,
        a[2].checked_add(b[2])?,
    ])
}
fn checked_sub3(a: [i32; 3], b: [i32; 3]) -> Option<[i32; 3]> {
    Some([
        a[0].checked_sub(b[0])?,
        a[1].checked_sub(b[1])?,
        a[2].checked_sub(b[2])?,
    ])
}
fn port(anchor: [i32; 3], turns: u8, local: Port, owner: usize) -> Option<GlobalPort> {
    let front = rotate([local.front[0], local.front[1], 0], turns);
    Some(GlobalPort {
        position: checked_add3(anchor, rotate(local.position, turns))?,
        front: [front[0], front[1]],
        owner,
    })
}
fn state(id: &str, properties: &[(&str, &str)]) -> std::result::Result<BlockState, PlacementError> {
    BlockState::new(
        ResourceId::parse(id).map_err(|_| PlacementError::InvalidState)?,
        properties
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string())),
    )
    .map_err(|_| PlacementError::InvalidState)
}
fn rotated_state(state: &BlockState, turns: u8) -> std::result::Result<BlockState, PlacementError> {
    let properties = surface_structures::rotate_properties(state.properties(), turns)?;
    BlockState::new(state.id().clone(), properties).map_err(|_| PlacementError::InvalidState)
}
fn error(problem: PlacementError) -> AssetError {
    AssetError::InvalidMetadata(format!("village placement: {problem:?}"))
}
fn checked(position: Option<[i32; 3]>) -> Result<[i32; 3]> {
    position.ok_or_else(|| error(PlacementError::CoordinateOverflow))
}
fn rectangle(positions: impl Iterator<Item = [i32; 3]>) -> Option<[[i32; 2]; 2]> {
    let mut bounds = [[i32::MAX; 2], [i32::MIN; 2]];
    let mut present = false;
    for position in positions {
        present = true;
        for axis in 0..2 {
            bounds[0][axis] = bounds[0][axis].min(position[axis]);
            bounds[1][axis] = bounds[1][axis].max(position[axis].checked_add(1)?);
        }
    }
    present.then_some(bounds)
}
fn intersects(a: [[i32; 2]; 2], b: [[i32; 2]; 2]) -> bool {
    (0..2).all(|axis| a[0][axis] < b[1][axis] && b[0][axis] < a[1][axis])
}
fn passable(state: Option<&BlockState>) -> bool {
    let Some(state) = state else {
        return true;
    };
    let id = state.id().as_str();
    // These are the kit's openable doors and passable aisle decorations.
    id.ends_with("_door")
        || id.ends_with("_carpet")
        || id == "minecraft:poppy"
        || (id.ends_with("_fence_gate")
            && state
                .properties()
                .get("open")
                .is_some_and(|value| value == "true"))
}
fn grounds_ok(
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    center: [i32; 3],
    position: [i32; 3],
) -> bool {
    let sample = fields.sample(position[0], position[1], settings.rivers);
    sample
        .water_level
        .is_none_or(|water| water <= sample.height)
        && (i64::from(sample.height) - i64::from(center[2])).abs() <= i64::from(MAX_EARTHWORK)
}
/// At most five heights and fifteen adjacent-height pairs survive each row.
/// A one-row valley would need two opposed stairs in one cell, so reject it.
fn fit_profile(
    ranges: &[HeightRange],
    start: i32,
    end: Option<i32>,
    junction: Option<usize>,
    flat_end: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Vec<i32>>> {
    if ranges.is_empty() || ranges.len() > 27 {
        return Err(error(PlacementError::Budget));
    }
    let mut frontier = BTreeMap::from([([start, start], (0_i64, Vec::<i32>::new()))]);
    for (index, range) in ranges.iter().enumerate() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        if range.lower > range.upper || range.upper - range.lower > MAX_EARTHWORK * 2 {
            return Ok(None);
        }
        let mut next = BTreeMap::<[i32; 2], (i64, Vec<i32>)>::new();
        for (&[before, previous], (cost, path)) in &frontier {
            for height in range.lower..=range.upper {
                let forced_flat = junction.is_some_and(|row| index >= row && index <= row + 1)
                    || (flat_end && index + 1 == ranges.len());
                if (index == 0 && height != start)
                    || (index + 1 == ranges.len() && end.is_some_and(|end| height != end))
                    || (height - previous).abs() > 1
                    || (before > previous && height > previous)
                    || (previous > height && height + 1 > range.upper)
                    || (index > 0
                        && (before > previous || height > previous)
                        && previous + 1 > ranges[index - 1].upper)
                    || (forced_flat && height != previous)
                {
                    continue;
                }
                let mut path = path.clone();
                path.push(height);
                let candidate = (*cost + i64::from((height - range.preferred).abs()), path);
                let key = [previous, height];
                if next.get(&key).is_none_or(|old| candidate < *old) {
                    next.insert(key, candidate);
                }
            }
        }
        if next.is_empty() {
            return Ok(None);
        }
        frontier = next;
    }
    Ok(frontier.into_values().min().map(|(_, path)| path))
}
impl Builder<'_> {
    fn check_cancelled(&self) -> Result<()> {
        if (self.cancelled)() {
            return Err(AssetError::Cancelled);
        }
        Ok(())
    }
    fn reject<T>(&mut self) -> Result<Option<T>> {
        self.village.rejected_pieces += 1;
        Ok(None)
    }
    fn within_radius(&self, bounds: [[i32; 2]; 2]) -> bool {
        (0..2).all(|axis| {
            i64::from(bounds[0][axis])
                >= i64::from(self.village.center[axis]) - i64::from(MAX_RADIUS)
                && i64::from(bounds[1][axis])
                    <= i64::from(self.village.center[axis]) + i64::from(MAX_RADIUS) + 1
        })
    }
    fn available(&self, bounds: [[i32; 2]; 2]) -> bool {
        self.within_radius(bounds)
            && !self
                .village
                .pieces
                .iter()
                .any(|piece| intersects(bounds, piece.bounds))
    }
    fn height(&mut self, position: [i32; 2]) -> Result<Option<i32>> {
        self.check_cancelled()?;
        let bounds = [
            position,
            [
                position[0]
                    .checked_add(1)
                    .ok_or_else(|| error(PlacementError::CoordinateOverflow))?,
                position[1]
                    .checked_add(1)
                    .ok_or_else(|| error(PlacementError::CoordinateOverflow))?,
            ],
        ];
        if !self.within_radius(bounds) {
            return Ok(None);
        }
        if let Some(height) = self.ground.get(&position) {
            return Ok(*height);
        }
        let sample = self
            .fields
            .sample(position[0], position[1], self.settings.rivers);
        let height = sample
            .water_level
            .is_none_or(|water| water <= sample.height)
            .then_some(i32::from(sample.height));
        self.ground.insert(position, height);
        Ok(height)
    }
    fn fit_range(&mut self, bounds: [[i32; 2]; 2]) -> Result<Option<HeightRange>> {
        if !self.available(bounds) {
            return Ok(None);
        }
        let mut minimum = i32::MAX;
        let mut maximum = i32::MIN;
        for x in bounds[0][0]..bounds[1][0] {
            for y in bounds[0][1]..bounds[1][1] {
                let Some(height) = self.height([x, y])? else {
                    return Ok(None);
                };
                minimum = minimum.min(height);
                maximum = maximum.max(height);
            }
        }
        let lower = maximum - MAX_EARTHWORK;
        let upper = minimum + MAX_EARTHWORK;
        Ok((lower <= upper).then_some(HeightRange {
            lower,
            upper,
            preferred: minimum + (maximum - minimum) / 2,
        }))
    }
    fn rigid(
        &mut self,
        piece: &village_kit::Piece<BlockState>,
        kind: PieceKind,
        anchor: [i32; 3],
        turns: u8,
    ) -> Result<Option<Draft>> {
        let mut cells = BTreeMap::new();
        for cell in &piece.template.cells {
            self.check_cancelled()?;
            let position = checked(checked_add3(anchor, rotate(cell.position, turns)))?;
            let value = cell
                .state
                .as_ref()
                .map(|value| rotated_state(value, turns))
                .transpose()
                .map_err(error)?;
            cells.insert(position, value);
        }
        let bounds =
            rectangle(cells.keys().copied()).ok_or_else(|| error(PlacementError::Bounds))?;
        let Some(range) = self.fit_range(bounds)? else {
            return Ok(None);
        };
        if anchor[2] < range.lower || anchor[2] > range.upper {
            return Ok(None);
        }
        // Extend only actual basal solid columns. Roof overhangs do not become
        // artificial solid platforms, and existing farm dirt remains its base.
        let mut bases = BTreeMap::<[i32; 2], (i32, BlockState)>::new();
        for (&position, value) in &cells {
            let Some(value) = value else {
                continue;
            };
            if position[2] > anchor[2] {
                continue;
            }
            let key = [position[0], position[1]];
            if bases
                .get(&key)
                .is_none_or(|(height, _)| position[2] < *height)
            {
                bases.insert(key, (position[2], value.clone()));
            }
        }
        for (xy, (base, value)) in bases {
            let Some(ground) = self.height(xy)? else {
                return Ok(None);
            };
            let support = if value.id().as_str().ends_with("_stairs") {
                &self.foundation
            } else {
                &value
            };
            for z in ground..base {
                cells
                    .entry([xy[0], xy[1], z])
                    .or_insert_with(|| Some(support.clone()));
            }
        }
        let mut walkable = Vec::new();
        for position in &piece.walkable {
            walkable.push(checked(checked_add3(anchor, rotate(*position, turns)))?);
        }
        let mut markers = Vec::new();
        for marker in &piece.markers {
            markers.push(VillageMarker {
                position: checked(checked_add3(anchor, rotate(marker.position, turns)))?,
                kind: marker.kind,
            });
        }
        Ok(Some(Draft {
            kind,
            source: piece.template.source.clone(),
            anchor,
            rotation: turns,
            cells,
            walkable,
            markers,
            contact: None,
        }))
    }
    fn commit(&mut self, draft: Draft) -> Result<Option<usize>> {
        self.check_cancelled()?;
        let bounds =
            rectangle(draft.cells.keys().copied()).ok_or_else(|| error(PlacementError::Bounds))?;
        if !self.available(bounds) {
            return self.reject();
        }
        if self.village.pieces.len() >= MAX_PIECES
            || draft.cells.len() > MAX_PIECE_WRITES
            || self.village.writes.len() + draft.cells.len() > MAX_WRITES
        {
            return Err(error(PlacementError::Budget));
        }
        let floors: BTreeSet<_> = draft.walkable.iter().copied().collect();
        if floors.is_empty() {
            return Err(error(PlacementError::InvalidState));
        }
        for &position in &floors {
            self.check_cancelled()?;
            if !draft.cells.get(&position).is_some_and(|value| {
                value
                    .as_ref()
                    .is_some_and(|value| value.id().as_str() != "minecraft:water")
            }) {
                return Err(error(PlacementError::InvalidState));
            }
            for dz in 1..=2 {
                let head = checked(checked_add3(position, [0, 0, dz]))?;
                if !draft
                    .cells
                    .get(&head)
                    .is_some_and(|value| passable(value.as_ref()))
                {
                    return Err(error(PlacementError::InvalidState));
                }
            }
        }
        let first = *floors
            .first()
            .ok_or_else(|| error(PlacementError::InvalidState))?;
        let mut reached = BTreeSet::from([first]);
        let mut frontier = VecDeque::from([first]);
        while let Some(position) = frontier.pop_front() {
            self.check_cancelled()?;
            for [dx, dy] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                for dz in -1..=1 {
                    let neighbor = checked(checked_add3(position, [dx, dy, dz]))?;
                    if floors.contains(&neighbor) && reached.insert(neighbor) {
                        frontier.push_back(neighbor);
                    }
                }
            }
        }
        if reached.len() != floors.len() {
            return self.reject();
        }
        if let Some((parent, child)) = draft.contact {
            let Some(owner) = self.village.pieces.get(parent.owner) else {
                return Err(error(PlacementError::InvalidState));
            };
            if !owner.walkable.contains(&parent.position)
                || !floors.contains(&child)
                || (i64::from(child[0]) - i64::from(parent.position[0])).abs()
                    + (i64::from(child[1]) - i64::from(parent.position[1])).abs()
                    != 1
                || (i64::from(child[2]) - i64::from(parent.position[2])).abs() > 1
            {
                return Err(error(PlacementError::InvalidState));
            }
        } else if !self.village.pieces.is_empty() {
            return Err(error(PlacementError::InvalidState));
        }
        let mut marker_positions: BTreeSet<_> = self
            .village
            .markers
            .iter()
            .map(|marker| marker.position)
            .collect();
        for marker in &draft.markers {
            self.check_cancelled()?;
            let floor = checked(checked_add3(marker.position, [0, 0, -1]))?;
            if !marker_positions.insert(marker.position)
                || !draft.cells.get(&floor).is_some_and(Option::is_some)
            {
                return Err(error(PlacementError::InvalidState));
            }
        }
        let mut cells = Vec::with_capacity(draft.cells.len());
        for (position, value) in draft.cells {
            self.check_cancelled()?;
            cells.push(TemplateCell {
                position: checked(checked_sub3(position, draft.anchor))?,
                state: value,
            });
        }
        let template = Template {
            source: draft.source,
            cells,
        };
        let prepared = Prepared::prepare(
            &template,
            draft.anchor,
            0,
            |value, _| Ok(value.clone()),
            |position| {
                if self.village.writes.contains_key(&position) {
                    Habitat::Protected
                } else {
                    Habitat::Replaceable
                }
            },
            self.cancelled,
        );
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(PlacementError::Cancelled) => return Err(AssetError::Cancelled),
            Err(PlacementError::Protected(_) | PlacementError::Unknown(_)) => return self.reject(),
            Err(problem) => return Err(error(problem)),
        };
        let mut writes = BTreeMap::new();
        for (position, value) in prepared.cells() {
            self.check_cancelled()?;
            writes.insert(
                position,
                VillageWrite {
                    state: value.cloned(),
                    piece_source: prepared.source().to_owned(),
                    piece_anchor: draft.anchor,
                },
            );
        }
        // Nothing observable is changed until every cell, route and marker has
        // passed, including a final cancellation check before bounded commit.
        self.check_cancelled()?;
        let owner = self.village.pieces.len();
        self.village.writes.extend(writes);
        self.village.markers.extend(draft.markers);
        self.village
            .piece_sources
            .push(prepared.source().to_owned());
        self.village.pieces.push(VillagePiece {
            kind: draft.kind,
            anchor: draft.anchor,
            rotation: draft.rotation,
            bounds,
            parent: draft.contact.map(|(parent, _)| parent.owner),
            contact: draft
                .contact
                .map(|(parent, child)| [parent.position, child]),
            walkable: floors.into_iter().collect(),
        });
        if matches!(draft.kind, PieceKind::Home(_) | PieceKind::Workplace(_)) {
            self.village.residential_pieces += 1;
        }
        Ok(Some(owner))
    }
    fn path_draft(
        &mut self,
        parent: GlobalPort,
        length: i32,
        half_width: i32,
        end: Option<i32>,
        junction: Option<usize>,
        source: String,
    ) -> Result<Option<Draft>> {
        let turns = (0..4)
            .find(|turns| rotate([0, 1, 0], *turns) == [parent.front[0], parent.front[1], 0])
            .ok_or_else(|| error(PlacementError::InvalidState))?;
        let mut ranges = Vec::new();
        for distance in 1..=length {
            let mut minimum = i32::MAX;
            let mut maximum = i32::MIN;
            for side in -half_width..=half_width {
                let position = checked(checked_add3(
                    parent.position,
                    rotate([side, distance, 0], turns),
                ))?;
                let Some(height) = self.height([position[0], position[1]])? else {
                    return Ok(None);
                };
                minimum = minimum.min(height);
                maximum = maximum.max(height);
            }
            ranges.push(HeightRange {
                lower: maximum - MAX_EARTHWORK,
                upper: minimum + MAX_EARTHWORK,
                preferred: minimum + (maximum - minimum) / 2,
            });
        }
        let Some(profile) = fit_profile(
            &ranges,
            parent.position[2],
            end,
            junction,
            junction.is_some(),
            self.cancelled,
        )?
        else {
            return Ok(None);
        };
        let mut cells = BTreeMap::new();
        let mut walkable = Vec::new();
        for (index, &deck) in profile.iter().enumerate() {
            let before = index
                .checked_sub(1)
                .map_or(parent.position[2], |index| profile[index]);
            let after = profile.get(index + 1).copied().or(end).unwrap_or(deck);
            let uphill = before > deck || after > deck;
            let floor = deck + if uphill { 1 } else { 0 };
            let surface = if uphill {
                rotated_state(
                    &self.stairs,
                    (turns + if before > deck { 2 } else { 0 }) % 4,
                )
                .map_err(error)?
            } else {
                self.path.clone()
            };
            for side in -half_width..=half_width {
                let mut position = checked(checked_add3(
                    parent.position,
                    rotate([side, index as i32 + 1, 0], turns),
                ))?;
                position[2] = floor;
                let Some(ground) = self.height([position[0], position[1]])? else {
                    return Ok(None);
                };
                if (floor - ground).abs() > MAX_EARTHWORK {
                    return Ok(None);
                }
                for z in ground..floor {
                    cells.insert([position[0], position[1], z], Some(self.foundation.clone()));
                }
                cells.insert(position, Some(surface.clone()));
                for dz in 1..=2 {
                    cells.insert(checked(checked_add3(position, [0, 0, dz]))?, None);
                }
                walkable.push(position);
            }
        }
        let first_xy = checked(checked_add3(
            parent.position,
            [parent.front[0], parent.front[1], 0],
        ))?;
        let first = *walkable
            .iter()
            .find(|position| position[..2] == first_xy[..2])
            .ok_or_else(|| error(PlacementError::InvalidState))?;
        Ok(Some(Draft {
            kind: PieceKind::Road(RoadShape::Straight),
            source,
            anchor: parent.position,
            rotation: turns,
            cells,
            walkable,
            markers: Vec::new(),
            contact: Some((parent, first)),
        }))
    }
    fn street(&mut self, task: StreetTask, seed: u64) -> Result<Option<[GlobalPort; 3]>> {
        let (length, station) = if task.segment == 0 { (6, 5) } else { (16, 15) };
        let source = format!(
            "homage:village/{}/Road(Straight)/seed{seed}/arm{}/segment{}",
            self.village.style.name(),
            task.arm,
            task.segment
        );
        let Some(draft) = self.path_draft(
            task.port,
            length,
            1,
            None,
            Some(station as usize - 1),
            source,
        )?
        else {
            return self.reject();
        };
        let right = [task.port.front[1], -task.port.front[0]];
        let mut ports = [task.port; 3];
        for (index, (distance, side, front)) in [
            (length, 0, task.port.front),
            (station, -1, [-right[0], -right[1]]),
            (station, 1, right),
        ]
        .into_iter()
        .enumerate()
        {
            let xy = checked(checked_add3(
                task.port.position,
                [
                    task.port.front[0] * distance + right[0] * side,
                    task.port.front[1] * distance + right[1] * side,
                    0,
                ],
            ))?;
            let position = *draft
                .walkable
                .iter()
                .find(|position| position[..2] == xy[..2])
                .ok_or_else(|| error(PlacementError::InvalidState))?;
            ports[index] = GlobalPort {
                position,
                front,
                owner: 0,
            };
        }
        let Some(owner) = self.commit(draft)? else {
            return Ok(None);
        };
        for port in &mut ports {
            port.owner = owner;
        }
        Ok(Some(ports))
    }
    fn lot_demand(&self, kind: PieceKind, seed: u64) -> Result<LotDemand> {
        let fallback = match kind {
            PieceKind::Home(form) if form != HomeForm::Cottage => {
                Some(PieceKind::Home(HomeForm::Cottage))
            }
            PieceKind::Farm(FarmForm::Long) => Some(PieceKind::Farm(FarmForm::Compact)),
            _ => None,
        };
        let mut variants = Vec::with_capacity(2);
        for variant in std::iter::once(kind).chain(fallback) {
            self.check_cancelled()?;
            variants.push((
                variant,
                village_kit::build(self.village.style, variant, seed, state).map_err(error)?,
            ));
        }
        Ok(LotDemand {
            requested: kind,
            variants,
        })
    }
    fn lot_variants(
        &mut self,
        parent: GlobalPort,
        variants: &[(PieceKind, village_kit::Piece<BlockState>)],
    ) -> Result<bool> {
        for (kind, piece) in variants {
            self.check_cancelled()?;
            let entrance = *piece
                .ports
                .first()
                .ok_or_else(|| error(PlacementError::InvalidState))?;
            if piece.ports.len() != 1 || entrance.role != PortRole::Entrance {
                return Err(error(PlacementError::InvalidState));
            }
            let turns = (0..4)
                .find(|turns| {
                    rotate([entrance.front[0], entrance.front[1], 0], *turns)
                        == [-parent.front[0], -parent.front[1], 0]
                })
                .ok_or_else(|| error(PlacementError::InvalidState))?;
            for setback in LOT_SETBACKS {
                let target = checked(checked_add3(
                    parent.position,
                    [
                        parent.front[0] * (setback - 1),
                        parent.front[1] * (setback - 1),
                        0,
                    ],
                ))?;
                let mut anchor = checked(checked_sub3(target, rotate(entrance.position, turns)))?;
                let mut corners = Vec::new();
                for x in [piece.bounds[0][0], piece.bounds[1][0] - 1] {
                    for y in [piece.bounds[0][1], piece.bounds[1][1] - 1] {
                        corners.push(checked(checked_add3(anchor, rotate([x, y, 0], turns)))?);
                    }
                }
                let bounds =
                    rectangle(corners.into_iter()).ok_or_else(|| error(PlacementError::Bounds))?;
                let Some(range) = self.fit_range(bounds)? else {
                    self.village.rejected_pieces += 1;
                    continue;
                };
                let mut heights: Vec<_> = (range.lower..=range.upper).collect();
                heights.sort_by_key(|height| {
                    (
                        (height - parent.position[2]).abs(),
                        (height - range.preferred).abs(),
                        *height,
                    )
                });
                for height in heights {
                    anchor[2] = height;
                    let Some(mut draft) = self.rigid(piece, *kind, anchor, turns)? else {
                        continue;
                    };
                    let Some(link) = self.path_draft(
                        parent,
                        setback - 2,
                        0,
                        Some(height),
                        None,
                        draft.source.clone(),
                    )?
                    else {
                        self.village.rejected_pieces += 1;
                        continue;
                    };
                    if link
                        .cells
                        .keys()
                        .any(|position| draft.cells.contains_key(position))
                    {
                        return Err(error(PlacementError::Duplicate(target)));
                    }
                    draft.cells.extend(link.cells);
                    draft.walkable.extend(link.walkable);
                    draft.contact = link.contact;
                    if self.commit(draft)?.is_some() {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}
/// Four continuing arms, three stations per arm and two lots per station.
/// Stations 10/26/42 reach the near-plaza basin; sixteen cells separate later
/// thirteen-wide parcels. Roads claim their complete corridors before lots.
/// The original 24 demands use at most 24 ports and 576 demand/port searches.
/// Candidate centers keep every complete village inside its owning 256 tile.
pub fn assemble(
    style: VillageStyle,
    center: [i32; 3],
    seed: u64,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<VillagePlacement> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    if center
        .iter()
        .any(|value| value.unsigned_abs() > (i32::MAX - 512) as u32)
    {
        return Err(AssetError::InvalidMetadata(
            "village center outside safe domain".into(),
        ));
    }
    let center_piece = village_kit::build(style, PieceKind::Center, seed, state).map_err(error)?;
    if center_piece.ports.len() != 4
        || center_piece
            .ports
            .iter()
            .any(|port| port.role != PortRole::Street)
    {
        return Err(error(PlacementError::InvalidState));
    }
    let slope =
        village_kit::build(style, PieceKind::Road(RoadShape::Slope), seed, state).map_err(error)?;
    let material = |piece: &village_kit::Piece<BlockState>, position| {
        piece
            .template
            .cells
            .iter()
            .find(|cell| cell.position == position)
            .and_then(|cell| cell.state.clone())
            .ok_or_else(|| error(PlacementError::InvalidState))
    };
    let mut builder = Builder {
        village: VillagePlacement {
            style,
            center,
            writes: BTreeMap::new(),
            markers: Vec::new(),
            piece_sources: Vec::new(),
            pieces: Vec::new(),
            residential_pieces: 0,
            rejected_pieces: 0,
        },
        fields,
        settings,
        cancelled: &cancelled,
        ground: BTreeMap::new(),
        foundation: material(&center_piece, [0, 0, 0])?,
        path: material(&slope, [5, 0, 0])?,
        stairs: material(&slope, [5, 5, 0])?,
    };
    let anchor = checked(checked_add3(center, [-5, -5, 0]))?;
    let Some(draft) = builder.rigid(&center_piece, PieceKind::Center, anchor, 0)? else {
        builder.village.rejected_pieces += 1;
        return Ok(builder.village);
    };
    let Some(owner) = builder.commit(draft)? else {
        return Ok(builder.village);
    };
    let mut frontier = VecDeque::new();
    for (arm, local) in center_piece.ports.iter().enumerate() {
        let port = port(anchor, 0, *local, owner)
            .ok_or_else(|| error(PlacementError::CoordinateOverflow))?;
        frontier.push_back(StreetTask {
            port,
            arm,
            segment: 0,
        });
    }
    let mut lots = LotAssignments {
        ports: Vec::with_capacity(MAX_LOTS),
        used_ports: [false; MAX_LOTS],
        placed: [false; MAX_LOTS],
        attempted: [[false; MAX_LOTS]; MAX_LOTS],
    };
    while let Some(task) = frontier.pop_front() {
        builder.check_cancelled()?;
        let Some([continuation, left, right]) = builder.street(task, seed)? else {
            continue;
        };
        lots.ports.extend([left, right]);
        if task.segment < 2 {
            frontier.push_back(StreetTask {
                port: continuation,
                segment: task.segment + 1,
                ..task
            });
        }
    }
    let mut demands = Vec::with_capacity(MAX_LOTS);
    for segment in 0..3 {
        for arm in 0..4 {
            for side in 0..2 {
                let slot = segment * 8 + arm * 2 + side;
                let choice = hash2(
                    seed ^ 0x0076_696c_6c61_6765,
                    arm as i64,
                    (segment * 2 + side) as i64,
                );
                let kind = match slot % 8 {
                    1 => PieceKind::Workplace(
                        Profession::ALL[choice as usize % Profession::ALL.len()],
                    ),
                    3 => PieceKind::Farm(FarmForm::ALL[choice as usize % FarmForm::ALL.len()]),
                    5 => PieceKind::Pen,
                    _ => PieceKind::Home(
                        HomeForm::ALL
                            [(slot / 2 + segment + (seed & 3) as usize) % HomeForm::ALL.len()],
                    ),
                };
                demands.push(builder.lot_demand(kind, choice)?);
            }
        }
    }
    for (index, demand) in demands.iter().enumerate() {
        if matches!(demand.requested, PieceKind::Workplace(_))
            && lots.place(index, demand, &mut builder)?
        {
            break;
        }
    }
    for (index, demand) in demands.iter().enumerate() {
        if matches!(demand.requested, PieceKind::Farm(_))
            && lots.place(index, demand, &mut builder)?
        {
            break;
        }
    }
    let mut homes = 0;
    let mut forms = [false; 4];
    for (index, demand) in demands.iter().enumerate() {
        if !matches!(demand.requested, PieceKind::Home(_))
            || !lots.place(index, demand, &mut builder)?
        {
            continue;
        }
        if let Some(PieceKind::Home(form)) = builder.village.pieces.last().map(|piece| piece.kind) {
            homes += 1;
            forms[HomeForm::ALL
                .iter()
                .position(|value| *value == form)
                .ok_or_else(|| error(PlacementError::InvalidState))?] = true;
        }
        if homes >= 6 && forms.iter().filter(|present| **present).count() >= 2 {
            break;
        }
    }
    for (index, demand) in demands.iter().enumerate() {
        if demand.requested == PieceKind::Pen && lots.place(index, demand, &mut builder)? {
            break;
        }
    }
    for (index, demand) in demands.iter().enumerate() {
        lots.place(index, demand, &mut builder)?;
    }
    builder.check_cancelled()?;
    Ok(builder.village)
}
pub fn style_for(biome: SurfaceBiome) -> Option<VillageStyle> {
    use SurfaceBiome::*;
    Some(match biome {
        Plains | SunflowerPlains => VillageStyle::Plains,
        Desert => VillageStyle::Desert,
        Savanna | SavannaPlateau => VillageStyle::Savanna,
        Taiga | OldGrowthPineTaiga | OldGrowthSpruceTaiga => VillageStyle::Taiga,
        SnowyPlains | SnowyTaiga => VillageStyle::Snowy,
        _ => return None,
    })
}
/// Deterministic grid candidate, independent of the requested render region.
/// A failed locality remains absent; it is never replaced by a clipped piece.
pub fn candidate(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<Option<VillagePlacement>> {
    candidate_with_observer(grid, fields, settings, cancelled, |_, _, _| {})
}
fn candidate_with_observer(
    grid: [i32; 2],
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
    mut observe: impl FnMut(usize, u64, [i32; 2]),
) -> Result<Option<VillagePlacement>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let density = settings.structures_percent.clamp(0, 200);
    if density == 0 {
        return Ok(None);
    }
    let seed = u64::from(settings.seed);
    let gate = hash2(
        seed ^ 0x0076_696c_6c61_6765,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if gate % 100 >= density as u64 * 15 / 100 {
        return Ok(None);
    }
    let Some(base_x) = grid[0]
        .checked_mul(256)
        .and_then(|value| value.checked_add(128))
    else {
        return Ok(None);
    };
    let Some(base_y) = grid[1]
        .checked_mul(256)
        .and_then(|value| value.checked_add(128))
    else {
        return Ok(None);
    };
    let mut best: Option<VillagePlacement> = None;
    for attempt in 0..LOCALITY_ATTEMPTS {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let hash = hash2(
            seed ^ 0x0073_7472_7563_7473,
            i64::from(grid[0]),
            i64::from(grid[1]) * 16 + attempt,
        );
        let Some(x) = base_x.checked_add(((hash & 7) as i32 - 4) * 16) else {
            continue;
        };
        let Some(y) = base_y.checked_add((((hash >> 3) & 7) as i32 - 4) * 16) else {
            continue;
        };
        if [x, y]
            .into_iter()
            .any(|value| value.unsigned_abs() > (i32::MAX - 512) as u32)
        {
            continue;
        }
        observe(attempt as usize, hash, [x, y]);
        let sample = fields.sample(x, y, settings.rivers);
        if sample
            .water_level
            .is_some_and(|water| water > sample.height)
        {
            continue;
        }
        let biome = surface_biome_selector::select(seed, [x, y], sample);
        let Some(style) = style_for(biome) else {
            continue;
        };
        let center = [x, y, i32::from(sample.height)];
        if !(-5..=5).all(|dx| {
            (-5..=5).all(|dy| grounds_ok(fields, settings, center, [x + dx, y + dy, center[2]]))
        }) {
            continue;
        }
        let village = assemble(style, center, hash, fields, settings, &cancelled)?;
        // Keep the original sixteen locality identities. A lone first home
        // no longer prevents evaluating a later, better connected settlement.
        if village.quality().0 {
            return Ok(Some(village));
        }
        if village.residential_pieces > 0
            && best
                .as_ref()
                .is_none_or(|best| village.quality() > best.quality())
        {
            best = Some(village);
        }
    }
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    Ok(best)
}
pub fn source(style: VillageStyle) -> &'static str {
    match style {
        VillageStyle::Plains => "minecraft:village_plains",
        VillageStyle::Desert => "minecraft:village_desert",
        VillageStyle::Savanna => "minecraft:village_savanna",
        VillageStyle::Taiga => "minecraft:village_taiga",
        VillageStyle::Snowy => "minecraft:village_snowy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_candidates_do_not_publish_road_and_farm_only_clearings() {
        let settings = VoxelLandscapeSettings {
            seed: 71839,
            ..Default::default()
        };
        let fields = TerrainFields::new(71839);
        for (style, center, seed, grid) in [
            (
                VillageStyle::Desert,
                [2880, -368, 86],
                10294309595677504680,
                [11, -2],
            ),
            (
                VillageStyle::Snowy,
                [656, 336, 74],
                4313055858617941837,
                [2, 1],
            ),
        ] {
            let direct = assemble(style, center, seed, &fields, &settings, || false).unwrap();
            let has_bed = direct.writes.values().any(|write| {
                write
                    .state
                    .as_ref()
                    .is_some_and(|state| state.id().as_str().ends_with("_bed"))
            });
            // These old empty-home localities may now fit complete residences.
            // Keep the publication guard, rather than requiring the old defect.
            assert_eq!(direct.residential_pieces > 0, has_bed);
            if let Some(village) = candidate(grid, &fields, &settings, || false).unwrap() {
                assert!(village.residential_pieces > 0);
                assert!(village.writes.values().any(|write| {
                    write
                        .state
                        .as_ref()
                        .is_some_and(|state| state.id().as_str().ends_with("_bed"))
                }));
            }
        }
    }

    #[test]
    fn five_styles_produce_connected_atomic_original_pieces() {
        let settings = VoxelLandscapeSettings {
            rivers: false,
            ..Default::default()
        };
        let fields = TerrainFields::new(42);
        let center = (0..1024)
            .step_by(32)
            .find_map(|x| {
                (0..1024).step_by(32).find_map(|y| {
                    let center = [x, y, i32::from(fields.sample(x, y, false).height)];
                    ((-5..=5).all(|dx| {
                        (-5..=5).all(|dy| {
                            grounds_ok(&fields, &settings, center, [x + dx, y + dy, center[2]])
                        })
                    }))
                    .then_some(center)
                })
            })
            .expect("no admissible center found in deterministic terrain search");
        for style in VillageStyle::ALL {
            let village = assemble(style, center, 7, &fields, &settings, || false).unwrap();
            assert!(
                village.piece_sources.len() > 1,
                "{}: {}",
                style.name(),
                village.piece_sources.len()
            );
            assert!(!village.writes.is_empty());
            let residences = village
                .piece_sources
                .iter()
                .filter(|source| source.contains("/Home(") || source.contains("/Workplace("))
                .count();
            assert_eq!(village.residential_pieces, residences);
            let again = assemble(style, center, 7, &fields, &settings, || false).unwrap();
            assert_eq!(village.writes.len(), again.writes.len());
        }
    }
}

#[cfg(test)]
#[path = "surface_village_forcing_tests.rs"]
mod village_layout_forcing_tests;

#[cfg(test)]
#[path = "surface_village_layout_tests.rs"]
mod village_layout_tests;

// Workstation consultation: test-only access to the actual kit/placement rotation.
#[cfg(test)]
pub(super) struct WorkstationCase {
    pub style: &'static str,
    pub profession: &'static str,
    pub turns: u8,
    pub source: String,
    pub position: [i32; 3],
    pub state: BlockState,
}

#[cfg(test)]
pub(super) fn workstation_cases() -> Vec<WorkstationCase> {
    let mut cases = Vec::new();
    for style in VillageStyle::ALL {
        for job in Profession::ALL {
            let piece = village_kit::build(style, PieceKind::Workplace(job), 97, state).unwrap();
            for turns in 0..4 {
                let prepared = Prepared::prepare(
                    &piece.template,
                    [0, 0, 64],
                    turns,
                    rotated_state,
                    |_| Habitat::Replaceable,
                    || false,
                )
                .unwrap();
                for (position, value) in prepared.project([-512; 2], [512; 2]).unwrap() {
                    if let Some(value) =
                        value.filter(|value| value.id().as_str() == job.workstation())
                    {
                        cases.push(WorkstationCase {
                            style: style.name(),
                            profession: job.workstation(),
                            turns,
                            source: prepared.source().to_owned(),
                            position,
                            state: value.clone(),
                        });
                    }
                }
            }
        }
    }
    cases
}
