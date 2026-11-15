//! Settings and settings rows of the Video scene.
//!
//! The keys are the property names of the manifest entry `video`. Colour (brightness, contrast,
//! gamma, invert, palette) belongs to the shared look of the host, so the scene has no tone rows
//! of its own; what stays is what is specific to video playback: where the clips come from, how
//! they are played, the render style, the fit and the decoder frame rate.
//!
//! A file or folder comes from a `picker` row: the host lends each non-empty path to the scene
//! as the read-only scope `picked/<n>`, `n` counting the picker rows in row order (`file` is 0,
//! `folder` 1, `folder_2` 2, `folder_3` 3, see [`PICKED_FILE`] and [`PICKED_FOLDERS`]).

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// The `picked/<n>` scope number of the `file` row.
pub const PICKED_FILE: usize = 0;
/// The `picked/<n>` scope numbers of the `folder`, `folder_2` and `folder_3` rows.
pub const PICKED_FOLDERS: [usize; 3] = [1, 2, 3];

macro_rules! choice_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const LABELS: &'static [&'static str] = &[$($label),+];

            pub fn index(self) -> usize {
                Self::ALL.iter().position(|item| *item == self).unwrap_or(0)
            }

            pub fn from_index(index: usize) -> Option<Self> {
                Self::ALL.get(index).copied()
            }
        }
    };
}

choice_enum!(
    /// Where the clips come from.
    VideoSource {
        Folders => "Folders",
        File => "Picked video file",
        Germination => "Germination series",
    }
);

choice_enum!(
    /// How the clips are played.
    PlaybackMode {
        Live => "Live",
        Slowed => "Slowed",
        RandomScenes => "Random scenes",
    }
);

choice_enum!(
    /// How a frame becomes dots.
    RenderStyle {
        Dithered => "Dithered",
        Colored => "Colored",
        MonoInk => "Mono ink",
    }
);

choice_enum!(
    /// How the picture is fitted to the screen.
    FitMode {
        Fit => "Fit (letterbox)",
        Fill => "Fill (crop)",
        Stretch => "Stretch",
    }
);

pub const SLOWED_PERCENT: (i32, i32, i32) = (5, 100, 5);
pub const SCENE_SECONDS: (i32, i32, i32) = (3, 120, 1);
pub const FRAME_RATE: (i32, i32, i32) = (6, 24, 1);
pub const DETAIL: (i32, i32, i32) = (0, 100, 5);
pub const SEED: (i32, i32, i32) = (0, 9999, 1);
/// Longest path text kept, in bytes.
const MAX_PATH_BYTES: usize = 1024;

/// Settings of the Video scene. Every field is clamped by `normalized`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    /// Where the clips come from. Default `folders`.
    pub source: VideoSource,
    /// Path of the picked video file (`source` = `file`).
    pub file: String,
    /// Folders of clips (`source` = `folders`).
    pub folder: String,
    pub folder_2: String,
    pub folder_3: String,
    /// Scan sub-folders of each folder. Default on.
    pub recursive: bool,
    pub mode: PlaybackMode,
    /// Speed in `Slowed` mode, 5..=100 percent of real time.
    pub slowed_percent: u16,
    /// Length of each excerpt in `RandomScenes` mode, 3..=120 seconds.
    pub scene_seconds: u16,
    /// Play the clips in random order.
    pub shuffle: bool,
    /// Keep replaying the first clip instead of moving to the next.
    pub repeat_one: bool,
    /// Random-number seed for shuffling and random scenes; 0 means a fresh random seed every
    /// time the scene starts.
    pub seed: u16,
    pub style: RenderStyle,
    pub fit: FitMode,
    /// Sharpening strength applied before dithering, 0..=100.
    pub detail: u16,
    /// Redraw rate cap, 6..=24 frames per second.
    pub frame_rate: u16,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            source: VideoSource::Folders,
            file: String::new(),
            folder: String::new(),
            folder_2: String::new(),
            folder_3: String::new(),
            recursive: true,
            mode: PlaybackMode::Live,
            slowed_percent: 50,
            scene_seconds: 20,
            shuffle: false,
            repeat_one: false,
            seed: 0,
            style: RenderStyle::Dithered,
            fit: FitMode::Fit,
            detail: 25,
            frame_rate: 15,
        }
    }
}

fn clamp_u16((min, max, _): (i32, i32, i32), value: u16) -> u16 {
    i32::from(value).clamp(min, max) as u16
}

