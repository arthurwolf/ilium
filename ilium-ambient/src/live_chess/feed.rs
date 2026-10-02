//! Lichess TV JSON events retain the authoritative full position.
use super::position::ChessPosition;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub position: ChessPosition,
    pub white_seconds: Option<u32>,
    pub black_seconds: Option<u32>,
    pub last_move: Option<String>,
}

/// Unknown event kinds and blank keepalives preserve the current game.
/// Malformed FEN updates preserve the current game. A featured event starts
/// a new identity epoch, so invalid replacements clear the decoder identity;
/// the live owner retains its independently published last-good snapshot.
pub fn apply_line(current: &mut Option<Game>, line: &[u8]) -> Result<bool, String> {
    if line.len() > 16_384 {
        return Err("chess event exceeds 16 KiB".into());
    }
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    let value: Value = serde_json::from_slice(line).map_err(|error| error.to_string())?;
    let data = &value["d"];
    match value["t"].as_str() {
        Some("featured") => {
            // Subsequent FEN events carry no id. Even an invalid featured
            // replacement must not let them update the previously featured game.
            *current = None;
            let id = data["id"]
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 32
                        && id.chars().all(|c| c.is_ascii_alphanumeric())
                })
                .ok_or_else(|| "featured chess event has no valid game id".to_owned())?;
            let position = position(data)?;
            let clock = |color| {
                data["players"]
                    .as_array()
                    .and_then(|players| players.iter().find(|player| player["color"] == color))
                    .and_then(|player| player["seconds"].as_u64())
                    .and_then(|value| u32::try_from(value).ok())
            };
            *current = Some(Game {
                id: id.into(),
                position,
                white_seconds: clock("white"),
                black_seconds: clock("black"),
                last_move: None,
            });
            Ok(true)
        }
        Some("fen") => {
            let position = position(data)?;
            let Some(game) = current.as_mut() else {
                return Err("chess position arrived before featured game".into());
            };
            game.position = position;
            game.white_seconds = data["wc"].as_u64().and_then(|v| u32::try_from(v).ok());
            game.black_seconds = data["bc"].as_u64().and_then(|v| u32::try_from(v).ok());
            game.last_move = data["lm"]
                .as_str()
                .filter(|text| text.len() <= 5 && text.is_ascii())
                .map(str::to_owned);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn position(data: &Value) -> Result<ChessPosition, String> {
    ChessPosition::from_fen(
        data["fen"]
            .as_str()
            .ok_or_else(|| "chess event has no FEN".to_owned())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_featured_replacement_clears_identity_before_following_fen() {
        for invalid_featured in [
            br#"{"t":"featured","d":{"id":"newgame","fen":"invalid"}}"#.as_slice(),
            br#"{"t":"featured","d":{"id":"","fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1"}}"#.as_slice(),
        ] {
            let mut game = None;
            apply_line(
                &mut game,
                br#"{"t":"featured","d":{"id":"oldgame","fen":"4k3/8/8/8/8/8/8/4K3 w - - 0 1"}}"#,
            )
            .unwrap();
            assert!(apply_line(&mut game, invalid_featured).is_err());
            assert!(
                game.is_none(),
                "invalid featured replacement retained old identity"
            );
            assert!(apply_line(
                &mut game,
                br#"{"t":"fen","d":{"fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1"}}"#
            )
            .is_err());
            assert!(game.is_none());
            apply_line(
                &mut game,
                br#"{"t":"featured","d":{"id":"newgame","fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1"}}"#,
            )
            .unwrap();
            assert_eq!(game.as_ref().unwrap().id, "newgame");
        }
    }
    #[test]
    fn authoritative_positions_replace_moves_and_invalid_events_preserve_game() {
        let mut game = None;
        apply_line(&mut game, br#"{"t":"featured","d":{"id":"abcdefgh","fen":"4k3/8/8/8/8/8/8/4K3 w - - 0 1","players":[{"color":"white","seconds":30},{"color":"black","seconds":40}]}}"#).unwrap();
        assert_eq!(game.as_ref().unwrap().white_seconds, Some(30));
        let before = game.clone();
        assert!(apply_line(&mut game, br#"{"t":"fen","d":{"fen":"invalid"}}"#).is_err());
        assert_eq!(game, before);
        apply_line(&mut game, br#"{"t":"fen","d":{"fen":"4k3/8/8/8/8/8/8/3K4 b - - 1 1","wc":29,"bc":40,"lm":"e1d1"}}"#).unwrap();
        assert_eq!(game.as_ref().unwrap().position.board[59], Some('K'));
        assert_eq!(game.as_ref().unwrap().id, "abcdefgh");
        assert!(!apply_line(&mut game, b"\n").unwrap());
    }
}
