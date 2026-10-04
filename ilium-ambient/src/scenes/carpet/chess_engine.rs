#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_legal_moves_and_perft() {
        let position = Position::new();
        assert_eq!(position.legal_moves().len(), 20);
        let nodes: usize = position
            .legal_moves()
            .into_iter()
            .map(|m| position.play(m).legal_moves().len())
            .sum();
        assert_eq!(nodes, 400);
    }
    #[test]
    fn special_moves_and_king_safety() {
        let mut p = Position::empty();
        p.board[60] = Some('K');
        p.board[4] = Some('k');
        p.board[56] = Some('R');
        p.board[63] = Some('R');
        p.rights = 3;
        assert!(p.legal_moves().iter().any(|m| m.from == 60 && m.to == 62));
        let castle = p.play(Move {
            from: 60,
            to: 62,
            promotion: None,
        });
        assert_eq!(castle.board[61], Some('R'));
        assert_eq!(castle.rights & 3, 0);
        p.board[5] = Some('r');
        assert!(!p.legal_moves().iter().any(|m| m.from == 60 && m.to == 62));
        p = Position::empty();
        p.board[60] = Some('K');
        p.board[4] = Some('k');
        p.board[28] = Some('P');
        p.board[27] = Some('p');
        p.en_passant = Some(19);
        assert!(p.legal_moves().iter().any(|m| m.from == 28 && m.to == 19));
        assert_eq!(
            p.play(Move {
                from: 28,
                to: 19,
                promotion: None
            })
            .board[27],
            None
        );
        p.board[8] = Some('P');
        assert_eq!(
            p.legal_moves()
                .iter()
                .filter(|m| m.from == 8 && m.to == 0)
                .count(),
            4
        );
        p = Position::empty();
        p.board[60] = Some('K');
        p.board[0] = Some('k');
        p.board[52] = Some('R');
        p.board[12] = Some('r');
        assert!(!p.legal_moves().iter().any(|m| m.from == 52 && m.to == 53));
    }
    #[test]
    fn bounded_ai_is_legal_deterministic_and_games_end() {
        let p = Position::new();
        let a = p.choose_move(2, 256, 7);
        assert_eq!(a, p.choose_move(2, 256, 7));
        assert!(p.legal_moves().contains(&a.unwrap()));
        let mut mate = Position::empty();
        mate.white = false;
        mate.board[0] = Some('k');
        mate.board[9] = Some('Q');
        mate.board[18] = Some('K');
        assert!(mate.in_check(false));
        assert!(mate.legal_moves().is_empty());
    }
}

