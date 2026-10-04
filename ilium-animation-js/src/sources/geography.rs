use super::*;
use ilium_ambient::{
    geocode as native_geocode,
    live_data::{model::Position, openseafeed, parse},
};
use serde_json::{json, Value};

fn entity(position: &Position) -> GeoEntity {
    GeoEntity {
        id: position.id.clone(),
        latitude: position.latitude,
        longitude: position.longitude,
        label: position.label.clone(),
        epoch_ms: position.observed_ms,
        magnitude: None,
        depth_km: None,
        altitude_m: None,
        heading_degrees: position.heading_degrees,
        speed_mps: position.speed_metres_per_second,
        callsign: None,
        vessel_type: None,
    }
}
fn filter(entities: &mut Vec<GeoEntity>, options: &GeoOptions) -> Option<i64> {
    entities.retain(|entity| options.bounds.contains(entity.latitude, entity.longitude));
    entities.truncate(options.max_entities);
    let observed = entities.iter().filter_map(|entity| entity.epoch_ms).max();
    for entity in entities.iter_mut() {
        if !options.requests_field("epoch_ms") {
            entity.epoch_ms = None;
        }
        if !options.requests_field("label") {
            entity.label = None;
        }
        if !options.requests_field("magnitude") {
            entity.magnitude = None;
        }
        if !options.requests_field("depth_km") {
            entity.depth_km = None;
        }
        if !options.requests_field("altitude_m") {
            entity.altitude_m = None;
        }
        if !options.requests_field("heading_degrees") {
            entity.heading_degrees = None;
        }
        if !options.requests_field("speed_mps") {
            entity.speed_mps = None;
        }
        if !options.requests_field("callsign") {
            entity.callsign = None;
        }
        if !options.requests_field("vessel_type") {
            entity.vessel_type = None;
        }
    }
    observed
}
pub fn earthquakes<C: BrokerSourceClient>(
    client: &mut C,
    options: &GeoOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<GeoSnapshot> {
    options.validate()?;
    let data = bytes(
        client,
        http_options(
            "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson".into(),
            8_000_000,
            "text",
        ),
        stop,
    )?;
    let decoded = parse::usgs(&data).map_err(AnimationError::Runtime)?;
    let mut entities: Vec<_> = decoded
        .items
        .into_iter()
        .map(|quake| {
            let mut point = entity(&quake.position);
            point.magnitude = quake.magnitude;
            point.depth_km = quake.depth_km;
            point
        })
        .collect();
    let observed = filter(&mut entities, options);
    Ok(GeoSnapshot {
        metadata: captured(revision, now_ms, observed),
        entities,
        attribution: "USGS — all reported magnitudes, including zero/negative/unknown".into(),
        rejected: decoded.rejected,
        coverage: Some("Reported events in the provider's past-day feed".into()),
    })
}
pub fn aircraft<C: BrokerSourceClient>(
    client: &mut C,
    options: &GeoOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<GeoSnapshot> {
    options.validate()?;
    let mut request = http_options(
        "https://opensky-network.org/api/states/all".into(),
        8_000_000,
        "text",
    );
    request.credential = options.credential.clone();
    let data = bytes(client, request, stop)?;
    let decoded = parse::opensky(&data).map_err(AnimationError::Runtime)?;
    let raw: Option<Value> = if options.requests_field("altitude_m") {
        Some(serde_json::from_slice(&data)?)
    } else {
        None
    };
    let altitudes: BTreeMap<&str, f64> = raw
        .as_ref()
        .and_then(|value| value["states"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row[0].as_str()?,
                row[13].as_f64().or_else(|| row[7].as_f64())?,
            ))
        })
        .filter(|(_, altitude)| altitude.is_finite())
        .collect();
    let mut entities: Vec<_> = decoded
        .items
        .iter()
        .map(|position| {
            let mut point = entity(position);
            point.callsign = position.label.clone();
            point.altitude_m = altitudes.get(position.id.as_str()).copied();
            point
        })
        .collect();
    let observed = filter(&mut entities, options);
    Ok(GeoSnapshot {metadata:captured(revision,now_ms,observed),entities,attribution:"OpenSky Network".into(),rejected:decoded.rejected,coverage:Some("Received positioned aircraft only; reception is incomplete, coordinate-fix time may be unknown".into())})
}
pub fn boats<C: BrokerSourceClient>(
    client: &mut C,
    options: &GeoOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<GeoSnapshot> {
    options.validate()?;
    if options.provider == Some(BoatProvider::Digitraffic) {
        let mut request = http_options(
            "https://meri.digitraffic.fi/api/ais/v1/locations".into(),
            8_000_000,
            "text",
        );
        request.credential = options.credential.clone();
        let data = bytes(client, request, stop)?;
        let decoded = parse::digitraffic(&data).map_err(AnimationError::Runtime)?;
        let mut entities: Vec<_> = decoded.items.iter().map(entity).collect();
        let observed = filter(&mut entities, options);
        return Ok(GeoSnapshot{metadata:captured(revision,now_ms,observed),entities,attribution:"Fintraffic / Digitraffic · CC BY 4.0; https://www.digitraffic.fi/en/marine-traffic/".into(),rejected:decoded.rejected,coverage:Some("Finnish waters only; incomplete received AIS positions; timestampExternal is provider fix time".into())});
    }
    let mut request = http_options(
        openseafeed::ENDPOINT.into(),
        openseafeed::MAX_RESPONSE_BYTES,
        "text",
    );
    request.credential = options.credential.clone();
    let data = bytes(client, request, stop)?;
    let decoded = openseafeed::decode(&data, stop).map_err(AnimationError::Runtime)?;
    let mut entities: Vec<_> = decoded.positions.iter().map(entity).collect();
    let _ = filter(&mut entities, options);
    Ok(GeoSnapshot {
        metadata: captured(revision, now_ms, None),
        entities,
        attribution: format!(
            "{} · {}",
            openseafeed::ATTRIBUTION,
            openseafeed::LICENSE_URL
        ),
        rejected: decoded.counts.malformed,
        coverage: Some(openseafeed::COVERAGE.into()),
    })
}

pub fn project(latitude: f64, longitude: f64, projection: &str) -> Result<Value> {
    GeographicBounds {
        west: longitude,
        east: longitude,
        south: latitude,
        north: latitude,
    }
    .validate()?;
    let (x, y) = match projection {
        "equirectangular" => ((longitude + 180.0) / 360.0, (90.0 - latitude) / 180.0),
        "mercator" => {
            let lat = latitude.clamp(-85.051_128_78, 85.051_128_78).to_radians();
            (
                (longitude + 180.0) / 360.0,
                (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0,
            )
        }
        // Default centre is Greenwich/equator; back hemisphere is explicitly
        // invisible rather than folded onto the front disk.
        "orthographic" => {
            let lat = latitude.to_radians();
            let lon = longitude.to_radians();
            if lat.cos() * lon.cos() < 0.0 {
                return Ok(json!({"x":0.0,"y":0.0,"visible":false}));
            }
            ((1.0 + lat.cos() * lon.sin()) / 2.0, (1.0 - lat.sin()) / 2.0)
        }
        _ => return types::fail("unknown map projection"),
    };
    Ok(json!({"x":x,"y":y,"visible":true}))
}

pub fn geocode<C: BrokerSourceClient>(
    client: &mut C,
    query: &str,
    max_results: usize,
    stop: &AtomicBool,
) -> Result<Value> {
    if query.trim().is_empty()
        || query.len() > 512
        || query.chars().any(char::is_control)
        || !(1..=50).contains(&max_results)
    {
        return types::fail("invalid geocoding request");
    }
    let mut url = url::Url::parse("https://geocoding-api.open-meteo.com/v1/search")
        .map_err(|_| AnimationError::Runtime("invalid built-in geocoder URL".into()))?;
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", &max_results.to_string())
        .append_pair("language", "en")
        .append_pair("format", "json");
    let data = bytes(
        client,
        http_options(url.to_string(), 1024 * 1024, "text"),
        stop,
    )?;
    let locations = native_geocode::parse_response(&data).map_err(AnimationError::Runtime)?;
    Ok(Value::Array(locations.into_iter().take(max_results).enumerate().map(|(index,location)|json!({"id":format!("city-{index}-{:.6}-{:.6}",location.latitude,location.longitude),"latitude":location.latitude,"longitude":location.longitude,"label":location.label,"attribution":"Open-Meteo geocoding / GeoNames"})).collect()))
}

pub fn osm_tile<C: BrokerSourceClient>(
    client: &mut C,
    x: u32,
    y: u32,
    zoom: u8,
    format: &str,
    stop: &AtomicBool,
) -> Result<Value> {
    if zoom > 19 || x >= 1u32 << zoom || y >= 1u32 << zoom {
        return types::fail("invalid tile coordinates");
    }
    if format == "raster" {
        let data = bytes(
            client,
            http_options(
                format!("https://tile.openstreetmap.org/{zoom}/{x}/{y}.png"),
                2 * 1024 * 1024,
                "bytes",
            ),
            stop,
        )?;
        let image = client.decode_image(&data, 256 * 256, stop)?;
        Ok(
            json!({"image":image,"attribution":"© OpenStreetMap contributors, https://www.openstreetmap.org/copyright"}),
        )
    } else if format == "vector" {
        let n = f64::from(1u32 << zoom);
        let longitude = |x: f64| x / n * 360.0 - 180.0;
        let latitude = |y: f64| {
            (std::f64::consts::PI * (1.0 - 2.0 * y / n))
                .sinh()
                .atan()
                .to_degrees()
        };
        let west = longitude(f64::from(x));
        let east = longitude(f64::from(x + 1));
        let north = latitude(f64::from(y));
        let south = latitude(f64::from(y + 1));
        if zoom < 14 {
            return types::fail(
                "vector tile extraction needs zoom>=14 for the bounded native local-map window",
            );
        }
        let bbox = format!("{south},{west},{north},{east}");
        // Same feature families as native openstreetmap/request.rs, including
        // relations and multipolygon holes; the decoder owns geometry limits.
        let selectors = [
            "highway",
            "building",
            "building:part",
            "waterway",
            "natural",
            "landuse",
            "leisure",
            "railway",
            "amenity",
            "tourism",
            "historic",
            "shop",
        ]
        .into_iter()
        .map(|tag| format!("nwr[\"{tag}\"]({bbox});"))
        .collect::<String>();
        let query =
            format!("[out:json][timeout:15][maxsize:8388608];({selectors});out geom({bbox});");
        let mut url = url::Url::parse("https://overpass-api.de/api/interpreter")
            .map_err(|_| AnimationError::Runtime("invalid Overpass URL".into()))?;
        url.query_pairs_mut().append_pair("data", &query);
        let data = bytes(
            client,
            http_options(url.to_string(), 8 * 1024 * 1024, "text"),
            stop,
        )?;
        let center = [((south + north) / 2.).clamp(-85., 85.), (west + east) / 2.];
        let map = ilium_ambient::animation_services::osm::parse_map(&data, center)
            .map_err(AnimationError::Runtime)?;
        geometry_paths(map, center, x, y, zoom)
    } else {
        types::fail("unknown OSM tile format")
    }
}

fn geometry_paths(
    map: ilium_ambient::animation_services::osm::GeometryMap,
    center: [f64; 2],
    x: u32,
    y: u32,
    zoom: u8,
) -> Result<Value> {
    use ilium_ambient::animation_services::osm::{MapLayer, MapShape};
    let n = f64::from(1u32 << zoom);
    let point = |position: [f64; 2]| {
        let latitude = center[0] + (position[1] / 6_371_008.8).to_degrees();
        let longitude =
            center[1] + (position[0] / (6_371_008.8 * center[0].to_radians().cos())).to_degrees();
        let lat = latitude.clamp(-85.051_128_78, 85.051_128_78).to_radians();
        json!({"x":(longitude+180.0)/360.0*n-f64::from(x),"y":(1.0-(lat.tan()+1.0/lat.cos()).ln()/std::f64::consts::PI)/2.0*n-f64::from(y)})
    };
    let features:Vec<_>=map.features.iter().map(|feature| {
        let layer=match feature.layer {MapLayer::Roads=>"roads",MapLayer::Buildings=>"buildings",MapLayer::Water=>"water",MapLayer::GreenSpace=>"green_space",MapLayer::Railways=>"railways",MapLayer::PointsOfInterest=>"points_of_interest"};
        match &feature.shape {
            MapShape::Point(position)=>json!({"layer":layer,"kind":"point","point":point(*position)}),
            MapShape::Line(points)=>json!({"layer":layer,"kind":"line","points":points.iter().copied().map(point).collect::<Vec<_>>()}),
            MapShape::Area{outer,holes}=>json!({"layer":layer,"kind":"area","outer":outer.iter().copied().map(point).collect::<Vec<_>>(),"holes":holes.iter().map(|ring|ring.iter().copied().map(point).collect::<Vec<_>>()).collect::<Vec<_>>()}),
        }
    }).collect();
    Ok(
        json!({"features":features,"space":"normalized_web_mercator_tile","timestamp":map.timestamp,"incomplete_rings":map.incomplete_rings,"orphan_holes":map.orphan_holes,"budget_exhausted":map.geometry_budget_exhausted,"attribution":"© OpenStreetMap contributors, ODbL, https://www.openstreetmap.org/copyright"}),
    )
}
