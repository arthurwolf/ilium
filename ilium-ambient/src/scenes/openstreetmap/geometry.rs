//! Bounded Overpass decoding and local-metre geometry; no I/O.
use super::render::{MapFeature, MapLayer, MapShape};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

type Ring = Vec<[f64; 2]>;
type Polygon = (Ring, Vec<Ring>);

const MAX_ELEMENTS: usize = 30_000;
const MAX_POINTS: usize = 250_000;
const MAX_MEMBERS: usize = 50_000;
const MAX_JOIN_CHECKS: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct SourceElement {
    pub kind: String,
    pub id: u64,
    pub tags: BTreeMap<String, String>,
}
pub struct GeometryMap {
    pub features: Vec<MapFeature>,
    pub sources: Vec<SourceElement>,
    pub timestamp: Option<String>,
    pub incomplete_rings: usize,
    pub orphan_holes: usize,
    pub geometry_budget_exhausted: bool,
}
#[derive(Deserialize)]
struct Document {
    elements: Vec<Element>,
    #[serde(default)]
    remark: Option<String>,
    #[serde(default)]
    osm3s: Metadata,
}
#[derive(Default, Deserialize)]
struct Metadata {
    timestamp_osm_base: Option<String>,
}
#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: u64,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    lat: Option<f64>,
    lon: Option<f64>,
    #[serde(default)]
    geometry: Vec<Option<GeoPoint>>,
    #[serde(default)]
    nodes: Vec<u64>,
    #[serde(default)]
    members: Vec<Member>,
}
#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "ref")]
    id: u64,
    #[serde(default)]
    role: String,
    #[serde(default)]
    geometry: Vec<Option<GeoPoint>>,
}
#[derive(Clone, Copy, Deserialize)]
struct GeoPoint {
    lat: f64,
    lon: f64,
}

