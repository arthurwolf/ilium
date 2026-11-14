//! The palette a scene follows (the part of the shared look a scene sees).
//!
//! The look itself (colour mode, palette presets, brightness, contrast, gamma, dithering)
//! stays in the host; a scene only receives the palette stops and shifts its natural colours
//! onto them with `ScenePalette::recolor` or `at`.

/// One 8-bit RGB colour.
pub type Rgb = [u8; 3];

/// The palette handed to every scene: empty unless the shared look uses a real palette.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScenePalette {
    pub stops: Vec<Rgb>,
    pub reverse: bool,
    pub shift_percent: u16,
}

impl ScenePalette {
    /// A palette of `0xRRGGBB` stops (the form the scene spec and frames carry).
    pub fn from_rgb_words(words: &[u32]) -> Self {
        Self {
            stops: words
                .iter()
                .map(|word| [(word >> 16) as u8, (word >> 8) as u8, *word as u8])
                .collect(),
            reverse: false,
            shift_percent: 0,
        }
    }

    /// True when a palette was supplied and scenes should follow it.
    pub fn is_provided(&self) -> bool {
        !self.stops.is_empty()
    }

    /// Colour at `position` (0..=1) along the palette, with the shared look's reverse and
    /// shift applied. `None` when no palette is provided.
    pub fn at(&self, position: f32) -> Option<Rgb> {
        if self.stops.is_empty() {
            return None;
        }
        let shifted =
            (position.clamp(0.0, 1.0) + f32::from(self.shift_percent) / 100.0).rem_euclid(1.0);
        let shifted = if self.shift_percent == 0 {
            position.clamp(0.0, 1.0)
        } else {
            shifted
        };
        Some(ramp(
            &self.stops,
            if self.reverse { 1.0 - shifted } else { shifted },
        ))
    }

    /// Shift one of the scene's own colours onto the palette: the palette colour at the same
    /// brightness. Returns `color` unchanged when no palette is provided.
    pub fn recolor(&self, color: Rgb) -> Rgb {
        self.at(luma(color)).unwrap_or(color)
    }

    /// `recolor` for every cell colour of a frame.
    pub fn recolor_cells(&self, colors: &mut [Rgb]) {
        if !self.is_provided() {
            return;
        }
        for color in colors {
            *color = self.recolor(*color);
        }
    }
}

fn luma(color: Rgb) -> f32 {
    (0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2]))
        / 255.0
}

fn mix(a: Rgb, b: Rgb, fraction: f32) -> Rgb {
    let fraction = fraction.clamp(0.0, 1.0);
    [0, 1, 2].map(|channel| {
        (f32::from(a[channel]) + (f32::from(b[channel]) - f32::from(a[channel])) * fraction).round()
            as u8
    })
}

/// Colour at `position` (0..=1) along `stops`.
pub fn ramp(stops: &[Rgb], position: f32) -> Rgb {
    match stops.len() {
        0 => [200, 200, 200],
        1 => stops[0],
        count => {
            let scaled = position.clamp(0.0, 1.0) * (count - 1) as f32;
            let index = (scaled.floor() as usize).min(count - 2);
            mix(stops[index], stops[index + 1], scaled - index as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_palette_keeps_the_scenes_own_colours() {
        let palette = ScenePalette::default();
        assert!(!palette.is_provided());
        assert_eq!(palette.recolor([10, 20, 30]), [10, 20, 30]);
    }

    #[test]
    fn rgb_words_become_stops_and_recolor_follows_brightness() {
        let palette = ScenePalette::from_rgb_words(&[0x000000, 0xff0000]);
        assert_eq!(palette.stops, vec![[0, 0, 0], [255, 0, 0]]);
        assert_eq!(palette.recolor([0, 0, 0]), [0, 0, 0]);
        assert_eq!(palette.recolor([255, 255, 255]), [255, 0, 0]);
    }
}
