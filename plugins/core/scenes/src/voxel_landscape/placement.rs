//! Deterministic, chunk-clipped feature overlays. Every chunk evaluates the
//! same anchors in its halo; a total ownership order makes overlap independent
//! of which chunk, worker or recipe was evaluated first.
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlacementKey {
    pub priority: u8,
    pub x: i32,
    pub y: i32,
    pub feature: u16,
}

pub struct Overlay<M> {
    minimum: [i32; 2],
    maximum: [i32; 2],
    blocks: BTreeMap<[i32; 3], (PlacementKey, Option<M>)>,
}
impl<M: Copy> Overlay<M> {
    /// Inclusive minimum and exclusive maximum world coordinates.
    pub fn new(minimum: [i32; 2], maximum: [i32; 2]) -> Self {
        Self {
            minimum,
            maximum,
            blocks: BTreeMap::new(),
        }
    }
    /// `Some(None)` is an explicit air carve; `None` is unchanged terrain.
    pub fn get(&self, position: [i32; 3]) -> Option<Option<M>> {
        self.blocks.get(&position).map(|(_, material)| *material)
    }
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    pub fn place(
        &mut self,
        anchor: [i32; 3],
        rotation: u8,
        key: PlacementKey,
        blocks: impl IntoIterator<Item = ([i32; 3], Option<M>)>,
    ) {
        for (local, material) in blocks {
            let [x, y, z] = local;
            let (x, y) = match rotation % 4 {
                1 => (-y, x),
                2 => (-x, -y),
                3 => (y, -x),
                _ => (x, y),
            };
            let position = [
                anchor[0].saturating_add(x),
                anchor[1].saturating_add(y),
                anchor[2].saturating_add(z),
            ];
            if position[0] < self.minimum[0]
                || position[1] < self.minimum[1]
                || position[0] >= self.maximum[0]
                || position[1] >= self.maximum[1]
            {
                continue;
            }
            // Equal keys preserve operation order inside one recipe, so a
            // later doorway carve can remove an earlier solid wall.
            if self
                .blocks
                .get(&position)
                .is_some_and(|(owner, _)| *owner > key)
            {
                continue;
            }
            self.blocks.insert(position, (key, material));
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = ([i32; 3], Option<M>)> + '_ {
        self.blocks
            .iter()
            .map(|(position, (_, material))| (*position, *material))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_halves_preserve_rotated_geometry_and_air_carves() {
        let mut left = Overlay::new([-16, 0], [0, 16]);
        let mut right = Overlay::new([0, 0], [16, 16]);
        for overlay in [&mut left, &mut right] {
            overlay.place(
                [-1, 5, 7],
                1,
                PlacementKey {
                    priority: 2,
                    x: -1,
                    y: 5,
                    feature: 3,
                },
                [
                    ([0, 0, 0], Some(4u16)),
                    ([0, -1, 0], Some(5)),
                    ([0, -2, 0], None),
                ],
            );
        }
        assert_eq!(left.get([-1, 5, 7]), Some(Some(4)));
        assert_eq!(right.get([0, 5, 7]), Some(Some(5)));
        assert_eq!(right.get([1, 5, 7]), Some(None));
        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 2);
    }
    #[test]
    fn recipe_air_carve_overwrites_its_own_solid_wall() {
        let mut overlay = Overlay::new([0, 0], [16, 16]);
        overlay.place(
            [1, 1, 1],
            0,
            PlacementKey {
                priority: 2,
                x: 1,
                y: 1,
                feature: 4,
            },
            [([0, 0, 0], Some(4)), ([0, 0, 0], None)],
        );
        assert_eq!(overlay.get([1, 1, 1]), Some(None));
        assert_eq!(overlay.iter().count(), 1);
    }
}