pub fn parse_map(bytes: &[u8], center: [f64; 2]) -> Result<GeometryMap, String> {
    if bytes.len() > super::bundle::MAX_JSON_BYTES {
        return Err("OSM JSON exceeds 16 MiB limit".into());
    }
    if !valid_coordinate(center[0], center[1]) || center[0].abs() > 85. {
        return Err("map center must be finite latitude -85..85, longitude -180..180".into());
    }
    let document: Document =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid Overpass JSON: {error}"))?;
    if let Some(remark) = document.remark.filter(|remark| !remark.trim().is_empty()) {
        return Err(format!(
            "Overpass returned an incomplete extract: {}",
            remark.chars().take(240).collect::<String>()
        ));
    }
    if document.elements.len() > MAX_ELEMENTS {
        return Err("extract exceeds element budget".into());
    }
    let mut points = 0usize;
    let mut members = 0usize;
    for element in &document.elements {
        points += element.geometry.len();
        members += element.members.len();
        points += element
            .members
            .iter()
            .map(|member| member.geometry.len())
            .sum::<usize>();
        if points > MAX_POINTS || members > MAX_MEMBERS {
            return Err("extract exceeds geometry budget".into());
        }
        for point in element
            .geometry
            .iter()
            .chain(element.members.iter().flat_map(|member| &member.geometry))
            .flatten()
        {
            if !valid_coordinate(point.lat, point.lon) {
                return Err("extract contains invalid coordinate".into());
            }
        }
        if let (Some(lat), Some(lon)) = (element.lat, element.lon) {
            if !valid_coordinate(lat, lon) {
                return Err("extract contains invalid node coordinate".into());
            }
        }
    }
    let mut map = GeometryMap {
        features: Vec::new(),
        sources: document
            .elements
            .iter()
            .map(|element| SourceElement {
                kind: element.kind.clone(),
                id: element.id,
                tags: element.tags.clone(),
            })
            .collect(),
        timestamp: document.osm3s.timestamp_osm_base,
        incomplete_rings: 0,
        orphan_holes: 0,
        geometry_budget_exhausted: false,
    };
    let ways: BTreeMap<u64, &Element> = document
        .elements
        .iter()
        .filter(|element| element.kind == "way")
        .map(|element| (element.id, element))
        .collect();
    let mut consumed = BTreeSet::new();
    let mut join_checks = 0;
    let mut containment_checks = 0;
    for element in document.elements.iter().filter(|element| {
        element.kind == "relation"
            && element
                .tags
                .get("type")
                .is_some_and(|value| value == "multipolygon")
    }) {
        let layers: Vec<_> = layers(&element.tags)
            .into_iter()
            .filter(|layer| area_layer(*layer))
            .collect();
        if layers.is_empty() {
            continue;
        }
        let mut outers = Vec::new();
        let mut inners = Vec::new();
        let mut member_ids = Vec::new();
        for member in element.members.iter().filter(|member| member.kind == "way") {
            let geometry = if member.geometry.is_empty() {
                ways.get(&member.id)
                    .map(|way| way.geometry.as_slice())
                    .unwrap_or(&[])
            } else {
                &member.geometry
            };
            let target = match member.role.as_str() {
                "outer" | "" => &mut outers,
                "inner" => &mut inners,
                _ => continue,
            };
            let nodes = ways
                .get(&member.id)
                .map(|way| way.nodes.as_slice())
                .unwrap_or(&[]);
            target.extend(boundary_paths(geometry, nodes, center));
            member_ids.push(member.id);
        }
        let outer_paths = stitch(outers, &mut join_checks);
        let inner_paths = stitch(inners, &mut join_checks);
        let mut areas: Vec<Polygon> = Vec::new();
        let mut incomplete = Vec::new();
        for outer in outer_paths {
            if outer.is_closed() {
                areas.push((outer.points, Vec::new()));
            } else {
                map.incomplete_rings += 1;
                incomplete.push(outer.points);
            }
        }
        for inner in inner_paths {
            if !inner.is_closed() {
                map.incomplete_rings += 1;
                incomplete.push(inner.points);
                continue;
            }
            let inner = inner.points;
            // A vertex-only test can accept edges crossing a concave outer.
            // Charge every edge comparison to one extract-wide work budget.
            let owner = areas
                .iter()
                .enumerate()
                .filter(|(_, (outer, _))| valid_hole(outer, &inner, &mut containment_checks))
                .min_by(|(_, (left, _)), (_, (right, _))| {
                    signed_area(left).abs().total_cmp(&signed_area(right).abs())
                })
                .map(|(index, _)| index);
            if let Some(index) = owner {
                areas[index].1.push(inner);
            } else {
                map.orphan_holes += 1;
                incomplete.push(inner);
            }
        }
        if containment_checks >= MAX_JOIN_CHECKS || join_checks >= MAX_JOIN_CHECKS {
            map.geometry_budget_exhausted = true;
            // Unvalidated holes must not silently disappear underneath a fill.
            for (outer, holes) in areas.drain(..) {
                incomplete.push(outer);
                incomplete.extend(holes);
            }
        }
        for layer in layers {
            for (outer, holes) in &areas {
                map.features.push(MapFeature {
                    layer,
                    shape: MapShape::Area {
                        outer: outer.clone(),
                        holes: holes.clone(),
                    },
                });
            }
            for path in &incomplete {
                map.features.push(MapFeature {
                    layer,
                    shape: MapShape::Line(path.clone()),
                });
            }
            // Suppress identical member geometry only for this relation's layer.
            // Otherwise a separately tagged outer would fill over its own holes.
            for id in &member_ids {
                consumed.insert((*id, layer_key(layer)));
            }
        }
    }
    for element in &document.elements {
        if element.kind == "relation" {
            if layers(&element.tags).contains(&MapLayer::PointsOfInterest) {
                let anchor = element.members.iter().find_map(|member| {
                    let geometry = if member.geometry.is_empty() {
                        ways.get(&member.id)
                            .map(|way| way.geometry.as_slice())
                            .unwrap_or(&[])
                    } else {
                        &member.geometry
                    };
                    geometry.iter().flatten().next().copied()
                });
                if let Some(anchor) = anchor {
                    map.features.push(MapFeature {
                        layer: MapLayer::PointsOfInterest,
                        shape: MapShape::Point(project(anchor, center)),
                    });
                }
            }
            continue;
        }
        for layer in layers(&element.tags) {
            if element.kind == "way" && consumed.contains(&(element.id, layer_key(layer))) {
                continue;
            }
            if element.kind == "node" {
                if let (Some(lat), Some(lon)) = (element.lat, element.lon) {
                    map.features.push(MapFeature {
                        layer,
                        shape: MapShape::Point(project(GeoPoint { lat, lon }, center)),
                    });
                }
                continue;
            }
            if element.kind != "way" {
                continue;
            }
            if layer == MapLayer::PointsOfInterest {
                // A surveyed boundary vertex is an honest anchor; it is not
                // advertised as an inferred centroid or interior location.
                if let Some(anchor) = element.geometry.iter().flatten().next() {
                    map.features.push(MapFeature {
                        layer,
                        shape: MapShape::Point(project(*anchor, center)),
                    });
                }
                continue;
            }
            for boundary in boundary_paths(&element.geometry, &element.nodes, center) {
                let is_closed = boundary.is_closed();
                let path = boundary.points;
                let shape = if area_layer(layer)
                    && is_closed
                    && element.tags.get("area").is_none_or(|value| value != "no")
                    && element
                        .tags
                        .get("natural")
                        .is_none_or(|value| value != "coastline")
                {
                    MapShape::Area {
                        outer: path,
                        holes: Vec::new(),
                    }
                } else {
                    MapShape::Line(path)
                };
                map.features.push(MapFeature { layer, shape });
            }
        }
    }
    Ok(map)
}
fn valid_coordinate(lat: f64, lon: f64) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && (-90. ..=90.).contains(&lat)
        && (-180. ..=180.).contains(&lon)
}
fn project(point: GeoPoint, center: [f64; 2]) -> [f64; 2] {
    let longitude = (point.lon - center[1] + 180.).rem_euclid(360.) - 180.;
    [
        longitude.to_radians() * 6_371_008.8 * center[0].to_radians().cos(),
        (point.lat - center[0]).to_radians() * 6_371_008.8,
    ]
}
struct BoundaryPath {
    points: Ring,
    endpoints: [Option<u64>; 2],
}
impl BoundaryPath {
    fn is_closed(&self) -> bool {
        closed(&self.points) && compatible_ids(self.endpoints[0], self.endpoints[1])
    }
}
fn compatible_ids(left: Option<u64>, right: Option<u64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        // Coordinate-only fragments may join each other, but cannot erase
        // known node identity through an intervening unknown fragment.
        (None, None) => true,
        _ => false,
    }
}
fn boundary_paths(
    geometry: &[Option<GeoPoint>],
    nodes: &[u64],
    center: [f64; 2],
) -> Vec<BoundaryPath> {
    let aligned = nodes.len() == geometry.len();
    let mut result = Vec::new();
    let mut start = 0;
    while start < geometry.len() {
        if geometry[start].is_none() {
            start += 1;
            continue;
        }
        let end = geometry[start..]
            .iter()
            .position(Option::is_none)
            .map_or(geometry.len(), |offset| start + offset);
        if end - start >= 2 {
            result.push(BoundaryPath {
                points: geometry[start..end]
                    .iter()
                    .flatten()
                    .map(|point| project(*point, center))
                    .collect(),
                endpoints: if aligned {
                    [Some(nodes[start]), Some(nodes[end - 1])]
                } else {
                    [None, None]
                },
            });
        }
        start = end;
    }
    result
}
fn closed(path: &[[f64; 2]]) -> bool {
    path.len() >= 4 && path.first() == path.last()
}
fn area_layer(layer: MapLayer) -> bool {
    matches!(
        layer,
        MapLayer::Buildings | MapLayer::Water | MapLayer::GreenSpace
    )
}
fn layer_key(layer: MapLayer) -> u8 {
    match layer {
        MapLayer::Roads => 0,
        MapLayer::Buildings => 1,
        MapLayer::Water => 2,
        MapLayer::GreenSpace => 3,
        MapLayer::Railways => 4,
        MapLayer::PointsOfInterest => 5,
    }
}
fn layers(tags: &BTreeMap<String, String>) -> Vec<MapLayer> {
    let mut layers = Vec::new();
    let tagged = |key: &str| {
        tags.get(key)
            .is_some_and(|value| !value.is_empty() && value != "no")
    };
    let matches = |key: &str, values: &[&str]| {
        tags.get(key)
            .is_some_and(|value| values.contains(&value.as_str()))
    };
    if tagged("highway") {
        layers.push(MapLayer::Roads);
    }
    if tagged("building") || tagged("building:part") {
        layers.push(MapLayer::Buildings);
    }
    if tagged("waterway")
        || matches("natural", &["water", "bay", "coastline"])
        || matches("landuse", &["reservoir", "basin"])
    {
        layers.push(MapLayer::Water);
    }
    if matches(
        "natural",
        &["wood", "scrub", "grassland", "heath", "wetland"],
    ) || matches(
        "landuse",
        &[
            "forest",
            "farmland",
            "farmyard",
            "grass",
            "meadow",
            "recreation_ground",
            "village_green",
            "orchard",
            "allotments",
        ],
    ) || matches(
        "leisure",
        &["park", "garden", "nature_reserve", "golf_course"],
    ) {
        layers.push(MapLayer::GreenSpace);
    }
    if tagged("railway") {
        layers.push(MapLayer::Railways);
    }
    if tagged("amenity") || tagged("tourism") || tagged("historic") || tagged("shop") {
        layers.push(MapLayer::PointsOfInterest);
    }
    layers
}
fn stitch(mut pieces: Vec<BoundaryPath>, checks: &mut usize) -> Vec<BoundaryPath> {
    let mut result = Vec::new();
    while let Some(mut path) = pieces.pop() {
        while !path.is_closed() && *checks < MAX_JOIN_CHECKS {
            let mut matched = None;
            for (index, candidate) in pieces.iter().enumerate() {
                *checks += 1;
                if *checks >= MAX_JOIN_CHECKS {
                    break;
                }
                let start = path.points[0];
                let end = path.points[path.points.len() - 1];
                let first = candidate.points[0];
                let last = candidate.points[candidate.points.len() - 1];
                let connection = if end == first
                    && compatible_ids(path.endpoints[1], candidate.endpoints[0])
                {
                    Some((false, false))
                } else if end == last && compatible_ids(path.endpoints[1], candidate.endpoints[1]) {
                    Some((false, true))
                } else if start == last && compatible_ids(path.endpoints[0], candidate.endpoints[1])
                {
                    Some((true, false))
                } else if start == first
                    && compatible_ids(path.endpoints[0], candidate.endpoints[0])
                {
                    Some((true, true))
                } else {
                    None
                };
                if let Some(connection) = connection {
                    matched = Some((index, connection));
                    break;
                }
            }
            let Some((index, (prepend, reverse))) = matched else {
                break;
            };
            let mut candidate = pieces.swap_remove(index);
            if reverse {
                candidate.points.reverse();
                candidate.endpoints.swap(0, 1);
            }
            if prepend {
                candidate.points.pop();
                candidate.points.extend(path.points);
                candidate.endpoints[1] = path.endpoints[1];
                path = candidate;
            } else {
                path.points.extend(candidate.points.into_iter().skip(1));
                path.endpoints[1] = candidate.endpoints[1];
            }
        }
        result.push(path);
    }
    result
}
fn signed_area(path: &[[f64; 2]]) -> f64 {
    path.windows(2)
        .map(|pair| pair[0][0] * pair[1][1] - pair[1][0] * pair[0][1])
        .sum::<f64>()
        * 0.5
}
fn valid_hole(outer: &[[f64; 2]], inner: &[[f64; 2]], checks: &mut usize) -> bool {
    for point in inner {
        let mut inside = false;
        for edge in outer.windows(2) {
            if !charge(checks) {
                return false;
            }
            let [a, b] = [edge[0], edge[1]];
            if (a[1] > point[1]) != (b[1] > point[1])
                && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                inside = !inside;
            }
        }
        if !inside {
            return false;
        }
    }
    for inner_edge in inner.windows(2) {
        for outer_edge in outer.windows(2) {
            if !charge(checks)
                || intersects(inner_edge[0], inner_edge[1], outer_edge[0], outer_edge[1])
            {
                return false;
            }
        }
    }
    true
}
fn charge(checks: &mut usize) -> bool {
    if *checks >= MAX_JOIN_CHECKS {
        return false;
    }
    *checks += 1;
    true
}
fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orientation = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let on_segment = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        p[0] >= a[0].min(b[0])
            && p[0] <= a[0].max(b[0])
            && p[1] >= a[1].min(b[1])
            && p[1] <= a[1].max(b[1])
    };
    let [ac, ad, ca, cb] = [
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    ];
    (ac * ad < 0. && ca * cb < 0.)
        || (ac == 0. && on_segment(a, b, c))
        || (ad == 0. && on_segment(a, b, d))
        || (ca == 0. && on_segment(c, d, a))
        || (cb == 0. && on_segment(c, d, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_paris_fixture_preserves_all_six_tagged_layers() {
        let map = parse_map(
            include_bytes!("fixtures/paris-layers.json"),
            [48.8584, 2.2945],
        )
        .unwrap();
        for layer in [
            MapLayer::Roads,
            MapLayer::Buildings,
            MapLayer::Water,
            MapLayer::GreenSpace,
            MapLayer::Railways,
            MapLayer::PointsOfInterest,
        ] {
            assert!(
                map.features.iter().any(|feature| feature.layer == layer),
                "{layer:?}"
            );
        }
    }
    #[test]
    fn clipped_geometry_does_not_connect_across_missing_nodes() {
        // Synthetic transport fixture: two independent pieces of one clipped way.
        let bytes = br#"{"elements":[{"type":"way","id":1,"tags":{"highway":"residential"},"geometry":[{"lat":0,"lon":0},{"lat":0,"lon":0.001},null,{"lat":0,"lon":0.003},{"lat":0,"lon":0.004}]}]}"#;
        let map = parse_map(bytes, [0., 0.]).unwrap();
        assert_eq!(map.features.len(), 2);
        assert!(map
            .features
            .iter()
            .all(|feature| matches!(&feature.shape, MapShape::Line(points) if points.len() == 2)));
    }
    #[test]
    fn every_catalogue_has_real_drawable_geometry() {
        for place in super::super::catalogue::PLACES {
            let bytes = super::super::bundle::decode_bundle(place.data).unwrap();
            let map = parse_map(&bytes, [place.latitude, place.longitude]).unwrap();
            assert!(map.features.len() > 100, "{}", place.name);
            let mut raster = crate::raster::Raster::default();
            raster.resize(160, 96);
            super::super::render::draw_map(
                &mut raster,
                &map.features,
                &super::super::settings::OpenStreetMapSettings::default(),
                0.,
            );
            assert!(raster
                .dots
                .iter()
                .all(|dot| dot.is_finite() && (0. ..=1.).contains(dot)));
            assert!(
                raster.dots.iter().filter(|dot| **dot > 0.1).count() > 100,
                "{} must actually rasterize",
                place.name
            );
        }
    }
    #[test]
    fn synthetic_multipolygon_joins_reversed_members_and_preserves_holes() {
        let point = |lat, lon| serde_json::json!({"lat":lat,"lon":lon});
        let a = point(0., 0.);
        let b = point(0., 0.01);
        let c = point(0.01, 0.01);
        let d = point(0.01, 0.);
        let inner = vec![
            point(0.003, 0.003),
            point(0.003, 0.007),
            point(0.007, 0.007),
            point(0.007, 0.003),
            point(0.003, 0.003),
        ];
        let document = serde_json::json!({"elements":[
            {"type":"way","id":10,"tags":{"natural":"water"},"geometry":[a,b,c,d,a]},
            {"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"water"},"members":[
                {"type":"way","ref":10,"role":"outer","geometry":[a,b,c]},
                {"type":"way","ref":11,"role":"outer","geometry":[a,d,c]},
                {"type":"way","ref":12,"role":"inner","geometry":inner}
            ]}
        ]});
        let map = parse_map(&serde_json::to_vec(&document).unwrap(), [0.005, 0.005]).unwrap();
        assert_eq!(
            map.features.len(),
            1,
            "tagged member must not refill its hole"
        );
        assert!(
            matches!(&map.features[0].shape,MapShape::Area{outer,holes} if outer.len()==5 && holes.len()==1)
        );
        assert_eq!(map.sources.len(), 2);
        assert_eq!(map.sources[1].id, 20);
        assert_eq!(map.sources[1].tags["natural"], "water");
        assert_eq!((map.incomplete_rings, map.orphan_holes), (0, 0));
    }
    #[test]
    fn partial_responses_invalid_coordinates_and_centers_are_rejected() {
        assert!(parse_map(
            br#"{"remark":"runtime error: timed out","elements":[]}"#,
            [0., 0.]
        )
        .is_err());
        assert!(parse_map(
            br#"{"elements":[{"type":"way","id":1,"geometry":[{"lat":91,"lon":0}]}]}"#,
            [0., 0.]
        )
        .is_err());
        for center in [[f64::NAN, 0.], [86., 0.], [0., 181.]] {
            assert!(parse_map(br#"{"elements":[]}"#, center).is_err());
        }
        assert!(parse_map(
            &vec![b' '; super::super::bundle::MAX_JSON_BYTES + 1],
            [0., 0.]
        )
        .is_err());
    }
    #[test]
    fn projection_crosses_dateline_by_the_short_arc_and_scales_longitude() {
        let origin = project(
            GeoPoint {
                lat: 0.,
                lon: -179.999,
            },
            [0., 179.999],
        );
        assert!((origin[0] - 222.39016).abs() < 0.01);
        assert_eq!(origin[1], 0.);
        let equator = project(GeoPoint { lat: 0., lon: 0.01 }, [0., 0.]);
        let north = project(
            GeoPoint {
                lat: 60.,
                lon: 0.01,
            },
            [60., 0.],
        );
        assert!((north[0] / equator[0] - 0.5).abs() < 1e-9);
    }
    #[test]
    fn concave_outer_rejects_hole_edge_escaping_between_inside_vertices() {
        let outer = vec![
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [3., 4.],
            [3., 1.],
            [1., 1.],
            [1., 4.],
            [0., 4.],
            [0., 0.],
        ];
        let inner = vec![[0.5, 3.], [3.5, 3.], [2., 0.5], [0.5, 3.]];
        assert!(!valid_hole(&outer, &inner, &mut 0));
        let mut checks = MAX_JOIN_CHECKS - 1;
        assert!(!valid_hole(&outer, &inner, &mut checks));
        assert_eq!(checks, MAX_JOIN_CHECKS);
        assert!(
            layers(&BTreeMap::from([("landuse".into(), "farmland".into())]))
                .contains(&MapLayer::GreenSpace)
        );
    }
    #[test]
    fn area_poi_is_a_separate_point_anchor_without_repeating_the_footprint() {
        let bytes=br#"{"elements":[{"type":"way","id":1,"tags":{"building":"yes","amenity":"school"},"geometry":[{"lat":0,"lon":0},{"lat":0,"lon":0.001},{"lat":0.001,"lon":0.001},{"lat":0,"lon":0}]}]}"#;
        let map = parse_map(bytes, [0., 0.]).unwrap();
        assert!(map
            .features
            .iter()
            .any(|feature| feature.layer == MapLayer::Buildings
                && matches!(feature.shape, MapShape::Area { .. })));
        assert!(map
            .features
            .iter()
            .filter(|feature| feature.layer == MapLayer::PointsOfInterest)
            .all(|feature| matches!(feature.shape, MapShape::Point(_))));
    }
    #[test]
    fn coincident_coordinates_with_distinct_node_ids_do_not_close_a_ring() {
        let document = serde_json::json!({"elements":[
            {"type":"way","id":10,"nodes":[1,2,3],"geometry":[{"lat":0.,"lon":0.},{"lat":0.,"lon":0.01},{"lat":0.01,"lon":0.01}]},
            {"type":"way","id":11,"nodes":[9,4,3],"geometry":[{"lat":0.,"lon":0.},{"lat":0.01,"lon":0.},{"lat":0.01,"lon":0.01}]},
            {"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"water"},"members":[{"type":"way","ref":10,"role":"outer"},{"type":"way","ref":11,"role":"outer"}]}
        ]});
        let map = parse_map(&serde_json::to_vec(&document).unwrap(), [0., 0.]).unwrap();
        assert!(map
            .features
            .iter()
            .all(|feature| !matches!(feature.shape, MapShape::Area { .. })));
        assert!(map.incomplete_rings > 0);
    }
    #[test]
    fn unknown_inline_connector_cannot_erase_known_node_conflict() {
        let p = |lat, lon| serde_json::json!({"lat":lat,"lon":lon});
        let a = p(0., 0.);
        let b = p(0., 0.01);
        let c = p(0.01, 0.01);
        let d = p(0.01, 0.);
        let doc = serde_json::json!({"elements":[
            {"type":"way","id":10,"nodes":[1,2,3],"geometry":[a,b,c]},
            {"type":"way","id":11,"nodes":[30,4,1],"geometry":[c,d,a]},
            {"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"water"},"members":[
                {"type":"way","ref":10,"role":"outer"},
                {"type":"way","ref":12,"role":"outer","geometry":[c,c]},
                {"type":"way","ref":11,"role":"outer"}
            ]}
        ]});
        let map = parse_map(&serde_json::to_vec(&doc).unwrap(), [0., 0.]).unwrap();
        let areas = map
            .features
            .iter()
            .filter(|f| matches!(f.shape, MapShape::Area { .. }))
            .count();
        println!(
            "{}",
            serde_json::json!({"type":"result","area_count":areas,"incomplete_rings":map.incomplete_rings})
        );
        assert_eq!(
            areas, 0,
            "inline connector must not erase known distinct endpoint IDs3 vs30"
        );
    }
}
