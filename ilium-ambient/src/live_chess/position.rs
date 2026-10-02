//! Chess TV delivers authoritative FEN snapshots. Applying those snapshots
//! preserves castling, en-passant and promotion without guessing legal moves
//! from a delayed animation queue. This is a FEN decoder, not a move engine.
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starting_board_and_side_decode_in_screen_rank_order() {
        let position =
            ChessPosition::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
                .unwrap();
        assert_eq!(position.board[0], Some('r'));
        assert_eq!(position.board[63], Some('R'));
        assert!(position.white_to_move);
    }
    #[test]
    fn special_moves_are_visible_in_authoritative_positions() {
        let castle = ChessPosition::from_fen("r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1").unwrap();
        assert_eq!(castle.board[62], Some('K'));
        assert_eq!(castle.board[61], Some('R'));
        let promotion = ChessPosition::from_fen("Q3k3/8/8/8/8/8/8/4K3 b - - 0 20").unwrap();
        assert_eq!(promotion.board[0], Some('Q'));
        let en_passant = ChessPosition::from_fen("4k3/8/3P4/8/8/8/8/4K3 b - - 0 20").unwrap();
        assert_eq!(en_passant.board[19], Some('P'));
        assert_eq!(en_passant.board[27], None);
    }
    #[test]
    fn malformed_ranks_pieces_and_clocks_never_replace_a_good_position() {
        for fen in [
            "8/8/8/8/8/8/8/8 w - - 0 1",
            "9/8/8/8/8/8/8/4K2k w - - 0 1",
            "4k3/8/8/8/8/8/8/4K3 x - - 0 1",
            "4k3/8/8/8/8/8/8/4K3 w - - -1 1",
            "4k3/8/8/8/8/8/8/4K3 w - - 0 0",
        ] {
            assert!(ChessPosition::from_fen(fen).is_err(), "accepted {fen}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChessPosition {
    /// a8..h8, then a7..h7, through a1..h1 (screen row order).
    pub board: [Option<char>; 64],
    pub white_to_move: bool,
    pub fullmove: u32,
}

impl ChessPosition {
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        if fen.len() > 256 {
            return Err("FEN exceeds length bound".into());
        }
        let fields: Vec<_> = fen.split_whitespace().collect();
        if fields.len() != 6 {
            return Err("FEN needs six fields".into());
        }
        let ranks: Vec<_> = fields[0].split('/').collect();
        if ranks.len() != 8 {
            return Err("FEN needs eight ranks".into());
        }
        let mut board = [None; 64];
        for (row, rank) in ranks.iter().enumerate() {
            let mut column = 0;
            for symbol in rank.chars() {
                if ('1'..='8').contains(&symbol) {
                    column += usize::from(symbol as u8 - b'0');
                } else if "prnbqkPRNBQK".contains(symbol) && column < 8 {
                    board[row * 8 + column] = Some(symbol);
                    column += 1;
                } else {
                    return Err("invalid FEN rank or piece".into());
                }
                if column > 8 {
                    return Err("FEN rank exceeds eight files".into());
                }
            }
            if column != 8 {
                return Err("FEN rank has fewer than eight files".into());
            }
        }
        if board.iter().filter(|piece| **piece == Some('K')).count() != 1
            || board.iter().filter(|piece| **piece == Some('k')).count() != 1
        {
            return Err("position needs one king of each color".into());
        }
        let white_to_move = match fields[1] {
            "w" => true,
            "b" => false,
            _ => return Err("invalid side to move".into()),
        };
        if fields[2] != "-"
            && !fields[2]
                .chars()
                .all(|c| "KQkqABCDEFGHabcdefgh".contains(c))
        {
            return Err("invalid castling field".into());
        }
        let target = fields[3].as_bytes();
        if fields[3] != "-"
            && (target.len() != 2
                || !(b'a'..=b'h').contains(&target[0])
                || ![b'3', b'6'].contains(&target[1]))
        {
            return Err("invalid en-passant square".into());
        }
        fields[4]
            .parse::<u32>()
            .map_err(|_| "invalid halfmove clock")?;
        let fullmove = fields[5]
            .parse::<u32>()
            .map_err(|_| "invalid fullmove number")?;
        if fullmove == 0 {
            return Err("fullmove number must be positive".into());
        }
        Ok(Self {
            board,
            white_to_move,
            fullmove,
        })
    }
}
