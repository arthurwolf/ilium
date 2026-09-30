//! The weather-satellite data sources of the clouds scene, which one serves a
//! location, and how a time-lapse is sampled in time.
//!
//! Sources verified against the live services on 2026-09-30:
//! * NASA GIBS WMTS (EPSG:4326, keyless): `GOES-East_ABI_GeoColor`,
//!   `GOES-West_ABI_GeoColor` (TileMatrixSet `1km`, PNG, one image every ten
//!   minutes, about one hour behind real time) and the daily polar mosaic
//!   `MODIS_Terra_CorrectedReflectance_TrueColor` (TileMatrixSet `250m`, JPEG).
//!   GIBS has no Meteosat layers, and the Himawari layers are colour-mapped or
//!   daylight-only, so the Pacific/Asia disc is served by the world map below.
//!   Layer catalogue: <https://nasa-gibs.github.io/gibs-api-docs/available-visualizations/>.
//! * EUMETSAT EUMETView WMS (keyless, `Fees: none`, `AccessConstraints: none`):
//!   Meteosat-0 (`msg_fes:ir108`) and Meteosat-IODC (`msg_iodc:ir108`) infrared
//!   every 15 minutes, and `mumi:worldcloudmap_ir108`, a global mosaic of all
//!   geostationary satellites every three hours. Credit: "Copyright EUMETSAT".
//!   Terms: <https://www.eumetsat.int/eumetsat-data-licensing>.
//! * RainViewer's satellite infrared list (`api.rainviewer.com/public/weather-maps.json`)
//!   was verified to be empty (`"satellite":{"infrared":[]}`) and is not used.

use crate::scenes::night_lights::tiles::UtcTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSource {
    /// Pick the best source for the location (or the world map when global).
    #[default]
    Auto,
    GoesEast,
    GoesWest,
    Meteosat,
    IndianOcean,
    /// Global infrared mosaic of every geostationary satellite, 3-hourly.
    WorldIr,
    /// Daily polar-orbiter (MODIS Terra) true-colour mosaic.
    PolarDaily,
}

