//! Persisted settings and the Settings-panel rows of the Images scene.
//!
//! The first block of fields (`source` .. `opacity_percent`) keeps the names
//! and semantics of the former single "static image" background so existing
//! project YAML keeps loading. Everything else is new and defaults to a quiet,
//! slowly moving single image.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const NIGHTCAFE_IMAGE_URL: &str =
    "https://creator.nightcafe.studio/jobs/blv0xCihNp7SICk9UBbZ/blv0xCihNp7SICk9UBbZ--1--qyiww.jpg";

/// Where a single image comes from (externally tagged, as before:
/// `source: { Builtin: 0 }`, `{ Local: /path }`, `{ Url: "https://..." }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageSource {
    Builtin(usize),
    Local(PathBuf),
    Url(String),
}

impl Default for ImageSource {
    fn default() -> Self {
        Self::Builtin(0)
    }
}

impl ImageSource {
    pub const BUILTIN: [(&'static str, &'static str); 4] = [
        ("NightCafe dreamscape", NIGHTCAFE_IMAGE_URL),
        (
            "Ubuntu-like mountain lake",
            "https://images.unsplash.com/photo-1470770841072-f978cf4d019e",
        ),
        (
            "Ubuntu-like golden desktop",
            "https://images.unsplash.com/photo-1500534623283-312aade485b7",
        ),
        (
            "Unsplash desk",
            "https://images.unsplash.com/photo-1497366754035-f200968a6e72",
        ),
    ];

    pub fn label(&self) -> String {
        match self {
            Self::Builtin(index) => Self::BUILTIN
                .get(*index)
                .map(|(label, _)| (*label).to_owned())
                .unwrap_or_else(|| "Built-in image".to_owned()),
            Self::Local(path) => format!("File: {}", path.display()),
            Self::Url(url) => format!("URL: {url}"),
        }
    }

    /// The https URL of a built-in or URL source.
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::Builtin(index) => Self::BUILTIN.get(*index).map(|(_, url)| *url),
            Self::Url(url) => Some(url),
            Self::Local(_) => None,
        }
    }

    fn kind_index(&self) -> usize {
        match self {
            Self::Builtin(_) => 0,
            Self::Local(_) => 1,
            Self::Url(_) => 2,
        }
    }
}

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

            pub fn label(self) -> &'static str {
                Self::LABELS[self.index()]
            }

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
        Single => "Single image",
        Folders => "Folders / globs",
        UrlList => "URL list",
    }
);

choice_enum!(
    /// Colour treatment presets (same numbers as the former static image).
    ImagePreset {
        Dimmed => "Dimmed",
        Vivid => "Vivid",
        Monochrome => "Monochrome",
        Cool => "Cool",
        Warm => "Warm",
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

/// Settings of the Images scene. Every field is clamped by `normalized`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImagesSettings {
    /// Single image source, used when `mode` is `single`. Default built-in 0.
    pub source: ImageSource,
    /// Colour preset. Default `dimmed`.
    pub preset: ImagePreset,
    /// 0..=200, default 72.
    pub brightness_percent: u16,
    /// 0..=200, default 82.
    pub contrast_percent: u16,
    /// 0..=360, default 180 (neutral).
    pub hue_degrees: u16,
    /// 0..=200, default 82.
    pub saturation_percent: u16,
    /// 0..=100, default 48.
    pub intensity_percent: u16,
    /// 0..=100, default 58.
    pub opacity_percent: u16,

    /// What to show. Default `single`.
    pub mode: ImagesMode,
    /// Directories and glob patterns separated by `;` (`~` is expanded), used
    /// in `folders` mode. Globs support `*`, `?`, `[a-z]` and `**`.
    pub folders: String,
    /// Descend into sub-directories of plain directory entries. Default on.
    pub recursive: bool,
    /// `;`-separated https image URLs (or a https `.txt` file with one image
    /// URL per line), used in `url_list` mode.
    pub urls: String,
    /// Seconds each image stays on screen and the length of one Ken-Burns
    /// move, 3..=1800, default 30.
    pub display_seconds: u16,
    /// Slideshow order. Default `sequential`.
    pub order: SlideOrder,
    /// Shuffle seed 0..=9999; 0 picks a different order at every start.
    pub shuffle_seed: u16,
    /// Cross-fade length in seconds, 0..=30, default 3 (capped at half the
    /// display time).
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
            source: ImageSource::default(),
            preset: ImagePreset::default(),
            brightness_percent: 72,
            contrast_percent: 82,
            hue_degrees: 180,
            saturation_percent: 82,
            intensity_percent: 48,
            opacity_percent: 58,
            mode: ImagesMode::Single,
            folders: String::new(),
            recursive: true,
            urls: String::new(),
            display_seconds: 30,
            order: SlideOrder::Sequential,
            shuffle_seed: 0,
            transition_seconds: 3,
            motion: Motion::ZoomIn,
            motion_strength_percent: 12,
            easing: Easing::EaseInOut,
            fit: FitMode::Fill,
        }
    }
}

