//! Typed per-scene settings and the numeric control contract shared by the UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slider {
    pub label: &'static str,
    pub value: u16,
    pub minimum: u16,
    pub maximum: u16,
    pub step: u16,
    pub unit: &'static str,
}

impl Slider {
    pub const fn new(
        label: &'static str,
        value: u16,
        minimum: u16,
        maximum: u16,
        step: u16,
        unit: &'static str,
    ) -> Self {
        Self {
            label,
            value,
            minimum,
            maximum,
            step,
            unit,
        }
    }

    pub fn adjusted(self, direction: i32) -> u16 {
        (i32::from(self.value) + direction.signum() * i32::from(self.step))
            .clamp(i32::from(self.minimum), i32::from(self.maximum)) as u16
    }

    /// The track endpoints are exact, including ranges not divisible by step.
    pub fn value_at(self, offset: u16, track_width: u16) -> u16 {
        if track_width < 2 {
            return self.value.clamp(self.minimum, self.maximum);
        }
        let offset = u32::from(offset.min(track_width - 1));
        let span = u32::from(self.maximum - self.minimum);
        let denominator = u32::from(track_width - 1);
        let raw = u32::from(self.minimum) + (span * offset + denominator / 2) / denominator;
        let step = u32::from(self.step.max(1));
        let rounded =
            u32::from(self.minimum) + ((raw - u32::from(self.minimum) + step / 2) / step) * step;
        rounded.min(u32::from(self.maximum)) as u16
    }

    pub fn thumb_offset(self, track_width: u16) -> u16 {
        let span = u32::from(self.maximum - self.minimum).max(1);
        let value = u32::from(self.value.clamp(self.minimum, self.maximum) - self.minimum);
        ((value * u32::from(track_width.saturating_sub(1)) + span / 2) / span) as u16
    }
}

// A scene always supplies four named controls. The macro keeps each field's
// default, bounds, keyboard step and label together, so persistence and UI
// normalization cannot disagree. The stored data remain concrete Rust types.
macro_rules! scene_parameters {
    ($name:ident { $( $field:ident : ($label:literal, $default:literal, $min:literal, $max:literal, $step:literal, $unit:literal) ),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name { $( pub $field: u16, )+ }

        impl Default for $name {
            fn default() -> Self { Self { $( $field: $default, )+ } }
        }

        impl $name {
            pub fn normalized(self) -> Self {
                Self { $( $field: self.$field.clamp($min, $max), )+ }
            }

            pub fn sliders(self) -> [Slider; 4] {
                [$( Slider::new($label, self.$field, $min, $max, $step, $unit), )+]
            }

            pub(super) fn set(&mut self, index: usize, value: u16) {
                let mut fields = [$( (&mut self.$field, $min, $max), )+];
                if let Some((field, minimum, maximum)) = fields.get_mut(index) {
                    **field = value.clamp(*minimum, *maximum);
                }
            }
        }
    };
}

scene_parameters!(ShorelineSettings {
    reach_percent: ("Tide reach", 100, 50, 150, 5, "%"),
    foam_width_percent: ("Foam width", 75, 25, 200, 5, "%"),
    grain_percent: ("Sand grains", 20, 0, 100, 5, "%"),
    cycle_seconds: ("Wash cycle", 12, 6, 30, 1, "s"),
});

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

scene_parameters!(QuietPondSettings {
    pad_count: ("Lily pads", 7, 3, 14, 1, ""),
    pad_size_percent: ("Pad size", 100, 50, 175, 5, "%"),
    ripple_strength_percent: ("Water ripples", 70, 0, 100, 5, "%"),
    drift_percent: ("Surface drift", 100, 25, 200, 5, "%"),
});