impl CloudSource {
    pub const LABELS: [&'static str; 7] = [
        "Automatic",
        "GOES-East (Americas)",
        "GOES-West (Pacific)",
        "Meteosat (Europe, Africa)",
        "Meteosat IODC (Indian Ocean)",
        "World infrared mosaic",
        "Daily true colour (MODIS)",
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::GoesEast => 1,
            Self::GoesWest => 2,
            Self::Meteosat => 3,
            Self::IndianOcean => 4,
            Self::WorldIr => 5,
            Self::PolarDaily => 6,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::GoesEast,
            2 => Self::GoesWest,
            3 => Self::Meteosat,
            4 => Self::IndianOcean,
            5 => Self::WorldIr,
            6 => Self::PolarDaily,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// GIBS colour layer with a time dimension (PNG tiles).
    GibsTimed {
        layer: &'static str,
        matrix_set: &'static str,
        max_level: u8,
    },
    /// GIBS daily layer addressed by date (JPEG tiles).
    GibsDaily {
        layer: &'static str,
        matrix_set: &'static str,
        max_level: u8,
    },
    /// EUMETView WMS layer, cropped and resampled by the server.
    Wms {
        layer: &'static str,
        workspace: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Provider {
    pub source: CloudSource,
    /// Short name for the status line.
    pub name: &'static str,
    /// Spacing of the images in minutes.
    pub cadence_minutes: i64,
    /// Longitude of the sub-satellite point, for geostationary sources.
    pub sub_longitude: Option<f64>,
    pub endpoint: Endpoint,
}

const PROVIDERS: [Provider; 6] = [
    Provider {
        source: CloudSource::GoesEast,
        name: "GOES-East",
        cadence_minutes: 10,
        sub_longitude: Some(-75.2),
        endpoint: Endpoint::GibsTimed {
            layer: "GOES-East_ABI_GeoColor",
            matrix_set: "1km",
            max_level: 6,
        },
    },
    Provider {
        source: CloudSource::GoesWest,
        name: "GOES-West",
        cadence_minutes: 10,
        sub_longitude: Some(-137.2),
        endpoint: Endpoint::GibsTimed {
            layer: "GOES-West_ABI_GeoColor",
            matrix_set: "1km",
            max_level: 6,
        },
    },
    Provider {
        source: CloudSource::Meteosat,
        name: "Meteosat",
        cadence_minutes: 15,
        sub_longitude: Some(0.0),
        endpoint: Endpoint::Wms {
            layer: "msg_fes:ir108",
            workspace: "msg_fes",
        },
    },
    Provider {
        source: CloudSource::IndianOcean,
        name: "Meteosat IODC",
        cadence_minutes: 15,
        sub_longitude: Some(45.5),
        endpoint: Endpoint::Wms {
            layer: "msg_iodc:ir108",
            workspace: "msg_iodc",
        },
    },
    Provider {
        source: CloudSource::WorldIr,
        name: "World IR",
        cadence_minutes: 180,
        sub_longitude: None,
        endpoint: Endpoint::Wms {
            layer: "mumi:worldcloudmap_ir108",
            workspace: "mumi",
        },
    },
    Provider {
        source: CloudSource::PolarDaily,
        name: "MODIS Terra",
        cadence_minutes: 24 * 60,
        sub_longitude: None,
        endpoint: Endpoint::GibsDaily {
            layer: "MODIS_Terra_CorrectedReflectance_TrueColor",
            matrix_set: "250m",
            max_level: 8,
        },
    },
];

/// The provider record of a concrete source (`Auto` maps to the world map).
pub fn provider(source: CloudSource) -> &'static Provider {
    PROVIDERS
        .iter()
        .find(|provider| provider.source == source)
        .unwrap_or(&PROVIDERS[4])
}

/// Great-circle angle in degrees between two points.
pub fn angular_distance(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    let (phi_a, phi_b) = (lat_a.to_radians(), lat_b.to_radians());
    let cosine =
        phi_a.sin() * phi_b.sin() + phi_a.cos() * phi_b.cos() * (lon_a - lon_b).to_radians().cos();
    cosine.clamp(-1.0, 1.0).acos().to_degrees()
}

/// Largest angle from the sub-satellite point at which a disc is still a
/// useful picture (beyond it the view is grazing and heavily stretched).
const MAX_DISC_ANGLE: f64 = 65.0;

/// The concrete source for a request. An explicit source is honoured.
pub fn choose_source(
    requested: CloudSource,
    global: bool,
    latitude: f64,
    longitude: f64,
) -> CloudSource {
    if requested != CloudSource::Auto {
        return requested;
    }
    if global {
        return CloudSource::WorldIr;
    }
    PROVIDERS
        .iter()
        .filter_map(|provider| {
            let sub_longitude = provider.sub_longitude?;
            Some((
                angular_distance(latitude, longitude, 0.0, sub_longitude),
                provider.source,
            ))
        })
        .filter(|(distance, _)| *distance <= MAX_DISC_ANGLE)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(CloudSource::WorldIr, |(_, source)| source)
}

/// What to try when a source yields nothing at all.
pub fn fallback(source: CloudSource) -> Option<CloudSource> {
    match source {
        CloudSource::PolarDaily | CloudSource::Auto => None,
        CloudSource::WorldIr => Some(CloudSource::PolarDaily),
        _ => Some(CloudSource::WorldIr),
    }
}

/// Instants of a time-lapse, newest first. The newest is `latest` itself; the
/// others lie on an epoch-aligned grid whose step is the smallest multiple of
/// the data cadence that keeps the total at or below `max_frames`, so
/// consecutive refreshes ask for the same (cached) instants.
pub fn plan_frame_times(
    latest: UtcTime,
    history_hours: i64,
    cadence_minutes: i64,
    max_frames: usize,
) -> Vec<UtcTime> {
    let mut times = vec![latest];
    if history_hours <= 0 || max_frames <= 1 {
        return times;
    }
    let span = history_hours * 3600;
    let cadence = (cadence_minutes * 60).max(60);
    let at_cadence = span / cadence;
    let steps = (max_frames - 1) as i64;
    let step = cadence * ((at_cadence + steps - 1) / steps).max(1);
    let mut time = latest.floor_to(step);
    if time == latest {
        time = time.plus_seconds(-step);
    }
    while time.0 >= latest.0 - span && times.len() < max_frames {
        times.push(time);
        time = time.plus_seconds(-step);
    }
    times
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(times: &[UtcTime]) -> Vec<String> {
        times.iter().map(|time| time.label()).collect()
    }

    #[test]
    fn auto_source_follows_the_nearest_geostationary_disc() {
        let cases = [
            ("Paris", 48.85, 2.35, CloudSource::Meteosat),
            ("Greenwich", 51.48, 0.0, CloudSource::Meteosat),
            ("Madrid", 40.4, -3.7, CloudSource::Meteosat),
            ("Cairo", 30.0, 31.2, CloudSource::IndianOcean),
            ("New York", 40.7, -74.0, CloudSource::GoesEast),
            ("Sao Paulo", -23.5, -46.6, CloudSource::GoesEast),
            ("Los Angeles", 34.05, -118.2, CloudSource::GoesWest),
            ("Mumbai", 19.1, 72.9, CloudSource::IndianOcean),
            ("Tokyo", 35.7, 139.7, CloudSource::WorldIr),
            ("Sydney", -33.9, 151.2, CloudSource::WorldIr),
            ("Reykjavik", 64.1, -21.9, CloudSource::WorldIr),
            ("Honolulu", 21.3, -157.9, CloudSource::GoesWest),
        ];
        for (name, latitude, longitude, expected) in cases {
            assert_eq!(
                choose_source(CloudSource::Auto, false, latitude, longitude),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn explicit_and_global_choices_are_respected() {
        assert_eq!(
            choose_source(CloudSource::GoesWest, false, 48.0, 2.0),
            CloudSource::GoesWest
        );
        assert_eq!(
            choose_source(CloudSource::Auto, true, 48.0, 2.0),
            CloudSource::WorldIr
        );
        assert_eq!(
            choose_source(CloudSource::PolarDaily, true, 0.0, 0.0),
            CloudSource::PolarDaily
        );
    }

    #[test]
    fn fallbacks_end_and_never_loop() {
        assert_eq!(fallback(CloudSource::Meteosat), Some(CloudSource::WorldIr));
        assert_eq!(
            fallback(CloudSource::WorldIr),
            Some(CloudSource::PolarDaily)
        );
        assert_eq!(fallback(CloudSource::PolarDaily), None);
        for source in [
            CloudSource::GoesEast,
            CloudSource::GoesWest,
            CloudSource::IndianOcean,
        ] {
            let mut chain = vec![source];
            while let Some(next) = fallback(*chain.last().unwrap()) {
                assert!(!chain.contains(&next));
                chain.push(next);
            }
            assert!(chain.len() <= 3);
        }
    }

    #[test]
    fn provider_table_matches_the_verified_layers() {
        let east = provider(CloudSource::GoesEast);
        assert_eq!(east.cadence_minutes, 10);
        assert!(matches!(
            east.endpoint,
            Endpoint::GibsTimed {
                layer: "GOES-East_ABI_GeoColor",
                matrix_set: "1km",
                ..
            }
        ));
        let meteosat = provider(CloudSource::Meteosat);
        assert!(matches!(
            meteosat.endpoint,
            Endpoint::Wms {
                layer: "msg_fes:ir108",
                workspace: "msg_fes"
            }
        ));
        assert_eq!(provider(CloudSource::WorldIr).cadence_minutes, 180);
        assert!(matches!(
            provider(CloudSource::PolarDaily).endpoint,
            Endpoint::GibsDaily {
                matrix_set: "250m",
                ..
            }
        ));
        for source in [
            CloudSource::GoesEast,
            CloudSource::GoesWest,
            CloudSource::Meteosat,
            CloudSource::IndianOcean,
            CloudSource::WorldIr,
            CloudSource::PolarDaily,
        ] {
            let record = provider(source);
            assert_eq!(record.source, source);
            assert!(!record.name.is_empty());
            assert_eq!(CloudSource::from_index(source.index()), source);
        }
        assert_eq!(CloudSource::from_index(99), CloudSource::Auto);
        assert_eq!(CloudSource::LABELS.len(), 7);
    }

    #[test]
    fn angular_distance_is_a_metric_on_the_sphere() {
        assert!(angular_distance(0.0, 0.0, 0.0, 0.0).abs() < 1e-9);
        assert!((angular_distance(0.0, 0.0, 0.0, 90.0) - 90.0).abs() < 1e-9);
        assert!((angular_distance(90.0, 0.0, -90.0, 33.0) - 180.0).abs() < 1e-9);
        assert!(
            (angular_distance(10.0, 20.0, 30.0, 40.0) - angular_distance(30.0, 40.0, 10.0, 20.0))
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn live_plan_is_just_the_latest_instant() {
        let latest = UtcTime::from_civil(2026, 9, 30, 12, 50, 0);
        assert_eq!(plan_frame_times(latest, 0, 10, 24), vec![latest]);
        assert_eq!(plan_frame_times(latest, 6, 10, 1), vec![latest]);
    }

    #[test]
    fn time_lapse_plan_is_bounded_aligned_and_newest_first() {
        let latest = UtcTime::from_civil(2026, 9, 30, 12, 50, 0);
        // 6 h of 10-minute data would be 37 images: every 20 min gives 19.
        let six = plan_frame_times(latest, 6, 10, 24);
        assert_eq!(six.len(), 19, "{:?}", labels(&six));
        assert_eq!(six[0], latest);
        assert_eq!(six[1].label(), "2026-09-30 12:40 UTC");
        assert_eq!(six[2].label(), "2026-09-30 12:20 UTC");
        assert_eq!(six[18].label(), "2026-09-30 07:00 UTC");
        // 24 h is thinned to at most 24 frames, still spanning the day.
        let day = plan_frame_times(latest, 24, 10, 24);
        assert!((20..=24).contains(&day.len()), "{}", day.len());
        assert!(day.last().unwrap().0 >= latest.0 - 24 * 3600);
        assert!(day.windows(2).all(|pair| pair[0] > pair[1]));
        // 15-minute Meteosat over 12 h.
        let meteosat = plan_frame_times(UtcTime::from_civil(2026, 9, 30, 13, 15, 0), 12, 15, 24);
        assert!(
            meteosat.len() <= 24 && meteosat.len() >= 12,
            "{}",
            meteosat.len()
        );
        assert!(
            meteosat.iter().skip(1).all(|time| time.0 % 2700 == 0),
            "aligned to 45 min"
        );
        // 3-hourly world map over 24 h: 9 images.
        let world = plan_frame_times(UtcTime::from_civil(2026, 9, 30, 12, 0, 0), 24, 180, 24);
        assert_eq!(world.len(), 9);
        assert_eq!(world[1].label(), "2026-09-30 09:00 UTC");
        // Daily layer, 24 h: today and yesterday.
        let daily = plan_frame_times(UtcTime::from_civil(2026, 9, 30, 0, 0, 0), 24, 1440, 24);
        assert_eq!(
            labels(&daily),
            ["2026-09-30 00:00 UTC", "2026-09-29 00:00 UTC"]
        );
    }

    #[test]
    fn plan_reuses_the_same_grid_after_a_refresh() {
        let first = plan_frame_times(UtcTime::from_civil(2026, 9, 30, 12, 50, 0), 6, 10, 24);
        let second = plan_frame_times(UtcTime::from_civil(2026, 9, 30, 13, 0, 0), 6, 10, 24);
        let shared = second.iter().filter(|time| first.contains(time)).count();
        assert!(shared >= 15, "most instants are already cached: {shared}");
    }
}
