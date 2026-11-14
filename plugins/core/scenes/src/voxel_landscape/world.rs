//! A bounded surface window separates terrain sampling from feature overlays
//! and visibility. One column halo prevents false walls at window boundaries.
use super::catalog::Material;
use super::placement::Overlay;
use super::terrain;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub height: i32,
    pub surface: Material,
    pub soil: Material,
    pub rock: Material,
    pub water_level: Option<i32>,
    /// Inclusive air interval below the surface: a genuine cave opening.
    pub cave: Option<(i32, i32)>,
}
impl Column {
    pub fn from_terrain(column: terrain::Column) -> Self {
        let height = i32::from(column.height) - 1;
        let mut skin = column;
        skin.height = skin.surface_height;
        skin.cave = None;
        Self {
            height,
            surface: column
                .block(column.height - 1)
                .map(material)
                .unwrap_or(Material::Stone),
            soil: skin
                .block(skin.surface_height - 2)
                .map(material)
                .unwrap_or(Material::Dirt),
            rock: Material::Stone,
            water_level: column.water_level.map(|level| i32::from(level) - 1),
            cave: column
                .cave
                .map(|span| (i32::from(span.bottom), i32::from(span.top) - 1)),
        }
    }
    fn block(self, z: i32) -> Option<Material> {
        if self
            .cave
            .is_some_and(|(bottom, top)| z >= bottom && z <= top)
        {
            return None;
        }
        if z > self.height {
            return self
                .water_level
                .filter(|level| z <= *level)
                .map(|_| Material::Water);
        }
        if z == self.height {
            return Some(self.surface);
        }
        if z >= self.height - 3 {
            return Some(self.soil);
        }
        Some(self.rock)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VisibleBlock {
    pub position: [i32; 3],
    pub material: Material,
    /// Top, y+1 side, x+1 side (the camera faces from positive ground axes).
    pub faces: [bool; 3],
}

pub struct WorldWindow {
    minimum: [i32; 2],
    maximum: [i32; 2],
    columns: BTreeMap<[i32; 2], Column>,
    pub overlay: Overlay<Material>,
    terrain_columns: BTreeMap<[i32; 2], terrain::Column>,
}
impl WorldWindow {
    pub fn new(
        minimum: [i32; 2],
        maximum: [i32; 2],
        mut sample: impl FnMut(i32, i32) -> Column,
    ) -> Self {
        let mut columns = BTreeMap::new();
        for y in minimum[1] - 1..=maximum[1] {
            for x in minimum[0] - 1..=maximum[0] {
                columns.insert([x, y], sample(x, y));
            }
        }
        Self::from_columns(minimum, maximum, columns)
    }
    /// The caller supplies a one-column halo; missing samples stay empty.
    pub fn from_columns(
        minimum: [i32; 2],
        maximum: [i32; 2],
        columns: BTreeMap<[i32; 2], Column>,
    ) -> Self {
        Self {
            minimum,
            maximum,
            columns,
            terrain_columns: BTreeMap::new(),
            overlay: Overlay::new(
                [minimum[0] - 1, minimum[1] - 1],
                [maximum[0] + 1, maximum[1] + 1],
            ),
        }
    }
    /// Preserve kernel occupancy and geological layers without changing the
    /// fixture-friendly heightfield contract. Both coordinate axes are ground.
    pub fn attach_terrain(&mut self, x: i32, y: i32, column: terrain::Column) {
        self.terrain_columns.insert([x, y], column);
    }
    pub fn column(&self, x: i32, y: i32) -> Option<Column> {
        self.columns.get(&[x, y]).copied()
    }
    pub fn block(&self, position: [i32; 3]) -> Option<Material> {
        if let Some(material) = self.overlay.get(position) {
            return material;
        }
        let column = self.column(position[0], position[1])?;
        if let Some(raw) = self.terrain_columns.get(&[position[0], position[1]]) {
            let height = i16::try_from(position[2]).ok()?;
            let result = raw.block(height).map(material);
            // Skin and substrate are sampled at the ORIGINAL natural surface.
            // A cut exposes deeper geology; it must never acquire a grass cap.
            let depth = i32::from(raw.surface_height) - 1 - position[2];
            if position[2] > 0 && result.is_some_and(|material| material != Material::Water) {
                return result.map(|source| match depth {
                    0 => column.surface,
                    1..=3 => column.soil,
                    _ if column.rock != Material::Stone => column.rock,
                    _ => source,
                });
            }
            return result;
        }
        column.block(position[2])
    }
    fn visible(&self, position: [i32; 3], material: Material) -> Option<VisibleBlock> {
        let [x, y, z] = position;
        if x < self.minimum[0]
            || y < self.minimum[1]
            || x >= self.maximum[0]
            || y >= self.maximum[1]
        {
            return None;
        }
        let faces = [[x, y, z + 1], [x, y + 1, z], [x + 1, y, z]]
            .map(|neighbor| self.block(neighbor).is_none());
        faces
            .into_iter()
            .any(|visible| visible)
            .then_some(VisibleBlock {
                position,
                material,
                faces,
            })
    }
    pub fn visible_blocks(&self) -> Vec<VisibleBlock> {
        self.visible_blocks_checked(|| false).unwrap_or_default()
    }
    pub fn visible_blocks_checked(
        &self,
        is_cancelled: impl Fn() -> bool,
    ) -> Option<Vec<VisibleBlock>> {
        let mut blocks = BTreeMap::new();
        let mut carved_bottoms = BTreeMap::<[i32; 2], i32>::new();
        for ([x, y, z], material) in self.overlay.iter() {
            if is_cancelled() {
                return None;
            }
            if material.is_none() {
                carved_bottoms
                    .entry([x, y])
                    .and_modify(|bottom| *bottom = (*bottom).min(z))
                    .or_insert(z);
            }
        }
        for y in self.minimum[1]..self.maximum[1] {
            if is_cancelled() {
                return None;
            }
            for x in self.minimum[0]..self.maximum[0] {
                let Some(column) = self.column(x, y) else {
                    continue;
                };
                let neighbor_min = [[x + 1, y], [x, y + 1]]
                    .into_iter()
                    .filter_map(|[a, b]| self.column(a, b))
                    .map(|neighbor| neighbor.height)
                    .min()
                    .unwrap_or(column.height);
                // Exposed walls can extend below either neighbor's terrain
                // surface. Include all touching cave/overlay air intervals,
                // plus their floors, rather than clipping to this heightfield.
                let void_bottom = [[x, y], [x + 1, y], [x, y + 1]]
                    .into_iter()
                    .flat_map(|position| {
                        let cave = self
                            .columns
                            .get(&position)
                            .and_then(|column| column.cave)
                            .filter(|(bottom, top)| bottom <= top)
                            .map(|(bottom, _)| bottom);
                        [cave, carved_bottoms.get(&position).copied()]
                            .into_iter()
                            .flatten()
                    })
                    .min()
                    .unwrap_or(column.height);
                let lower = neighbor_min.min(column.height).min(void_bottom) - 1;
                let upper = column
                    .water_level
                    .unwrap_or(column.height)
                    .max(column.height);
                for z in lower..=upper {
                    let position = [x, y, z];
                    let Some(material) = self.block(position) else {
                        continue;
                    };
                    if let Some(block) = self.visible(position, material) {
                        blocks.insert(position, block);
                    }
                }
            }
        }
        for (position, material) in self.overlay.iter() {
            if is_cancelled() {
                return None;
            }
            let Some(material) = material else {
                continue;
            };
            if let Some(block) = self.visible(position, material) {
                blocks.insert(position, block);
            }
        }
        Some(blocks.into_values().collect())
    }
}

/// Kernel material conversion is centralized at the world boundary.
fn material(source: terrain::Material) -> Material {
    match source {
        terrain::Material::Bedrock => Material::Basalt,
        terrain::Material::Stone => Material::Stone,
        terrain::Material::Sandstone => Material::Sandstone,
        terrain::Material::Dirt => Material::Dirt,
        terrain::Material::Grass => Material::Grass,
        terrain::Material::Sand => Material::Sand,
        terrain::Material::Snow => Material::Snow,
        terrain::Material::Mud => Material::Mud,
        terrain::Material::Water => Material::Water,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flat(_: i32, _: i32) -> Column {
        Column {
            height: 0,
            surface: Material::Grass,
            soil: Material::Dirt,
            rock: Material::Stone,
            water_level: None,
            cave: None,
        }
    }
    #[test]
    fn flat_surface_contains_thousands_of_actual_blocks_without_interior_faces() {
        let world = WorldWindow::new([-32, -32], [32, 32], flat);
        let blocks = world.visible_blocks();
        assert_eq!(blocks.len(), 4096);
        assert!(blocks
            .iter()
            .all(|block| block.faces == [true, false, false] && block.material == Material::Grass));
    }
    #[test]
    fn neighboring_deep_cave_exposes_full_wall() {
        let world = WorldWindow::new([0, 0], [3, 2], |x, y| {
            let mut column = flat(x, y);
            if x == 1 {
                column.cave = Some((-8, -2));
            }
            column
        });
        assert!(world
            .visible_blocks()
            .iter()
            .any(|block| block.position == [0, 0, -5] && block.faces[2]));
    }
    #[test]
    fn deep_feature_shaft_exposes_wall_and_floor() {
        use super::super::placement::PlacementKey;
        let mut world = WorldWindow::new([0, 0], [3, 2], flat);
        world.overlay.place(
            [1, 0, 0],
            0,
            PlacementKey {
                priority: 2,
                x: 1,
                y: 0,
                feature: 1,
            },
            (-8..=0).map(|z| ([0, 0, z], None)),
        );
        let blocks = world.visible_blocks();
        assert!(blocks
            .iter()
            .any(|block| block.position == [0, 0, -5] && block.faces[2]));
        assert!(blocks
            .iter()
            .any(|block| block.position == [1, 0, -9] && block.faces[0]));
    }
    #[test]
    fn carved_air_is_removed_and_exposed_roof_wall_remains() {
        let world = WorldWindow::new([-2, -2], [2, 2], |x, y| {
            let mut column = flat(x, y);
            if x < 0 {
                column.height = 6;
                column.cave = Some((1, 4));
            }
            column
        });
        assert_eq!(world.block([-1, 0, 2]), None);
        assert!(world
            .visible_blocks()
            .iter()
            .any(|block| block.position == [-1, 0, 5] && block.faces[2]));
    }
}
