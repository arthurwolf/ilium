//! Settings and settings rows of the Images scene.
//!
//! The keys are the property names of the manifest entry `images`. Colour (brightness,
//! contrast, hue, saturation, palette, intensity) belongs to the shared look of the host, so the
//! scene has no colour rows of its own; what stays is what is specific to a slideshow: where
//! the pictures come from, the order, the timing, the Ken-Burns motion and the fit.
//!
//! Folders and files come from `picker` rows: the host lends each non-empty path to the scene as
//! the read-only scope `picked/<n>`, `n` counting the picker rows in row order (`file` is 0,
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
    /// What the scene shows.
    ImagesMode {
        Builtin => "Built-in picture",
        File => "Picked image file",
        Folders => "Folder slideshow",
    }
);

choice_enum!(
    /// Which built-in picture to show.
    SingleSource {
        NightCafe => "NightCafe dreamscape",
        MountainLake => "Ubuntu-like mountain lake",
        GoldenDesktop => "Ubuntu-like golden desktop",
        UnsplashDesk => "Unsplash desk",
    }
);

choice_enum!(
    /// Slideshow order.
    SlideOrder {
        Sequential => "Sequential",
        Shuffle => "Shuffle",
    }
);

choice_enum!(
    /// Ken-Burns motion applied to every image.
    Motion {
        ZoomIn => "Slow zoom in",
        ZoomOut => "Slow zoom out",
        PanHorizontal => "Pan left-right",
        PanVertical => "Pan up-down",
        Drift => "Random drift",
        ZoomPan => "Zoom + pan",
        None => "None",
    }
);

choice_enum!(
    /// Easing of the motion progress within one image.
    Easing {
        EaseInOut => "Smooth",
        Linear => "Linear",
        EaseOut => "Ease out",
    }
);

choice_enum!(
    /// How an image is fitted to the screen.
    FitMode {
        Fill => "Fill (crop)",
        Fit => "Fit (borders)",
        Stretch => "Stretch",
    }
);

/// The pictures the single-image row can fetch: label, URL. Each host is a grant of the
/// package manifest (`http:<host>`).
pub const BUILTIN_IMAGES: [(&str, &str); 4] = [
    (
        "NightCafe dreamscape",
        "https://creator.nightcafe.studio/jobs/blv0xCihNp7SICk9UBbZ/blv0xCihNp7SICk9UBbZ--1--qyiww.jpg",
    ),
    (
        "Ubuntu-like mountain lake",
        "https://images.unsplash.com/photo-1470770841072-f978cf4d019e?w=2560&q=80",
    ),
    (
        "Ubuntu-like golden desktop",
        "https://images.unsplash.com/photo-1500534623283-312aade485b7?w=2560&q=80",
    ),
    (
        "Unsplash desk",
        "https://images.unsplash.com/photo-1497366754035-f200968a6e72?w=2560&q=80",
    ),
];

impl SingleSource {
    /// The label and the https URL of the built-in picture.
    pub fn picture(self) -> (&'static str, &'static str) {
        match self {
            Self::NightCafe => BUILTIN_IMAGES[0],
            Self::MountainLake => BUILTIN_IMAGES[1],
            Self::GoldenDesktop => BUILTIN_IMAGES[2],
            Self::UnsplashDesk => BUILTIN_IMAGES[3],
        }
    }
}

/// Settings of the Images scene. Every field is clamped by `normalized`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImagesSettings {
    /// What to show. Default `builtin`.
    pub mode: ImagesMode,
    /// The built-in picture, used when `mode` is `builtin`. Default NightCafe.
    pub source: SingleSource,
    /// Path of the picked image file (`mode` = `file`).
    pub file: String,
    /// Folders of the slideshow; each is read recursively when `recursive` is on.
    pub folder: String,
    pub folder_2: String,
    pub folder_3: String,
    /// Descend into sub-folders. Default on.
    pub recursive: bool,
    /// Slideshow order. Default `sequential`.
    pub order: SlideOrder,
    /// Shuffle seed 0..=9999; 0 picks a different order at every start.
    pub shuffle_seed: u16,
    /// Seconds each image stays on screen and the length of one Ken-Burns move, 3..=1800,
    /// default 30.
    pub display_seconds: u16,
    /// Cross-fade length in seconds, 0..=30, default 3 (capped at half the display time).
    pub transition_seconds: u16,
    /// Ken-Burns motion. Default `zoom_in`.
    pub motion: Motion,
    /// Zoom amount / pan range in percent of the image, 0..=50, default 12.
    pub motion_strength_percent: u16,
    /// Easing of the motion. Default `ease_in_out`.
    pub easing: Easing,
    /// Fit mode. Default `fill`.
    pub fit: FitMode,
}

