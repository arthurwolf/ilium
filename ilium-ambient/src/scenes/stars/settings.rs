//! User-facing settings of the star map and their Settings-panel rows.

use super::astro;
use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// A closed set of choices: labels for the UI, stable index mapping.
macro_rules! choice_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

choice_enum! {
    /// How the sky is laid out on the panel.
    Projection {
        Dome => "Dome (looking up)",
        Panorama => "Panorama (horizon strip)",
        Patch => "Patch (aimed window)",
    }
}

choice_enum! {
    /// Angular distortion of the dome and patch views.
    Lens {
        Stereographic => "Stereographic",
        Equidistant => "Equidistant fisheye",
    }
}

choice_enum! {
    StarStyle {
        Realistic => "Realistic",
        Monochrome => "Monochrome dots",
    }
}

choice_enum! {
    StarSize {
        Normal => "Normal",
        Small => "Small",
        Large => "Large",
    }
}

choice_enum! {
    StartFrom {
        Now => "Now",
        FixedTime => "Fixed date and time",
    }
}

choice_enum! {
    /// Simulated seconds per real second.
    TimeSpeed {
        X1 => "x1 (real time)",
        X10 => "x10",
        X60 => "x60 (1 min/s)",
        X600 => "x600 (10 min/s)",
        X3600 => "x3600 (1 h/s)",
        X86400 => "x86400 (1 day/s)",
    }
}

impl TimeSpeed {
    pub fn factor(self) -> f64 {
        match self {
            Self::X1 => 1.0,
            Self::X10 => 10.0,
            Self::X60 => 60.0,
            Self::X600 => 600.0,
            Self::X3600 => 3600.0,
            Self::X86400 => 86_400.0,
        }
    }
}

/// Settings of the "Stars overhead" scene. The observer position is not part
/// of them: it is the shared location of the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StarsSettings {
    /// Dome, panorama or aimed patch. Default: dome.
    pub projection: Projection,
    /// Lens of the dome and patch views. Default: stereographic.
    pub lens: Lens,
    /// Compass bearing 0..359 (0 = north, 90 = east): the direction at the top
    /// of the dome, the centre of the panorama or of the patch. Default 0.
    pub look_azimuth_degrees: i32,
    /// Elevation of the patch centre above the horizon, 0..90. Default 50.
    pub look_altitude_degrees: i32,
    /// Angular width of the view, 10..180: across the smaller panel side for
    /// the dome, across the panel width otherwise. Default 180.
    pub field_of_view_degrees: i32,
    /// Faintest star drawn, in tenths of a magnitude, 10..65. Default 50.
    pub magnitude_limit_tenths: i32,
    /// Realistic (size and brightness follow magnitude) or one-dot monochrome.
    pub star_style: StarStyle,
    /// Size of the dot clusters bright stars are drawn with. Default normal.
    pub star_size: StarSize,
    /// Tint stars with their B-V colour (realistic style only). Default on.
    pub star_colors: bool,
    /// Brightness gamma in percent, 50..200; higher brightens faint stars.
    /// Default 100.
    pub brightness_gamma_percent: i32,
    /// Subtle deterministic shimmer, stronger near the horizon. Default off.
    pub twinkle: bool,
    /// Faint stippled band of the Milky Way. Default on.
    pub milky_way: bool,
    /// Draw the horizon line or ring. Default on.
    pub horizon: bool,
    /// Draw ticks at north, east, south and west, north longer. Default on.
    pub cardinal_marks: bool,
    /// Draw traditional constellation stick figures. Default off.
    pub constellation_lines: bool,
    /// Draw the Moon with its current phase. Default on.
    pub moon: bool,
    /// Draw Mercury, Venus, Mars, Jupiter and Saturn. Default on.
    pub planets: bool,
    /// Illustrative low-Earth orbit satellites, not a live catalogue. Default off.
    pub satellites: bool,
    /// Precess catalogue positions from J2000 to the date shown. Default on.
    pub precession: bool,
    /// Simulated time speed. Default x1 (real time).
    pub time_speed: TimeSpeed,
    /// Hours added to the simulated time, -168..168. Default 0.
    pub time_offset_hours: i32,
    /// Start from the real clock or from a fixed date and time.
    pub start_from: StartFrom,
    /// Fixed start "YYYY-MM-DD HH:MM[:SS]" in UTC; used with `fixed_time`.
    pub start_datetime: String,
}