impl ImagesSettings {
    /// True when the scene can show more than one image.
    pub fn is_slideshow_mode(&self) -> bool {
        self.mode != ImagesMode::Single
    }

    pub fn source_label(&self) -> String {
        match self.mode {
            ImagesMode::Single => self.source.label(),
            ImagesMode::Folders => format!("Folders: {}", self.folders),
            ImagesMode::UrlList => format!("URLs: {}", self.urls),
        }
    }
}

/// Expand a leading `~/` with the user's home directory.
pub fn expand_home(text: &str) -> PathBuf {
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(dirs) = directories::BaseDirs::new() {
            return dirs.home_dir().join(rest);
        }
    }
    PathBuf::from(text)
}

fn has_glob_chars(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

fn validate_file(text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter the path of an image file".to_owned());
    }
    let path = expand_home(text);
    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }
    if !super::discover::is_image_path(&path) {
        return Err(format!(
            "Not a supported image type (use {})",
            super::discover::IMAGE_EXTENSIONS.join(" ")
        ));
    }
    Ok(())
}

fn validate_https(text: &str) -> Result<String, String> {
    let text = text.trim();
    if !text.starts_with("https://") || text.len() < 12 || text.contains(char::is_whitespace) {
        return Err(format!("Only https:// URLs are supported: {text}"));
    }
    if text.len() > 2000 {
        return Err("URL is too long".to_owned());
    }
    Ok(text.to_owned())
}

fn validate_folders(text: &str) -> Result<String, String> {
    let items = super::discover::split_list(text);
    if items.is_empty() {
        return Err("Enter one or more folders or globs separated by ;".to_owned());
    }
    for item in &items {
        let expanded = expand_home(item);
        if has_glob_chars(item) {
            let base = super::discover::glob_base(&expanded);
            if !base.is_dir() {
                return Err(format!("Folder not found: {}", base.display()));
            }
        } else if !expanded.is_dir() {
            return Err(format!("Folder not found: {}", expanded.display()));
        }
    }
    Ok(items.join(";"))
}

fn validate_urls(text: &str) -> Result<String, String> {
    let items = super::discover::split_list(text);
    if items.is_empty() {
        return Err("Enter one or more https:// image URLs separated by ;".to_owned());
    }
    let mut checked = Vec::new();
    for item in items {
        checked.push(validate_https(item)?);
    }
    Ok(checked.join(";"))
}

fn clamp_u16(value: i32, min: u16, max: u16) -> u16 {
    value.clamp(i32::from(min), i32::from(max)) as u16
}

fn set_number(field: &mut u16, value: &ControlValue, min: u16, max: u16) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| "Expected a number".to_owned())?;
    let next = clamp_u16(number, min, max);
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

