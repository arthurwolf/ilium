//! Original, connected tree geometry in scene coordinates (Z is elevation).
//! This kernel preserves branch axes for end-grain bindings. Habitat, source
//! configuration selection, decorations and asset resolution belong to callers.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogAxis {
    X,
    GroundY,
    Vertical,
}
impl LogAxis {
    /// Convert the scene's Z-up coordinates into Java blockstate coordinates.
    pub const fn java_value(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::GroundY => "z",
            Self::Vertical => "y",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeCell {
    Log(LogAxis),
    Leaf,
    Root,
}

/// Integer local positions prevent world-origin/camera-dependent seams.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeGeometry {
    cells: BTreeMap<[i16; 3], TreeCell>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    CoordinateLimit,
    CellLimit,
    InvalidCanopy,
}

impl TreeGeometry {
    pub const COORDINATE_LIMIT: i16 = 48;
    pub const CELL_LIMIT: usize = 16_384;

    pub fn cells(&self) -> impl Iterator<Item = ([i16; 3], TreeCell)> + '_ {
        self.cells.iter().map(|(position, cell)| (*position, *cell))
    }

    /// Rotate geometry and semantic bark/end-grain axes together. The shape
    /// remains local; absolute placement and habitat clipping are caller-owned.
    pub fn rotated(&self, quarter_turns: u8) -> Self {
        let turns = quarter_turns % 4;
        let cells = self
            .cells()
            .map(|(position, cell)| {
                let [x, y, z] = position;
                let position = match turns {
                    1 => [-y, x, z],
                    2 => [-x, -y, z],
                    3 => [y, -x, z],
                    _ => position,
                };
                let cell = match (turns % 2, cell) {
                    (1, TreeCell::Log(LogAxis::X)) => TreeCell::Log(LogAxis::GroundY),
                    (1, TreeCell::Log(LogAxis::GroundY)) => TreeCell::Log(LogAxis::X),
                    _ => cell,
                };
                (position, cell)
            })
            .collect();
        Self { cells }
    }

    fn valid(position: [i16; 3]) -> bool {
        position
            .iter()
            .all(|value| value.unsigned_abs() <= Self::COORDINATE_LIMIT as u16)
    }

    fn insert(
        cells: &mut BTreeMap<[i16; 3], TreeCell>,
        position: [i16; 3],
        cell: TreeCell,
    ) -> Result<(), GeometryError> {
        if !Self::valid(position) {
            return Err(GeometryError::CoordinateLimit);
        }
        if !cells.contains_key(&position) && cells.len() == Self::CELL_LIMIT {
            return Err(GeometryError::CellLimit);
        }
        // Logs replace leaves; intersecting branches preserve the earlier axis.
        match (cells.get(&position), cell) {
            (Some(TreeCell::Log(_)), _) => {}
            (Some(TreeCell::Root), TreeCell::Leaf) => {}
            _ => {
                cells.insert(position, cell);
            }
        }
        Ok(())
    }

    /// A six-connected integer path. Diagonal samples gain connecting steps
    /// rather than leaving visibly disconnected branch cubes. Endpoint bounds
    /// imply every interpolated step is safe. Mutations publish atomically.
    pub fn branch(&mut self, start: [i16; 3], end: [i16; 3]) -> Result<(), GeometryError> {
        if !Self::valid(start) || !Self::valid(end) {
            return Err(GeometryError::CoordinateLimit);
        }
        let delta = std::array::from_fn::<_, 3, _>(|axis| end[axis] - start[axis]);
        let steps = delta
            .iter()
            .map(|value| value.unsigned_abs())
            .max()
            .unwrap_or(0);
        let dominant = if delta[2].unsigned_abs() >= delta[0].unsigned_abs()
            && delta[2].unsigned_abs() >= delta[1].unsigned_abs()
        {
            LogAxis::Vertical
        } else if delta[0].unsigned_abs() >= delta[1].unsigned_abs() {
            LogAxis::X
        } else {
            LogAxis::GroundY
        };
        let mut cells = self.cells.clone();
        Self::insert(&mut cells, start, TreeCell::Log(dominant))?;
        let mut cursor = start;
        for step in 1..=i32::from(steps) {
            let target: [i16; 3] = std::array::from_fn(|axis| {
                start[axis] + (i32::from(delta[axis]) * step / i32::from(steps)) as i16
            });
            for axis in [2, 0, 1] {
                while cursor[axis] != target[axis] {
                    cursor[axis] += (target[axis] - cursor[axis]).signum();
                    let orientation = match axis {
                        0 => LogAxis::X,
                        1 => LogAxis::GroundY,
                        _ => LogAxis::Vertical,
                    };
                    Self::insert(&mut cells, cursor, TreeCell::Log(orientation))?;
                }
            }
        }
        self.cells = cells;
        Ok(())
    }

    /// Mangrove roots retain their own material role rather than becoming bark.
    pub fn root_branch(&mut self, start: [i16; 3], end: [i16; 3]) -> Result<(), GeometryError> {
        let mut branch = Self::default();
        branch.branch(start, end)?;
        let mut cells = self.cells.clone();
        for (position, _) in branch.cells() {
            Self::insert(&mut cells, position, TreeCell::Root)?;
        }
        self.cells = cells;
        Ok(())
    }