impl Default for StarsSettings {
    fn default() -> Self {
        Self {
            projection: Projection::Dome,
            lens: Lens::Stereographic,
            look_azimuth_degrees: 0,
            look_altitude_degrees: 50,
            field_of_view_degrees: 180,
            magnitude_limit_tenths: 50,
            star_style: StarStyle::Realistic,
            star_size: StarSize::Normal,
            star_colors: true,
            brightness_gamma_percent: 100,
            twinkle: false,
            milky_way: true,
            horizon: true,
            cardinal_marks: true,
            constellation_lines: false,
            moon: true,
            planets: true,
            satellites: false,
            precession: true,
            time_speed: TimeSpeed::X1,
            time_offset_hours: 0,
            start_from: StartFrom::Now,
            start_datetime: String::new(),
        }
    }
}

impl StarsSettings {
    /// The fixed start as Unix seconds, when the fixed mode is chosen and the
    /// text parses. `None` means "use the real clock".
    pub fn fixed_start_unix(&self) -> Option<f64> {
        if self.start_from == StartFrom::FixedTime {
            astro::parse_utc(&self.start_datetime)
        } else {
            None
        }
    }

    /// True when fixed mode is chosen but the text cannot be used.
    pub fn has_unusable_start(&self) -> bool {
        self.start_from == StartFrom::FixedTime
            && !self.start_datetime.trim().is_empty()
            && astro::parse_utc(&self.start_datetime).is_none()
    }
}

fn clamp_field(value: &mut i32, min: i32, max: i32, new_value: i32) -> bool {
    let clamped = new_value.clamp(min, max);
    let changed = *value != clamped;
    *value = clamped;
    changed
}

fn set_toggle(field: &mut bool, value: &ControlValue) -> Result<bool, String> {
    let new_value = control::boolean(value).ok_or_else(|| "Expected on or off".to_owned())?;
    let changed = *field != new_value;
    *field = new_value;
    Ok(changed)
}

fn set_choice<T: Copy + PartialEq>(
    field: &mut T,
    value: &ControlValue,
    from_index: impl Fn(usize) -> Option<T>,
) -> Result<bool, String> {
    let index = control::index(value).ok_or_else(|| "Expected a choice".to_owned())?;
    let new_value = from_index(index).ok_or_else(|| "Unknown choice".to_owned())?;
    let changed = *field != new_value;
    *field = new_value;
    Ok(changed)
}

fn set_number(field: &mut i32, value: &ControlValue, min: i32, max: i32) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| "Expected a number".to_owned())?;
    Ok(clamp_field(field, min, max, number))
}

