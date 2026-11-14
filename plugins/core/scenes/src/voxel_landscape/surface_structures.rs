//! Complete surface-template preparation before chunk projection.
//! Template decoding, jigsaw assembly, terrain adaptation and processors belong
//! to callers. This layer never substitutes a clipped fragment for a template.
use std::collections::BTreeMap;

pub const MAX_CELLS: usize = 65_536;
pub const MAX_OFFSET: i32 = 256;

#[derive(Clone, Debug)]
pub struct TemplateCell<S> {
    /// Local scene coordinates: X/Y horizontal, Z elevation.
    pub position: [i32; 3],
    /// None explicitly carves air. Omitted positions leave terrain unchanged.
    pub state: Option<S>,
}

#[derive(Clone, Debug)]
pub struct Template<S> {
    pub source: String,
    pub cells: Vec<TemplateCell<S>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habitat {
    Replaceable,
    Protected,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementError {
    InvalidSource,
    Budget,
    Bounds,
    CoordinateOverflow,
    Duplicate([i32; 3]),
    Protected([i32; 3]),
    Unknown([i32; 3]),
    InvalidState,
    InvalidWindow,
    Cancelled,
}

/// A whole validated template. Generation retains its global owner key;
/// projection does not reevaluate habitat or change ownership at chunk seams.
pub struct Prepared<S> {
    source: String,
    anchor: [i32; 3],
    rotation: u8,
    cells: BTreeMap<[i32; 3], Option<S>>,
}

fn rotate_position([x, y, z]: [i32; 3], turns: u8) -> [i32; 3] {
    // Inputs are bounded before rotation, so negation cannot overflow.
    match turns {
        1 => [-y, x, z],
        2 => [-x, -y, z],
        3 => [y, -x, z],
        _ => [x, y, z],
    }
}

impl<S> Prepared<S> {
    /// Both callbacks must observe one stable generation snapshot. Unknown
    /// habitat and protected cells reject the entire template, including air.
    /// Rotation preserves exact state identity through the caller's adapter.
    pub fn prepare(
        template: &Template<S>,
        anchor: [i32; 3],
        quarter_turns: u8,
        rotate_state: impl Fn(&S, u8) -> Result<S, PlacementError>,
        habitat: impl Fn([i32; 3]) -> Habitat,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, PlacementError> {
        if template.source.is_empty()
            || template.source.len() > 1024
            || template.source.chars().any(char::is_control)
        {
            return Err(PlacementError::InvalidSource);
        }
        if template.cells.is_empty() || template.cells.len() > MAX_CELLS {
            return Err(PlacementError::Budget);
        }
        let rotation = quarter_turns % 4;
        let mut cells = BTreeMap::new();
        for cell in &template.cells {
            if cancelled() {
                return Err(PlacementError::Cancelled);
            }
            if cell
                .position
                .iter()
                .any(|v| v.unsigned_abs() > MAX_OFFSET as u32)
            {
                return Err(PlacementError::Bounds);
            }
            let local = rotate_position(cell.position, rotation);
            let mut global = anchor;
            for axis in 0..3 {
                global[axis] = global[axis]
                    .checked_add(local[axis])
                    .ok_or(PlacementError::CoordinateOverflow)?;
            }
            if cells.contains_key(&global) {
                return Err(PlacementError::Duplicate(global));
            }
            match habitat(global) {
                Habitat::Replaceable => {}
                Habitat::Protected => return Err(PlacementError::Protected(global)),
                Habitat::Unknown => return Err(PlacementError::Unknown(global)),
            }
            let state = cell
                .state
                .as_ref()
                .map(|s| rotate_state(s, rotation))
                .transpose()?;
            cells.insert(global, state);
        }
        if cancelled() {
            return Err(PlacementError::Cancelled);
        }
        Ok(Self {
            source: template.source.clone(),
            anchor,
            rotation,
            cells,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn anchor(&self) -> [i32; 3] {
        self.anchor
    }
    pub fn rotation(&self) -> u8 {
        self.rotation
    }

    pub fn cells(&self) -> impl Iterator<Item = ([i32; 3], Option<&S>)> {
        self.cells.iter().map(|(p, state)| (*p, state.as_ref()))
    }

    /// Inclusive minimum and exclusive maximum, with no elevation clipping.
    /// Wider bounds keep i32::MAX projectable through its exclusive MAX + 1
    /// window edge. Stored block positions retain their i32 world identity.
    /// Air writes remain distinguishable from absent template positions.
    pub fn project(
        &self,
        minimum: [i64; 2],
        maximum: [i64; 2],
    ) -> Result<impl Iterator<Item = ([i32; 3], Option<&S>)>, PlacementError> {
        if minimum.iter().zip(maximum).any(|(min, max)| *min >= max) {
            return Err(PlacementError::InvalidWindow);
        }
        Ok(self.cells().filter(move |(p, _)| {
            i64::from(p[0]) >= minimum[0]
                && i64::from(p[0]) < maximum[0]
                && i64::from(p[1]) >= minimum[1]
                && i64::from(p[1]) < maximum[1]
        }))
    }
}

fn direction(value: &str, turns: u8) -> Option<&'static str> {
    const CARDINAL: [&str; 4] = ["north", "east", "south", "west"];
    CARDINAL
        .iter()
        .position(|v| *v == value)
        .map(|i| CARDINAL[(i + usize::from(turns)) % 4])
}

fn rail_shape(value: &str, turns: u8) -> Option<String> {
    match value {
        "north_south" | "east_west" => Some(if turns & 1 == 0 {
            value.to_owned()
        } else if value == "north_south" {
            "east_west".into()
        } else {
            "north_south".into()
        }),
        _ => {
            if let Some(slope) = value.strip_prefix("ascending_") {
                return direction(slope, turns).map(|d| format!("ascending_{d}"));
            }
            const CORNERS: [&str; 4] = ["north_east", "south_east", "south_west", "north_west"];
            CORNERS
                .iter()
                .position(|v| *v == value)
                .map(|i| CORNERS[(i + usize::from(turns)) % 4].to_owned())
        }
    }
}

/// Rotate Java blockstate orientation in the same clockwise direction as scene
/// positions. Y is Java elevation; scene elevation is Z. No mirrors are applied,
/// so stair handedness, chest halves and door hinges retain their source values.
/// This transforms orientation, not registry defaults or legal state validation.
pub fn rotate_properties(
    properties: &BTreeMap<String, String>,
    quarter_turns: u8,
) -> Result<BTreeMap<String, String>, PlacementError> {
    let turns = quarter_turns % 4;
    let mut result = BTreeMap::new();
    for (key, value) in properties {
        let rotated_key = direction(key, turns).unwrap_or(key).to_owned();
        let rotated_value = match key.as_str() {
            "facing" => match value.as_str() {
                "up" | "down" => value.clone(),
                _ => direction(value, turns)
                    .ok_or(PlacementError::InvalidState)?
                    .into(),
            },
            "axis" => match value.as_str() {
                "x" if turns % 2 == 1 => "z".into(),
                "z" if turns % 2 == 1 => "x".into(),
                "x" | "y" | "z" => value.clone(),
                _ => return Err(PlacementError::InvalidState),
            },
            "rotation" => {
                let rotation: u8 = value.parse().map_err(|_| PlacementError::InvalidState)?;
                if rotation > 15 {
                    return Err(PlacementError::InvalidState);
                }
                ((rotation + 4 * turns) % 16).to_string()
            }
            "shape" => rail_shape(value, turns).unwrap_or_else(|| value.clone()),
            "orientation" => {
                let parts: Vec<_> = value.split('_').collect();
                if parts.len() != 2
                    || parts
                        .iter()
                        .any(|p| !matches!(*p, "up" | "down" | "north" | "south" | "east" | "west"))
                {
                    return Err(PlacementError::InvalidState);
                }
                parts
                    .iter()
                    .map(|p| direction(p, turns).unwrap_or(p))
                    .collect::<Vec<_>>()
                    .join("_")
            }
            _ => value.clone(),
        };
        result.insert(rotated_key, rotated_value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(position: [i32; 3], state: Option<u8>) -> TemplateCell<u8> {
        TemplateCell { position, state }
    }

    fn template(cells: Vec<TemplateCell<u8>>) -> Template<u8> {
        Template {
            source: "synthetic:test_house".into(),
            cells,
        }
    }

    fn prepare(
        template: &Template<u8>,
        anchor: [i32; 3],
        turns: u8,
    ) -> Result<Prepared<u8>, PlacementError> {
        Prepared::prepare(
            template,
            anchor,
            turns,
            |s, _| Ok(*s),
            |_| Habitat::Replaceable,
            || false,
        )
    }

    #[test]
    fn all_rotations_preserve_chunk_seams_and_explicit_air() {
        let source = template(vec![
            cell([0, 0, 0], Some(1)),
            cell([0, -1, 0], None),
            cell([0, -2, 1], Some(2)),
        ]);
        for turns in 0..4 {
            let prepared = prepare(&source, [-1, 0, 63], turns).unwrap();
            let all: BTreeMap<_, _> = prepared.cells().collect();
            let halves: BTreeMap<_, _> = prepared
                .project([-8, -8], [0, 8])
                .unwrap()
                .chain(prepared.project([0, -8], [8, 8]).unwrap())
                .collect();
            assert_eq!(all, halves);
            assert_eq!(all.len(), 3);
            assert_eq!(all.values().filter(|state| state.is_none()).count(), 1);
            assert_eq!(prepared.source(), "synthetic:test_house");
            assert_eq!(prepared.anchor(), [-1, 0, 63]);
            assert_eq!(prepared.rotation(), turns);
        }
    }

    #[test]
    fn signed_world_edges_include_their_exclusive_air_write_window() {
        let source = template(vec![cell([0; 3], None)]);
        for edge in [i32::MIN, i32::MAX] {
            let prepared = prepare(&source, [edge, edge, 63], 0).unwrap();
            let cells: Vec<_> = prepared
                .project([i64::from(edge); 2], [i64::from(edge) + 1; 2])
                .unwrap()
                .collect();
            assert_eq!(cells, vec![([edge, edge, 63], None)]);
        }
    }

    #[test]
    fn invalid_final_cell_rejects_complete_template() {
        let source = template(vec![cell([0; 3], Some(1)), cell([1, 0, 0], None)]);
        assert!(matches!(
            prepare(&source, [i32::MAX, 0, 0], 0),
            Err(PlacementError::CoordinateOverflow)
        ));
        for blocked in [Habitat::Unknown, Habitat::Protected] {
            assert!(Prepared::prepare(
                &source,
                [0; 3],
                0,
                |s, _| Ok(*s),
                |p| if p[0] == 1 {
                    blocked
                } else {
                    Habitat::Replaceable
                },
                || false
            )
            .is_err());
        }
        let duplicate = template(vec![cell([0; 3], Some(1)), cell([0; 3], None)]);
        assert!(matches!(
            prepare(&duplicate, [0; 3], 0),
            Err(PlacementError::Duplicate(_))
        ));
    }

    #[test]
    fn bounds_budget_cancel_and_window_are_explicit() {
        assert!(matches!(
            prepare(
                &template(vec![cell([MAX_OFFSET + 1, 0, 0], Some(1))]),
                [0; 3],
                0
            ),
            Err(PlacementError::Bounds)
        ));
        assert!(matches!(
            prepare(
                &template(vec![cell([0; 3], Some(1)); MAX_CELLS + 1]),
                [0; 3],
                0
            ),
            Err(PlacementError::Budget)
        ));
        let source = template(vec![cell([0; 3], Some(1))]);
        assert!(matches!(
            Prepared::prepare(
                &source,
                [0; 3],
                0,
                |s, _| Ok(*s),
                |_| Habitat::Replaceable,
                || true
            ),
            Err(PlacementError::Cancelled)
        ));
        assert!(matches!(
            prepare(&source, [0; 3], 0).unwrap().project([0; 2], [0; 2]),
            Err(PlacementError::InvalidWindow)
        ));
    }

    #[test]
    fn state_rotation_preserves_axes_connections_and_handedness() {
        let source = BTreeMap::from([
            ("facing".into(), "north".into()),
            ("axis".into(), "x".into()),
            ("north".into(), "tall".into()),
            ("east".into(), "none".into()),
            ("rotation".into(), "15".into()),
            ("orientation".into(), "up_north".into()),
        ]);
        let rotated = rotate_properties(&source, 1).unwrap();
        for (key, expected) in [
            ("facing", "east"),
            ("axis", "z"),
            ("east", "tall"),
            ("south", "none"),
            ("rotation", "3"),
            ("orientation", "up_east"),
        ] {
            assert_eq!(rotated[key], expected);
        }
        let mut repeated = source.clone();
        for _ in 0..4 {
            repeated = rotate_properties(&repeated, 1).unwrap();
        }
        assert_eq!(repeated, source);
        for (before, after) in [
            ("north_south", "east_west"),
            ("ascending_north", "ascending_east"),
            ("north_east", "south_east"),
            ("south_east", "south_west"),
            ("south_west", "north_west"),
            ("north_west", "north_east"),
            ("inner_left", "inner_left"),
        ] {
            assert_eq!(
                rotate_properties(&BTreeMap::from([("shape".into(), before.into())]), 1).unwrap()
                    ["shape"],
                after
            );
        }
        assert_eq!(
            rotate_properties(
                &BTreeMap::from([
                    ("axis".into(), "y".into()),
                    ("hinge".into(), "left".into()),
                    ("type".into(), "right".into())
                ]),
                1
            )
            .unwrap(),
            BTreeMap::from([
                ("axis".into(), "y".into()),
                ("hinge".into(), "left".into()),
                ("type".into(), "right".into())
            ])
        );
        assert!(rotate_properties(&BTreeMap::from([("rotation".into(), "16".into())]), 1).is_err());
    }
}
