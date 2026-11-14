//! Pure native algorithms shared with the animation authorization broker.
//! No fetcher, filesystem worker or device owner is exported here.
pub mod osm {
    pub use crate::scenes::openstreetmap::geometry::{parse_map, GeometryMap, SourceElement};
    pub use crate::scenes::openstreetmap::render::{MapFeature, MapLayer, MapShape};
}
