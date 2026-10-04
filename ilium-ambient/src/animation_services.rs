//! Pure native algorithms shared with the animation authorization broker.
//! No fetcher, filesystem worker or device owner is exported here.
pub mod astronomy {
    pub use crate::scenes::stars::astro::{
        horizon_matrix, julian_date, local_sidereal_degrees, mat_vec, moon_sight, planet_sight,
        precession_matrix, sun_direction, Planet,
    };
    pub use crate::scenes::stars::catalog::{catalog, Catalog, Star};

    /// Heliocentric J2000 ecliptic position in astronomical units. Invalid
    /// body indices and non-finite dates never become a synthetic origin.
    pub fn solar_heliocentric(index: usize, julian_day: f64) -> Option<[f64; 3]> {
        (index < 8 && julian_day.is_finite())
            .then(|| crate::scenes::stars::astro::solar_heliocentric(index, julian_day))
    }

    /// Closed native J2000 orbit, parameterized by a turn fraction.
    pub fn solar_orbit_position(index: usize, fraction: f64) -> Option<[f64; 3]> {
        (index < 8 && fraction.is_finite())
            .then(|| crate::scenes::stars::astro::solar_orbit_position(index, fraction))
    }
}
pub mod satellite {
    pub use crate::scenes::clouds::providers::{
        choose_source, fallback, plan_frame_times, provider, CloudSource, Endpoint, Provider,
    };
    pub use crate::scenes::night_lights::tiles::{
        gibs_domains_url, gibs_tile_url, level_for_density, level_within_budget, matrix_size,
        parse_domains, tile_span_degrees, tiles_for_box, wms_capabilities_url, wms_default_time,
        wms_map_url, GeoBox, TileId, UtcTime,
    };
}
pub mod topography {
    pub const MEASURED_BODY_MANIFEST: &str = include_str!("../assets/topography/manifest.json");
    pub use crate::scenes::topographic_maps::data::{load_world, Heightfield};
    pub use crate::scenes::topographic_maps::settings::WorldId;
}
pub mod osm {
    pub use crate::scenes::openstreetmap::geometry::{parse_map, GeometryMap, SourceElement};
    pub use crate::scenes::openstreetmap::render::{MapFeature, MapLayer, MapShape};
}
