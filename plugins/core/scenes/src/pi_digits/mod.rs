//! Exact Pi as native terminal text or as coverage rasterized from the bundled real font (the
//! `pi-digits` scene).
//!
//! The digits come from a bounded integer spigot (`digits`), computed a slice per frame; the
//! font coverage comes from swash (`atlas`). In native mode the scene writes one character per
//! cell through the glyph plane (`Scene::native_glyph`) plus a colour per cell; in Font Braille
//! mode it blits the atlas into the dot raster and colours the cells under the ink.
//!
//! PALETTE (future plugin contract): the cell colours are this scene's own digit colours; the
//! dispatcher's palette handling recolours them onto the shared palette, so the scene does not
//! follow the palette itself.

mod atlas;
mod digits;
mod settings;
#[cfg(test)]
mod tests;

pub use settings::PiSettings;

use crate::kit::control::SceneSettings;
use crate::kit::scene::{Frame, Scene, SceneEnv};
use atlas::Atlas;
use digits::DigitGenerator;
use serde_json::Value;

/// The most cells a frame may have.
const MAX_CELLS: usize = 131_072;
/// Spigot steps spent per frame while the digits are being prepared.
const STEP_BUDGET: usize = 1_500_000;
/// The frame rate the scene asks for.
pub const FRAMES_PER_SECOND: u32 = 12;

pub struct PiScene {
    settings: PiSettings,
    /// The digit generation in progress, if any.
    generator: Option<DigitGenerator>,
    /// The exact digits computed so far for a limit at least as large as the current one.
    digits: Option<String>,
    /// The font atlas and the size it was built for.
    atlas: Option<(i32, Atlas)>,
    native: Vec<Option<char>>,
    native_width: usize,
    status: Option<String>,
}

/// The scene of a spec (the host's spec or a bare settings object).
pub fn create(spec: &Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: PiSettings = crate::scenes2::settings_from_spec(spec);
    Box::new(PiScene::new(settings, env))
}

impl PiScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current palette. This
    // scene's colours are its own digit colours; the dispatcher shifts them onto the palette.
    pub fn new(settings: PiSettings, _environment: &SceneEnv) -> Self {
        let mut scene = Self {
            settings: settings.normalized(),
            generator: None,
            digits: None,
            atlas: None,
            native: Vec::new(),
            native_width: 0,
            status: None,
        };
        scene.start_digits();
        scene
    }

    /// Begins computing the digits for the current limit, unless the ones held are enough.
    fn start_digits(&mut self) {
        let wanted = self.settings.digits_limit as usize;
        self.generator = None;
        if self
            .digits
            .as_ref()
            .is_some_and(|text| text.len() >= wanted)
        {
            self.status = None;
            return;
        }
        match DigitGenerator::new(wanted) {
            Ok(generator) => {
                self.generator = Some(generator);
                self.status = Some("Preparing exact Pi digits".into());
            }
            Err(error) => self.status = Some(error),
        }
    }

    /// Spends this frame's share of the digit generation.
    fn advance_digits(&mut self) {
        let Some(generator) = &mut self.generator else {
            return;
        };
        if !generator.step(STEP_BUDGET) {
            let (done, total) = generator.progress();
            self.status = Some(format!(
                "Preparing exact Pi digits ({}%)",
                done * 100 / total.max(1)
            ));
            return;
        }
        match generator.result() {
            Some(Ok(text)) => {
                self.digits = Some(text.clone());
                self.status = None;
            }
            Some(Err(error)) => self.status = Some(error.clone()),
            None => {}
        }
        self.generator = None;
    }

    /// The atlas for the current font size, built on first use.
    fn ensure_atlas(&mut self) -> Option<&Atlas> {
        let size = self.settings.font_size;
        if !matches!(&self.atlas, Some((built, _)) if *built == size) {
            match atlas::build(size) {
                Ok(atlas) => self.atlas = Some((size, atlas)),
                Err(error) => {
                    self.atlas = None;
                    self.status = Some(error);
                }
            }
        }
        self.atlas.as_ref().map(|(_, atlas)| atlas)
    }

    pub fn apply_settings(&mut self, settings: PiSettings) {
        let settings = settings.normalized();
        let restart = self.settings.digits_limit != settings.digits_limit;
        self.settings = settings;
        self.native.fill(None);
        if restart {
            self.start_digits();
        }
    }
}

