//! One observer position shared by every location-aware scene, so the user
//! sets their place once.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeoLocation {
    /// Human label: the address the user typed, or "48.857, 2.352".
    pub label: String,
    /// Degrees, north positive, -90..=90.
    pub latitude: f64,
    /// Degrees, east positive, -180..=180.
    pub longitude: f64,
}

impl Default for GeoLocation {
    /// Greenwich Observatory: a neutral, well-known default.
    fn default() -> Self {
        Self {
            label: "Greenwich, London".to_owned(),
            latitude: 51.4779,
            longitude: -0.0015,
        }
    }
}

impl GeoLocation {
    pub fn new(label: impl Into<String>, latitude: f64, longitude: f64) -> Self {
        Self {
            label: label.into(),
            latitude,
            longitude,
        }
        .normalized()
    }

    pub fn normalized(&self) -> Self {
        let finite = |value: f64| if value.is_finite() { value } else { 0.0 };
        let mut longitude = finite(self.longitude);
        if !(-180.0..=180.0).contains(&longitude) {
            longitude = (longitude + 180.0).rem_euclid(360.0) - 180.0;
        }
        Self {
            label: self.label.clone(),
            latitude: finite(self.latitude).clamp(-90.0, 90.0),
            longitude,
        }
    }

    /// "48.857N 2.352E"
    pub fn coordinate_text(&self) -> String {
        format!(
            "{:.3}{} {:.3}{}",
            self.latitude.abs(),
            if self.latitude >= 0.0 { 'N' } else { 'S' },
            self.longitude.abs(),
            if self.longitude >= 0.0 { 'E' } else { 'W' },
        )
    }

    /// Parse "48.857, 2.352", "48.857 2.352", "48.857N 2.352E", "-33.9 151.2".
    /// Returns `None` for anything that is not two coordinates in range.
    pub fn parse_coordinates(text: &str) -> Option<Self> {
        let cleaned = text.replace(',', " ");
        let parts: Vec<&str> = cleaned.split_whitespace().collect();
        if parts.len() != 2 {
            return None;
        }
        let parse = |part: &str, positive: char, negative: char| -> Option<f64> {
            let upper = part.to_ascii_uppercase();
            let (digits, sign) = if let Some(rest) = upper.strip_suffix(positive) {
                (rest.to_owned(), 1.0)
            } else if let Some(rest) = upper.strip_suffix(negative) {
                (rest.to_owned(), -1.0)
            } else {
                (upper, 1.0)
            };
            let value: f64 = digits.parse().ok()?;
            value.is_finite().then_some(value * sign)
        };
        let latitude = parse(parts[0], 'N', 'S')?;
        let longitude = parse(parts[1], 'E', 'W')?;
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }
        let mut location = Self {
            label: String::new(),
            latitude,
            longitude,
        };
        location.label = location.coordinate_text();
        Some(location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_coordinate_spellings() {
        for text in [
            "48.857, 2.352",
            "48.857 2.352",
            "48.857N 2.352E",
            "48.857n,2.352e",
        ] {
            let location = GeoLocation::parse_coordinates(text).unwrap();
            assert!((location.latitude - 48.857).abs() < 1e-9, "{text}");
            assert!((location.longitude - 2.352).abs() < 1e-9, "{text}");
        }
        let south_west = GeoLocation::parse_coordinates("33.9S 70.6W").unwrap();
        assert!(south_west.latitude < 0.0 && south_west.longitude < 0.0);
        assert!(GeoLocation::parse_coordinates("91 0").is_none());
        assert!(GeoLocation::parse_coordinates("Paris").is_none());
        assert!(GeoLocation::parse_coordinates("1 2 3").is_none());
    }

    #[test]
    fn normalization_wraps_longitude_and_clamps_latitude() {
        let location = GeoLocation::new("x", 120.0, 190.0);
        assert_eq!(location.latitude, 90.0);
        assert!((location.longitude + 170.0).abs() < 1e-9);
        assert_eq!(
            GeoLocation::new("nan", f64::NAN, f64::INFINITY).latitude,
            0.0
        );
    }
}
