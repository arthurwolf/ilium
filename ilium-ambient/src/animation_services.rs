//! Pure native algorithms shared with the animation authorization broker.
//! No fetcher, filesystem worker or device owner is exported here.
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
pub mod osm {
    pub use crate::scenes::openstreetmap::geometry::{parse_map, GeometryMap, SourceElement};
    pub use crate::scenes::openstreetmap::render::{MapFeature, MapLayer, MapShape};
}
