//! Original, bounded five-style surface village graph using the source-grounded
//! authored kit. This is an Ilium homage, not Java's jigsaw RNG or NBT templates.
//! Each connected piece is prepared atomically in global coordinates before its
//! complete writes (including air) are projected into any camera/chunk window.
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
    surface_structures::{self, Habitat, PlacementError, Prepared},
    terrain_fields::TerrainFields,
    village_kit::{
        self, FarmForm, HomeForm, MarkerKind, PieceKind, Port, Profession, RoadShape, VillageStyle,
    },
};
use std::collections::BTreeMap;

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
pub struct VillagePlacement {
    pub style: VillageStyle,
    pub center: [i32; 3],
    pub writes: BTreeMap<[i32; 3], VillageWrite>,
    pub markers: Vec<VillageMarker>,
    pub piece_sources: Vec<String>,
    pub rejected_pieces: usize,
}
#[derive(Clone, Copy)]
struct GlobalPort {
    position: [i32; 3],
    front: [i32; 2],
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
fn port(anchor: [i32; 3], turns: u8, port: Port) -> Option<GlobalPort> {
    let local = rotate(port.position, turns);
    Some(GlobalPort {
        position: checked_add3(anchor, local)?,
        front: {
            let vector = rotate([port.front[0], port.front[1], 0], turns);
            [vector[0], vector[1]]
        },
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
fn grounds_ok(
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    center: [i32; 3],
    position: [i32; 3],
) -> bool {
    let sample = fields.sample(position[0], position[1], settings.rivers);
    let ground = i32::from(sample.height);
    sample
        .water_level
        .is_none_or(|water| water <= sample.height)
        && (ground - center[2]).abs() <= 2
}
fn try_piece(
    village: &mut VillagePlacement,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    piece: &village_kit::Piece<BlockState>,
    anchor: [i32; 3],
    turns: u8,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<Vec<GlobalPort>>> {
    if cancelled() {
        return Err(AssetError::Cancelled);
    }
    let prepared = Prepared::prepare(
        &piece.template,
        anchor,
        turns,
        rotated_state,
        |position| {
            if village.writes.contains_key(&position) {
                return Habitat::Protected;
            }
            if position[2] <= anchor[2] + 1
                && !grounds_ok(fields, settings, village.center, position)
            {
                return Habitat::Protected;
            }
            Habitat::Replaceable
        },
        cancelled,
    );
    let prepared = match prepared {
        Ok(value) => value,
        Err(PlacementError::Cancelled) => return Err(AssetError::Cancelled),
        Err(
            problem @ (PlacementError::InvalidState
            | PlacementError::InvalidSource
            | PlacementError::Budget),
        ) => return Err(error(problem)),
        Err(_) => {
            village.rejected_pieces += 1;
            return Ok(None);
        }
    };
    let mut global_ports = Vec::new();
    for local in &piece.ports {
        let Some(transformed) = port(anchor, turns, *local) else {
            return Ok(None);
        };
        global_ports.push(transformed);
    }
    let mut global_markers = Vec::new();
    for marker in &piece.markers {
        let local = rotate(marker.position, turns);
        let Some(position) = checked_add3(anchor, local) else {
            return Ok(None);
        };
        global_markers.push(VillageMarker {
            position,
            kind: marker.kind,
        });
    }
    let complete = prepared
        .project([i64::from(i32::MIN); 2], [i64::from(i32::MAX) + 1; 2])
        .map_err(error)?;
    for (position, value) in complete {
        village.writes.insert(
            position,
            VillageWrite {
                state: value.cloned(),
                piece_source: prepared.source().to_owned(),
                piece_anchor: prepared.anchor(),
            },
        );
    }
    village.piece_sources.push(prepared.source().to_owned());
    village.markers.extend(global_markers);
    Ok(Some(global_ports))
}
fn attach(
    village: &mut VillagePlacement,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    parent: GlobalPort,
    kind: PieceKind,
    seed: u64,
    cancelled: &impl Fn() -> bool,
) -> Result<Option<Vec<GlobalPort>>> {
    let piece = village_kit::build(village.style, kind, seed, state).map_err(error)?;
    let Some(target) = checked_add3(parent.position, [parent.front[0], parent.front[1], 0]) else {
        return Ok(None);
    };
    for turns in 0..4 {
        for candidate in &piece.ports {
            let front = rotate([candidate.front[0], candidate.front[1], 0], turns);
            if [front[0], front[1]] != [-parent.front[0], -parent.front[1]] {
                continue;
            }
            let offset = rotate(candidate.position, turns);
            let Some(anchor) = checked_sub3(target, offset) else {
                continue;
            };
            if let Some(ports) =
                try_piece(village, fields, settings, &piece, anchor, turns, cancelled)?
            {
                return Ok(Some(
                    ports
                        .into_iter()
                        .filter(|port| port.position != target)
                        .collect(),
                ));
            }
        }
    }
    Ok(None)
}
/// Build one whole village in global space. Candidate order and ports do not
/// depend on the render window. The caller clips `writes` only after completion.
pub fn assemble(
    style: VillageStyle,
    center: [i32; 3],
    seed: u64,
    fields: &TerrainFields,
    settings: &VoxelLandscapeSettings,
    cancelled: impl Fn() -> bool,
) -> Result<VillagePlacement> {
    if center[0].unsigned_abs() > (i32::MAX - 512) as u32
        || center[1].unsigned_abs() > (i32::MAX - 512) as u32
    {
        return Err(AssetError::InvalidMetadata(
            "village center outside safe domain".into(),
        ));
    }
    let mut village = VillagePlacement {
        style,
        center,
        writes: BTreeMap::new(),
        markers: Vec::new(),
        piece_sources: Vec::new(),
        rejected_pieces: 0,
    };
    let center_piece = village_kit::build(style, PieceKind::Center, seed, state).map_err(error)?;
    let anchor = [center[0] - 5, center[1] - 5, center[2]];
    let Some(center_ports) = try_piece(
        &mut village,
        fields,
        settings,
        &center_piece,
        anchor,
        0,
        &cancelled,
    )?
    else {
        return Ok(village);
    };
    for (arm_index, center_port) in center_ports.into_iter().enumerate() {
        if cancelled() {
            return Err(AssetError::Cancelled);
        }
        let Some(road_ports) = attach(
            &mut village,
            fields,
            settings,
            center_port,
            PieceKind::Road(RoadShape::Cross),
            seed ^ arm_index as u64,
            &cancelled,
        )?
        else {
            continue;
        };
        for (branch_index, road_port) in road_ports.into_iter().enumerate() {
            let choice = hash2(
                seed ^ 0x0076_696c_6c61_6765,
                arm_index as i64,
                branch_index as i64,
            );
            let kind = match (arm_index + branch_index) % 5 {
                0 => PieceKind::Home(HomeForm::ALL[(choice as usize) % HomeForm::ALL.len()]),
                1 => {
                    PieceKind::Workplace(Profession::ALL[(choice as usize) % Profession::ALL.len()])
                }
                2 => PieceKind::Farm(FarmForm::ALL[(choice as usize) % FarmForm::ALL.len()]),
                3 => PieceKind::Pen,
                _ => PieceKind::Home(HomeForm::ALL[((choice >> 8) as usize) % HomeForm::ALL.len()]),
            };
            let _ = attach(
                &mut village,
                fields,
                settings,
                road_port,
                kind,
                choice,
                &cancelled,
            )?;
        }
    }
    Ok(village)
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
    if settings.structures_percent == 0 {
        return Ok(None);
    }
    let seed = u64::from(settings.seed);
    let gate = hash2(
        seed ^ 0x0076_696c_6c61_6765,
        i64::from(grid[0]),
        i64::from(grid[1]),
    );
    if gate % 100 >= (settings.structures_percent.min(200) as u64) * 15 / 100 {
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
    for attempt in 0..16 {
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
        if village.piece_sources.len() > 1 {
            return Ok(Some(village));
        }
    }
    Ok(None)
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
            let again = assemble(style, center, 7, &fields, &settings, || false).unwrap();
            assert_eq!(village.writes.len(), again.writes.len());
        }
    }
}
