//! Original geometric chess silhouettes. The host thresholds these intensities
//! into Braille; no font or Unicode chess glyph is substituted for the pieces.
use super::position::ChessPosition;
use crate::control::{Control, ControlValue, SceneSettings};
use crate::raster::Raster;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChessSettings {
    pub black_at_bottom: bool,
    pub brightness_percent: i32,
    pub white_rgb: [u8; 3],
    pub black_rgb: [u8; 3],
    pub board_rgb: [u8; 3],
}
impl Default for ChessSettings {
    fn default() -> Self {
        Self {
            black_at_bottom: false,
            brightness_percent: 100,
            white_rgb: [235, 222, 184],
            black_rgb: [100, 164, 205],
            board_rgb: [80, 95, 105],
        }
    }
}
impl SceneSettings for ChessSettings {
    fn normalized(&self) -> Self {
        let mut next = self.clone();
        next.brightness_percent = next.brightness_percent.clamp(0, 200);
        next
    }
    fn controls(&self) -> Vec<Control> {
        let mut controls = vec![
            Control::toggle(
                "black_at_bottom",
                "Black at bottom",
                self.black_at_bottom,
                "Rotate the board by 180 degrees. Pieces remain upright.",
            ),
            Control::slider(
                "brightness_percent",
                "Chess brightness",
                self.brightness_percent,
                (0, 200, 5),
                "%",
                "Intensity before Braille dithering; zero hides the board and pieces.",
            ),
        ];
        for (prefix, rgb) in [
            ("white", self.white_rgb),
            ("black", self.black_rgb),
            ("board", self.board_rgb),
        ] {
            for (channel, component) in rgb.iter().enumerate() {
                let id = color_id(prefix, channel);
                let label = match (prefix, channel) {
                    ("white", 0) => "White red",
                    ("white", 1) => "White green",
                    ("white", _) => "White blue",
                    ("black", 0) => "Black red",
                    ("black", 1) => "Black green",
                    ("black", _) => "Black blue",
                    (_, 0) => "Board red",
                    (_, 1) => "Board green",
                    (_, _) => "Board blue",
                };
                controls.push(Control::slider(
                    id,
                    label,
                    i32::from(*component),
                    (0, 255, 5),
                    "",
                    "Independent RGB channel; changing color preserves dot density.",
                ));
            }
        }
        controls
    }
    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        if id == "black_at_bottom" {
            let Some(value) = crate::control::boolean(&value) else {
                return Ok(false);
            };
            next.black_at_bottom = value;
        } else if id == "brightness_percent" {
            let Some(value) = crate::control::number(&value) else {
                return Ok(false);
            };
            next.brightness_percent = value;
        } else {
            let Some(value) = crate::control::number(&value) else {
                return Ok(false);
            };
            let Some((group, channel)) = ["white", "black", "board"]
                .iter()
                .flat_map(|group| (0..3).map(move |channel| (*group, channel)))
                .find(|(group, channel)| color_id(group, *channel) == id)
            else {
                return Ok(false);
            };
            let rgb = match group {
                "white" => &mut next.white_rgb,
                "black" => &mut next.black_rgb,
                _ => &mut next.board_rgb,
            };
            rgb[channel] = value.clamp(0, 255) as u8;
        }
        next = next.normalized();
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}
fn color_id(group: &str, channel: usize) -> &'static str {
    match (group, channel) {
        ("white", 0) => "white_red",
        ("white", 1) => "white_green",
        ("white", _) => "white_blue",
        ("black", 0) => "black_red",
        ("black", 1) => "black_green",
        ("black", _) => "black_blue",
        (_, 0) => "board_red",
        (_, 1) => "board_green",
        (_, _) => "board_blue",
    }
}

