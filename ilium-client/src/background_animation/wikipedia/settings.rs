//! Project-persisted Wikipedia presentation policy; no network or UI state.
use ilium_ambient::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    #[default]
    Braille,
    Text,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Wikipedia,
    Pastel,
    Sepia,
    Night,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WikipediaSettings {
    pub render_mode: RenderMode,
    pub palette: Palette,
    pub greyscale: bool,
    pub zoom_percent: u16,
    /// Tenths of a terminal row per second; zero pauses without rotating.
    pub scroll_tenths: u16,
    pub dwell_seconds: u16,
    pub hue_degrees: u16,
    pub saturation_percent: u16,
    pub lightness_percent: u16,
}

impl WikipediaSettings {
    pub fn uses_native_text(&self) -> bool {
        self.render_mode == RenderMode::Text
    }
}

impl Default for WikipediaSettings {
    fn default() -> Self {
        Self {
            render_mode: RenderMode::Braille,
            palette: Palette::Wikipedia,
            greyscale: true,
            zoom_percent: 100,
            scroll_tenths: 2,
            dwell_seconds: 10,
            hue_degrees: 0,
            saturation_percent: 100,
            lightness_percent: 60,
        }
    }
}

impl SceneSettings for WikipediaSettings {
    fn normalized(&self) -> Self {
        Self {
            zoom_percent: self.zoom_percent.clamp(50, 300),
            scroll_tenths: self.scroll_tenths.min(100),
            dwell_seconds: self.dwell_seconds.clamp(2, 60),
            hue_degrees: self.hue_degrees.min(359),
            saturation_percent: self.saturation_percent.min(200),
            lightness_percent: self.lightness_percent.clamp(10, 100),
            ..*self
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut controls = vec![
            Control::choice("wiki_render_mode", "Page rendering", usize::from(self.render_mode == RenderMode::Text), &["Braille", "Text"], "Braille draws real font outlines as dots; Text uses readable terminal characters."),
            Control::toggle("wiki_greyscale", "Greyscale", self.greyscale, "Neutral ink and images. Turn off to use the selected color palette."),
            Control::choice("wiki_palette", "Page palette", self.palette as usize, &["Wikipedia", "Pastel", "Sepia", "Night"], "A palette for article ink, links, rules and images. Color sliders adjust every preset."),
        ];
        if self.render_mode == RenderMode::Braille {
            controls.push(Control::slider("wiki_zoom", "Page zoom", i32::from(self.zoom_percent), (50, 300, 5), "%", "Scale the page's real font, headings and images. Reflow keeps the page within the terminal width."));
        }
        controls.extend([
            Control::slider("wiki_scroll", "Page scroll", i32::from(self.scroll_tenths), (0, 100, 1), " × 0.1 row/s", "Slowly scroll the entire article; zero pauses."),
            Control::slider("wiki_dwell", "Page dwell", i32::from(self.dwell_seconds), (2, 60, 1), " s", "Pause at the top and bottom, then select another article from today's English Wikipedia Main Page."),
            Control::slider("wiki_hue", "Palette hue shift", i32::from(self.hue_degrees), (0, 359, 1), "°", "Rotate the hue of text and images; visible with Greyscale off."),
            Control::slider("wiki_saturation", "Palette saturation", i32::from(self.saturation_percent), (0, 200, 5), "%", "Scale color intensity; zero gives neutral greys, 100% keeps the preset."),
            Control::slider("wiki_lightness", "Page lightness", i32::from(self.lightness_percent), (10, 100, 1), "%", "Adjust ink and image brightness against the dark workspace background."),
        ]);
        controls
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = *self;
        let number = control::number(&value).map(|n| n.clamp(0, i32::from(u16::MAX)) as u16);
        match id {
            "wiki_render_mode" => match control::index(&value) {
                Some(0) => self.render_mode = RenderMode::Braille,
                Some(1) => self.render_mode = RenderMode::Text,
                _ => return Ok(false),
            },
            "wiki_palette" => match control::index(&value) {
                Some(0) => self.palette = Palette::Wikipedia,
                Some(1) => self.palette = Palette::Pastel,
                Some(2) => self.palette = Palette::Sepia,
                Some(3) => self.palette = Palette::Night,
                _ => return Ok(false),
            },
            "wiki_greyscale" => match control::boolean(&value) {
                Some(on) => self.greyscale = on,
                None => return Ok(false),
            },
            "wiki_zoom" | "wiki_scroll" | "wiki_dwell" | "wiki_hue" | "wiki_saturation"
            | "wiki_lightness" => {
                let Some(number) = number else {
                    return Ok(false);
                };
                match id {
                    "wiki_zoom" => self.zoom_percent = number,
                    "wiki_scroll" => self.scroll_tenths = number,
                    "wiki_dwell" => self.dwell_seconds = number,
                    "wiki_hue" => self.hue_degrees = number,
                    "wiki_saturation" => self.saturation_percent = number,
                    _ => self.lightness_percent = number,
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

impl WikipediaSettings {
    /// Palette transform shared by native text, font ink and decoded images.
    pub fn color(&self, rgb: [u8; 3]) -> [u8; 3] {
        let [mut r, mut g, mut b] = rgb.map(|v| f32::from(v) / 255.0);
        match self.palette {
            Palette::Wikipedia => {}
            Palette::Pastel => {
                r = 0.45 + r * 0.55;
                g = 0.45 + g * 0.55;
                b = 0.45 + b * 0.55;
            }
            Palette::Sepia => {
                let l = r * 0.2126 + g * 0.7152 + b * 0.0722;
                r = l * 0.9 + 0.18;
                g = l * 0.7 + 0.12;
                b = l * 0.4 + 0.08;
            }
            Palette::Night => {
                r *= 0.55;
                g = g * 0.7 + 0.1;
                b = b * 0.8 + 0.2;
            }
        }
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let mut hue = if delta <= f32::EPSILON {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        hue = (hue + f32::from(self.hue_degrees.min(359)) / 60.0).rem_euclid(6.0);
        let luminance = (max + min) / 2.0;
        let saturation = if delta <= f32::EPSILON {
            0.0
        } else {
            delta / (1.0 - (2.0 * luminance - 1.0).abs()).max(0.001)
        };
        let saturation = if self.greyscale {
            0.0
        } else {
            (saturation * f32::from(self.saturation_percent.min(200)) / 100.0).min(1.0)
        };
        // Keep non-black image detail while making ink naturally dimmable.
        let luminance =
            (luminance * f32::from(self.lightness_percent.clamp(10, 100)) / 60.0).min(0.95);
        let chroma = (1.0 - (2.0 * luminance - 1.0).abs()) * saturation;
        let secondary = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
        let channels = match hue as u8 {
            0 => [chroma, secondary, 0.0],
            1 => [secondary, chroma, 0.0],
            2 => [0.0, chroma, secondary],
            3 => [0.0, secondary, chroma],
            4 => [secondary, 0.0, chroma],
            _ => [chroma, 0.0, secondary],
        };
        channels.map(|v| {
            ((v + luminance - chroma / 2.0) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8
        })
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
