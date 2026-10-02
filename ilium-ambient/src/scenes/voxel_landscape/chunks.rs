//! Bounded deterministic terrain cache. Chunk arithmetic uses Euclidean
//! division, so the origin and negative coordinates have identical boundaries.
use super::world::Column;
use std::collections::BTreeMap;
const EDGE: i32 = 16;
struct Chunk<T> {
    columns: Vec<T>,
    used: u64,
}
pub struct ColumnCache<T = Column> {
    capacity: usize,
    clock: u64,
    chunks: BTreeMap<[i32; 2], Chunk<T>>,
}
impl<T: Copy> ColumnCache<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, 256),
            clock: 0,
            chunks: BTreeMap::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
    pub fn get_with_chunk(
        &mut self,
        x: i32,
        y: i32,
        mut generate: impl FnMut(i32, i32) -> Option<Vec<T>>,
    ) -> Option<T> {
        self.clock = self.clock.saturating_add(1);
        let key = [x.div_euclid(EDGE), y.div_euclid(EDGE)];
        let index = (y.rem_euclid(EDGE) * EDGE + x.rem_euclid(EDGE)) as usize;
        if let Some(chunk) = self.chunks.get_mut(&key) {
            chunk.used = self.clock;
            return Some(chunk.columns[index]);
        }
        if self.chunks.len() >= self.capacity {
            let oldest = self
                .chunks
                .iter()
                .min_by_key(|(_, chunk)| chunk.used)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.chunks.remove(&oldest);
            }
        }
        let columns = generate(key[0], key[1])?;
        if columns.len() != (EDGE * EDGE) as usize {
            return None;
        }
        let result = columns[index];
        self.chunks.insert(
            key,
            Chunk {
                columns,
                used: self.clock,
            },
        );
        Some(result)
    }
}
impl ColumnCache<Column> {
    pub fn get(&mut self, x: i32, y: i32, mut sample: impl FnMut(i32, i32) -> Column) -> Column {
        self.get_with_chunk(x, y, |cx, cy| {
            let mut columns = Vec::with_capacity((EDGE * EDGE) as usize);
            for ly in 0..EDGE {
                for lx in 0..EDGE {
                    columns.push(sample(cx * EDGE + lx, cy * EDGE + ly));
                }
            }
            Some(columns)
        })
        .unwrap_or_else(|| sample(x, y))
    }
}
#[cfg(test)]
mod tests {
    use super::super::catalog::Material;
    use super::*;
    fn sample(x: i32, y: i32) -> Column {
        Column {
            height: x + y,
            surface: Material::Grass,
            soil: Material::Dirt,
            rock: Material::Stone,
            water_level: None,
            cave: None,
        }
    }
    #[test]
    fn adjacent_negative_coordinates_use_one_chunk_and_reuse_samples() {
        let mut cache = ColumnCache::new(2);
        let mut calls = 0;
        assert_eq!(
            cache
                .get(-1, -1, |x, y| {
                    calls += 1;
                    sample(x, y)
                })
                .height,
            -2
        );
        assert_eq!(calls, 256);
        assert_eq!(
            cache
                .get(-2, -1, |_, _| panic!("cache miss in same chunk"))
                .height,
            -3
        );
    }
    #[test]
    fn least_recently_used_chunk_is_evicted_with_bounded_capacity() {
        let mut cache = ColumnCache::new(2);
        cache.get(0, 0, sample);
        cache.get(16, 0, sample);
        cache.get(0, 0, sample);
        cache.get(32, 0, sample);
        assert_eq!(cache.len(), 2);
        cache.get(0, 0, |_, _| panic!("recent chunk was evicted"));
        let mut calls = 0;
        cache.get(16, 0, |x, y| {
            calls += 1;
            sample(x, y)
        });
        assert_eq!(calls, 256);
    }
}