impl Default for ImagesSettings {
    fn default() -> Self {
        Self {
            mode: ImagesMode::Builtin,
            source: SingleSource::NightCafe,
            file: String::new(),
            folder: String::new(),
            folder_2: String::new(),
            folder_3: String::new(),
            recursive: true,
            order: SlideOrder::Sequential,
            shuffle_seed: 0,
            display_seconds: 30,
            transition_seconds: 3,
            motion: Motion::ZoomIn,
            motion_strength_percent: 12,
            easing: Easing::EaseInOut,
            fit: FitMode::Fill,
        }
    }
}

const SHUFFLE_SEED: (i32, i32) = (0, 9999);
const DISPLAY_SECONDS: (i32, i32) = (3, 1800);
const TRANSITION_SECONDS: (i32, i32) = (0, 30);
const MOTION_STRENGTH: (i32, i32) = (0, 50);
/// Longest path text kept, in bytes.
const MAX_PATH_BYTES: usize = 1024;

fn clamp_u16((min, max): (i32, i32), value: u16) -> u16 {
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

impl ImagesSettings {
    /// True when the scene can show more than one image.
    pub fn is_slideshow_mode(&self) -> bool {
        self.mode == ImagesMode::Folders
    }

    /// The non-empty folder texts with the `picked/<n>` scope number of each.
    pub fn folders(&self) -> Vec<(usize, &str)> {
        [&self.folder, &self.folder_2, &self.folder_3]
            .into_iter()
            .zip(PICKED_FOLDERS)
            .filter(|(text, _)| !text.is_empty())
            .map(|(text, scope)| (scope, text.as_str()))
            .collect()
    }

    /// Cross-fade length actually used: none for a single image, at most half the display time.
    pub fn effective_transition_seconds(&self) -> u16 {
        if self.is_slideshow_mode() {
            self.transition_seconds.min(self.display_seconds / 2)
        } else {
            0
        }
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

    fn set_number(
        field: &mut u16,
        range: (i32, i32),
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

impl SceneSettings for ImagesSettings {
    fn normalized(&self) -> Self {
        Self {
            file: clamp_text(&self.file),
            folder: clamp_text(&self.folder),
            folder_2: clamp_text(&self.folder_2),
            folder_3: clamp_text(&self.folder_3),
            shuffle_seed: clamp_u16(SHUFFLE_SEED, self.shuffle_seed),
            display_seconds: clamp_u16(DISPLAY_SECONDS, self.display_seconds),
            transition_seconds: clamp_u16(TRANSITION_SECONDS, self.transition_seconds),
            motion_strength_percent: clamp_u16(MOTION_STRENGTH, self.motion_strength_percent),
            ..self.clone()
        }
    }

    /// Every row. The host shows only the rows whose `visible_when` matches the chosen mode
    /// (the manifest entry carries the rule), so the rows here and the manifest properties are
    /// the same set.
    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "mode",
                "Show",
                self.mode.index(),
                ImagesMode::LABELS,
                "A built-in picture, an image file you pick, or every picture found in the chosen folders as a slideshow.",
            ),
            Control::choice(
                "source",
                "Picture",
                self.source.index(),
                SingleSource::LABELS,
                "One of the built-in pictures, downloaded when the scene starts.",
            ),
            Control::text(
                "file",
                "Image file",
                &self.file,
                "image file",
                "The picture to show: png, jpg, gif, bmp or webp.",
            ),
            Control::text(
                "folder",
                "Folder",
                &self.folder,
                "folder",
                "A folder of pictures.",
            ),
            Control::text(
                "folder_2",
                "Second folder",
                &self.folder_2,
                "folder",
                "A second folder of pictures, optional.",
            ),
            Control::text(
                "folder_3",
                "Third folder",
                &self.folder_3,
                "folder",
                "A third folder of pictures, optional.",
            ),
            Control::toggle(
                "recursive",
                "Include sub-folders",
                self.recursive,
                "Also show pictures inside the sub-folders of each folder.",
            ),
            Control::choice(
                "order",
                "Order",
                self.order.index(),
                SlideOrder::LABELS,
                "Show the pictures in name order or shuffled.",
            ),
            Control::slider(
                "shuffle_seed",
                "Shuffle seed",
                i32::from(self.shuffle_seed),
                (SHUFFLE_SEED.0, SHUFFLE_SEED.1, 1),
                "",
                "0 picks a different order every time; any other number repeats one order.",
            ),
            Control::slider(
                "transition_seconds",
                "Cross-fade",
                i32::from(self.transition_seconds),
                (TRANSITION_SECONDS.0, TRANSITION_SECONDS.1, 1),
                " s",
                "Length of the fade between two pictures; at most half the display time.",
            ),
        ];
        rows.push(Control::slider(
            "display_seconds",
            "Display time",
            i32::from(self.display_seconds),
            (DISPLAY_SECONDS.0, DISPLAY_SECONDS.1, 1),
            " s",
            "How long a picture stays and how long one Ken-Burns move takes.",
        ));
        rows.push(Control::choice(
            "motion",
            "Motion",
            self.motion.index(),
            Motion::LABELS,
            "Slow zoom or pan applied to every picture.",
        ));
        rows.push(Control::slider(
            "motion_strength_percent",
            "Motion strength",
            i32::from(self.motion_strength_percent),
            (MOTION_STRENGTH.0, MOTION_STRENGTH.1, 1),
            "%",
            "Zoom amount or pan range, in percent of the picture.",
        ));
        rows.push(Control::choice(
            "easing",
            "Easing",
            self.easing.index(),
            Easing::LABELS,
            "How the motion speeds up and slows down within one picture.",
        ));
        rows.push(Control::choice(
            "fit",
            "Fit",
            self.fit.index(),
            FitMode::LABELS,
            "Fill crops the picture to the screen, Fit keeps all of it with borders, Stretch distorts it.",
        ));
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "mode" => Self::set_choice(&mut self.mode, &value, "Show", ImagesMode::from_index),
            "source" => Self::set_choice(
                &mut self.source,
                &value,
                "Picture",
                SingleSource::from_index,
            ),
            "file" => Self::set_text(&mut self.file, &value, "Image file"),
            "folder" => Self::set_text(&mut self.folder, &value, "Folder"),
            "folder_2" => Self::set_text(&mut self.folder_2, &value, "Second folder"),
            "folder_3" => Self::set_text(&mut self.folder_3, &value, "Third folder"),
            "recursive" => {
                let on = control::boolean(&value)
                    .ok_or_else(|| "Include sub-folders expects on or off".to_owned())?;
                let changed = self.recursive != on;
                self.recursive = on;
                Ok(changed)
            }
            "order" => Self::set_choice(&mut self.order, &value, "Order", SlideOrder::from_index),
            "shuffle_seed" => {
                Self::set_number(&mut self.shuffle_seed, SHUFFLE_SEED, &value, "Shuffle seed")
            }
            "display_seconds" => Self::set_number(
                &mut self.display_seconds,
                DISPLAY_SECONDS,
                &value,
                "Display time",
            ),
            "transition_seconds" => Self::set_number(
                &mut self.transition_seconds,
                TRANSITION_SECONDS,
                &value,
                "Cross-fade",
            ),
            "motion" => Self::set_choice(&mut self.motion, &value, "Motion", Motion::from_index),
            "motion_strength_percent" => Self::set_number(
                &mut self.motion_strength_percent,
                MOTION_STRENGTH,
                &value,
                "Motion strength",
            ),
            "easing" => Self::set_choice(&mut self.easing, &value, "Easing", Easing::from_index),
            "fit" => Self::set_choice(&mut self.fit, &value, "Fit", FitMode::from_index),
            _ => Ok(false),
        }
    }
}
