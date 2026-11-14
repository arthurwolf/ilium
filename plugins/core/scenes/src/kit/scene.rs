//! The contract between a scene and its host, as the scenes of this package see it: the
//! `Scene` trait, the `Frame` it draws into, the `SceneEnv` it is built with and the screen
//! occupancy mask. The shapes follow the kernel's `ilium-ambient` scene engine so a scene
//! moved here keeps its code; the host (the client's plugin scene adapter) hides the difference.
//!
//! PALETTE (future plugin contract): `SceneEnv::palette` is the shared look's palette when the
//! scene is built, `Scene::set_palette` delivers later changes; a scene with natural colours
//! follows it with `ScenePalette::recolor` / `at`.

use super::raster::Raster;
use super::style::ScenePalette;
use std::time::{Duration, SystemTime};

/// Everything a scene may need besides its own settings.
#[derive(Clone, Debug, Default)]
pub struct SceneEnv {
    /// The shared look's current palette (empty unless a real palette is in use).
    pub palette: ScenePalette,
    /// The shared observer position (the `location` member of the spec): Greenwich when the
    /// host sends none.
    pub location: SceneLocation,
}

/// The shared observer position, in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneLocation {
    /// North positive, -90..=90.
    pub latitude: f64,
    /// East positive, -180..=180.
    pub longitude: f64,
}

impl SceneLocation {
    /// The position a spec carries as `"location": {"latitude": .., "longitude": ..}`; a
    /// missing or malformed member is Greenwich (0, 0).
    pub fn from_spec(spec: &serde_json::Value) -> Self {
        let member = |key: &str| {
            spec.get("location")
                .and_then(|location| location.get(key))
                .and_then(serde_json::Value::as_f64)
                .filter(|degrees| degrees.is_finite())
                .unwrap_or(0.0)
        };
        Self {
            latitude: member("latitude").clamp(-90.0, 90.0),
            longitude: member("longitude").clamp(-180.0, 180.0),
        }
    }
}

impl SceneEnv {
    /// An environment holding `palette` and no location.
    pub fn new(palette: ScenePalette) -> Self {
        Self {
            palette,
            location: SceneLocation::default(),
        }
    }

    /// The same environment with the observer at `location`.
    pub fn with_location(mut self, location: SceneLocation) -> Self {
        self.location = location;
        self
    }

    /// Test environment: no palette.
    pub fn for_test() -> Self {
        Self::default()
    }
}

/// Which terminal cells of the host screen show something other than empty
/// space, rebuilt by the host from the live workspace for every frame request.
/// Scenes that interact with the screen (`Scene::wants_occupancy`) receive it
/// through `Scene::occupancy`; it never contains animation ink itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OccupancyMask {
    width: u16,
    height: u16,
    occupied: Vec<bool>,
    /// Content-free foreground anchors; forbidden cells are not necessarily text.
    characters: Vec<bool>,
}

impl OccupancyMask {
    /// An all-empty mask of the given size.
    pub fn empty(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            occupied: vec![false; usize::from(width) * usize::from(height)],
            characters: vec![false; usize::from(width) * usize::from(height)],
        }
    }

    /// Builds a mask by asking `is_occupied(column, row)` for every cell.
    pub fn from_fn(width: u16, height: u16, mut is_occupied: impl FnMut(u16, u16) -> bool) -> Self {
        let mut mask = Self::empty(width, height);
        for row in 0..height {
            for column in 0..width {
                if is_occupied(column, row) {
                    mask.set(column, row, true);
                }
            }
        }
        mask
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn set(&mut self, column: u16, row: u16, is_occupied: bool) {
        if column < self.width && row < self.height {
            let index = usize::from(row) * usize::from(self.width) + usize::from(column);
            self.occupied[index] = is_occupied;
            if !is_occupied {
                self.characters[index] = false;
            }
        }
    }

    /// Mark visible foreground without exposing its text. Anchors are always
    /// excluded from animation ink; removing an anchor does not relax exclusion.
    pub fn set_character(&mut self, column: u16, row: u16, is_character: bool) {
        if column < self.width && row < self.height {
            let index = usize::from(row) * usize::from(self.width) + usize::from(column);
            self.characters[index] = is_character;
            if is_character {
                self.occupied[index] = true;
            }
        }
    }

    /// Outside-screen walls never act as character-growth sources.
    pub fn is_character(&self, column: i32, row: i32) -> bool {
        if column < 0 || row < 0 || column >= i32::from(self.width) || row >= i32::from(self.height)
        {
            return false;
        }
        self.characters[row as usize * usize::from(self.width) + column as usize]
    }

    /// Cells outside the screen count as occupied: they are walls.
    pub fn is_occupied(&self, column: i32, row: i32) -> bool {
        if column < 0 || row < 0 || column >= i32::from(self.width) || row >= i32::from(self.height)
        {
            return true;
        }
        self.occupied[row as usize * usize::from(self.width) + column as usize]
    }

    /// Bytes retained, for admission accounting.
    pub fn retained_bytes(&self) -> usize {
        self.occupied.len().saturating_add(self.characters.len())
    }
}

