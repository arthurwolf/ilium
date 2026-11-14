//! The tile library: every way to lay up to three rails through a square
//! tile, built once per tile size.
//!
//! Each tile edge has three port slots at the quarter points. A route joins
//! two ports; a tile holds up to three routes that share no port and never
//! touch. Because neighbouring tiles must agree on which slots are occupied
//! and the border is closed, every rail ends up on a closed loop.

pub(super) const EDGES: usize = 4;
pub(super) const SLOTS: usize = 3;
pub(super) const MASK_COUNT: usize = 1 << SLOTS;
pub(super) const MAX_ROUTES_PER_TILE: usize = 3;

/// Edge order used everywhere: north, east, south, west.
pub(super) const NORTH: usize = 0;
pub(super) const EAST: usize = 1;
pub(super) const SOUTH: usize = 2;
pub(super) const WEST: usize = 3;

pub(super) type Point = (i32, i32);

#[derive(Debug, Clone)]
pub(super) struct Route {
    /// Port ids (`edge * 3 + slot`) of both ends.
    pub ports: [usize; 2],
    /// Polyline from the first port to the second, tile-local dots.
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TileKind {
    Empty,
    Screen,
    Tracks,
}

#[derive(Debug, Clone)]
pub(super) struct Tile {
    pub kind: TileKind,
    pub routes: Vec<usize>,
    /// Occupied-slot bit mask of each edge.
    pub edge_masks: [u8; EDGES],
}

pub(super) struct Library {
    pub tile_size: i32,
    pub routes: Vec<Route>,
    /// Index 0 is the empty tile, index 1 the screen tile.
    pub tiles: Vec<Tile>,
    /// `edge_sets[edge][mask]`: bitset of tiles showing `mask` on `edge`.
    pub edge_sets: Vec<Vec<Vec<u64>>>,
    pub words: usize,
    /// Tiles carrying exactly n routes, indexed by n (0 unused).
    pub tracks_by_route_count: [usize; MAX_ROUTES_PER_TILE + 1],
}

pub(super) const EMPTY_TILE: usize = 0;
pub(super) const SCREEN_TILE: usize = 1;

/// Tile-local position of a port.
pub(super) fn port_point(tile_size: i32, port: usize) -> Point {
    let edge = port / SLOTS;
    let along = tile_size * (port % SLOTS) as i32 / 4 + tile_size / 4;
    match edge {
        NORTH => (along, 0),
        EAST => (tile_size, along),
        SOUTH => (along, tile_size),
        _ => (0, along),
    }
}

fn candidate_routes(tile_size: i32) -> Vec<Route> {
    let port = |edge: usize, slot: usize| edge * SLOTS + slot;
    let mut routes = Vec::new();
    for slot in 0..SLOTS {
        for (from, to) in [(NORTH, SOUTH), (WEST, EAST)] {
            let (a, b) = (port(from, slot), port(to, slot));
            routes.push(Route {
                ports: [a, b],
                points: vec![port_point(tile_size, a), port_point(tile_size, b)],
            });
        }
    }
    for (first, second) in [(NORTH, EAST), (EAST, SOUTH), (SOUTH, WEST), (WEST, NORTH)] {
        for slot_a in 0..SLOTS {
            for slot_b in 0..SLOTS {
                let (a, b) = (port(first, slot_a), port(second, slot_b));
                let (start, end) = (port_point(tile_size, a), port_point(tile_size, b));
                // The bend sits where the two rails would cross.
                let bend = if first % 2 == 0 {
                    (start.0, end.1)
                } else {
                    (end.0, start.1)
                };
                routes.push(Route {
                    ports: [a, b],
                    points: vec![start, bend, end],
                });
            }
        }
    }
    routes
}

fn route_dots(route: &Route) -> Vec<Point> {
    let mut dots = Vec::new();
    for pair in route.points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs());
        for step in 0..=steps {
            let fraction = if steps == 0 { 0 } else { step };
            dots.push((
                from.0 + (to.0 - from.0).signum() * fraction,
                from.1 + (to.1 - from.1).signum() * fraction,
            ));
        }
    }
    dots
}

/// Two routes conflict when they share a port or come within one dot.
fn conflict_matrix(routes: &[Route], tile_size: i32) -> Vec<Vec<bool>> {
    let side = (tile_size + 1) as usize;
    let dots: Vec<Vec<Point>> = routes.iter().map(route_dots).collect();
    let mut matrix = vec![vec![false; routes.len()]; routes.len()];
    let mut halo = vec![false; side * side];
    for (a, first) in dots.iter().enumerate() {
        halo.fill(false);
        for &(x, y) in first {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if (0..=tile_size).contains(&nx) && (0..=tile_size).contains(&ny) {
                        halo[ny as usize * side + nx as usize] = true;
                    }
                }
            }
        }
        for (b, second) in dots.iter().enumerate() {
            let shares_port = routes[a].ports.iter().any(|p| routes[b].ports.contains(p));
            let touches = second
                .iter()
                .any(|&(x, y)| halo[y as usize * side + x as usize]);
            matrix[a][b] = a == b || shares_port || touches;
        }
    }
    matrix
}

fn edge_masks_of(routes: &[Route], chosen: &[usize]) -> [u8; EDGES] {
    let mut masks = [0u8; EDGES];
    for &index in chosen {
        for port in routes[index].ports {
            masks[port / SLOTS] |= 1 << (port % SLOTS);
        }
    }
    masks
}

impl Library {
    pub fn new(tile_size: u32) -> Self {
        let tile_size = tile_size as i32;
        let routes = candidate_routes(tile_size);
        let conflicts = conflict_matrix(&routes, tile_size);
        let mut tiles = vec![
            Tile {
                kind: TileKind::Empty,
                routes: Vec::new(),
                edge_masks: [0; EDGES],
            },
            Tile {
                kind: TileKind::Screen,
                routes: Vec::new(),
                edge_masks: [0; EDGES],
            },
        ];
        let mut chosen: Vec<usize> = Vec::new();
        Self::enumerate(&routes, &conflicts, &mut chosen, 0, &mut tiles);

        let words = tiles.len().div_ceil(64);
        let mut edge_sets = vec![vec![vec![0u64; words]; MASK_COUNT]; EDGES];
        let mut tracks_by_route_count = [0usize; MAX_ROUTES_PER_TILE + 1];
        for (index, tile) in tiles.iter().enumerate() {
            for (edge, sets) in edge_sets.iter_mut().enumerate() {
                sets[usize::from(tile.edge_masks[edge])][index / 64] |= 1 << (index % 64);
            }
            if tile.kind == TileKind::Tracks {
                tracks_by_route_count[tile.routes.len()] += 1;
            }
        }
        Self {
            tile_size,
            routes,
            tiles,
            edge_sets,
            words,
            tracks_by_route_count,
        }
    }

    fn enumerate(
        routes: &[Route],
        conflicts: &[Vec<bool>],
        chosen: &mut Vec<usize>,
        first_candidate: usize,
        tiles: &mut Vec<Tile>,
    ) {
        if !chosen.is_empty() {
            tiles.push(Tile {
                kind: TileKind::Tracks,
                routes: chosen.clone(),
                edge_masks: edge_masks_of(routes, chosen),
            });
        }
        if chosen.len() == MAX_ROUTES_PER_TILE {
            return;
        }
        for candidate in first_candidate..routes.len() {
            if chosen.iter().any(|&other| conflicts[candidate][other]) {
                continue;
            }
            chosen.push(candidate);
            Self::enumerate(routes, conflicts, chosen, candidate + 1, tiles);
            chosen.pop();
        }
    }
}
