//! Provider-specific validation. Invalid individual rows are omitted with
//! counts; a malformed top-level response is an error, never a new empty feed.
use super::model::{Candle, Earthquake, Position};
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usgs_retains_negative_and_unknown_magnitudes_and_provider_time() {
        let json = r#"{"type":"FeatureCollection","features":[
          {"id":"tiny","properties":{"mag":-0.7,"time":100,"place":"Test"},"geometry":{"type":"Point","coordinates":[180,-90,3]}},
          {"id":"unknown","properties":{"mag":null,"time":200},"geometry":{"type":"Point","coordinates":[0,1,2]}},
          {"id":"bad","properties":{"mag":2,"time":300},"geometry":{"type":"Point","coordinates":[181,1,2]}}
        ]}"#;
        let decoded = usgs(json.as_bytes()).unwrap();
        assert_eq!(decoded.items.len(), 2);
        assert_eq!(decoded.rejected, 1);
        assert_eq!(decoded.items[0].magnitude, Some(-0.7));
        assert_eq!(decoded.items[1].magnitude, None);
        assert_eq!(decoded.items[0].position.observed_ms, Some(100)); // Preserve the known event time.
        assert!(usgs(br#"{"error":"outage"}"#).is_err());
    }

    #[test]
    fn coinbase_order_is_time_low_high_open_close_volume_not_ohlc() {
        let candles =
            coinbase_candles(b"[[100,1,4,2,3,5],[90,0,2,1,1.5,6],[80,4,1,2,3,5]]").unwrap();
        assert_eq!(candles.rejected, 1);
        assert_eq!(candles.items[0].observed_ms, 90_000);
        assert_eq!(candles.items[1].open, 2.0);
        assert_eq!(candles.items[1].low, 1.0);
        assert_eq!(candles.items[1].close, 3.0);
        assert!(coinbase_candles(b"{}").is_err());
    }

    #[test]
    fn malformed_or_oversized_provider_payload_is_an_error() {
        assert!(usgs(b"not json").is_err());
        assert!(coinbase_candles(&vec![b' '; MAX_RESPONSE_BYTES + 1]).is_err());
    }
}

pub const MAX_RESPONSE_BYTES: usize = 8_000_000;
pub const MAX_ITEMS: usize = 100_000;

#[derive(Debug)]
pub struct Decoded<T> {
    pub items: Vec<T>,
    pub rejected: usize,
    pub unpositioned: usize, // Valid identities without usable coordinate pairs.
    pub filtered: usize,     // Valid rows omitted by the explicit source policy.
}

fn json(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("provider response exceeds 8 MB".into());
    }
    serde_json::from_slice(bytes).map_err(|error| format!("invalid provider JSON: {error}"))
}

fn rows<'a>(value: &'a Value, key: Option<&str>) -> Result<&'a [Value], String> {
    let array = key
        .map_or(value, |key| &value[key])
        .as_array()
        .ok_or_else(|| "provider response is missing its data array".to_owned())?;
    if array.len() > MAX_ITEMS {
        return Err("provider item limit exceeded".into());
    }
    Ok(array)
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn label(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|text| text.chars().filter(|c| !c.is_control()).take(120).collect())
}

/// https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php
pub fn usgs(bytes: &[u8]) -> Result<Decoded<Earthquake>, String> {
    let value = json(bytes)?;
    if value["type"] != "FeatureCollection" {
        return Err("expected USGS FeatureCollection".into());
    }
    let rows = rows(&value, Some("features"))?;
    let mut items = Vec::new();
    for row in rows {
        let Some(event) = usgs_event(row) else {
            continue;
        };
        items.push(event);
    }
    reject_all_malformed(rows.len(), rows.len() - items.len())?; // Do not replace last-good data with an all-invalid feed.
    Ok(Decoded {
        rejected: rows.len() - items.len(),
        items,
        unpositioned: 0, // This schema requires located events or positions.
        filtered: 0,     // No intentional filter applies to this adapter.
    })
}

