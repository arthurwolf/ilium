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
        assert_eq!(decoded.items[0].position.observed_ms, 100);
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
    Ok(Decoded {
        rejected: rows.len() - items.len(),
        items,
    })
}

fn usgs_event(row: &Value) -> Option<Earthquake> {
    if row["geometry"]["type"] != "Point" {
        return None;
    }
    let coordinates = row["geometry"]["coordinates"].as_array()?;
    let mut position = Position::new(
        row["id"].as_str()?.into(),
        number(coordinates.first()?)?,
        number(coordinates.get(1)?)?,
        row["properties"]["time"].as_i64()?,
    )?;
    position.label = label(&row["properties"]["place"]);
    Some(Earthquake {
        position,
        magnitude: number(&row["properties"]["mag"]),
        depth_km: coordinates.get(2).and_then(number),
    })
}

/// Coinbase's tuple order is [seconds, low, high, open, close, volume].
pub fn coinbase_candles(bytes: &[u8]) -> Result<Decoded<Candle>, String> {
    let value = json(bytes)?;
    let rows = rows(&value, None)?;
    let mut items: Vec<_> = rows.iter().filter_map(coinbase_candle).collect();
    let rejected = rows.len() - items.len();
    items.sort_by_key(|candle| candle.observed_ms);
    items.dedup_by_key(|candle| candle.observed_ms);
    Ok(Decoded { items, rejected })
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
        assert_eq!(decoded.items[0].observed_ms, 1700000000123);
        assert!((decoded.items[0].speed_metres_per_second.unwrap() - 5.144444444).abs() < 1e-7);
        assert_eq!(decoded.items[0].heading_degrees, None);
    }
    #[test]
    fn opensky_uses_position_time_skips_ground_and_unknown_positions() {
        let decoded=opensky(br#"{"time":999,"states":[["abcdef","CALL","Country",100,120,2,48,1000,false,30,45,0,null,1000,"1200",false,0],["ground",null,"Country",101,120,2,48,0,true,0,null,0,null,0,null,false,0],["unknown",null,"Country",null,120,null,null,null,false,null,null,null,null,null,null,false,0]]}"#).unwrap();
        assert_eq!(decoded.items.len(), 1);
        assert_eq!(decoded.rejected, 2);
        assert_eq!(decoded.items[0].observed_ms, 100000);
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
    Ok(Decoded {
        rejected: rows.len() - items.len(),
        items,
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
        properties["timestampExternal"].as_i64()?,
    )?;
    position.heading_degrees = number(&properties["heading"])
        .filter(|heading| (0.0..360.0).contains(heading))
        .or_else(|| number(&properties["cog"]).filter(|heading| (0.0..360.0).contains(heading)));
    position.speed_metres_per_second = number(&properties["sog"])
        .filter(|speed| (0.0..102.3).contains(speed))
        .map(|knots| knots * 1852.0 / 3600.0);
    Some(position)
}

/// OpenSky state vectors retain the last actual position timestamp, rather
/// than the newer response or last-contact time. Ground vehicles are omitted.
pub fn opensky(bytes: &[u8]) -> Result<Decoded<Position>, String> {
    let value = json(bytes)?;
    let states = value.get("states").ok_or("missing OpenSky states")?;
    if states.is_null() {
        return Ok(Decoded {
            items: Vec::new(),
            rejected: 0,
        });
    }
    let rows = rows(&value, Some("states"))?;
    let items = rows.iter().filter_map(opensky_position).collect::<Vec<_>>();
    Ok(Decoded {
        rejected: rows.len() - items.len(),
        items,
    })
}

fn opensky_position(row: &Value) -> Option<Position> {
    let values = row.as_array()?;
    if values.get(8)?.as_bool()? {
        return None;
    }
    let mut position = Position::new(
        values.first()?.as_str()?.into(),
        number(values.get(5)?)?,
        number(values.get(6)?)?,
        values.get(3)?.as_i64()?.checked_mul(1000)?,
    )?;
    position.label = values
        .get(1)
        .and_then(label)
        .map(|label| label.trim().to_owned());
    position.speed_metres_per_second = values.get(9).and_then(number).filter(|speed| *speed >= 0.0);
    position.heading_degrees = values
        .get(10)
        .and_then(number)
        .filter(|heading| (0.0..360.0).contains(heading));
    Some(position)
}