/// Filled silhouettes in a normalized square. All six designs share a plinth;
/// heads distinguish ball, crenellations, horse, mitre, crown and king's cross.
pub fn piece_coverage(piece: char, x: f32, y: f32) -> f32 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    let base = rectangle(x, y, 0.19, 0.81, 0.83, 0.91) || rectangle(x, y, 0.26, 0.74, 0.76, 0.84);
    let body = polygon(
        x,
        y,
        &[(0.34, 0.74), (0.42, 0.43), (0.58, 0.43), (0.66, 0.74)],
    );
    let shape = match piece.to_ascii_lowercase() {
        'p' => base || body || ellipse(x, y, 0.5, 0.32, 0.15, 0.15),
        'r' => {
            base || rectangle(x, y, 0.34, 0.66, 0.32, 0.76)
                || rectangle(x, y, 0.23, 0.77, 0.23, 0.36)
                || ((0.12..=0.26).contains(&y)
                    && [(0.23, 0.35), (0.44, 0.56), (0.65, 0.77)]
                        .iter()
                        .any(|(a, b)| (*a..=*b).contains(&x)))
        }
        'n' => {
            base || polygon(
                x,
                y,
                &[
                    (0.28, 0.76),
                    (0.38, 0.51),
                    (0.31, 0.42),
                    (0.19, 0.43),
                    (0.22, 0.31),
                    (0.42, 0.18),
                    (0.49, 0.1),
                    (0.58, 0.2),
                    (0.68, 0.36),
                    (0.71, 0.76),
                ],
            ) && !ellipse(x, y, 0.41, 0.30, 0.035, 0.035)
        }
        'b' => {
            base || body
                || (ellipse(x, y, 0.5, 0.30, 0.17, 0.22) && !(x > 0.47 && x < 0.55 && y < 0.32))
                || ellipse(x, y, 0.5, 0.1, 0.04, 0.045)
        }
        'q' => {
            base || body
                || polygon(
                    x,
                    y,
                    &[
                        (0.33, 0.43),
                        (0.23, 0.19),
                        (0.39, 0.28),
                        (0.5, 0.10),
                        (0.61, 0.28),
                        (0.77, 0.19),
                        (0.67, 0.43),
                    ],
                )
                || ellipse(x, y, 0.23, 0.16, 0.045, 0.045)
                || ellipse(x, y, 0.77, 0.16, 0.045, 0.045)
        }
        'k' => {
            base || body
                || ellipse(x, y, 0.5, 0.35, 0.19, 0.12)
                || rectangle(x, y, 0.46, 0.54, 0.08, 0.30)
                || rectangle(x, y, 0.36, 0.64, 0.15, 0.22)
        }
        _ => false,
    };
    if shape {
        1.0
    } else {
        0.0
    }
}
fn rectangle(x: f32, y: f32, left: f32, right: f32, top: f32, bottom: f32) -> bool {
    (left..=right).contains(&x) && (top..=bottom).contains(&y)
}
fn ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> bool {
    ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
}
fn polygon(x: f32, y: f32, vertices: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut previous = vertices[vertices.len() - 1];
    for &current in vertices {
        if (current.1 > y) != (previous.1 > y)
            && x < (previous.0 - current.0) * (y - current.1) / (previous.1 - current.1) + current.0
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// Center an eight-by-eight board using whole terminal cells. Physical squares
/// use two cell columns per row (2x4 raster dots). Too-small targets stay blank.
/// Supersampled silhouette edges yield coverage suitable for ordered/stippled
/// dithering. The strongest piece sample selects each cell's foreground color.
pub fn draw_board(
    position: &ChessPosition,
    settings: &ChessSettings,
    raster: &mut Raster,
    colors: &mut Vec<[u8; 3]>,
) {
    let width = raster.width / 2;
    let height = raster.height / 4;
    colors.resize(width * height, [0; 3]);
    colors.fill([0; 3]);
    raster.dots.fill(0.0);
    let square_rows = (width / 16).min(height / 8);
    if square_rows == 0 || settings.brightness_percent <= 0 {
        return;
    }
    let side = square_rows * 4;
    let left = ((width - 16 * square_rows) / 2) * 2;
    let top = ((height - 8 * square_rows) / 2) * 4;
    let brightness = settings.brightness_percent.clamp(0, 200) as f32 / 100.0;
    let mut piece_strength = vec![0.0f32; width * height];
    for y in top..top + side * 8 {
        for x in left..left + side * 8 {
            let row = (y - top) / side;
            let column = (x - left) / side;
            let board_index = row * 8 + column;
            let index = if settings.black_at_bottom {
                63 - board_index
            } else {
                board_index
            };
            let cell = (y / 4) * width + x / 2;
            let board_light = if (row + column).is_multiple_of(2) {
                0.18
            } else {
                0.035
            };
            if piece_strength[cell] == 0.0 {
                colors[cell] = settings.board_rgb;
            }
            let mut coverage = 0.0;
            if let Some(piece) = position.board[index] {
                for dy in [0.25, 0.75] {
                    for dx in [0.25, 0.75] {
                        coverage += piece_coverage(
                            piece,
                            ((x - left) % side) as f32 / side as f32 + dx / side as f32,
                            ((y - top) % side) as f32 / side as f32 + dy / side as f32,
                        ) * 0.25;
                    }
                }
                if coverage > piece_strength[cell] {
                    piece_strength[cell] = coverage;
                    colors[cell] = if piece.is_ascii_uppercase() {
                        settings.white_rgb
                    } else {
                        settings.black_rgb
                    };
                }
            }
            raster.dots[y * raster.width + x] =
                ((board_light + (0.94 - board_light) * coverage) * brightness).clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_control_edits_and_settings_round_trip() {
        let mut settings = ChessSettings::default();
        let mut ids = std::collections::HashSet::new();
        for control in settings.controls() {
            assert!(ids.insert(control.id));
            assert!(settings
                .set_control(control.id, control.stepped(1).unwrap())
                .unwrap());
        }
        assert_eq!(settings.controls().len(), 11);
        assert_eq!(
            serde_json::from_str::<ChessSettings>(&serde_json::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
        assert!(!settings
            .set_control("unknown", ControlValue::Number(10))
            .unwrap());
        assert!(!settings
            .set_control("white_red", ControlValue::Bool(true))
            .unwrap());
        settings
            .set_control("white_red", ControlValue::Number(999))
            .unwrap();
        assert_eq!(settings.white_rgb[0], 255);
        settings
            .set_control("brightness_percent", ControlValue::Number(-5))
            .unwrap();
        assert_eq!(settings.brightness_percent, 0);
    }
    #[test]
    fn independent_colors_do_not_change_piece_density() {
        let position =
            ChessPosition::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
                .unwrap();
        let mut raster = Raster::default();
        raster.resize(128, 128);
        let mut colors = vec![];
        let mut settings = ChessSettings::default();
        draw_board(&position, &settings, &mut raster, &mut colors);
        let original_dots = raster.dots.clone();
        settings.white_rgb = [251, 1, 2];
        settings.black_rgb = [3, 252, 4];
        settings.board_rgb = [5, 6, 253];
        draw_board(&position, &settings, &mut raster, &mut colors);
        assert_eq!(original_dots, raster.dots);
        for rgb in [settings.white_rgb, settings.black_rgb, settings.board_rgb] {
            assert!(colors.contains(&rgb));
        }
    }
    #[test]
    fn all_piece_silhouettes_are_nonempty_and_distinct_at_multiple_scales() {
        for size in [12, 20, 32] {
            let mut shapes = std::collections::HashSet::new();
            for piece in "PNBRQKpnbrqk".chars() {
                let samples: Vec<_> = (0..size * size)
                    .map(|i| {
                        piece_coverage(
                            piece,
                            (i % size) as f32 / size as f32,
                            (i / size) as f32 / size as f32,
                        ) > 0.5
                    })
                    .collect();
                assert!(samples.iter().any(|on| *on));
                if piece.is_ascii_uppercase() {
                    assert!(shapes.insert(samples), "duplicate {piece} at {size}");
                }
            }
        }
    }
    #[test]
    fn orientation_moves_authoritative_a8_to_opposite_corner() {
        let mut position = ChessPosition::from_fen("r3k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        position.board[4] = None;
        position.board[60] = None;
        let mut raster = Raster::default();
        raster.resize(96, 96);
        let mut colors = vec![];
        let mut settings = ChessSettings {
            white_rgb: [1, 2, 3],
            black_rgb: [201, 202, 203],
            ..Default::default()
        };
        draw_board(&position, &settings, &mut raster, &mut colors);
        let first = colors.clone();
        settings.black_at_bottom = true;
        draw_board(&position, &settings, &mut raster, &mut colors);
        assert_ne!(first, colors);
        assert_eq!(
            first.iter().filter(|c| **c == [201, 202, 203]).count(),
            colors.iter().filter(|c| **c == [201, 202, 203]).count()
        );
        assert!(colors.contains(&[201, 202, 203]));
    }
    #[test]
    fn full_board_and_tiny_or_empty_sizes_remain_bounded() {
        let position =
            ChessPosition::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
                .unwrap();
        for (width, height) in [(0, 0), (2, 4), (32, 32), (128, 128)] {
            let mut raster = Raster::default();
            raster.resize(width, height);
            let mut colors = vec![];
            draw_board(
                &position,
                &ChessSettings::default(),
                &mut raster,
                &mut colors,
            );
            assert_eq!(colors.len(), (width / 2) * (height / 4));
            assert!(raster
                .dots
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            let settings = ChessSettings {
                brightness_percent: 0,
                ..Default::default()
            };
            draw_board(&position, &settings, &mut raster, &mut colors);
            assert!(raster.dots.iter().all(|v| *v == 0.0));
        }
    }
}