impl SceneSettings for StarsSettings {
    fn normalized(&self) -> Self {
        let mut settings = self.clone();
        settings.look_azimuth_degrees = settings.look_azimuth_degrees.rem_euclid(360);
        settings.look_altitude_degrees = settings.look_altitude_degrees.clamp(0, 90);
        settings.field_of_view_degrees = settings.field_of_view_degrees.clamp(10, 180);
        settings.magnitude_limit_tenths = settings.magnitude_limit_tenths.clamp(10, 65);
        settings.brightness_gamma_percent = settings.brightness_gamma_percent.clamp(50, 200);
        settings.time_offset_hours = settings.time_offset_hours.clamp(-168, 168);
        settings.start_datetime = settings.start_datetime.trim().to_owned();
        settings
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "projection",
                "Projection",
                self.projection.index(),
                Projection::LABELS,
                "Dome looks straight up with the horizon at the edge, panorama unrolls the sky along the horizon, patch is a window aimed at one spot.",
            ),
        ];
        if self.projection != Projection::Panorama {
            rows.push(Control::choice(
                "lens",
                "Lens",
                self.lens.index(),
                Lens::LABELS,
                "Stereographic keeps shapes true and stretches the edges; equidistant fisheye keeps angles proportional to distance.",
            ));
        }
        rows.push(Control::slider(
            "look_azimuth",
            match self.projection {
                Projection::Dome => "Top of the dome",
                _ => "Look direction",
            },
            self.look_azimuth_degrees,
            (0, 360, 5),
            " deg",
            "Compass bearing: 0 is north, 90 east, 180 south, 270 west. It is the direction at the top of the dome, or the centre of the panorama and patch.",
        ));
        if self.projection == Projection::Patch {
            rows.push(Control::slider(
                "look_altitude",
                "Look elevation",
                self.look_altitude_degrees,
                (0, 90, 5),
                " deg",
                "How high above the horizon the patch is aimed; 90 looks at the zenith.",
            ));
        }
        rows.push(Control::slider(
            "field_of_view",
            "Field of view",
            self.field_of_view_degrees,
            (10, 180, 5),
            " deg",
            "Angular size of the view: across the panel height for the dome, across its width for panorama and patch. Small values zoom in.",
        ));
        rows.push(Control::slider(
            "magnitude_limit",
            "Magnitude limit",
            self.magnitude_limit_tenths,
            (10, 65, 1),
            " /10 mag",
            "Faintest star drawn, in tenths of a magnitude (50 = magnitude 5.0, 65 = the naked-eye limit of 6.5). Lower shows fewer stars.",
        ));
        rows.push(Control::choice(
            "star_style",
            "Star style",
            self.star_style.index(),
            StarStyle::LABELS,
            "Realistic draws bright stars as bigger dot clusters and fades faint ones; monochrome draws every star as one full dot.",
        ));
        if self.star_style == StarStyle::Realistic {
            rows.push(Control::choice(
                "star_size",
                "Star size",
                self.star_size.index(),
                StarSize::LABELS,
                "How large the clusters of the brightest stars are.",
            ));
            rows.push(Control::toggle(
                "star_colors",
                "Star colors",
                self.star_colors,
                "Tint each star with its real color (blue-white to orange-red) instead of the global palette.",
            ));
        }
        rows.push(Control::slider(
            "brightness_gamma",
            "Brightness gamma",
            self.brightness_gamma_percent,
            (50, 200, 5),
            "%",
            "Contrast curve of the star brightness. Above 100 brightens faint stars, below 100 keeps only the brighter ones prominent.",
        ));
        rows.push(Control::toggle(
            "twinkle",
            "Twinkle",
            self.twinkle,
            "A subtle shimmer of the stars, stronger close to the horizon.",
        ));
        rows.push(Control::toggle(
            "milky_way",
            "Milky Way",
            self.milky_way,
            "A faint stippled band along the galactic plane, brightest towards Sagittarius.",
        ));
        rows.push(Control::toggle(
            "horizon",
            "Horizon line",
            self.horizon,
            "On clips the sky at the ground and draws a horizon ring or line. Off includes objects below the horizon and expands the view to fill the panel.",
        ));
        rows.push(Control::toggle(
            "cardinal_marks",
            "Compass marks",
            self.cardinal_marks,
            "Ticks at north, east, south and west on the horizon; north has a longer tick and a marker.",
        ));
        rows.push(Control::toggle(
            "constellation_lines",
            "Constellation lines",
            self.constellation_lines,
            "Faint traditional stick figures joining the stars of the main constellations.",
        ));
        rows.push(Control::toggle(
            "moon",
            "Moon",
            self.moon,
            "Show the Moon with its current phase (low-precision ephemeris, about 0.3 degrees).",
        ));
        rows.push(Control::toggle(
            "planets",
            "Planets",
            self.planets,
            "Show Mercury, Venus, Mars, Jupiter and Saturn where they really are.",
        ));
        rows.push(Control::toggle(
            "satellites",
            "Simulated satellites",
            self.satellites,
            "Illustrative low-Earth orbit satellites in inclined orbits. These are simulated, not real tracked satellite positions or predictions.",
        ));
        rows.push(Control::toggle(
            "precession",
            "Precession",
            self.precession,
            "Correct the J2000 catalogue positions for the slow wobble of the Earth's axis up to the date shown.",
        ));
        rows.push(Control::choice(
            "time_speed",
            "Time speed",
            self.time_speed.index(),
            TimeSpeed::LABELS,
            "Simulated seconds per real second. x1 is the real sky; higher values make the sky visibly turn.",
        ));
        rows.push(Control::slider(
            "time_offset",
            "Time offset",
            self.time_offset_hours,
            (-168, 168, 1),
            " h",
            "Shift the simulated time by this many hours, to see the sky of earlier or later hours or days.",
        ));
        rows.push(Control::choice(
            "start_from",
            "Start from",
            self.start_from.index(),
            StartFrom::LABELS,
            "Begin at the real current time or at a fixed date and time.",
        ));
        if self.start_from == StartFrom::FixedTime {
            rows.push(Control::text(
                "start_datetime",
                "Start date and time",
                &self.start_datetime,
                "YYYY-MM-DD HH:MM (UTC)",
                "Universal time to start from, for example 2026-12-21 22:00. Empty means now.",
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "projection" => set_choice(&mut self.projection, &value, Projection::from_index),
            "lens" => set_choice(&mut self.lens, &value, Lens::from_index),
            "look_azimuth" => {
                let number =
                    control::number(&value).ok_or_else(|| "Expected a number".to_owned())?;
                let wrapped = number.rem_euclid(360);
                let changed = self.look_azimuth_degrees != wrapped;
                self.look_azimuth_degrees = wrapped;
                Ok(changed)
            }
            "look_altitude" => set_number(&mut self.look_altitude_degrees, &value, 0, 90),
            "field_of_view" => set_number(&mut self.field_of_view_degrees, &value, 10, 180),
            "magnitude_limit" => set_number(&mut self.magnitude_limit_tenths, &value, 10, 65),
            "star_style" => set_choice(&mut self.star_style, &value, StarStyle::from_index),
            "star_size" => set_choice(&mut self.star_size, &value, StarSize::from_index),
            "star_colors" => set_toggle(&mut self.star_colors, &value),
            "brightness_gamma" => set_number(&mut self.brightness_gamma_percent, &value, 50, 200),
            "twinkle" => set_toggle(&mut self.twinkle, &value),
            "milky_way" => set_toggle(&mut self.milky_way, &value),
            "horizon" => set_toggle(&mut self.horizon, &value),
            "cardinal_marks" => set_toggle(&mut self.cardinal_marks, &value),
            "constellation_lines" => set_toggle(&mut self.constellation_lines, &value),
            "moon" => set_toggle(&mut self.moon, &value),
            "planets" => set_toggle(&mut self.planets, &value),
            "satellites" => set_toggle(&mut self.satellites, &value),
            "precession" => set_toggle(&mut self.precession, &value),
            "time_speed" => set_choice(&mut self.time_speed, &value, TimeSpeed::from_index),
            "time_offset" => set_number(&mut self.time_offset_hours, &value, -168, 168),
            "start_from" => set_choice(&mut self.start_from, &value, StartFrom::from_index),
            "start_datetime" => {
                let text = control::text(&value).ok_or_else(|| "Expected text".to_owned())?;
                let text = text.trim();
                if !text.is_empty() && astro::parse_utc(text).is_none() {
                    return Err(format!(
                        "\"{text}\" is not a date and time; use YYYY-MM-DD HH:MM in UTC"
                    ));
                }
                let changed = self.start_datetime != text;
                self.start_datetime = text.to_owned();
                Ok(changed)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn all_mode_settings() -> Vec<StarsSettings> {
        let mut variants = Vec::new();
        for projection in Projection::ALL {
            for style in StarStyle::ALL {
                for start in StartFrom::ALL {
                    variants.push(StarsSettings {
                        projection: *projection,
                        star_style: *style,
                        start_from: *start,
                        ..StarsSettings::default()
                    });
                }
            }
        }
        variants
    }

    #[test]
    fn every_row_has_unique_id_label_and_help() {
        for settings in all_mode_settings() {
            let rows = settings.controls();
            let mut ids = HashSet::new();
            for row in &rows {
                assert!(ids.insert(row.id), "duplicate id {}", row.id);
                assert!(
                    !row.label.is_empty() && row.label.len() <= 24,
                    "{}",
                    row.label
                );
                assert!(row.help.len() > 20, "{}", row.id);
            }
        }
    }

    #[test]
    fn rows_that_do_not_apply_are_hidden() {
        let ids = |settings: &StarsSettings| -> Vec<&'static str> {
            settings.controls().iter().map(|row| row.id).collect()
        };
        let dome = ids(&StarsSettings::default());
        assert!(dome.contains(&"lens") && !dome.contains(&"look_altitude"));
        assert!(!dome.contains(&"start_datetime"));
        let panorama = ids(&StarsSettings {
            projection: Projection::Panorama,
            ..StarsSettings::default()
        });
        assert!(!panorama.contains(&"lens") && !panorama.contains(&"look_altitude"));
        let patch = ids(&StarsSettings {
            projection: Projection::Patch,
            start_from: StartFrom::FixedTime,
            star_style: StarStyle::Monochrome,
            ..StarsSettings::default()
        });
        assert!(patch.contains(&"look_altitude") && patch.contains(&"start_datetime"));
        assert!(!patch.contains(&"star_colors") && !patch.contains(&"star_size"));
    }

    #[test]
    fn set_control_reports_changes_and_clamps() {
        let mut settings = StarsSettings::default();
        assert_eq!(
            settings.set_control("twinkle", ControlValue::Bool(true)),
            Ok(true)
        );
        assert_eq!(
            settings.set_control("twinkle", ControlValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("magnitude_limit", ControlValue::Number(999)),
            Ok(true)
        );
        assert_eq!(settings.magnitude_limit_tenths, 65);
        assert_eq!(
            settings.set_control("look_azimuth", ControlValue::Number(-90)),
            Ok(true)
        );
        assert_eq!(settings.look_azimuth_degrees, 270);
        assert_eq!(
            settings.set_control("projection", ControlValue::Index(2)),
            Ok(true)
        );
        assert_eq!(settings.projection, Projection::Patch);
        assert!(settings
            .set_control("projection", ControlValue::Index(9))
            .is_err());
        assert!(settings
            .set_control("twinkle", ControlValue::Number(1))
            .is_err());
        assert_eq!(
            settings.set_control("no_such_row", ControlValue::Bool(true)),
            Ok(false)
        );
        assert_eq!(
            settings.set_control("time_speed", ControlValue::Index(5)),
            Ok(true)
        );
        assert_eq!(settings.time_speed.factor(), 86_400.0);
    }

    #[test]
    fn start_datetime_is_validated() {
        let mut settings = StarsSettings::default();
        let bad = settings.set_control(
            "start_datetime",
            ControlValue::Text("next tuesday".to_owned()),
        );
        assert!(bad.unwrap_err().contains("YYYY-MM-DD"));
        assert_eq!(settings.start_datetime, "");
        assert_eq!(
            settings.set_control(
                "start_datetime",
                ControlValue::Text(" 2026-12-21 22:00 ".to_owned())
            ),
            Ok(true)
        );
        assert_eq!(settings.start_datetime, "2026-12-21 22:00");
        assert_eq!(
            settings.set_control(
                "start_datetime",
                ControlValue::Text("2026-12-21 22:00".to_owned())
            ),
            Ok(false)
        );
        assert_eq!(
            settings.fixed_start_unix(),
            None,
            "fixed mode is not selected yet"
        );
        settings.start_from = StartFrom::FixedTime;
        assert_eq!(
            settings.fixed_start_unix(),
            astro::parse_utc("2026-12-21 22:00")
        );
        assert!(!settings.has_unusable_start());
        settings.start_datetime = "garbage".to_owned();
        assert!(settings.has_unusable_start());
        assert_eq!(settings.fixed_start_unix(), None);
    }

    #[test]
    fn normalization_clamps_every_field() {
        let wild = StarsSettings {
            look_azimuth_degrees: -10,
            look_altitude_degrees: 400,
            field_of_view_degrees: 1,
            magnitude_limit_tenths: -5,
            brightness_gamma_percent: 9000,
            time_offset_hours: 1_000_000,
            start_datetime: "  x ".to_owned(),
            ..StarsSettings::default()
        }
        .normalized();
        assert_eq!(wild.look_azimuth_degrees, 350);
        assert_eq!(wild.look_altitude_degrees, 90);
        assert_eq!(wild.field_of_view_degrees, 10);
        assert_eq!(wild.magnitude_limit_tenths, 10);
        assert_eq!(wild.brightness_gamma_percent, 200);
        assert_eq!(wild.time_offset_hours, 168);
        assert_eq!(wild.start_datetime, "x");
        assert_eq!(
            StarsSettings::default().normalized(),
            StarsSettings::default()
        );
    }

    #[test]
    fn serde_uses_snake_case_keys_defaults_and_round_trips() {
        let empty: StarsSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, StarsSettings::default());
        let json = serde_json::to_string(&StarsSettings::default()).unwrap();
        for key in [
            "projection",
            "magnitude_limit_tenths",
            "time_speed",
            "start_datetime",
            "constellation_lines",
        ] {
            assert!(json.contains(&format!("\"{key}\"")), "{key}");
        }
        assert!(json.contains("\"x1\""), "{json}");
        let custom = StarsSettings {
            projection: Projection::Patch,
            time_speed: TimeSpeed::X3600,
            start_from: StartFrom::FixedTime,
            star_size: StarSize::Large,
            ..StarsSettings::default()
        };
        let back: StarsSettings =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(back, custom);
        let partial: StarsSettings = serde_json::from_str(
            r#"{"projection":"panorama","time_speed":"x86400","twinkle":true}"#,
        )
        .unwrap();
        assert_eq!(partial.projection, Projection::Panorama);
        assert_eq!(partial.time_speed, TimeSpeed::X86400);
        assert!(partial.twinkle && partial.milky_way);
    }

    #[test]
    fn choice_labels_match_variants() {
        assert_eq!(Projection::ALL.len(), Projection::LABELS.len());
        assert_eq!(TimeSpeed::ALL.len(), TimeSpeed::LABELS.len());
        for (index, item) in TimeSpeed::ALL.iter().enumerate() {
            assert_eq!(item.index(), index);
            assert_eq!(TimeSpeed::from_index(index), Some(*item));
        }
        assert_eq!(Lens::from_index(5), None);
    }
}

#[cfg(test)]
mod overhaul_regression_tests {
    use super::*;
    #[test]
    fn constellation_lines_start_off_but_saved_on_survives() {
        assert!(!StarsSettings::default().constellation_lines);
        let saved: StarsSettings = serde_json::from_str(r#"{"constellation_lines":true}"#).unwrap();
        assert!(saved.constellation_lines);
    }
}
