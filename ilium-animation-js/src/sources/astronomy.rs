use super::*;
use ilium_ambient::animation_services::astronomy as native;
use serde_json::json;

pub fn catalogue<C: BrokerSourceClient>(
    client: &mut C,
    name: &str,
    max_stars: usize,
) -> Result<Value> {
    if !matches!(name, "bright_stars" | "yale_bright_stars") || !(1..=8404).contains(&max_stars) {
        return types::fail("unknown or oversized star catalogue");
    }
    client.admit_process_baseline("yale_bright_stars", 4 * 1024 * 1024)?;
    let catalogue = native::catalog();
    let stars:Vec<_>=catalogue.stars.iter().take(max_stars).enumerate().map(|(index,star)|json!({"id":format!("bsc-{index}"),"x":star.vector[0],"y":star.vector[1],"z":star.vector[2],"right_ascension_degrees":star.vector[1].atan2(star.vector[0]).to_degrees().rem_euclid(360.0),"declination_degrees":star.vector[2].clamp(-1.0,1.0).asin().to_degrees(),"magnitude":star.magnitude,"color_index":star.color_index})).collect();
    let lines: Vec<_> = catalogue
        .lines
        .iter()
        .filter(|(a, b)| usize::from(*a) < max_stars && usize::from(*b) < max_stars)
        .map(|(a, b)| json!([a, b]))
        .collect();
    Ok(
        json!({"frame":"J2000_equatorial_unit_direction","stars":stars,"constellations":lines,"attribution":"Yale Bright Star Catalogue, 5th revised edition, Hoffleit and Warren 1991, VizieR V/50"}),
    )
}
pub fn observe(epoch_ms: i64, latitude: f64, longitude: f64) -> Result<Value> {
    GeographicBounds {
        west: longitude,
        east: longitude,
        south: latitude,
        north: latitude,
    }
    .validate()?;
    let jd = native::julian_date(epoch_ms as f64 / 1000.0);
    if !(2_378_496.5..=2_469_806.5).contains(&jd) {
        return types::fail("ephemerides supported only within 1800–2050");
    }
    let lst = native::local_sidereal_degrees(jd, longitude);
    let horizon = native::horizon_matrix(latitude, lst);
    let precession = native::precession_matrix(jd);
    let direction = |id: &str, vector: [f64; 3], is_j2000: bool, magnitude: Option<f64>| {
        let dated = if is_j2000 {
            native::mat_vec(&precession, &vector)
        } else {
            vector
        };
        let local = native::mat_vec(&horizon, &dated);
        json!({"id":id,"equatorial_of_date":dated,"east_north_up":local,"altitude_degrees":local[2].clamp(-1.0,1.0).asin().to_degrees(),"azimuth_degrees":local[0].atan2(local[1]).to_degrees().rem_euclid(360.0),"magnitude":magnitude})
    };
    let moon = native::moon_sight(jd);
    let mut bodies = vec![
        direction("sun", native::sun_direction(jd), true, None),
        direction("moon", moon.direction, false, None),
    ];
    for (id, planet) in [
        ("mercury", native::Planet::Mercury),
        ("venus", native::Planet::Venus),
        ("mars", native::Planet::Mars),
        ("jupiter", native::Planet::Jupiter),
        ("saturn", native::Planet::Saturn),
    ] {
        let sight = native::planet_sight(planet, jd);
        bodies.push(direction(id, sight.direction, true, Some(sight.magnitude)));
    }
    let heliocentric: Vec<_> = [
        "mercury",
        "venus",
        "earth_moon_barycenter",
        "mars",
        "jupiter",
        "saturn",
        "uranus",
        "neptune",
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(index, id)| {
        native::solar_heliocentric(index, jd)
            .map(|position| json!({"id":id,"position_au":position}))
    })
    .collect();
    Ok(
        json!({"heliocentric":{"frame":"J2000_ecliptic","units":"astronomical_units","bodies":heliocentric},"epoch_ms":epoch_ms,"julian_date":jd,"local_sidereal_degrees":lst,"units":"unit_direction_not_physical_position","bodies":bodies,"moon":{"horizontal_parallax_degrees":moon.horizontal_parallax_deg,"elongation_degrees":moon.elongation_deg},"attribution":"Native Meeus / JPL approximate ephemerides; geometric geocentric directions, no refraction"}),
    )
}
pub fn weather_layer(layer: &str) -> bool {
    super::satellite::weather_layer(layer)
}
pub fn weather<C: BrokerSourceClient>(
    client: &mut C,
    options: &WeatherOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<WeatherSnapshot> {
    super::satellite::weather(client, options, now_ms, revision, stop)
}
