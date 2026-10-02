//! Descriptive paired furnishing and connected construction in shifted windows.
//! Eight-block window spacing lets one 16x16 support mask straddle any chunk seam.
use super::*;

const MAX_WINDOWS: usize = 1024;
const MATERIAL_PLANK: u8 = 1;
const MATERIAL_FENCE: u8 = 2;
const MATERIAL_STAIR: u8 = 4;
const MATERIAL_WALL: u8 = 8;
const MATERIAL_MASONRY: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Built {
    None,
    Material(u8),
    DoorLower,
    DoorUpper,
    BedFoot,
    BedHead,
}

fn wood(name: &str) -> bool {
    [
        "oak", "birch", "spruce", "jungle", "acacia", "dark_oak", "mangrove", "crimson", "warped",
    ]
    .contains(&name)
}

fn built(state: &BlockState) -> Built {
    let Some(name) = state.name.strip_prefix("minecraft:") else {
        return Built::None;
    };
    if name.strip_suffix("_planks").is_some_and(wood) && state.properties.is_empty() {
        return Built::Material(MATERIAL_PLANK);
    }
    if name.strip_suffix("_fence").is_some_and(wood)
        && schema(
            state,
            &[
                ("east", BOOL),
                ("north", BOOL),
                ("south", BOOL),
                ("waterlogged", BOOL),
                ("west", BOOL),
            ],
        )
    {
        return Built::Material(MATERIAL_FENCE);
    }
    let direction = &["north", "south", "east", "west"];
    if name.strip_suffix("_stairs").is_some_and(wood)
        && schema(
            state,
            &[
                ("facing", direction),
                ("half", &["top", "bottom"]),
                (
                    "shape",
                    &[
                        "straight",
                        "inner_left",
                        "inner_right",
                        "outer_left",
                        "outer_right",
                    ],
                ),
                ("waterlogged", BOOL),
            ],
        )
    {
        return Built::Material(MATERIAL_STAIR);
    }
    if [
        "cobblestone_wall",
        "mossy_cobblestone_wall",
        "stone_brick_wall",
    ]
    .contains(&name)
        && schema(
            state,
            &[
                ("east", &["none", "low", "tall"]),
                ("north", &["none", "low", "tall"]),
                ("south", &["none", "low", "tall"]),
                ("up", BOOL),
                ("waterlogged", BOOL),
                ("west", &["none", "low", "tall"]),
            ],
        )
    {
        return Built::Material(MATERIAL_WALL);
    }
    if family(role(state)) != 0 {
        return Built::Material(MATERIAL_MASONRY);
    }
    if name.strip_suffix("_door").is_some_and(wood)
        && schema(
            state,
            &[
                ("facing", direction),
                ("half", &["lower", "upper"]),
                ("hinge", &["left", "right"]),
                ("open", BOOL),
                ("powered", BOOL),
            ],
        )
    {
        return if state.properties.get("half").is_some_and(|v| v == "lower") {
            Built::DoorLower
        } else {
            Built::DoorUpper
        };
    }
    let colors = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    if name
        .strip_suffix("_bed")
        .is_some_and(|color| colors.contains(&color))
        && schema(
            state,
            &[
                ("facing", direction),
                ("occupied", BOOL),
                ("part", &["head", "foot"]),
            ],
        )
    {
        return if state.properties.get("part").is_some_and(|v| v == "foot") {
            Built::BedFoot
        } else {
            Built::BedHead
        };
    }
    Built::None
}

#[derive(Clone, Copy, Default)]
struct Cell<'a> {
    representative: Option<Observation<'a>>,
    material_witness: Option<Observation<'a>>,
    materials: u8,
    lower: Option<Observation<'a>>,
    upper: Option<Observation<'a>>,
    foot: Option<Observation<'a>>,
    head: Option<Observation<'a>>,
    water_y: Option<i32>,
}
impl<'a> Cell<'a> {
    fn witness(self, block: Observation<'a>) -> Witness<'a> {
        Witness {
            block,
            water_above: self.water_y.is_some_and(|y| y > block.position[1]),
        }
    }
}

fn paired_door(lower: Observation<'_>, upper: Observation<'_>) -> bool {
    lower.position[0] == upper.position[0]
        && lower.position[2] == upper.position[2]
        && lower.position[1] + 1 == upper.position[1]
        && lower.state.name == upper.state.name
        && lower
            .state
            .properties
            .iter()
            .filter(|(key, _)| key.as_str() != "half")
            .eq(upper
                .state
                .properties
                .iter()
                .filter(|(key, _)| key.as_str() != "half"))
}

fn facing(state: &BlockState) -> [i32; 2] {
    match state.properties.get("facing").map(String::as_str) {
        Some("north") => [0, -1],
        Some("south") => [0, 1],
        Some("east") => [1, 0],
        Some("west") => [-1, 0],
        _ => [0, 0], // built() has already validated this property.
    }
}

fn index_at(origin: [i32; 2], position: [i32; 2]) -> Option<usize> {
    let x = i64::from(position[0]) - i64::from(origin[0]);
    let z = i64::from(position[1]) - i64::from(origin[1]);
    ((0..16).contains(&x) && (0..16).contains(&z)).then_some((z * 16 + x) as usize)
}

#[derive(Clone, Copy)]
struct Pair<'a> {
    anchor: Witness<'a>,
    corroboration: Witness<'a>,
}