fn set_choice<T: Copy + PartialEq>(
    field: &mut T,
    value: &ControlValue,
    from_index: fn(usize) -> Option<T>,
) -> Result<bool, String> {
    let index = control::index(value).ok_or_else(|| "Expected a choice".to_owned())?;
    let next = from_index(index).ok_or_else(|| "Unknown choice".to_owned())?;
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

fn set_text(field: &mut String, next: String) -> Result<bool, String> {
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

impl SceneSettings for ImagesSettings {
    fn normalized(&self) -> Self {
        let source = match &self.source {
            ImageSource::Builtin(index) => {
                ImageSource::Builtin((*index).min(ImageSource::BUILTIN.len() - 1))
            }
            other => other.clone(),
        };
        Self {
            source,
            brightness_percent: self.brightness_percent.min(200),
            contrast_percent: self.contrast_percent.min(200),
            hue_degrees: self.hue_degrees.min(360),
            saturation_percent: self.saturation_percent.min(200),
            intensity_percent: self.intensity_percent.min(100),
            opacity_percent: self.opacity_percent.min(100),
            folders: self.folders.trim().to_owned(),
            urls: self.urls.trim().to_owned(),
            display_seconds: self.display_seconds.clamp(3, 1800),
            shuffle_seed: self.shuffle_seed.min(9999),
            transition_seconds: self.transition_seconds.min(30),
            motion_strength_percent: self.motion_strength_percent.min(50),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice(
            "mode",
            "Source",
            self.mode.index(),
            ImagesMode::LABELS,
            "One image, every image in some folders, or a list of https image URLs shown as a slideshow.",
        )];
        match self.mode {
            ImagesMode::Single => {
                rows.push(Control::choice(
                    "source_kind",
                    "Image from",
                    self.source.kind_index(),
                    &["Built-in list", "Local file", "URL"],
                    "Pick a built-in picture, a file on this machine, or an https URL (downloaded once and cached).",
                ));
                match &self.source {
                    ImageSource::Builtin(index) => {
                        let labels: Vec<&'static str> =
                            ImageSource::BUILTIN.iter().map(|(label, _)| *label).collect();
                        rows.push(Control::choice(
                            "builtin_image",
                            "Built-in image",
                            *index,
                            &labels,
                            "One of the bundled picture links.",
                        ));
                    }
                    ImageSource::Local(path) => rows.push(Control::text(
                        "file",
                        "Image file",
                        &path.to_string_lossy(),
                        "path to a png/jpg/gif/bmp/webp",
                        "Absolute path of an image file; ~/ is expanded.",
                    )),
                    ImageSource::Url(url) => rows.push(Control::text(
                        "url",
                        "Image URL",
                        url,
                        "https://...",
                        "An https:// link to an image. Downloaded in the background and cached.",
                    )),
                }
            }
            ImagesMode::Folders => {
                rows.push(Control::text(
                    "folders",
                    "Folders",
                    &self.folders,
                    "~/Pictures;/data/*/wallpapers/**/*.jpg",
                    "Directories or glob patterns separated by ;. Glob: * ? [a-z] and ** for any depth. Hidden entries are skipped.",
                ));
                rows.push(Control::toggle(
                    "recursive",
                    "Include subfolders",
                    self.recursive,
                    "Also scan folders below each plain directory entry (glob patterns choose their own depth with **).",
                ));
            }
            ImagesMode::UrlList => rows.push(Control::text(
                "urls",
                "Image URLs",
                &self.urls,
                "https://a/1.jpg;https://a/2.jpg",
                "https:// image links separated by ;, or one link to a .txt file with one image URL per line.",
            )),
        }
        if self.is_slideshow_mode() {
            rows.push(Control::choice(
                "order",
                "Order",
                self.order.index(),
                SlideOrder::LABELS,
                "Show the images in name order or in a shuffled order.",
            ));
            if self.order == SlideOrder::Shuffle {
                rows.push(Control::slider(
                    "shuffle_seed",
                    "Shuffle seed",
                    i32::from(self.shuffle_seed),
                    (0, 9999, 1),
                    "",
                    "The same seed always gives the same order. 0 shuffles differently at every start.",
                ));
            }
        }
        if self.is_slideshow_mode() || self.motion != Motion::None {
            rows.push(Control::slider(
                "display_seconds",
                "Seconds per image",
                i32::from(self.display_seconds),
                (3, 1800, 5),
                " s",
                "How long each image stays; also the length of one slow pan/zoom move.",
            ));
        }
        if self.is_slideshow_mode() {
            rows.push(Control::slider(
                "transition_seconds",
                "Cross-fade",
                i32::from(self.transition_seconds),
                (0, 30, 1),
                " s",
                "Length of the smooth blend between two images (at most half the display time).",
            ));
        }
        rows.push(Control::choice(
            "motion",
            "Motion",
            self.motion.index(),
            Motion::LABELS,
            "Slow Ken-Burns pan and zoom so the picture is not frozen. None keeps it perfectly still and saves redraws.",
        ));
        if self.motion != Motion::None {
            rows.push(Control::slider(
                "motion_strength",
                "Motion strength",
                i32::from(self.motion_strength_percent),
                (0, 50, 1),
                "%",
                "How far the view zooms or pans, as a percentage of the image.",
            ));
            rows.push(Control::choice(
                "easing",
                "Easing",
                self.easing.index(),
                Easing::LABELS,
                "Speed profile of one move: smooth starts and ends gently.",
            ));
        }
        rows.push(Control::choice(
            "fit",
            "Fit",
            self.fit.index(),
            FitMode::LABELS,
            "Fill crops the picture to cover the screen, Fit shows all of it with dark borders, Stretch distorts it to the screen.",
        ));
        rows.push(Control::choice(
            "preset",
            "Preset",
            self.preset.index(),
            ImagePreset::LABELS,
            "Base colour treatment; the sliders below scale it.",
        ));
        let sliders: [(
            &'static str,
            &'static str,
            u16,
            i32,
            &'static str,
            &'static str,
        ); 6] = [
            (
                "brightness",
                "Brightness",
                self.brightness_percent,
                200,
                "%",
                "Overall lightness of the picture.",
            ),
            (
                "contrast",
                "Contrast",
                self.contrast_percent,
                200,
                "%",
                "Difference between light and dark areas.",
            ),
            (
                "saturation",
                "Saturation",
                self.saturation_percent,
                200,
                "%",
                "Colour strength; 0 is grey.",
            ),
            (
                "hue",
                "Hue tint",
                self.hue_degrees,
                360,
                "deg",
                "Shifts the tint between cool (below 180) and warm (above 180); 180 is neutral.",
            ),
            (
                "intensity",
                "Intensity",
                self.intensity_percent,
                100,
                "%",
                "How many dots light up: higher lights darker areas too.",
            ),
            (
                "opacity",
                "Opacity",
                self.opacity_percent,
                100,
                "%",
                "How strongly the picture shows over the terminal background.",
            ),
        ];
        for (id, label, value, max, unit, help) in sliders {
            rows.push(Control::slider(
                id,
                label,
                i32::from(value),
                (0, max, 1),
                unit,
                help,
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "mode" => set_choice(&mut self.mode, &value, ImagesMode::from_index),
            "source_kind" => {
                let index = control::index(&value).ok_or_else(|| "Expected a choice".to_owned())?;
                if index > 2 {
                    return Err("Unknown choice".to_owned());
                }
                if index == self.source.kind_index() {
                    return Ok(false);
                }
                self.source = match index {
                    0 => ImageSource::Builtin(0),
                    1 => ImageSource::Local(PathBuf::new()),
                    _ => ImageSource::Url(String::new()),
                };
                Ok(true)
            }
            "builtin_image" => {
                let index = control::index(&value).ok_or_else(|| "Expected a choice".to_owned())?;
                if index >= ImageSource::BUILTIN.len() {
                    return Err("Unknown built-in image".to_owned());
                }
                let next = ImageSource::Builtin(index);
                let changed = self.source != next;
                self.source = next;
                Ok(changed)
            }
            "file" => {
                let text = control::text(&value).ok_or_else(|| "Expected text".to_owned())?;
                validate_file(text)?;
                let next = ImageSource::Local(PathBuf::from(text.trim()));
                let changed = self.source != next;
                self.source = next;
                Ok(changed)
            }
            "url" => {
                let text = control::text(&value).ok_or_else(|| "Expected text".to_owned())?;
                let next = ImageSource::Url(validate_https(text)?);
                let changed = self.source != next;
                self.source = next;
                Ok(changed)
            }
            "folders" => {
                let text = control::text(&value).ok_or_else(|| "Expected text".to_owned())?;
                let next = validate_folders(text)?;
                set_text(&mut self.folders, next)
            }
            "urls" => {
                let text = control::text(&value).ok_or_else(|| "Expected text".to_owned())?;
                let next = validate_urls(text)?;
                set_text(&mut self.urls, next)
            }
            "recursive" => {
                let on = control::boolean(&value).ok_or_else(|| "Expected on/off".to_owned())?;
                let changed = self.recursive != on;
                self.recursive = on;
                Ok(changed)
            }
            "order" => set_choice(&mut self.order, &value, SlideOrder::from_index),
            "shuffle_seed" => set_number(&mut self.shuffle_seed, &value, 0, 9999),
            "display_seconds" => set_number(&mut self.display_seconds, &value, 3, 1800),
            "transition_seconds" => set_number(&mut self.transition_seconds, &value, 0, 30),
            "motion" => set_choice(&mut self.motion, &value, Motion::from_index),
            "motion_strength" => set_number(&mut self.motion_strength_percent, &value, 0, 50),
            "easing" => set_choice(&mut self.easing, &value, Easing::from_index),
            "fit" => set_choice(&mut self.fit, &value, FitMode::from_index),
            "preset" => set_choice(&mut self.preset, &value, ImagePreset::from_index),
            "brightness" => set_number(&mut self.brightness_percent, &value, 0, 200),
            "contrast" => set_number(&mut self.contrast_percent, &value, 0, 200),
            "saturation" => set_number(&mut self.saturation_percent, &value, 0, 200),
            "hue" => set_number(&mut self.hue_degrees, &value, 0, 360),
            "intensity" => set_number(&mut self.intensity_percent, &value, 0, 100),
            "opacity" => set_number(&mut self.opacity_percent, &value, 0, 100),
            _ => Ok(false),
        }
    }
}

/// Path used by tests and diagnostics to show a file name.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
