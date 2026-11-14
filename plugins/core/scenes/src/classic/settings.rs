//! Typed per-scene settings of the classic scenes. A scene has up to four named sliders
//! (one macro keeps each field's default, bounds, step and label together, so the rows the
//! host shows and the values the scene reads cannot disagree). The stored data stay plain
//! integers; the row ids are the field names, which are also the property names of the
//! scene's `settings_schema` in the manifest.

use crate::kit::control::{self, Control, ControlValue, SceneSettings};

/// Help of a slider that has no longer text of its own.
const SLIDER_HELP: &str = "A control of this scene; each scene keeps its own saved values.";

macro_rules! scene_parameters {
    ($name:ident { $( $field:ident : ($label:literal, $default:literal, $min:literal, $max:literal, $step:literal, $unit:literal) ),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name { $( pub $field: u16, )+ }

        impl Default for $name {
            fn default() -> Self { Self { $( $field: $default, )+ } }
        }

        impl SceneSettings for $name {
            fn normalized(&self) -> Self {
                Self { $( $field: self.$field.clamp($min, $max), )+ }
            }

            fn controls(&self) -> Vec<Control> {
                vec![$(
                    Control::slider(
                        stringify!($field),
                        $label,
                        i32::from(self.$field),
                        ($min as i32, $max as i32, $step as i32),
                        $unit,
                        SLIDER_HELP,
                    ),
                )+]
            }

            fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
                $(
                    if id == stringify!($field) {
                        let number = control::number(&value)
                            .ok_or_else(|| format!("{id} expects a number"))?
                            .clamp($min as i32, $max as i32) as u16;
                        let changed = self.$field != number;
                        self.$field = number;
                        return Ok(changed);
                    }
                )+
                Ok(false)
            }
        }
    };
}

scene_parameters!(MoonlitWaterSettings {
    wave_strength_percent: ("Wave strength", 100, 0, 200, 5, "%"),
    ripple_scale_percent: ("Ripple scale", 100, 50, 200, 5, "%"),
    reflection_width_percent: ("Reflection width", 100, 25, 200, 5, "%"),
    moon_size_percent: ("Moon size", 100, 50, 150, 5, "%"),
});

scene_parameters!(SleepingRidgeSettings {
    cloud_cover_percent: ("Cloud cover", 60, 0, 100, 5, "%"),
    mist_percent: ("Valley mist", 40, 0, 100, 5, "%"),
    ridge_height_percent: ("Ridge height", 100, 50, 150, 5, "%"),
    drift_percent: ("Cloud drift", 100, 25, 200, 5, "%"),
});

scene_parameters!(WindyHillsideSettings {
    plant_density_percent: ("Plant density", 100, 25, 200, 5, "%"),
    wind_strength_percent: ("Wind strength", 100, 0, 200, 5, "%"),
    plant_height_percent: ("Plant height", 100, 50, 150, 5, "%"),
    gust_scale_percent: ("Gust breadth", 100, 50, 200, 5, "%"),
});

scene_parameters!(TeaSteamSettings {
    cup_size_percent: ("Cup size", 100, 60, 140, 5, "%"),
    steam_ribbons: ("Steam wisps", 4, 1, 6, 1, ""),
    curl_percent: ("Steam curl", 100, 25, 200, 5, "%"),
    steam_height_percent: ("Steam height", 100, 50, 150, 5, "%"),
});

scene_parameters!(KelpSettings {
    plant_density_percent: ("Plant density", 100, 25, 200, 5, "%"),
    current_strength_percent: ("Current strength", 100, 0, 200, 5, "%"),
    leaf_length_percent: ("Ribbon length", 100, 50, 175, 5, "%"),
    cluster_percent: ("Clustering", 65, 0, 100, 5, "%"),
});

scene_parameters!(StoneCausticsSettings {
    dome_height_percent: ("Stone relief", 100, 25, 200, 5, "%"),
    caustic_scale_percent: ("Light web scale", 100, 50, 200, 5, "%"),
    small_stones_percent: ("Small stones", 65, 0, 100, 5, "%"),
    sand_percent: ("Sand grains", 20, 0, 100, 5, "%"),
});

scene_parameters!(CloudletSettings {
    form_count: ("Cloud islands", 6, 2, 10, 1, ""),
    form_size_percent: ("Island size", 100, 50, 175, 5, "%"),
    cohesion_percent: ("Joining softness", 100, 25, 200, 5, "%"),
    orbit_radius_percent: ("Drift breadth", 100, 25, 150, 5, "%"),
});

scene_parameters!(TwoRipplesSettings {
    source_separation_percent: ("Source separation", 54, 10, 90, 2, "%"),
    wavelength_percent: ("Wavelength", 100, 50, 200, 5, "%"),
    interference_percent: ("Interference", 65, 0, 100, 5, "%"),
    damping_percent: ("Damping", 25, 0, 100, 5, "%"),
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietPondSettings {
    pub pad_count: u16,
    pub pad_size_percent: u16,
    pub ripple_strength_percent: u16,
    pub drift_percent: u16,
    pub natural_placement: bool,
}

impl Default for QuietPondSettings {
    fn default() -> Self {
        Self {
            pad_count: 7,
            pad_size_percent: 100,
            ripple_strength_percent: 70,
            drift_percent: 100,
            natural_placement: false,
        }
    }
}

impl SceneSettings for QuietPondSettings {
    fn normalized(&self) -> Self {
        Self {
            pad_count: self.pad_count.clamp(3, 64),
            pad_size_percent: self.pad_size_percent.clamp(50, 175),
            ripple_strength_percent: self.ripple_strength_percent.min(100),
            drift_percent: self.drift_percent.clamp(25, 200),
            ..*self
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            Control::slider(
                "pad_count",
                "Lily pads",
                i32::from(self.pad_count),
                (3, 64, 1),
                "",
                SLIDER_HELP,
            ),
            Control::slider(
                "pad_size_percent",
                "Pad size",
                i32::from(self.pad_size_percent),
                (50, 175, 5),
                "%",
                SLIDER_HELP,
            ),
            Control::slider(
                "ripple_strength_percent",
                "Water ripples",
                i32::from(self.ripple_strength_percent),
                (0, 100, 5),
                "%",
                SLIDER_HELP,
            ),
            Control::slider(
                "drift_percent",
                "Surface drift",
                i32::from(self.drift_percent),
                (25, 200, 5),
                "%",
                SLIDER_HELP,
            ),
            Control::toggle(
                "natural_placement",
                "Rooted placement",
                self.natural_placement,
                "Cluster leaves around underwater root groups, with bounded petiole reach and spacing.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = *self;
        if id == "natural_placement" {
            self.natural_placement =
                control::boolean(&value).ok_or_else(|| format!("{id} expects a switch"))?;
            return Ok(*self != before);
        }
        let (field, minimum, maximum) = match id {
            "pad_count" => (&mut self.pad_count, 3, 64),
            "pad_size_percent" => (&mut self.pad_size_percent, 50, 175),
            "ripple_strength_percent" => (&mut self.ripple_strength_percent, 0, 100),
            "drift_percent" => (&mut self.drift_percent, 25, 200),
            _ => return Ok(false),
        };
        let number = control::number(&value)
            .ok_or_else(|| format!("{id} expects a number"))?
            .clamp(minimum, maximum) as u16;
        *field = number;
        Ok(*self != before)
    }
}