#[derive(Clone, Copy)]
struct Context {
    origin: [i32; 2],
    first: [i32; 2],
    last: [i32; 2],
    source: Source,
}

fn pair_at<'a>(cells: &[Cell<'a>; TILE_CELLS], origin: [i32; 2], index: usize) -> Option<Pair<'a>> {
    let cell = cells[index];
    if let (Some(lower), Some(upper)) = (cell.lower, cell.upper) {
        if paired_door(lower, upper) {
            return Some(Pair {
                anchor: cell.witness(lower),
                corroboration: cell.witness(upper),
            });
        }
    }
    let foot = cell.foot?;
    let [dx, dz] = facing(foot.state);
    let head_x = foot.position[0].checked_add(dx)?;
    let head_z = foot.position[2].checked_add(dz)?;
    let head_index = index_at(origin, [head_x, head_z])?;
    let head_cell = cells[head_index];
    let head = head_cell.head?;
    if head.position != [head_x, foot.position[1], head_z]
        || foot.state.name != head.state.name
        || foot.state.properties.get("facing") != head.state.properties.get("facing")
        || foot.state.properties.get("occupied") != head.state.properties.get("occupied")
    {
        return None;
    }
    Some(Pair {
        anchor: cell.witness(foot),
        corroboration: head_cell.witness(head),
    })
}

fn canonical(origin: [i32; 2], anchor: [i32; 3], first: [i32; 2], last: [i32; 2]) -> bool {
    [(0, 0), (2, 1)].into_iter().all(|(axis, horizontal)| {
        let preferred = (i64::from(anchor[axis]) - 4).div_euclid(8) * 8;
        i64::from(origin[horizontal])
            == preferred.clamp(i64::from(first[horizontal]), i64::from(last[horizontal]))
    })
}

fn include_bounds(bounds: &mut Support, block: Observation<'_>) {
    for (axis, coordinate) in block.position.into_iter().enumerate() {
        bounds.minimum[axis] = bounds.minimum[axis].min(coordinate);
        bounds.maximum[axis] = bounds.maximum[axis].max(coordinate);
    }
}

fn component<'a>(
    cells: &[Cell<'a>; TILE_CELLS],
    context: Context,
    indices: &[usize],
    mask: [u64; 4],
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<Option<Target<'a>>, Error> {
    if indices.len() < 9 {
        return Ok(None);
    }
    let mut materials = 0_u8;
    let mut material_columns = 0_u16;
    let mut sectors = 0_u16;
    let (mut low, mut high) = ([16_usize; 2], [0_usize; 2]);
    let mut pairs = 0_u16;
    let mut selected: Option<Pair<'a>> = None;
    let mut support = Support {
        columns: indices.len() as u16,
        primary_columns: 0,
        secondary_columns: 0,
        secondary_sectors: 0,
        links: 0,
        minimum: [i32::MAX; 3],
        maximum: [i32::MIN; 3],
        origin: context.origin,
        footprint: mask,
    };
    let mut landmarks = [None; 4];
    for &index in indices {
        budget.tick(work)?;
        let cell = cells[index];
        let Some(representative) = cell.representative else {
            continue;
        };
        include_bounds(&mut support, representative);
        let x = index % 16;
        let z = index / 16;
        low[0] = low[0].min(x);
        low[1] = low[1].min(z);
        high[0] = high[0].max(x);
        high[1] = high[1].max(z);
        if cell.materials != 0 {
            material_columns += 1;
            materials |= cell.materials;
            sectors |= 1 << (x / 4 + z / 4 * 4);
            if let Some(material) = cell.material_witness {
                include_bounds(&mut support, material);
                landmark(&mut landmarks, cell.witness(material));
            }
        }
        let Some(pair) = pair_at(cells, context.origin, index) else {
            continue;
        };
        let corroboration = pair.corroboration.block.position;
        let Some(other) = index_at(context.origin, [corroboration[0], corroboration[2]]) else {
            continue;
        };
        if !included(&mask, other) {
            continue;
        }
        pairs += 1;
        if !canonical(
            context.origin,
            pair.anchor.block.position,
            context.first,
            context.last,
        ) {
            continue;
        }
        if selected.is_none_or(|old| pair.anchor.block.position < old.anchor.block.position) {
            selected = Some(pair);
        }
    }
    let Some(pair) = selected else {
        return Ok(None);
    };
    if material_columns < 8
        || materials == 0
        || sectors.count_ones() < 2
        || high[0] < low[0] + 3
        || high[1] < low[1] + 3
    {
        return Ok(None);
    }
    include_bounds(&mut support, pair.anchor.block);
    include_bounds(&mut support, pair.corroboration.block);
    support.primary_columns = material_columns;
    support.secondary_columns = pairs;
    support.secondary_sectors = sectors.count_ones() as u8;
    support.links = pairs;
    Ok(Some(Target {
        source: context.source,
        key: TargetKey {
            map: context.source.map,
            revision: RULE_REVISION,
            category: Category::DwellingLikeConstruction,
            tile: [
                pair.anchor.block.position[0].div_euclid(16),
                pair.anchor.block.position[2].div_euclid(16),
            ],
            anchor: pair.anchor.block.position,
        },
        confidence: if pairs >= 2 && material_columns >= 16 && materials.count_ones() >= 2 {
            Confidence::Corroborated
        } else {
            Confidence::Supported
        },
        support,
        anchor: pair.anchor,
        corroboration: pair.corroboration,
        landmarks,
    }))
}