#[cfg(test)]
mod character_occupancy_tests {
    use super::OccupancyMask;

    #[test]
    fn character_anchors_preserve_exclusion_and_distinguish_forbidden_cells() {
        let mut mask = OccupancyMask::from_fn(4, 2, |column, _| column == 0);
        assert!(mask.is_occupied(0, 0));
        assert!(!mask.is_character(0, 0));
        assert!(mask.is_occupied(-1, 0));
        assert!(!mask.is_character(-1, 0));
        mask.set_character(2, 1, true);
        assert!(mask.is_occupied(2, 1));
        assert!(mask.is_character(2, 1));
        let cloned = mask.clone();
        assert_eq!(cloned, mask);
        mask.set_character(2, 1, false);
        assert!(mask.is_occupied(2, 1));
        assert!(!mask.is_character(2, 1));
        assert_ne!(cloned, mask);
        mask.set_character(2, 1, true);
        mask.set(2, 1, false);
        assert!(!mask.is_occupied(2, 1));
        assert!(!mask.is_character(2, 1));
        assert_eq!(mask.retained_bytes(), 16);
    }
}

/// One frame request. The raster is already resized to `2 * width` by `4 * height` dots
/// (Braille sub-cells) and cleared to zero.
pub struct Frame<'a> {
    /// Dot intensities 0.0..=1.0.
    pub raster: &'a mut Raster,
    /// One RGB value per terminal cell, row-major, `width * height` long. Only read by the
    /// host when the scene reports `uses_cell_colors()`.
    pub cell_colors: &'a mut Vec<[u8; 3]>,
    pub width: u16,
    pub height: u16,
    /// Animation clock: time since the scene started, scaled by the user's Speed setting.
    pub time: Duration,
    /// Unscaled wall time since the scene started.
    pub wall: Duration,
    /// Current civil time, for scenes that show the real sky "now".
    pub now: SystemTime,
}

impl Frame<'_> {
    /// The colour of cell (`cell_x`, `cell_y`), when inside the field.
    pub fn cell_color_mut(&mut self, cell_x: u16, cell_y: u16) -> Option<&mut [u8; 3]> {
        if cell_x >= self.width || cell_y >= self.height {
            return None;
        }
        self.cell_colors
            .get_mut(usize::from(cell_y) * usize::from(self.width) + usize::from(cell_x))
    }
}

/// One styled terminal grapheme a scene shows in place of the Braille dots of a cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCell {
    pub symbol: String,
    pub bold: bool,
    pub italic: bool,
}

/// A stateful scene. Created by the package registry from the settings of the spec.
///
/// Rules: time may only be read from `Frame`, never from a clock; `render` must overwrite the
/// whole raster (it arrives zeroed) and, when `uses_cell_colors()`, every cell colour.
pub trait Scene: Send {
    /// Latest pointer position in normalized screen coordinates.
    fn pointer(&mut self, _position: Option<[f32; 2]>) {}

    /// Whether pointer changes can affect the rendered frame.
    fn wants_pointer(&self) -> bool {
        false
    }

    /// Draws one frame.
    fn render(&mut self, frame: &mut Frame<'_>);

    /// Optional single-cell text above the Braille ink.
    fn native_glyph(&self, _x: u16, _y: u16) -> Option<char> {
        None
    }

    /// Optional styled terminal text above the Braille ink: one terminal grapheme (one or two
    /// columns) with a bold and an italic flag, in the cell's colour. A grapheme two columns
    /// wide is reported in its left cell only.
    fn text_cell(&self, _x: u16, _y: u16) -> Option<TextCell> {
        None
    }

    /// The shared palette changed while the scene runs.
    fn set_palette(&mut self, _palette: &ScenePalette) {}

    /// True when the scene reacts to what the workspace draws.
    fn wants_occupancy(&self) -> bool {
        false
    }

    /// The latest screen occupancy, delivered before `render`.
    fn occupancy(&mut self, _mask: &OccupancyMask) {}

    /// True when the scene applies the palette to its own colours itself.
    fn follows_palette(&self) -> bool {
        false
    }

    /// True when the scene supplies `Frame::cell_colors`.
    fn uses_cell_colors(&self) -> bool {
        false
    }

    /// Requested redraw cadence, 1..=30.
    fn frames_per_second(&self) -> u32 {
        12
    }

    /// Takes changed settings (the `settings` object of the spec) without being rebuilt.
    /// `false` makes the package build a fresh scene.
    fn reconfigure(&mut self, _settings: &serde_json::Value) -> bool {
        false
    }

    /// One short status line for the Settings panel; `None` when all is well.
    fn status(&self) -> Option<String> {
        None
    }
}