    /// Filled ellipsoid with declared integer radii. Integer cross-products
    /// keep membership stable between debug/release and large world origins.
    /// Radius zero is an intentional flat crown layer.
    pub fn canopy(&mut self, center: [i16; 3], radii: [i16; 3]) -> Result<(), GeometryError> {
        if radii.iter().any(|radius| !(0..=12).contains(radius)) {
            return Err(GeometryError::InvalidCanopy);
        }
        if !Self::valid(center)
            || (0..3).any(|axis| {
                i32::from(center[axis]).abs() + i32::from(radii[axis])
                    > i32::from(Self::COORDINATE_LIMIT)
            })
        {
            return Err(GeometryError::CoordinateLimit);
        }
        let squared: [i64; 3] = radii.map(|radius| i64::from(radius.max(1)).pow(2));
        let product = squared.iter().product::<i64>();
        let mut cells = self.cells.clone();
        for x in -radii[0]..=radii[0] {
            for y in -radii[1]..=radii[1] {
                for z in -radii[2]..=radii[2] {
                    let offsets = [x, y, z];
                    let distance = offsets
                        .iter()
                        .enumerate()
                        .map(|(axis, offset)| i64::from(*offset).pow(2) * (product / squared[axis]))
                        .sum::<i64>();
                    if distance <= product {
                        Self::insert(
                            &mut cells,
                            std::array::from_fn(|axis| center[axis] + offsets[axis]),
                            TreeCell::Leaf,
                        )?;
                    }
                }
            }
        }
        self.cells = cells;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, VecDeque};

    #[test]
    fn diagonal_branch_has_face_connected_wood_and_both_endpoints() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([-9, 3, 1], [7, -8, 17]).unwrap();
        let cells: HashSet<_> = geometry.cells().map(|(p, _)| p).collect();
        let mut seen = HashSet::from([[-9, 3, 1]]);
        let mut queue = VecDeque::from([[-9, 3, 1]]);
        while let Some(position) = queue.pop_front() {
            for axis in 0..3 {
                for direction in [-1, 1] {
                    let mut neighbor = position;
                    neighbor[axis] += direction;
                    if cells.contains(&neighbor) && seen.insert(neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        assert!(seen.contains(&[7, -8, 17]));
        assert_eq!(seen, cells);
    }

    #[test]
    fn horizontal_and_vertical_wood_keep_distinct_java_end_grain_axes() {
        for (end, expected) in [([5, 0, 0], "x"), ([0, 5, 0], "z"), ([0, 0, 5], "y")] {
            let mut geometry = TreeGeometry::default();
            geometry.branch([0; 3], end).unwrap();
            assert!(geometry.cells().all(
                |(_, cell)| matches!(cell, TreeCell::Log(axis) if axis.java_value() == expected)
            ));
        }
    }

    #[test]
    fn canopy_does_not_replace_a_connected_trunk() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0; 3], [0, 0, 8]).unwrap();
        geometry.canopy([0, 0, 6], [4, 3, 3]).unwrap();
        assert!((0..=8).all(|z| matches!(
            geometry.cells.get(&[0, 0, z]),
            Some(TreeCell::Log(LogAxis::Vertical))
        )));
        assert!(geometry.cells().any(|(_, cell)| cell == TreeCell::Leaf));
    }

    #[test]
    fn failed_geometry_keeps_previous_shape() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0; 3], [0, 0, 6]).unwrap();
        let before = geometry.clone();
        assert_eq!(
            geometry.branch([0; 3], [49, 0, 0]),
            Err(GeometryError::CoordinateLimit)
        );
        assert_eq!(
            geometry.canopy([47, 0, 0], [2, 1, 1]),
            Err(GeometryError::CoordinateLimit)
        );
        assert_eq!(
            geometry.canopy([0; 3], [-1, 1, 1]),
            Err(GeometryError::InvalidCanopy)
        );
        assert_eq!(geometry, before);
    }

    #[test]
    fn flat_crown_and_zero_length_branch_are_supported() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0, 0, 3], [0, 0, 3]).unwrap();
        geometry.canopy([0, 0, 3], [4, 3, 0]).unwrap();
        assert!(geometry.cells().all(|(position, _)| position[2] == 3));
        assert_eq!(
            geometry.cells.get(&[0, 0, 3]),
            Some(&TreeCell::Log(LogAxis::Vertical))
        );
    }

    #[test]
    fn placement_rotation_rotates_log_axis_with_the_fallen_trunk() {
        let mut geometry = TreeGeometry::default();
        geometry.branch([0; 3], [6, 0, 0]).unwrap();
        let turned = geometry.rotated(1);
        assert!(turned
            .cells()
            .all(|(position, cell)| position[0] == 0
                && matches!(cell, TreeCell::Log(LogAxis::GroundY))));
        assert_eq!(turned.rotated(3), geometry);
        assert_eq!(geometry.rotated(4), geometry);
    }

    #[test]
    fn cell_admission_failure_is_transactional() {
        let mut geometry = TreeGeometry::default();
        for x in -20..=20 {
            for y in -20..=20 {
                for z in 0..10 {
                    if geometry.cells.len() < TreeGeometry::CELL_LIMIT {
                        geometry.cells.insert([x, y, z], TreeCell::Leaf);
                    }
                }
            }
        }
        assert_eq!(geometry.cells.len(), TreeGeometry::CELL_LIMIT);
        let before = geometry.clone();
        assert_eq!(
            geometry.branch([48, 48, 48], [48, 48, 48]),
            Err(GeometryError::CellLimit)
        );
        assert_eq!(geometry, before);
    }

    #[test]
    fn canopy_cannot_replace_mangrove_roots() {
        let mut geometry = TreeGeometry::default();
        geometry.root_branch([0, 0, 3], [4, 0, 0]).unwrap();
        let roots: Vec<_> = geometry.cells().map(|(p, _)| p).collect();
        geometry.canopy([2, 0, 2], [4, 2, 2]).unwrap();
        assert!(roots
            .iter()
            .all(|p| geometry.cells.get(p) == Some(&TreeCell::Root)));
    }
}