pub(super) struct Input<'a, 'b> {
    pub window: &'b SurfaceWindow<'a>,
    pub source: Source,
    pub lower: [i64; 2],
    pub upper: [i64; 2],
    pub tops: &'b [i16],
    pub width: usize,
    pub limits: Limits,
}

pub(super) fn append_targets<'a>(
    input: Input<'a, '_>,
    targets: &mut Vec<Target<'a>>,
    stats: &mut Stats,
    budget: &mut Budget,
    work: &Work<'_>,
) -> Result<(), Error> {
    let Input {
        window,
        source,
        lower,
        upper,
        tops,
        width,
        limits,
    } = input;
    if upper[0] < lower[0] || upper[1] < lower[1] {
        return Ok(());
    }
    let start = [lower[0] * 16, lower[1] * 16];
    let end = [upper[0] * 16, upper[1] * 16];
    let spans = [(end[0] - start[0]) / 8 + 1, (end[1] - start[1]) / 8 + 1];
    let count = (spans[0] * spans[1]) as usize;
    if limits.max_construction_windows > MAX_WINDOWS || count > limits.max_construction_windows {
        return Err(Error::Limit("construction windows"));
    }
    let first = [start[0] as i32, start[1] as i32];
    let last = [end[0] as i32, end[1] as i32];
    for oz in (start[1]..=end[1]).step_by(8) {
        for ox in (start[0]..=end[0]).step_by(8) {
            budget.tick(work)?;
            let origin = [ox as i32, oz as i32];
            let mut cells = [Cell::default(); TILE_CELLS];
            for (index, cell) in cells.iter_mut().enumerate() {
                let x = origin[0] + (index % 16) as i32;
                let z = origin[1] + (index / 16) as i32;
                let tx = (i64::from(x) - start[0]) as usize;
                let tz = (i64::from(z) - start[1]) as usize;
                let top = tops[tz * width + tx];
                if top == i16::MIN {
                    continue;
                }
                for depth in 0..BAND {
                    let y = i32::from(top) - depth;
                    if y < MIN_Y {
                        break;
                    }
                    let block = probe(window, [x, y, z], i32::from(top), budget, work)?;
                    if role(block.state) == Role::Water && cell.water_y.is_none() {
                        cell.water_y = Some(y);
                    }
                    match built(block.state) {
                        Built::None => {}
                        Built::Material(kind) => {
                            cell.representative.get_or_insert(block);
                            cell.material_witness.get_or_insert(block);
                            cell.materials |= kind;
                        }
                        Built::DoorLower => {
                            cell.representative.get_or_insert(block);
                            cell.lower.get_or_insert(block);
                        }
                        Built::DoorUpper => {
                            cell.representative.get_or_insert(block);
                            cell.upper.get_or_insert(block);
                        }
                        Built::BedFoot => {
                            cell.representative.get_or_insert(block);
                            cell.foot.get_or_insert(block);
                        }
                        Built::BedHead => {
                            cell.representative.get_or_insert(block);
                            cell.head.get_or_insert(block);
                        }
                    }
                }
            }
            stats.construction_windows += 1;
            let mut visited = [0_u64; 4];
            for seed in 0..TILE_CELLS {
                budget.tick(work)?;
                if included(&visited, seed) || cells[seed].representative.is_none() {
                    continue;
                }
                let mut queue = [0_usize; TILE_CELLS];
                let (mut head, mut length) = (0, 1);
                let mut mask = [0_u64; 4];
                queue[0] = seed;
                insert(&mut visited, seed);
                insert(&mut mask, seed);
                while head < length {
                    budget.tick(work)?;
                    let index = queue[head];
                    head += 1;
                    let y = cells[index]
                        .representative
                        .map(|block| block.position[1])
                        .unwrap_or(MIN_Y);
                    for other in neighbors(index).into_iter().flatten() {
                        if included(&visited, other)
                            || cells[other]
                                .representative
                                .is_none_or(|block| (block.position[1] - y).abs() > 4)
                        {
                            continue;
                        }
                        insert(&mut visited, other);
                        insert(&mut mask, other);
                        queue[length] = other;
                        length += 1;
                    }
                }
                let context = Context {
                    origin,
                    first,
                    last,
                    source,
                };
                let Some(target) =
                    component(&cells, context, &queue[..length], mask, budget, work)?
                else {
                    continue;
                };
                if targets.len() >= limits.max_targets {
                    return Err(Error::Limit("targets"));
                }
                stats.qualifying_components += 1;
                targets.push(target);
            }
        }
    }
    Ok(())
}
