//! User-facing settings of the video scene: ranges, defaults, settings rows.

use super::discover;
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// A settings enum shown as a `Choice` row.
trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    const LABELS: &'static [&'static str];

    fn index(self) -> usize {
        Self::ALL.iter().position(|item| *item == self).unwrap_or(0)
    }

    fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    /// Real speed, files one after another, forever.
    #[default]
    Live,
    /// Real content at a fraction of its speed.
    Slowed,
    /// Short excerpts from random files at random positions.
    RandomScenes,
}

impl Choice for PlaybackMode {
    const ALL: &'static [Self] = &[Self::Live, Self::Slowed, Self::RandomScenes];
    const LABELS: &'static [&'static str] = &["Live", "Slowed", "Random scenes"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStyle {
    /// Grey levels dithered by the host into monochrome dots.
    #[default]
    Dithered,
    /// Dithered dots tinted with the colour of each cell.
    Colored,
    /// Line-art look: edges plus faint tones, like pen and ink.
    MonoInk,
}

impl Choice for RenderStyle {
    const ALL: &'static [Self] = &[Self::Dithered, Self::Colored, Self::MonoInk];
    const LABELS: &'static [&'static str] = &["Dithered", "Colored", "Mono ink"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    /// Whole picture visible, black bars where the shapes differ.
    #[default]
    Fit,
    /// Picture covers the screen, edges are cropped.
    Fill,
    /// Picture is distorted to the screen shape.
    Stretch,
}

impl Choice for FitMode {
    const ALL: &'static [Self] = &[Self::Fit, Self::Fill, Self::Stretch];
    const LABELS: &'static [&'static str] = &["Fit (letterbox)", "Fill (crop)", "Stretch"];
}

pub const SLOWED_PERCENT: (i32, i32, i32) = (5, 100, 5);
pub const SCENE_SECONDS: (i32, i32, i32) = (3, 120, 1);
pub const FRAME_RATE: (i32, i32, i32) = (6, 24, 1);
pub const BRIGHTNESS: (i32, i32, i32) = (-100, 100, 5);
pub const CONTRAST: (i32, i32, i32) = (-100, 100, 5);
pub const GAMMA_PERCENT: (i32, i32, i32) = (50, 300, 10);
pub const DETAIL: (i32, i32, i32) = (0, 100, 5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSeries {
    #[default]
    Custom,
    Germination,
}

impl Choice for VideoSeries {
    const ALL: &'static [Self] = &[Self::Custom, Self::Germination];
    const LABELS: &'static [&'static str] = &["Custom", "Germination"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    /// Curated remote playlist or the user's retained custom sources.
    pub series: VideoSeries,
    /// One or more entries separated by `;`: a video file, a folder, a glob
    /// such as `~/Videos/**/*.mkv`, or an http(s) URL. Empty: nothing plays.
    pub source: String,
    /// Scan sub-folders of folder entries.
    pub recursive: bool,
    pub mode: PlaybackMode,
    /// Speed in `Slowed` mode, 5..=100 percent of real time.
    pub slowed_percent: u32,
    /// Length of each excerpt in `RandomScenes` mode, 3..=120 seconds.
    pub scene_seconds: u32,
    /// Play the files of a folder or glob in random order.
    pub shuffle: bool,
    /// Keep replaying the first file instead of moving to the next.
    pub repeat_one: bool,
    /// Random-number seed for shuffling and random scenes; 0 means a fresh
    /// random seed every time the scene starts.
    pub seed: u32,
    pub style: RenderStyle,
    pub fit: FitMode,
    /// Added to the brightness of every dot, -100..=100 (0 is neutral).
    pub brightness: i32,
    /// Contrast around mid grey, -100..=100 (0 is neutral).
    pub contrast: i32,
    /// Gamma in percent, 50..=300 (100 is neutral, higher is brighter shadows).
    pub gamma_percent: u32,
    /// Swap light and dark.
    pub invert: bool,
    /// Sharpening strength applied before dithering, 0..=100.
    pub detail: u32,
    /// Frames per second requested from the decoder and the host, 6..=24.
    pub frame_rate: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            series: VideoSeries::Custom,
            source: String::new(),
            recursive: true,
            mode: PlaybackMode::Live,
            slowed_percent: 50,
            scene_seconds: 20,
            shuffle: false,
            repeat_one: false,
            seed: 0,
            style: RenderStyle::Dithered,
            fit: FitMode::Fit,
            brightness: 0,
            contrast: 10,
            gamma_percent: 100,
            invert: false,
            detail: 25,
            frame_rate: 15,
        }
    }
}

