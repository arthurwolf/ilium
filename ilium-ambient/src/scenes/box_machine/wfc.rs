//! Wave function collapse over the tile library: seeded, minimum-entropy
//! first, restarting on contradiction.

use super::library::{Library, EDGES, EMPTY_TILE, MASK_COUNT};

const MAX_ATTEMPTS: usize = 50;

/// SplitMix64: tiny, fast, and identical on every platform.
pub(super) struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.0)
    }

    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }
}

/// The SplitMix64 finalizer, usable as a stateless hash.
pub(super) fn mix(value: u64) -> u64 {
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Neighbour offset across each edge: north, east, south, west.
const OFFSETS: [(i32, i32); EDGES] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

struct Grid<'a> {
    library: &'a Library,
    weights: &'a [f32],
    width: usize,
    height: usize,
    domains: Vec<u64>,
    counts: Vec<u32>,
    /// NaN marks an entropy that must be recomputed.
    entropy: Vec<f32>,
}

impl Grid<'_> {
    fn domain(&self, cell: usize) -> &[u64] {
        let words = self.library.words;
        &self.domains[cell * words..(cell + 1) * words]
    }

    fn neighbour(&self, cell: usize, edge: usize) -> Option<usize> {
        let (x, y) = ((cell % self.width) as i32, (cell / self.width) as i32);
        let (nx, ny) = (x + OFFSETS[edge].0, y + OFFSETS[edge].1);
        if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
            return None;
        }
        Some(ny as usize * self.width + nx as usize)
    }

    fn for_each_tile(&self, cell: usize, mut visit: impl FnMut(usize)) {
        for (word_index, &word) in self.domain(cell).iter().enumerate() {
            let mut bits = word;
            while bits != 0 {
                visit(word_index * 64 + bits.trailing_zeros() as usize);
                bits &= bits - 1;
            }
        }
    }

    /// Restrict `cell` to `allowed`. Returns whether it changed, or `None`
    /// on contradiction.
    fn restrict(&mut self, cell: usize, allowed: &[u64]) -> Option<bool> {
        let words = self.library.words;
        let mut changed = false;
        let mut count = 0;
        for (slot, allowed_word) in self.domains[cell * words..(cell + 1) * words]
            .iter_mut()
            .zip(allowed)
        {
            let narrowed = *slot & allowed_word;
            changed |= narrowed != *slot;
            *slot = narrowed;
            count += narrowed.count_ones();
        }
        if count == 0 {
            return None;
        }
        if changed {
            self.counts[cell] = count;
            self.entropy[cell] = f32::NAN;
        }
        Some(changed)
    }

    /// Propagate edge constraints from every cell on the stack.
    fn propagate(&mut self, stack: &mut Vec<usize>) -> bool {
        let words = self.library.words;
        let mut support = vec![0u64; words];
        while let Some(cell) = stack.pop() {
            let mut present = [0u8; EDGES];
            self.for_each_tile(cell, |tile| {
                for (edge, mask_bits) in present.iter_mut().enumerate() {
                    *mask_bits |= 1 << self.library.tiles[tile].edge_masks[edge];
                }
            });
            for (edge, present_masks) in present.into_iter().enumerate() {
                let Some(neighbour) = self.neighbour(cell, edge) else {
                    continue;
                };
                support.fill(0);
                let facing = (edge + 2) % EDGES;
                for mask in 0..MASK_COUNT {
                    if present_masks & (1 << mask) == 0 {
                        continue;
                    }
                    for (slot, set) in support
                        .iter_mut()
                        .zip(&self.library.edge_sets[facing][mask])
                    {
                        *slot |= set;
                    }
                }
                match self.restrict(neighbour, &support.clone()) {
                    None => return false,
                    Some(true) => stack.push(neighbour),
                    Some(false) => {}
                }
            }
        }
        true
    }

    fn cell_entropy(&mut self, cell: usize) -> f32 {
        if !self.entropy[cell].is_nan() {
            return self.entropy[cell];
        }
        let (mut total, mut weighted_log) = (0.0f32, 0.0f32);
        self.for_each_tile(cell, |tile| {
            let weight = self.weights[tile];
            total += weight;
            weighted_log += weight * weight.ln();
        });
        let value = total.ln() - weighted_log / total;
        self.entropy[cell] = value;
        value
    }
}

fn attempt(
    library: &Library,
    weights: &[f32],
    (width, height): (usize, usize),
    rng: &mut SplitMix64,
) -> Option<Vec<usize>> {
    let words = library.words;
    let cells = width * height;
    let mut full = vec![0u64; words];
    for (tile, weight) in weights.iter().enumerate() {
        if *weight > 0.0 {
            full[tile / 64] |= 1 << (tile % 64);
        }
    }
    let count = full.iter().map(|word| word.count_ones()).sum();
    let mut grid = Grid {
        library,
        weights,
        width,
        height,
        domains: full.repeat(cells),
        counts: vec![count; cells],
        entropy: vec![f32::NAN; cells],
    };
    // Closed border: edges facing outside carry no rail.
    for cell in 0..cells {
        for edge in 0..EDGES {
            if grid.neighbour(cell, edge).is_none() {
                grid.restrict(cell, &library.edge_sets[edge][0])?;
            }
        }
    }
    let mut stack: Vec<usize> = (0..cells).collect();
    if !grid.propagate(&mut stack) {
        return None;
    }
    loop {
        let mut best: Option<(usize, f32)> = None;
        for cell in 0..cells {
            if grid.counts[cell] <= 1 {
                continue;
            }
            let value = grid.cell_entropy(cell) + rng.next_f32() * 1e-6;
            if best.is_none_or(|(_, lowest)| value < lowest) {
                best = Some((cell, value));
            }
        }
        let Some((cell, _)) = best else { break };
        let mut total = 0.0;
        grid.for_each_tile(cell, |tile| total += weights[tile]);
        let mut pick = rng.next_f32() * total;
        let mut chosen = None;
        grid.for_each_tile(cell, |tile| {
            if chosen.is_none() {
                pick -= weights[tile];
                if pick <= 0.0 {
                    chosen = Some(tile);
                }
            }
        });
        // Rounding can leave `pick` marginally positive: take the last tile.
        let mut last = EMPTY_TILE;
        grid.for_each_tile(cell, |tile| last = tile);
        let tile = chosen.unwrap_or(last);
        let mut only = vec![0u64; words];
        only[tile / 64] = 1 << (tile % 64);
        grid.restrict(cell, &only)?;
        stack.push(cell);
        if !grid.propagate(&mut stack) {
            return None;
        }
    }
    let mut tiles = Vec::with_capacity(cells);
    for cell in 0..cells {
        let mut only = EMPTY_TILE;
        grid.for_each_tile(cell, |tile| only = tile);
        tiles.push(only);
    }
    Some(tiles)
}

/// Collapse a `width` x `height` grid into tile indices. After too many
/// contradictions the machine is simply empty, so this never fails.
pub(super) fn collapse_grid(
    library: &Library,
    weights: &[f32],
    size: (usize, usize),
    seed: u64,
) -> Vec<usize> {
    let mut rng = SplitMix64(seed);
    for _ in 0..MAX_ATTEMPTS {
        if let Some(tiles) = attempt(library, weights, size, &mut rng) {
            return tiles;
        }
    }
    vec![EMPTY_TILE; size.0 * size.1]
}