fn usgs_event(row: &Value) -> Option<Earthquake> {
    if row["geometry"]["type"] != "Point" {
        return None;
    }
    let coordinates = row["geometry"]["coordinates"].as_array()?;
    let magnitude = match row["properties"].get("mag") {
        None | Some(Value::Null) => None,
        Some(value) => Some(number(value)?),
    };
    let mut position = Position::new(
        row["id"].as_str()?.into(),
        number(coordinates.first()?)?,
        number(coordinates.get(1)?)?,
        Some(row["properties"]["time"].as_i64()?), // USGS events require a real source event time.
    )?;
    position.label = label(&row["properties"]["place"]);
    Some(Earthquake {
        position,
        magnitude,
        depth_km: coordinates.get(2).and_then(number),
    })
}

#[cfg(test)]
mod usgs_magnitude_validation_tests {
    use super::*;

    #[test]
    fn malformed_magnitude_is_rejected_without_filtering_small_or_unknown_magnitudes() {
        let feature = |magnitude: Value| {
            serde_json::json!({
                "id": "synthetic-magnitude-fixture",
                "properties": {"time": 500, "mag": magnitude},
                "geometry": {"type": "Point", "coordinates": [0, 0, 1]}
            })
        };
        for invalid in [
            serde_json::json!("invalid"),
            serde_json::json!(true),
            serde_json::json!({}),
            serde_json::json!([]),
        ] {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "type": "FeatureCollection", "features": [feature(invalid)]
            }))
            .unwrap();
            assert!(
                usgs(&bytes).is_err(),
                "invalid magnitude hid malformed data"
            );
        }
        let magnitudes = [Some(-0.7), Some(0.0), Some(0.001), Some(1e-14), None];
        let features: Vec<_> = magnitudes
            .iter()
            .map(|value| feature(serde_json::json!(value)))
            .collect();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "type": "FeatureCollection", "features": features
        }))
        .unwrap();
        let decoded = usgs(&bytes).unwrap();
        assert_eq!(decoded.rejected, 0);
        assert_eq!(
            decoded
                .items
                .iter()
                .map(|event| event.magnitude)
                .collect::<Vec<_>>(),
            magnitudes
        );
    }
}

/// Coinbase's tuple order is [seconds, low, high, open, close, volume].
pub fn coinbase_candles(bytes: &[u8]) -> Result<Decoded<Candle>, String> {
    let value = json(bytes)?;
    let rows = rows(&value, None)?;
    let mut items: Vec<_> = rows.iter().filter_map(coinbase_candle).collect();
    let rejected = rows.len() - items.len();
    items.sort_by_key(|candle| candle.observed_ms);
    items.dedup_by_key(|candle| candle.observed_ms);
    reject_all_malformed(rows.len(), rejected)?; // Empty is valid; nonempty all-invalid is not.
    Ok(Decoded {
        items,
        rejected,
        unpositioned: 0,
        filtered: 0,
    }) // Coinbase has no intentional geographic omissions.
}

fn coinbase_candle(row: &Value) -> Option<Candle> {
    let values = row.as_array()?;
    if values.len() != 6 {
        return None;
    }
    Candle::new(
        values[0].as_i64()?.checked_mul(1000)?,
        number(&values[3])?,
        number(&values[2])?,
        number(&values[1])?,
        number(&values[4])?,
        number(&values[5])?,
    )
}