fn clamp_text(text: &str) -> String {
    let text = text.trim();
    if text.len() <= MAX_PATH_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_PATH_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl VideoSettings {
    /// The non-empty folder texts with the `picked/<n>` scope number of each.
    pub fn folders(&self) -> Vec<(usize, &str)> {
        [&self.folder, &self.folder_2, &self.folder_3]
            .into_iter()
            .zip(PICKED_FOLDERS)
            .filter(|(text, _)| !text.is_empty())
            .map(|(text, scope)| (scope, text.as_str()))
            .collect()
    }

    fn set_text(field: &mut String, value: &ControlValue, label: &str) -> Result<bool, String> {
        let text = control::text(value).ok_or_else(|| format!("{label} expects text"))?;
        let text = clamp_text(text);
        if *field == text {
            return Ok(false);
        }
        *field = text;
        Ok(true)
    }

    fn set_bool(field: &mut bool, value: &ControlValue, label: &str) -> Result<bool, String> {
        let on = control::boolean(value).ok_or_else(|| format!("{label} expects on or off"))?;
        if *field == on {
            return Ok(false);
        }
        *field = on;
        Ok(true)
    }

    fn set_number(
        field: &mut u16,
        range: (i32, i32, i32),
        value: &ControlValue,
        label: &str,
    ) -> Result<bool, String> {
        let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
        let clamped = number.clamp(range.0, range.1) as u16;
        if *field == clamped {
            return Ok(false);
        }
        *field = clamped;
        Ok(true)
    }

    fn set_choice<T: Copy + PartialEq>(
        field: &mut T,
        value: &ControlValue,
        label: &str,
        from_index: impl Fn(usize) -> Option<T>,
    ) -> Result<bool, String> {
        let index = control::index(value).ok_or_else(|| format!("{label} expects an option"))?;
        let chosen = from_index(index).ok_or_else(|| format!("{label}: unknown option"))?;
        if *field == chosen {
            return Ok(false);
        }
        *field = chosen;
        Ok(true)
    }
}

impl SceneSettings for VideoSettings {
    fn normalized(&self) -> Self {
        Self {
            file: clamp_text(&self.file),
            folder: clamp_text(&self.folder),
            folder_2: clamp_text(&self.folder_2),
            folder_3: clamp_text(&self.folder_3),
            slowed_percent: clamp_u16(SLOWED_PERCENT, self.slowed_percent),
            scene_seconds: clamp_u16(SCENE_SECONDS, self.scene_seconds),
            seed: clamp_u16(SEED, self.seed),
            detail: clamp_u16(DETAIL, self.detail),
            frame_rate: clamp_u16(FRAME_RATE, self.frame_rate),
            ..self.clone()
        }
    }

    /// Every row. The host shows only the rows whose `visible_when` matches the chosen source
    /// or mode (the manifest entry carries the rule), so the rows here and the manifest
    /// properties are the same set.
    fn controls(&self) -> Vec<Control> {
        vec![
            Control::choice(
                "source",
                "Source",
                self.source.index(),
                VideoSource::LABELS,
                "Play the videos of folders, one picked video file, or the Germination series of plant-growth clips downloaded from Wikimedia Commons.",
            ),
            Control::text(
                "file",
                "Video file",
                &self.file,
                "video file",
                "The video to play: mp4, mkv, avi, mov, webm, m4v, mpg, mpeg, wmv, flv or ts.",
            ),
            Control::text(
                "folder",
                "Folder",
                &self.folder,
                "folder",
                "A folder of videos.",
            ),
            Control::text(
                "folder_2",
                "Second folder",
                &self.folder_2,
                "folder",
                "A second folder of videos, optional.",
            ),
            Control::text(
                "folder_3",
                "Third folder",
                &self.folder_3,
                "folder",
                "A third folder of videos, optional.",
            ),
            Control::toggle(
                "recursive",
                "Include sub-folders",
                self.recursive,
                "Also play videos found in the folders below each folder.",
            ),
            Control::choice(
                "mode",
                "Playback",
                self.mode.index(),
                PlaybackMode::LABELS,
                "Live plays at normal speed, Slowed stretches time, Random scenes jumps between short excerpts.",
            ),
            Control::slider(
                "slowed_percent",
                "Speed",
                i32::from(self.slowed_percent),
                SLOWED_PERCENT,
                "%",
                "Playback speed relative to real time. Lower is slower and dreamier.",
            ),
            Control::slider(
                "scene_seconds",
                "Scene length",
                i32::from(self.scene_seconds),
                SCENE_SECONDS,
                " s",
                "How long each random excerpt plays before another one is picked.",
            ),
            Control::toggle(
                "shuffle",
                "Shuffle",
                self.shuffle,
                "Play the clips in random order.",
            ),
            Control::toggle(
                "repeat_one",
                "Repeat one clip",
                self.repeat_one,
                "Keep replaying the first clip instead of moving on to the next one.",
            ),
            Control::slider(
                "seed",
                "Random seed",
                i32::from(self.seed),
                SEED,
                "",
                "0 picks something new every time; any other number replays the same sequence.",
            ),
            Control::choice(
                "style",
                "Style",
                self.style.index(),
                RenderStyle::LABELS,
                "Dithered uses your dot palette, Colored tints each cell with the video colors, Mono ink draws edges like a pen sketch.",
            ),
            Control::choice(
                "fit",
                "Fit",
                self.fit.index(),
                FitMode::LABELS,
                "How the picture is mapped to the screen: letterboxed, cropped to fill, or stretched.",
            ),
            Control::slider(
                "detail",
                "Detail",
                i32::from(self.detail),
                DETAIL,
                "",
                "Sharpens the picture before dithering so small features survive the low resolution.",
            ),
            Control::slider(
                "frame_rate",
                "Frame rate",
                i32::from(self.frame_rate),
                FRAME_RATE,
                " fps",
                "Redraw rate. Lower values use less CPU.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "source" => {
                Self::set_choice(&mut self.source, &value, "Source", VideoSource::from_index)
            }
            "file" => Self::set_text(&mut self.file, &value, "Video file"),
            "folder" => Self::set_text(&mut self.folder, &value, "Folder"),
            "folder_2" => Self::set_text(&mut self.folder_2, &value, "Second folder"),
            "folder_3" => Self::set_text(&mut self.folder_3, &value, "Third folder"),
            "recursive" => Self::set_bool(&mut self.recursive, &value, "Include sub-folders"),
            "mode" => {
                Self::set_choice(&mut self.mode, &value, "Playback", PlaybackMode::from_index)
            }
            "slowed_percent" => {
                Self::set_number(&mut self.slowed_percent, SLOWED_PERCENT, &value, "Speed")
            }
            "scene_seconds" => Self::set_number(
                &mut self.scene_seconds,
                SCENE_SECONDS,
                &value,
                "Scene length",
            ),
            "shuffle" => Self::set_bool(&mut self.shuffle, &value, "Shuffle"),
            "repeat_one" => Self::set_bool(&mut self.repeat_one, &value, "Repeat one clip"),
            "seed" => Self::set_number(&mut self.seed, SEED, &value, "Random seed"),
            "style" => Self::set_choice(&mut self.style, &value, "Style", RenderStyle::from_index),
            "fit" => Self::set_choice(&mut self.fit, &value, "Fit", FitMode::from_index),
            "detail" => Self::set_number(&mut self.detail, DETAIL, &value, "Detail"),
            "frame_rate" => {
                Self::set_number(&mut self.frame_rate, FRAME_RATE, &value, "Frame rate")
            }
            _ => Ok(false),
        }
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
    fn normalization_clamps_numeric_fields_and_trims_paths() {
        let wild = VideoSettings {
            slowed_percent: 0,
            scene_seconds: 9999,
            detail: 400,
            frame_rate: 60,
            seed: 20_000,
            file: "  /x/clip.mp4  ".to_owned(),
            ..VideoSettings::default()
        };
        let clean = wild.normalized();
        assert_eq!(clean.slowed_percent, 5);
        assert_eq!(clean.scene_seconds, 120);
        assert_eq!(clean.detail, 100);
        assert_eq!(clean.frame_rate, 24);
        assert_eq!(clean.seed, 9999);
        assert_eq!(clean.file, "/x/clip.mp4");
    }

    #[test]
    fn serde_defaults_fill_missing_keys_and_use_snake_case_names() {
        let parsed: VideoSettings = serde_json::from_str(
            r#"{"mode":"random_scenes","style":"mono_ink","fit":"fill","source":"germination"}"#,
        )
        .unwrap();
        assert_eq!(parsed.mode, PlaybackMode::RandomScenes);
        assert_eq!(parsed.style, RenderStyle::MonoInk);
        assert_eq!(parsed.fit, FitMode::Fill);
        assert_eq!(parsed.source, VideoSource::Germination);
        assert_eq!(parsed.frame_rate, 15);
        let text = serde_json::to_string(&VideoSettings::default()).unwrap();
        for key in ["source", "slowed_percent", "scene_seconds", "frame_rate"] {
            assert!(text.contains(&format!("\"{key}\"")), "{key}");
        }
    }

    #[test]
    fn folders_lists_the_non_empty_rows_with_their_picked_scopes() {
        let settings = VideoSettings {
            folder: "/a".into(),
            folder_3: "/c".into(),
            ..VideoSettings::default()
        };
        assert_eq!(settings.folders(), vec![(1, "/a"), (3, "/c")]);
    }

    #[test]
    fn every_row_has_a_unique_id_label_and_help() {
        let rows = VideoSettings::default().controls();
        let mut ids: Vec<_> = rows.iter().map(|row| row.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), rows.len());
        assert!(rows
            .iter()
            .all(|row| !row.label.is_empty() && !row.help.is_empty()));
        assert!(row(&rows, "source").is_some());
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
        assert!(settings
            .set_control("mode", ControlValue::Index(99))
            .is_err());
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
            settings.set_control("file", ControlValue::Text(" /v/a.mp4 ".into())),
            Ok(true)
        );
        assert_eq!(settings.file, "/v/a.mp4");
        assert_eq!(
            settings.set_control("source", ControlValue::Index(2)),
            Ok(true)
        );
        assert_eq!(settings.source, VideoSource::Germination);
    }
}