// Small orthodox chess engine. Search visits a fixed node budget and never
// shares feed state: the TV's full FEN remains authoritative in live mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Move {
    pub from: usize,
    pub to: usize,
    pub promotion: Option<char>,
}
pub(super) type PositionKey = ([Option<char>; 64], bool, u8, Option<usize>);
#[derive(Clone)]
pub(super) struct Position {
    pub board: [Option<char>; 64],
    pub white: bool,
    pub rights: u8,
    pub en_passant: Option<usize>,
    pub halfmove: u16,
}
impl Position {
    fn empty() -> Self {
        Self {
            board: [None; 64],
            white: true,
            rights: 0,
            en_passant: None,
            halfmove: 0,
        }
    }
    pub fn new() -> Self {
        let mut p = Self::empty();
        for (i, c) in "rnbqkbnrpppppppp".chars().enumerate() {
            p.board[i] = Some(c);
        }
        for (i, c) in "PPPPPPPPRNBQKBNR".chars().enumerate() {
            p.board[48 + i] = Some(c);
        }
        p.rights = 15;
        p
    }
    fn attacked(&self, square: usize, white: bool) -> bool {
        let (x, y) = ((square % 8) as i32, (square / 8) as i32);
        for (i, piece) in self.board.iter().enumerate() {
            let Some(piece) = piece else { continue };
            if piece.is_ascii_uppercase() != white {
                continue;
            }
            let (dx, dy) = (x - (i % 8) as i32, y - (i / 8) as i32);
            let hit = match piece.to_ascii_lowercase() {
                'p' => dy == if white { -1 } else { 1 } && dx.abs() == 1,
                'n' => (dx.abs() == 1 && dy.abs() == 2) || (dx.abs() == 2 && dy.abs() == 1),
                'k' => dx.abs().max(dy.abs()) == 1,
                'b' | 'r' | 'q' => {
                    let aligned = match piece.to_ascii_lowercase() {
                        'b' => dx.abs() == dy.abs(),
                        'r' => dx == 0 || dy == 0,
                        _ => dx == 0 || dy == 0 || dx.abs() == dy.abs(),
                    };
                    if !aligned || (dx == 0 && dy == 0) {
                        false
                    } else {
                        let (sx, sy) = (dx.signum(), dy.signum());
                        let mut tx = (i % 8) as i32 + sx;
                        let mut ty = (i / 8) as i32 + sy;
                        let mut clear = true;
                        while tx != x || ty != y {
                            if self.board[(ty * 8 + tx) as usize].is_some() {
                                clear = false;
                                break;
                            }
                            tx += sx;
                            ty += sy;
                        }
                        clear
                    }
                }
                _ => false,
            };
            if hit {
                return true;
            }
        }
        false
    }
    pub fn in_check(&self, white: bool) -> bool {
        self.board
            .iter()
            .position(|p| *p == Some(if white { 'K' } else { 'k' }))
            .is_none_or(|k| self.attacked(k, !white))
    }
    pub fn legal_moves(&self) -> Vec<Move> {
        let mut moves = Vec::with_capacity(96);
        for from in 0..64 {
            let Some(piece) = self.board[from] else {
                continue;
            };
            if piece.is_ascii_uppercase() != self.white {
                continue;
            }
            let (x, y) = ((from % 8) as i32, (from / 8) as i32);
            let mut add = |tx: i32, ty: i32| {
                if !(0..8).contains(&tx) || !(0..8).contains(&ty) {
                    return false;
                }
                let to = (ty * 8 + tx) as usize;
                if self.board[to].is_some_and(|p| {
                    p.is_ascii_uppercase() == self.white || p.eq_ignore_ascii_case(&'k')
                }) {
                    return false;
                }
                if piece.eq_ignore_ascii_case(&'p') && (ty == 0 || ty == 7) {
                    for promotion in ['q', 'r', 'b', 'n'] {
                        moves.push(Move {
                            from,
                            to,
                            promotion: Some(if self.white {
                                promotion.to_ascii_uppercase()
                            } else {
                                promotion
                            }),
                        });
                    }
                } else {
                    moves.push(Move {
                        from,
                        to,
                        promotion: None,
                    });
                }
                self.board[to].is_none()
            };
            match piece.to_ascii_lowercase() {
                'p' => {
                    let direction = if self.white { -1 } else { 1 };
                    let ny = y + direction;
                    if (0..8).contains(&ny) {
                        if self.board[(ny * 8 + x) as usize].is_none() {
                            add(x, ny);
                            if y == if self.white { 6 } else { 1 }
                                && self.board[((ny + direction) * 8 + x) as usize].is_none()
                            {
                                add(x, ny + direction);
                            }
                        }
                        for nx in [x - 1, x + 1] {
                            if (0..8).contains(&nx) {
                                let to = (ny * 8 + nx) as usize;
                                if self.board[to]
                                    .is_some_and(|p| p.is_ascii_uppercase() != self.white)
                                    || (self.en_passant == Some(to)
                                        && self.board[(y * 8 + nx) as usize]
                                            == Some(if self.white { 'p' } else { 'P' }))
                                {
                                    add(nx, ny);
                                }
                            }
                        }
                    }
                }
                'n' => {
                    for (dx, dy) in [
                        (1, 2),
                        (2, 1),
                        (-1, 2),
                        (-2, 1),
                        (1, -2),
                        (2, -1),
                        (-1, -2),
                        (-2, -1),
                    ] {
                        add(x + dx, y + dy);
                    }
                }
                'k' => {
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            if dx != 0 || dy != 0 {
                                add(x + dx, y + dy);
                            }
                        }
                    }
                }
                kind => {
                    for (dx, dy) in [
                        (1, 0),
                        (-1, 0),
                        (0, 1),
                        (0, -1),
                        (1, 1),
                        (1, -1),
                        (-1, 1),
                        (-1, -1),
                    ] {
                        if (kind == 'b' && (dx == 0 || dy == 0))
                            || (kind == 'r' && dx != 0 && dy != 0)
                        {
                            continue;
                        }
                        let mut nx = x + dx;
                        let mut ny = y + dy;
                        while add(nx, ny) {
                            nx += dx;
                            ny += dy;
                        }
                    }
                }
            }
            if piece.eq_ignore_ascii_case(&'k')
                && from == if self.white { 60 } else { 4 }
                && !self.in_check(self.white)
            {
                let offset = if self.white { 56 } else { 0 };
                let mask = if self.white { 1 } else { 4 };
                let rook = if self.white { 'R' } else { 'r' };
                for (right, rook_file, path, destination) in
                    [(mask, 7, &[5, 6][..], 6), (mask * 2, 0, &[1, 2, 3][..], 2)]
                {
                    if self.rights & right != 0
                        && self.board[offset + rook_file] == Some(rook)
                        && path.iter().all(|i| self.board[offset + i].is_none())
                        && [if destination == 6 { 5 } else { 3 }, destination]
                            .iter()
                            .all(|i| !self.attacked(offset + i, !self.white))
                    {
                        moves.push(Move {
                            from,
                            to: offset + destination,
                            promotion: None,
                        });
                    }
                }
            }
        }
        moves.retain(|m| !self.play(*m).in_check(self.white));
        moves
    }
    pub fn play(&self, m: Move) -> Self {
        let mut p = self.clone();
        let piece = p.board[m.from];
        let capture = p.board[m.to].is_some();
        p.board[m.from] = None;
        p.board[m.to] = m.promotion.or(piece);
        p.en_passant = None;
        if piece.is_some_and(|c| c.eq_ignore_ascii_case(&'p')) {
            if self.en_passant == Some(m.to) && !capture && m.from % 8 != m.to % 8 {
                p.board[if self.white { m.to + 8 } else { m.to - 8 }] = None;
            }
            if m.from.abs_diff(m.to) == 16 {
                p.en_passant = Some((m.from + m.to) / 2);
            }
        }
        if piece.is_some_and(|c| c.eq_ignore_ascii_case(&'k')) {
            p.rights &= if self.white { 12 } else { 3 };
            if m.from.abs_diff(m.to) == 2 {
                let (a, b) = if m.to > m.from {
                    (m.from + 3, m.from + 1)
                } else {
                    (m.from - 4, m.from - 1)
                };
                p.board[b] = p.board[a];
                p.board[a] = None;
            }
        }
        for (square, mask) in [(63, 1), (56, 2), (7, 4), (0, 8)] {
            if m.from == square || m.to == square {
                p.rights &= !mask;
            }
        }
        p.halfmove = if capture || piece.is_some_and(|c| c.eq_ignore_ascii_case(&'p')) {
            0
        } else {
            p.halfmove.saturating_add(1)
        };
        p.white = !self.white;
        p
    }
    fn value(&self) -> i32 {
        self.board
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.map(|p| (i, p)))
            .map(|(i, p)| {
                let value = match p.to_ascii_lowercase() {
                    'p' => 100,
                    'n' => 320,
                    'b' => 330,
                    'r' => 500,
                    'q' => 900,
                    _ => 0,
                };
                let center = 6 - ((i % 8) as i32 - 3).abs() - ((i / 8) as i32 - 3).abs();
                (value + center * 3)
                    * if p.is_ascii_uppercase() == self.white {
                        1
                    } else {
                        -1
                    }
            })
            .sum()
    }
    fn search(
        &self,
        depth: u8,
        budget: &mut usize,
        mut alpha: i32,
        beta: i32,
        stop: &dyn Fn() -> bool,
    ) -> i32 {
        if *budget == 0 || stop() {
            return self.value();
        }
        *budget -= 1;
        // Ordinary leaves need evaluation only; generating another full legal
        // frontier at every leaf dominated rendering-frame CPU. Checking leaves
        // still recognize immediate checkmate.
        if depth == 0 && !self.in_check(self.white) {
            return self.value();
        }
        let moves = self.legal_moves();
        if moves.is_empty() {
            return if self.in_check(self.white) {
                -30000 - i32::from(depth)
            } else {
                0
            };
        }
        if depth == 0 || self.halfmove >= 100 {
            return self.value();
        }
        let mut best = -32000;
        for m in moves {
            let score = -self.play(m).search(depth - 1, budget, -beta, -alpha, stop);
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta || *budget == 0 || stop() {
                break;
            }
        }
        best
    }
    #[cfg(test)]
    pub fn choose_move(&self, depth: u8, node_budget: usize, seed: u64) -> Option<Move> {
        self.choose_move_cancellable(depth, node_budget, seed, None)
    }
    #[cfg(test)]
    pub fn choose_move_cancellable(
        &self,
        depth: u8,
        node_budget: usize,
        seed: u64,
        stop: Option<&std::sync::atomic::AtomicBool>,
    ) -> Option<Move> {
        self.choose_move_with_stop(depth, node_budget, seed, &|| {
            stop.is_some_and(|s| s.load(std::sync::atomic::Ordering::Relaxed))
        })
    }
    pub fn choose_move_with_stop(
        &self,
        depth: u8,
        node_budget: usize,
        seed: u64,
        stop: &dyn Fn() -> bool,
    ) -> Option<Move> {
        if stop() {
            return None;
        }
        let mut moves = self.legal_moves();
        if moves.is_empty() {
            return None;
        }
        let rotation = seed as usize % moves.len();
        moves.rotate_left(rotation);
        let mut budget = node_budget.clamp(32, 8192);
        let mut best = None;
        let mut best_score = -32001;
        for m in moves {
            let score =
                -self
                    .play(m)
                    .search(depth.clamp(1, 3) - 1, &mut budget, -32000, 32000, stop);
            if score > best_score {
                best_score = score;
                best = Some(m);
            }
            if budget == 0 || stop() {
                break;
            }
        }
        best
    }
    pub fn signature(&self) -> PositionKey {
        (self.board, self.white, self.rights, self.en_passant)
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    #[test]
    fn third_ply_perft_and_illegal_en_passant_discovered_check() {
        let p = Position::new();
        let mut nodes = 0;
        for a in p.legal_moves() {
            let p = p.play(a);
            for b in p.legal_moves() {
                nodes += p.play(b).legal_moves().len();
            }
        }
        assert_eq!(nodes, 8902);
        let mut p = Position::empty();
        p.board[28] = Some('K');
        p.board[21] = Some('P');
        p.board[22] = Some('p');
        p.board[23] = Some('r');
        p.board[0] = Some('k');
        p.en_passant = Some(14);
        // e5 king is not aligned with these pawns: use rank-five horizontal pin.
        p.board[28] = None;
        p.board[24] = Some('K');
        p.board[21] = None;
        p.board[22] = None;
        p.board[23] = None;
        p.board[29] = Some('P');
        p.board[30] = Some('p');
        p.board[31] = Some('r');
        p.en_passant = Some(22);
        assert!(!p.legal_moves().iter().any(|m| m.from == 29 && m.to == 22));
    }
    #[test]
    fn black_castling_moves_rook_and_rook_capture_revokes_right() {
        let mut p = Position::empty();
        p.white = false;
        p.board[4] = Some('k');
        p.board[0] = Some('r');
        p.board[60] = Some('K');
        p.rights = 8;
        assert!(p.legal_moves().iter().any(|m| m.from == 4 && m.to == 2));
        let moved = p.play(Move {
            from: 4,
            to: 2,
            promotion: None,
        });
        assert_eq!(moved.board[3], Some('r'));
        assert_eq!(moved.rights & 12, 0);
        p.white = true;
        p.board[8] = Some('R');
        let captured = p.play(Move {
            from: 8,
            to: 0,
            promotion: None,
        });
        assert_eq!(captured.rights & 8, 0);
    }
}

#[cfg(test)]
mod callback_cancellation_tests {
    use super::*;
    #[test]
    fn callback_cancellation_is_observed_inside_recursive_search_nodes() {
        let calls = std::cell::Cell::new(0usize);
        let stop = || {
            let next = calls.get() + 1;
            calls.set(next);
            next >= 8
        };
        let _ = Position::new().choose_move_with_stop(3, 8192, 7, &stop);
        assert!(
            calls.get() >= 8,
            "cancellation must be checked within search, not just at submission"
        );
        assert!(
            calls.get() < 32,
            "search must stop promptly after its finite internal cancellation check"
        );
    }
}