#[cfg(test)]
mod position_tests {
    use super::*;
    #[test]
    fn ais_uses_external_millisecond_timestamp_and_nautical_speed_units() {
        let bytes=br#"{"type":"FeatureCollection","features":[{"mmsi":123,"type":"Feature","geometry":{"type":"Point","coordinates":[17.8,57.6]},"properties":{"timestamp":34,"timestampExternal":1700000000123,"sog":10,"cog":361,"heading":511}}]}"#;
        let decoded = digitraffic(bytes).unwrap();
        assert_eq!(decoded.items[0].observed_ms, Some(1700000000123)); // External milliseconds remain the fix time.
        assert!((decoded.items[0].speed_metres_per_second.unwrap() - 5.144444444).abs() < 1e-7);
        assert_eq!(decoded.items[0].heading_degrees, None);
    }
    #[test]
    fn opensky_uses_position_time_skips_ground_and_unknown_positions() {
        let decoded=opensky(br#"{"time":999,"states":[["abcdef","CALL","Country",100,120,2,48,1000,false,30,45,0,null,1000,"1200",false,0],["ground",null,"Country",101,120,2,48,0,true,0,null,0,null,0,null,false,0],["unknown",null,"Country",null,120,null,null,null,false,null,null,null,null,null,null,false,0]]}"#).unwrap();
        assert_eq!(decoded.items.len(), 1);
        assert_eq!(decoded.rejected, 0); // Valid omissions are not malformed rows.
        assert_eq!(decoded.filtered, 1); // Surface reports remain outside the airborne scope.
        assert_eq!(decoded.unpositioned, 1); // Missing coordinates are reported separately.
        assert_eq!(decoded.items[0].observed_ms, Some(100000)); // Convert only the position timestamp.
        assert_eq!(decoded.items[0].heading_degrees, Some(45.0));
        assert_eq!(
            opensky(br#"{"time":999,"states":null}"#)
                .unwrap()
                .items
                .len(),
            0
        );
    }
}

/// Digitraffic locations use `timestampExternal` milliseconds. `timestamp`
/// is the AIS second-within-minute field, not a Unix timestamp.
pub fn digitraffic(bytes: &[u8]) -> Result<Decoded<Position>, String> {
    let value = json(bytes)?;
    if value["type"] != "FeatureCollection" {
        return Err("expected AIS FeatureCollection".into());
    }
    let rows = rows(&value, Some("features"))?;
    let items = rows.iter().filter_map(ais_position).collect::<Vec<_>>();
    reject_all_malformed(rows.len(), rows.len() - items.len())?; // Do not replace last-good data with an all-invalid feed.
    Ok(Decoded {
        rejected: rows.len() - items.len(),
        items,
        unpositioned: 0, // This schema requires located events or positions.
        filtered: 0,     // No intentional filter applies to this adapter.
    })
}

fn ais_position(row: &Value) -> Option<Position> {
    if row["geometry"]["type"] != "Point" {
        return None;
    }
    let coordinates = row["geometry"]["coordinates"].as_array()?;
    let properties = &row["properties"];
    let id = row["mmsi"]
        .as_u64()
        .or_else(|| properties["mmsi"].as_u64())?
        .to_string();
    let mut position = Position::new(
        id,
        number(coordinates.first()?)?,
        number(coordinates.get(1)?)?,
        Some(properties["timestampExternal"].as_i64()?), // Never use the AIS second-within-minute field.
    )?;
    position.heading_degrees =
        number(&properties["heading"]).filter(|heading| (0.0..360.0).contains(heading)); // Unknown true heading must not borrow a course-over-ground value.
    position.speed_metres_per_second = number(&properties["sog"])
        .filter(|speed| (0.0..102.3).contains(speed))
        .map(|knots| knots * 1852.0 / 3600.0);
    Some(position)
}

/// OpenSky retains airborne coordinates, including those with unknown fix time.
/// Surface reports are outside this scene's scope; last_contact is not a fix.
pub fn opensky(bytes: &[u8]) -> Result<Decoded<Position>, String> {
    // Keep omission reasons distinct.
    let value = json(bytes)?; // Retain the ordinary eight-megabyte body limit.
    let states = value.get("states").ok_or("missing OpenSky states")?; // Missing differs from explicit null.
    let mut decoded = Decoded {
        items: Vec::new(),
        rejected: 0,
        unpositioned: 0,
        filtered: 0,
    }; // Initialize all counts.
    if states.is_null() {
        return Ok(decoded);
    } // The provider may report no states.
    let rows = rows(&value, Some("states"))?; // Enforce the existing row-count bound.
    for row in rows {
        // Inspect every supplied state without sampling.
        match opensky_position(row) {
            // Preserve independently classified omissions.
            Ok(position) => decoded.items.push(position), // Keep every usable airborne coordinate.
            Err(PositionOmission::Malformed) => decoded.rejected += 1, // Structurally invalid state.
            Err(PositionOmission::Unpositioned) => decoded.unpositioned += 1, // Coordinates unavailable.
            Err(PositionOmission::Surface) => decoded.filtered += 1, // Airborne-only policy.
        } // Finish classification of one row.
    } // Finish the complete bounded array.
    reject_all_malformed(rows.len(), decoded.rejected)?; // Fail closed only when every row is malformed.
    Ok(decoded) // Empty, unpositioned-only and filtered-only results remain distinguishable.
} // End the OpenSky adapter.
enum PositionOmission {
    Malformed,
    Unpositioned,
    Surface,
} // Disjoint reasons for excluding a state.
fn opensky_position(row: &Value) -> Result<Position, PositionOmission> {
    // Decode one airborne state.
    use PositionOmission::Malformed as malformed; // Keep guard errors local and explicit.
    let values = row
        .as_array()
        .filter(|values| values.len() >= 11)
        .ok_or(malformed)?; // Required indexes must exist.
    let id = values[0]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or(malformed)?; // Bound identity.
    let surface = values[8].as_bool().ok_or(malformed)?; // Do not guess airborne status.
    let observed_ms = if values[3].is_null() {
        None
    } else {
        // Null fix time remains unknown.
        Some(
            values[3]
                .as_i64()
                .filter(|time| *time >= 0)
                .and_then(|time| time.checked_mul(1000))
                .ok_or(malformed)?,
        ) // Reject invalid known times.
    }; // No substitution from response time or last_contact.
    let longitude = optional_number(&values[5]).map_err(|_| malformed)?; // Wrong types are not missing coordinates.
    let latitude = optional_number(&values[6]).map_err(|_| malformed)?; // Validate each supplied component.
    if longitude.is_some_and(|x| !(-180.0..=180.0).contains(&x))
        || latitude.is_some_and(|y| !(-90.0..=90.0).contains(&y))
    {
        return Err(malformed);
    } // An invalid supplied component is not a valid partial position.
    if surface {
        return Err(PositionOmission::Surface);
    } // Preserve the original flying-planes scope.
    let (Some(longitude), Some(latitude)) = (longitude, latitude) else {
        return Err(PositionOmission::Unpositioned);
    }; // Missing pair is legitimate.
    let mut position =
        Position::new(id.into(), longitude, latitude, observed_ms).ok_or(malformed)?; // Check geographic bounds.
    position.label = label(&values[1]).map(|label| label.trim().to_owned()); // Keep bounded display text.
    position.speed_metres_per_second = number(&values[9]).filter(|speed| *speed >= 0.0); // Source units are metres per second.
    position.heading_degrees = number(&values[10]).filter(|heading| (0.0..360.0).contains(heading)); // Preserve the existing track orientation.
    Ok(position) // No motion extrapolation or new timestamp.
} // End state decoding.
pub(super) fn optional_number(value: &Value) -> Result<Option<f64>, ()> {
    // Shared optional-coordinate validation.
    if value.is_null() {
        return Ok(None);
    } // Absence is not numeric zero.
    number(value).map(Some).ok_or(()) // Malformed non-null values remain errors.
} // End optional-number validation.
fn reject_all_malformed(total: usize, malformed: usize) -> Result<(), String> {
    // Shared empty-result safeguard.
    if total > 0 && total == malformed {
        return Err("provider response contains only malformed rows".into());
    } // Preserve last-good data.
    Ok(()) // A genuinely empty or partially valid response is allowed.
} // End the safeguard.

#[cfg(test)] // Deterministic provider fixtures, with no network access.
mod admission_tests {
    // Regressions for optional fix times and valid omission categories.
    use super::*; // Exercise the actual public parsers.
    #[test] // Unknown fix time is allowed only with valid airborne metadata and coordinates.
    fn airborne_unknown_time_is_not_last_contact() {
        // Large last_contact must remain irrelevant to fix time.
        let bytes = br#"{"states":[["abc123","TEST","Country",null,999999,2,48,null,false,0,90],["surface",null,"Country",null,999999,2,48,null,true,0,0]]}"#; // Synthetic states.
        let decoded = opensky(bytes).unwrap(); // Decode through the production adapter.
        assert_eq!(
            (
                decoded.items.len(),
                decoded.filtered,
                decoded.unpositioned,
                decoded.rejected
            ),
            (1, 1, 0, 0)
        ); // Surface reports stay excluded.
        assert_eq!(decoded.items[0].observed_ms, None); // Neither last_contact nor wrapper time supplies a fix.
        assert_eq!(decoded.items[0].heading_degrees, Some(90.0)); // Keep the source-reported track orientation.
        for time in ["-1", "9223372036854775807", "\"bad\""] {
            // Invalid known times must not turn into unknown times.
            let bad = format!(
                r#"{{"states":[["abc123",null,"Country",{time},100,2,48,null,false,0,0]]}}"#
            ); // Exact source row indexes.
            assert!(opensky(bad.as_bytes()).is_err()); // Fail closed when this is the only row.
        } // End invalid known-time fixtures.
    } // End airborne timestamp test.
    #[test] // All-malformed input differs from legitimately empty or intentionally omitted input.
    fn empty_and_valid_omissions_are_not_all_invalid() {
        // Preserve each parser's actual empty schema.
        for parse_feed in [usgs_empty_result as fn(&[u8]) -> bool, ais_empty_result] {
            // Both GeoJSON adapters share the top-level shape.
            assert!(parse_feed(br#"{"type":"FeatureCollection","features":[]}"#)); // An explicit empty array is valid.
            assert!(!parse_feed(
                br#"{"type":"FeatureCollection","features":[null,{}]}"#
            )); // All malformed is a failure.
        } // End GeoJSON fixtures.
        assert!(coinbase_candles(b"[]").is_ok()); // Coinbase may have no buckets in a requested interval.
        assert!(coinbase_candles(b"[null,[]]").is_err()); // Invalid tuples cannot masquerade as an empty interval.
        let filtered = opensky(
            br#"{"states":[["abc123",null,"Country",null,1,null,null,null,true,null,null]]}"#,
        )
        .unwrap(); // Valid surface-only feed.
        assert_eq!(
            (filtered.items.len(), filtered.filtered, filtered.rejected),
            (0, 1, 0)
        ); // Deliberate filtering is not malformed input.
        let missing = opensky(
            br#"{"states":[["abc123",null,"Country",null,1,null,null,null,false,null,null]]}"#,
        )
        .unwrap(); // Airborne but unpositioned.
        assert_eq!(
            (missing.items.len(), missing.unpositioned, missing.rejected),
            (0, 1, 0)
        ); // No fabricated coordinates.
        assert!(opensky(br#"{"states":[null,{}]}"#).is_err()); // Invalid states cannot clear a last-good scene.
        assert!(opensky(
            br#"{"states":[["abc123",null,"Country",null,1,null,91,null,false,null,null]]}"#
        )
        .is_err()); // Null longitude cannot legitimize an invalid latitude.
    } // End empty-result test.
    fn usgs_empty_result(bytes: &[u8]) -> bool {
        usgs(bytes).is_ok()
    } // Keep the generic test free of heterogeneous output types.
    fn ais_empty_result(bytes: &[u8]) -> bool {
        digitraffic(bytes).is_ok()
    } // Exercise actual Digitraffic validation.
    #[test] // A vessel heading cannot be synthesized from its course.
    fn digitraffic_unknown_heading_does_not_borrow_cog() {
        // Coordinates and timestamp remain usable.
        let decoded = digitraffic(br#"{"type":"FeatureCollection","features":[{"mmsi":123,"geometry":{"type":"Point","coordinates":[0,0]},"properties":{"timestampExternal":100,"heading":511,"cog":90}}]}"#).unwrap(); // Known course, unavailable heading.
        assert_eq!(decoded.items[0].heading_degrees, None); // Preserve the missing true-heading distinction.
        assert_eq!(decoded.items[0].observed_ms, Some(100)); // Keep the actual external coordinate timestamp.
    } // End heading test.
} // End parser regressions.

#[cfg(test)]
mod surface_validation_regression {
    use super as parse;
    #[test]
    fn malformed_surface_metadata_does_not_masquerade_as_a_valid_filtered_feed() {
        for row in [
            r#"["surface",null,"Country","bad",100,2,48,null,true,0,0]"#,
            r#"["surface",null,"Country",100,100,181,48,null,true,0,0]"#,
            r#"["surface",null,"Country",100,100,2,91,null,true,0,0]"#,
        ] {
            let body = format!(r#"{{"states":[{row}]}}"#);
            assert!(
                parse::opensky(body.as_bytes()).is_err(),
                "malformed surface row: {row}"
            );
        }
    }
}