fn clamp_u32(value: u32, (min, max, _): (i32, i32, i32)) -> u32 {
    value.clamp(min as u32, max as u32)
}

fn clamp_i32(value: i32, (min, max, _): (i32, i32, i32)) -> i32 {
    value.clamp(min, max)
}

impl VideoSettings {
    /// True when every entry of a non-empty source is a URL (no folder options apply).
    fn is_url_only(&self) -> bool {
        let entries = discover::split_entries(&self.source);
        !entries.is_empty() && entries.iter().all(|entry| discover::is_http_url(entry))
    }

    /// True when the source contains an unencrypted URL worth a warning.
    pub fn uses_plain_http(&self) -> bool {
        if self.series != VideoSeries::Custom {
            return false;
        }
        discover::split_entries(&self.source).iter().any(|entry| {
            entry
                .get(..7)
                .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        })
    }
}

fn number_of(value: &ControlValue) -> Option<i32> {
    control::number(value)
}

impl SceneSettings for VideoSettings {
    fn normalized(&self) -> Self {
        Self {
            // Saving a series choice must retain dormant Custom text. Explicit
            // Source edits and discovery trim entries at their own boundary.
            source: self.source.clone(),
            slowed_percent: clamp_u32(self.slowed_percent, SLOWED_PERCENT),
            scene_seconds: clamp_u32(self.scene_seconds, SCENE_SECONDS),
            brightness: clamp_i32(self.brightness, BRIGHTNESS),
            contrast: clamp_i32(self.contrast, CONTRAST),
            gamma_percent: clamp_u32(self.gamma_percent, GAMMA_PERCENT),
            detail: clamp_u32(self.detail, DETAIL),
            frame_rate: clamp_u32(self.frame_rate, FRAME_RATE),
            seed: self.seed.min(9999),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice(
            "series",
            "Series",
            self.series.index(),
            VideoSeries::LABELS,
            "Custom uses your sources. Germination plays a remote plant-growth playlist, holding the current clip in RAM only.",
        )];
        if self.series == VideoSeries::Custom {
            rows.push(Control::text(
            "source",
            "Source",
            &self.source,
            "file, folder, glob or URL",
            "A video file, a folder, a glob like ~/Videos/**/*.mkv, an http(s) URL, or several of these separated by semicolons.",
            ));
        }
        if self.series == VideoSeries::Custom && !self.is_url_only() {
            rows.push(Control::toggle(
                "recursive",
                "Include sub-folders",
                self.recursive,
                "Also play videos found in folders below a folder entry.",
            ));
        }
        rows.push(Control::choice(
            "mode",
            "Playback",
            self.mode.index(),
            PlaybackMode::LABELS,
            "Live plays at normal speed, Slowed stretches time, Random scenes jumps between short excerpts.",
        ));
        match self.mode {
            PlaybackMode::Slowed => rows.push(Control::slider(
                "slowed_percent",
                "Speed",
                self.slowed_percent as i32,
                SLOWED_PERCENT,
                "%",
                "Playback speed relative to real time. Lower is slower and dreamier.",
            )),
            PlaybackMode::RandomScenes => rows.push(Control::slider(
                "scene_seconds",
                "Scene length",
                self.scene_seconds as i32,
                SCENE_SECONDS,
                " s",
                "How long each random excerpt plays before another one is picked.",
            )),
            PlaybackMode::Live => {}
        }
        if self.mode != PlaybackMode::RandomScenes {
            rows.push(Control::toggle(
                "shuffle",
                "Shuffle",
                self.shuffle,
                "Play the files of a folder or glob in random order.",
            ));
            rows.push(Control::toggle(
                "repeat_one",
                "Repeat one file",
                self.repeat_one,
                "Keep replaying the first file instead of moving on to the next one.",
            ));
        }
        rows.push(Control::choice(
            "style",
            "Style",
            self.style.index(),
            RenderStyle::LABELS,
            "Dithered uses your dot palette, Colored tints each cell with the video colors, Mono ink draws edges like a pen sketch.",
        ));
        rows.push(Control::choice(
            "fit",
            "Fit",
            self.fit.index(),
            FitMode::LABELS,
            "How the picture is mapped to the screen: letterboxed, cropped to fill, or stretched.",
        ));
        rows.push(Control::slider(
            "brightness",
            "Brightness",
            self.brightness,
            BRIGHTNESS,
            "",
            "Lightens or darkens the whole picture before dithering.",
        ));
        rows.push(Control::slider(
            "contrast",
            "Contrast",
            self.contrast,
            CONTRAST,
            "",
            "Spreads dark and light tones apart. Higher values give crisper dither patterns.",
        ));
        rows.push(Control::slider(
            "gamma_percent",
            "Gamma",
            self.gamma_percent as i32,
            GAMMA_PERCENT,
            "%",
            "Above 100% reveals detail in dark scenes, below 100% deepens the shadows.",
        ));
        rows.push(Control::toggle(
            "invert",
            "Invert",
            self.invert,
            "Swap light and dark, useful for bright videos on a dark terminal.",
        ));
        rows.push(Control::slider(
            "detail",
            "Detail",
            self.detail as i32,
            DETAIL,
            "",
            "Sharpens the picture before dithering so small features survive the low resolution.",
        ));
        rows.push(Control::slider(
            "frame_rate",
            "Frame rate",
            self.frame_rate as i32,
            FRAME_RATE,
            " fps",
            "Redraw rate. Lower values use less CPU in both ilium and ffmpeg.",
        ));
        if self.mode == PlaybackMode::RandomScenes || self.shuffle {
            rows.push(Control::slider(
                "seed",
                "Random seed",
                self.seed.min(9999) as i32,
                (0, 9999, 1),
                "",
                "0 picks something new every time; any other number replays the same sequence.",
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        if id == "series" {
            set_choice(&value, &mut self.series);
            return Ok(*self != before);
        }
        match id {
            "source" => {
                let Some(text) = control::text(&value) else {
                    return Ok(false);
                };
                let text = text.trim();
                discover::validate_source(text)?;
                self.source = text.to_owned();
            }
            "recursive" => set_bool(&value, &mut self.recursive),
            "shuffle" => set_bool(&value, &mut self.shuffle),
            "repeat_one" => set_bool(&value, &mut self.repeat_one),
            "invert" => set_bool(&value, &mut self.invert),
            "mode" => set_choice(&value, &mut self.mode),
            "style" => set_choice(&value, &mut self.style),
            "fit" => set_choice(&value, &mut self.fit),
            "slowed_percent" => set_unsigned(&value, &mut self.slowed_percent),
            "scene_seconds" => set_unsigned(&value, &mut self.scene_seconds),
            "gamma_percent" => set_unsigned(&value, &mut self.gamma_percent),
            "detail" => set_unsigned(&value, &mut self.detail),
            "frame_rate" => set_unsigned(&value, &mut self.frame_rate),
            "seed" => set_unsigned(&value, &mut self.seed),
            "brightness" => {
                if let Some(number) = number_of(&value) {
                    self.brightness = number;
                }
            }
            "contrast" => {
                if let Some(number) = number_of(&value) {
                    self.contrast = number;
                }
            }
            _ => return Ok(false),
        }
        *self = self.normalized();
        Ok(*self != before)
    }
}

fn set_bool(value: &ControlValue, target: &mut bool) {
    if let Some(on) = control::boolean(value) {
        *target = on;
    }
}

fn set_choice<T: Choice>(value: &ControlValue, target: &mut T) {
    if let Some(choice) = control::index(value).and_then(T::from_index) {
        *target = choice;
    }
}

fn set_unsigned(value: &ControlValue, target: &mut u32) {
    if let Some(number) = number_of(value) {
        *target = number.max(0) as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(rows: &'a [Control], id: &str) -> Option<&'a Control> {
        rows.iter().find(|row| row.id == id)
    }

    #[test]
    fn defaults_are_inside_their_ranges() {
        let defaults = VideoSettings::default();
        assert_eq!(defaults.normalized(), defaults);
    }

    #[test]
    fn normalization_clamps_numeric_fields_and_preserves_source() {
        let wild = VideoSettings {
            slowed_percent: 0,
            scene_seconds: 9999,
            brightness: -5000,
            contrast: 5000,
            gamma_percent: 1,
            detail: 400,
            frame_rate: 60,
            seed: 100_000,
            source: "  x  ".to_owned(),
            ..VideoSettings::default()
        };
        let clean = wild.normalized();
        assert_eq!(clean.slowed_percent, 5);
        assert_eq!(clean.scene_seconds, 120);
        assert_eq!(clean.brightness, -100);
        assert_eq!(clean.contrast, 100);
        assert_eq!(clean.gamma_percent, 50);
        assert_eq!(clean.detail, 100);
        assert_eq!(clean.frame_rate, 24);
        assert_eq!(clean.seed, 9999);
        assert_eq!(clean.source, "  x  ");
    }

    #[test]
    fn serde_defaults_fill_missing_keys_and_use_snake_case_names() {
        let parsed: VideoSettings =
            serde_json::from_str(r#"{"mode":"random_scenes","style":"mono_ink","fit":"fill"}"#)
                .unwrap();
        assert_eq!(parsed.mode, PlaybackMode::RandomScenes);
        assert_eq!(parsed.style, RenderStyle::MonoInk);
        assert_eq!(parsed.fit, FitMode::Fill);
        assert_eq!(parsed.frame_rate, 15);
        let text = serde_json::to_string(&VideoSettings::default()).unwrap();
        for key in [
            "source",
            "slowed_percent",
            "scene_seconds",
            "gamma_percent",
            "frame_rate",
        ] {
            assert!(text.contains(&format!("\"{key}\"")), "{key}");
        }
    }

    #[test]
    fn rows_depend_on_the_mode() {
        let mut settings = VideoSettings::default();
        let live = settings.controls();
        assert!(row(&live, "slowed_percent").is_none());
        assert!(row(&live, "scene_seconds").is_none());
        assert!(row(&live, "shuffle").is_some());
        assert!(row(&live, "seed").is_none());
        settings.mode = PlaybackMode::Slowed;
        assert!(row(&settings.controls(), "slowed_percent").is_some());
        settings.mode = PlaybackMode::RandomScenes;
        let random = settings.controls();
        assert!(row(&random, "scene_seconds").is_some());
        assert!(row(&random, "shuffle").is_none());
        assert!(row(&random, "seed").is_some());
        settings.source = "https://example.com/a.mp4".to_owned();
        assert!(row(&settings.controls(), "recursive").is_none());
    }

    #[test]
    fn every_row_has_a_unique_id_label_and_help() {
        for mode in PlaybackMode::ALL {
            let settings = VideoSettings {
                mode: *mode,
                shuffle: true,
                ..VideoSettings::default()
            };
            let rows = settings.controls();
            let mut ids: Vec<_> = rows.iter().map(|row| row.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), rows.len());
            assert!(rows
                .iter()
                .all(|row| !row.label.is_empty() && !row.help.is_empty()));
        }
    }

    #[test]
    fn set_control_reports_change_and_validates_input() {
        let mut settings = VideoSettings::default();
        assert_eq!(
            settings.set_control("mode", ControlValue::Index(1)),
            Ok(true)
        );
        assert_eq!(settings.mode, PlaybackMode::Slowed);
        assert_eq!(
            settings.set_control("mode", ControlValue::Index(1)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("mode", ControlValue::Index(99)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("nonsense", ControlValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("slowed_percent", ControlValue::Number(1)),
            Ok(true)
        );
        assert_eq!(settings.slowed_percent, 5);
        assert_eq!(
            settings.set_control("contrast", ControlValue::Number(-500)),
            Ok(true)
        );
        assert_eq!(settings.contrast, -100);
        assert_eq!(
            settings.set_control("invert", ControlValue::Bool(true)),
            Ok(true)
        );
        assert_eq!(
            settings.set_control(
                "source",
                ControlValue::Text("https://a.example/v.mp4".into())
            ),
            Ok(true)
        );
        assert_eq!(
            settings.set_control("source", ControlValue::Text("/no/such/folder/x.mp4".into())),
            Ok(true)
        );
        assert_eq!(settings.source, "/no/such/folder/x.mp4");
        assert!(settings
            .set_control("source", ControlValue::Text("gopher://x".into()))
            .is_err());
    }

    #[test]
    fn plain_http_is_detected_for_the_warning() {
        let mut settings = VideoSettings::default();
        assert!(!settings.uses_plain_http());
        settings.source = "https://a/x; HTTP://b/y".to_owned();
        assert!(settings.uses_plain_http());
    }
    #[test]
    fn germination_series_is_selectable_persisted_and_preserves_custom_source() {
        let mut settings = VideoSettings {
            source: "/synthetic/retained.mp4".into(),
            ..VideoSettings::default()
        };
        assert_eq!(
            settings.set_control("series", ControlValue::Index(1)),
            Ok(true)
        );
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["series"], "germination");
        assert!(row(&settings.controls(), "source").is_none());
        assert!(row(&settings.controls(), "recursive").is_none());
        assert_eq!(
            settings.set_control("series", ControlValue::Index(0)),
            Ok(true)
        );
        assert_eq!(settings.source, "/synthetic/retained.mp4");
        assert!(row(&settings.controls(), "source").is_some());
        let old: VideoSettings = serde_json::from_str(r#"{"source":"x"}"#).unwrap();
        assert_eq!(serde_json::to_value(old).unwrap()["series"], "custom");
    }
    #[test]
    fn series_choice_preserves_every_other_saved_setting_exactly() {
        let original = VideoSettings {
            source: "  /synthetic/retained.mp4  ".into(),
            brightness: 1000,
            ..VideoSettings::default()
        };
        let mut settings = original.clone();
        assert_eq!(
            settings.set_control("series", ControlValue::Index(99)),
            Ok(false)
        );
        assert_eq!(settings, original);
        assert_eq!(
            settings.set_control("series", ControlValue::Index(1)),
            Ok(true)
        );
        let mut expected = original.clone();
        expected.series = VideoSeries::Germination;
        assert_eq!(settings, expected);
        assert_eq!(
            settings.set_control("series", ControlValue::Index(0)),
            Ok(true)
        );
        assert_eq!(settings, original);
    }
    #[test]
    fn series_source_survives_save_normalization_and_serde_reload() {
        let original = VideoSettings {
            source: "  /synthetic/retained.mp4  ".into(),
            ..VideoSettings::default()
        };
        let mut settings = original.clone();
        for selection in [1, 0] {
            assert_eq!(
                settings.set_control("series", ControlValue::Index(selection)),
                Ok(true)
            );
            settings = settings.normalized();
            let saved = serde_json::to_string(&settings).unwrap();
            settings = serde_json::from_str::<VideoSettings>(&saved)
                .unwrap()
                .normalized();
            assert_eq!(settings.source, original.source);
        }
        assert_eq!(settings, original);
    }
}