/// The character `index` places along the prefix: the decimal separator after the 3, the
/// digits otherwise, wrapping after `limit` digits.
fn character_at(digits: &str, limit: usize, index: usize) -> char {
    let count = limit.min(digits.len());
    if count == 0 {
        return ' ';
    }
    let index = index % (count + 1);
    if index == 1 {
        '.'
    } else {
        char::from(digits.as_bytes()[index.saturating_sub(1)])
    }
}

/// The colour of a character: the common colour angle, or the angle of its digit.
fn digit_color(settings: &PiSettings, character: char) -> [u8; 3] {
    let angle = if settings.digit_colors {
        character
            .to_digit(10)
            .map_or(settings.colour_angle, |digit| {
                settings.digit_angles[digit as usize]
            })
    } else {
        settings.colour_angle
    };
    let sector = angle.rem_euclid(360) as f32 / 60.0;
    let c = 0.75;
    let secondary = c * (1.0 - (sector % 2.0 - 1.0).abs());
    let rgb = match sector as usize {
        0 => [c, secondary, 0.0],
        1 => [secondary, c, 0.0],
        2 => [0.0, c, secondary],
        3 => [0.0, secondary, c],
        4 => [secondary, 0.0, c],
        _ => [c, 0.0, secondary],
    };
    rgb.map(|component| {
        ((component + 0.25) * 255.0 * settings.ink_level as f32 / 100.0).round() as u8
    })
}

impl Scene for PiScene {
    fn reconfigure(&mut self, settings: &Value) -> bool {
        let settings: PiSettings = crate::scenes2::settings_from_spec(settings);
        self.apply_settings(settings);
        true
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        self.advance_digits();
        let width = usize::from(frame.width);
        let height = usize::from(frame.height);
        let count = width * height;
        self.native_width = width;
        self.native.clear();
        frame.raster.dots.fill(0.0);
        frame.cell_colors.clear();
        if count > MAX_CELLS {
            self.status = Some("Pi viewport exceeds 131072-cell rendering bound".into());
            return;
        }
        self.native.resize(count, None);
        frame.cell_colors.resize(count, [0; 3]);
        if self.generator.is_some() || self.settings.ink_level == 0 {
            return;
        }
        let Some(digits) = self.digits.clone() else {
            return;
        };
        let limit = self.settings.digits_limit as usize;
        let offset = ((frame.time.as_secs_f64() * self.settings.scroll_speed as f64 / 10.0).floor()
            % (limit + 1) as f64) as usize;
        if self.settings.mode == 0 {
            for (index, cell) in self.native.iter_mut().enumerate() {
                let character = character_at(&digits, limit, index + offset);
                *cell = Some(character);
                frame.cell_colors[index] = digit_color(&self.settings, character);
            }
            return;
        }
        let settings = self.settings.clone();
        let Some(atlas) = self.ensure_atlas() else {
            return;
        };
        let columns = frame.raster.width / atlas.width;
        let rows = frame.raster.height / atlas.height;
        if columns == 0 || rows == 0 {
            return;
        }
        for row in 0..rows {
            for column in 0..columns {
                let character = character_at(&digits, limit, row * columns + column + offset);
                let glyph_index = character.to_digit(10).map_or(10, |digit| digit as usize);
                let ink = &atlas.glyphs[glyph_index];
                let color = digit_color(&settings, character);
                for y in 0..atlas.height {
                    for x in 0..atlas.width {
                        let sample = ink[y * atlas.width + x];
                        if sample <= 0.0 {
                            continue;
                        }
                        let px = column * atlas.width + x;
                        let py = row * atlas.height + y;
                        frame.raster.dots[py * frame.raster.width + px] = sample;
                        let cell = (py / 4) * width + px / 2;
                        if let Some(cell) = frame.cell_colors.get_mut(cell) {
                            *cell = color;
                        }
                    }
                }
            }
        }
    }

    fn native_glyph(&self, x: u16, y: u16) -> Option<char> {
        if usize::from(x) >= self.native_width {
            return None;
        }
        self.native
            .get(usize::from(y) * self.native_width + usize::from(x))
            .copied()
            .flatten()
    }

    fn uses_cell_colors(&self) -> bool {
        true
    }

    fn frames_per_second(&self) -> u32 {
        FRAMES_PER_SECOND
    }

    fn status(&self) -> Option<String> {
        self.status.clone()
    }
}
