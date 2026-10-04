//! Worker-only saved-cell admission for model binding. The position list retains
//! the original decoded map; states are borrowed from its exact palettes rather
//! than cloned into another raw-world catalog. Missing cells never become air.
use super::{surface, tours::PreparedMap};
use std::sync::Arc;

const MAX_CELLS: usize = 1_000_000;
const MAX_BYTES: usize = 16 << 20;
const MAX_WORK: usize = 16_000_000;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: u8,
    pub cells: usize,
    pub owned_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: 24,
            cells: MAX_CELLS,
            owned_bytes: MAX_BYTES,
            work_units: MAX_WORK,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid saved render-cell bounds or limits")]
    Invalid,
    #[error("saved render volume contains only exact air states")]
    Empty,
    #[error("saved render-cell resource limit: {0}")]
    Limit(&'static str),
    #[error(transparent)]
    Surface(#[from] surface::Error),
}

pub struct RenderCells {
    pub map: Arc<PreparedMap>,
    pub core: surface::Bounds,
    /// Inclusive saved CELL heights, not geometry/visible-surface certification.
    pub heights: [i32; 2],
    pub positions: Vec<[i32; 3]>,
    pub work_used: usize,
    pub storage_charge: usize,
}

pub fn prepare(
    map: Arc<PreparedMap>,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<RenderCells, Error> {
    let mut work = surface::Work::new(limits.work_units, cancelled);
    work.checkpoint()?;
    if limits.depth > 64
        || !(1..=MAX_CELLS).contains(&limits.cells)
        || !(std::mem::size_of::<RenderCells>()..=MAX_BYTES).contains(&limits.owned_bytes)
        || !(1..=MAX_WORK).contains(&limits.work_units)
    {
        return Err(Error::Invalid);
    }
    let chunks = &map.loaded().chunks;
    if chunks.is_empty() {
        return Err(Error::Invalid);
    }
    if chunks.len() > 128 {
        return Err(Error::Limit("input chunks"));
    }
    let mut core = surface::Bounds {
        minimum: [i32::MAX; 2],
        maximum: [i32::MIN; 2],
    };
    for position in chunks.keys() {
        for (axis, coordinate) in position.iter().enumerate() {
            let origin = coordinate.checked_mul(16).ok_or(Error::Invalid)?;
            let maximum = origin.checked_add(15).ok_or(Error::Invalid)?;
            // The binding boundary uses an exclusive maximum.
            maximum.checked_add(1).ok_or(Error::Invalid)?;
            core.minimum[axis] = core.minimum[axis].min(origin);
            core.maximum[axis] = core.maximum[axis].max(maximum);
        }
    }
    for axis in 0..2 {
        if i64::from(core.maximum[axis]) - i64::from(core.minimum[axis]) + 1 > 256 {
            return Err(Error::Limit("render width"));
        }
    }
    prepare_region(map, core, 0, limits, &mut work)
}

/// Render only an explicit exact saved core. Eight decoded support blocks on
/// each side cover the native blend-radius-two quart lookup (up to seven blocks)
/// and immediate face-neighbor queries; missing support rejects the core.
/// A planner must independently keep every viewport inside a one-chunk inset
/// from this core so boundary mesh faces cannot enter the painted image.
pub fn prepare_core(
    map: Arc<PreparedMap>,
    core: surface::Bounds,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<RenderCells, Error> {
    let mut work = surface::Work::new(limits.work_units, cancelled);
    work.checkpoint()?;
    if limits.depth > 64
        || !(1..=MAX_CELLS).contains(&limits.cells)
        || !(std::mem::size_of::<RenderCells>()..=MAX_BYTES).contains(&limits.owned_bytes)
        || !(1..=MAX_WORK).contains(&limits.work_units)
    {
        return Err(Error::Invalid);
    }
    if map.loaded().chunks.is_empty() || map.loaded().chunks.len() > 128 {
        return Err(Error::Invalid);
    }
    prepare_region(map, core, 8, limits, &mut work)
}

fn prepare_region(
    map: Arc<PreparedMap>,
    core: surface::Bounds,
    halo: u16,
    limits: Limits,
    work: &mut surface::Work<'_>,
) -> Result<RenderCells, Error> {
    for axis in 0..2 {
        if i64::from(core.maximum[axis]) - i64::from(core.minimum[axis]) + 1 > 256 {
            return Err(Error::Limit("render width"));
        }
    }
    let chunks = &map.loaded().chunks;
    let (heights, positions, storage_charge) = {
        let view = surface::SurfaceWindow::overworld_refs(
            core,
            halo,
            chunks.values().map(Arc::as_ref),
            surface::Limits {
                max_chunks: 128,
                max_columns: 32768,
                max_owned_bytes: 16384,
            },
            work,
        )?;
        let heights = view.surface_band(limits.depth, work)?.ok_or(Error::Empty)?;
        let mut positions = Vec::new();
        view.visit_band(heights, work, |cell| {
            if positions.len() >= limits.cells {
                return Err(surface::Error::Limit("render cell positions"));
            }
            if positions.len() == positions.capacity() {
                let grow = 4096.min(limits.cells - positions.len());
                let capacity = positions
                    .capacity()
                    .checked_add(grow)
                    .ok_or(surface::Error::Limit("render position capacity"))?;
                if storage_bytes(capacity)? > limits.owned_bytes {
                    return Err(surface::Error::Limit("render position bytes"));
                }
                positions
                    .try_reserve_exact(grow)
                    .map_err(|_| surface::Error::Limit("render position allocation"))?;
                if storage_bytes(positions.capacity())? > limits.owned_bytes {
                    return Err(surface::Error::Limit("render position capacity"));
                }
            }
            positions.push(cell.position);
            Ok(())
        })?;
        work.checkpoint()?;
        let charge = storage_bytes(positions.capacity())?;
        (heights, positions, charge)
    };
    Ok(RenderCells {
        map,
        core,
        heights,
        positions,
        work_used: work.used(),
        storage_charge,
    })
}

/// Logical retained-position charge; decoded map and temporary borrowed index
/// have separate admission budgets. This is not an allocator/RSS measurement.
fn storage_bytes(capacity: usize) -> Result<usize, surface::Error> {
    capacity
        .checked_mul(std::mem::size_of::<[i32; 3]>())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<RenderCells>()))
        .ok_or(surface::Error::Limit("render position bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::{
        chunk, evidence, loader,
        nbt::{Compound, Document, Tag, Text},
        tours,
    };
    fn fields(values: Vec<(&str, Tag)>) -> Compound {
        values
            .into_iter()
            .map(|(key, value)| (Text::from(key), value))
            .collect()
    }
    fn map(positions: &[[i32; 2]]) -> Arc<PreparedMap> {
        let mut loaded = loader::LoadedWindow::default();
        for &position in positions {
            let sections = (-4..=19)
                .map(|y| {
                    Tag::Compound(fields(vec![
                        ("Y", Tag::Byte(y)),
                        (
                            "block_states",
                            Tag::Compound(fields(vec![(
                                "palette",
                                Tag::List {
                                    kind: 10,
                                    values: vec![Tag::Compound(fields(vec![(
                                        "Name",
                                        Tag::String("minecraft:stone".into()),
                                    )]))],
                                },
                            )])),
                        ),
                    ]))
                })
                .collect();
            let decoded = chunk::decode(
                &Document {
                    name: "Synthetic render-cell admission".into(),
                    root: fields(vec![
                        ("DataVersion", Tag::Int(3218)),
                        ("xPos", Tag::Int(position[0])),
                        ("zPos", Tag::Int(position[1])),
                        ("Status", Tag::String("full".into())),
                        (
                            "sections",
                            Tag::List {
                                kind: 10,
                                values: sections,
                            },
                        ),
                    ]),
                },
                position,
                chunk::Limits::default(),
                &|| false,
            )
            .unwrap();
            loaded.chunks.insert(position, Arc::new(decoded));
            loaded.coverage.chunks.insert(position);
        }
        Arc::new(
            PreparedMap::new(
                evidence::Source {
                    map: evidence::MapId([1; 16]),
                    generation: 1,
                },
                0,
                Arc::new(loaded),
                Vec::new(),
                &mut tours::Budget::new(u64::MAX, &|| false),
            )
            .unwrap(),
        )
    }

    #[test]
    fn retains_exact_map_and_cell_positions_without_cloning_palettes() {
        let map = map(&[[-1, 2]]);
        let input = prepare(Arc::clone(&map), Limits::default(), &|| false).unwrap();
        assert!(Arc::ptr_eq(&input.map, &map));
        assert_eq!(Arc::strong_count(&map), 2);
        assert_eq!(
            input.core,
            surface::Bounds {
                minimum: [-16, 32],
                maximum: [-1, 47]
            }
        );
        assert_eq!(input.heights, [295, 319]);
        assert_eq!(input.positions.len(), 6400);
        assert_eq!(input.positions[0], [-16, 319, 32]);
        assert_eq!(*input.positions.last().unwrap(), [-1, 295, 47]);
        for &position in &input.positions {
            assert!(std::ptr::eq(
                input.map.state(position).unwrap(),
                map.state(position).unwrap()
            ));
        }
        assert!(input.storage_charge >= input.positions.capacity() * 12);
        assert!(input.storage_charge <= Limits::default().owned_bytes);
    }

    #[test]
    fn explicit_core_uses_real_saved_halo_without_rendering_halo_cells() {
        let positions = (-1..=1)
            .flat_map(|z| (-1..=1).map(move |x| [x, z]))
            .collect::<Vec<_>>();
        let map = map(&positions);
        let core = surface::Bounds {
            minimum: [0, 0],
            maximum: [15, 15],
        };
        let cells = prepare_core(Arc::clone(&map), core, Limits::default(), &|| false).unwrap();
        assert_eq!(cells.core, core);
        assert_eq!(cells.positions.len(), 6400);
        assert!(cells
            .positions
            .iter()
            .all(|position| core.contains([position[0], position[2]])));
        assert!(Arc::ptr_eq(&cells.map, &map));
        assert!(matches!(
            prepare_core(
                map,
                surface::Bounds {
                    minimum: [16, 0],
                    maximum: [31, 15]
                },
                Limits::default(),
                &|| false
            ),
            Err(Error::Surface(surface::Error::Unqualified {
                reason: surface::Rejection::MissingChunk,
                ..
            }))
        ));
    }

    #[test]
    fn work_cells_storage_and_cancellation_reject_the_whole_output() {
        let map = map(&[[0, 0]]);
        for limits in [
            Limits {
                cells: 32,
                ..Limits::default()
            },
            Limits {
                owned_bytes: 128,
                ..Limits::default()
            },
            Limits {
                work_units: 10,
                ..Limits::default()
            },
        ] {
            assert!(prepare(Arc::clone(&map), limits, &|| false).is_err());
        }
        assert!(matches!(
            prepare(Arc::clone(&map), Limits::default(), &|| true),
            Err(Error::Surface(surface::Error::Cancelled))
        ));
        assert_eq!(Arc::strong_count(&map), 1);
    }

    #[test]
    fn nonrectangular_saved_coverage_cannot_fill_a_missing_chunk() {
        let map = map(&[[0, 0], [2, 0]]);
        assert!(matches!(
            prepare(map, Limits::default(), &|| false),
            Err(Error::Surface(surface::Error::Unqualified { .. }))
        ));
    }
}
