//! Convert each documented provider schema to observations in provider time.
use super::{
    catalog::{GraphSource, Provider},
    model::{Candle, Observation},
    parse,
};
use serde_json::Value;

#[derive(Debug, Default)]
pub struct DataSeries {
    pub samples: Vec<Observation>,
    pub candles: Vec<Candle>,
    pub detail: Option<String>,
    pub rejected: usize,
}

pub fn decode(source: &GraphSource, bytes: &[u8]) -> Result<DataSeries, String> {
    if bytes.len() > parse::MAX_RESPONSE_BYTES {
        return Err("provider response exceeds 8 MB".into());
    }
    match source.provider {
        Provider::Coinbase(_) => {
            let decoded = parse::coinbase_candles(bytes)?;
            Ok(DataSeries {
                samples: decoded
                    .items
                    .iter()
                    .map(|candle| Observation {
                        observed_ms: candle.observed_ms,
                        value: candle.close,
                    })
                    .collect(),
                candles: decoded.items,
                rejected: decoded.rejected,
                ..DataSeries::default()
            })
        }
        Provider::SolarWind(field) | Provider::SolarMagnetometer(field) => noaa(bytes, field),
        Provider::EcbReference(quote) => ecb(bytes, quote),
        Provider::DrandRandom => drand(bytes),
        Provider::Wikipedia(_) => Err("Wikipedia metrics require the event stream adapter".into()),
        Provider::Iss(field) => {
            let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            if value["units"] != "kilometers" {
                return Err("ISS response has unexpected units".into());
            }
            let sample = value["timestamp"]
                .as_i64()
                .and_then(|time| time.checked_mul(1000))
                .zip(value[field].as_f64())
                .and_then(|(time, number)| Observation::new(time, number))
                .ok_or_else(|| "ISS response is missing a valid observation".to_owned())?;
            Ok(DataSeries {
                samples: vec![sample],
                ..DataSeries::default()
            })
        }
        Provider::EarthquakeCount | Provider::EarthquakeMagnitude => {
            let decoded = parse::usgs(bytes)?;
            let mut samples = Vec::new();
            if source.provider == Provider::EarthquakeCount {
                // Count actual events by provider hour, including unknown magnitudes.
                let mut hours = std::collections::BTreeMap::<i64, u32>::new();
                for event in &decoded.items {
                    let Some(observed_ms) = event.position.observed_ms else {
                        continue;
                    };
                    *hours
                        .entry(observed_ms / 3_600_000 * 3_600_000)
                        .or_default() += 1;
                }
                if let (Some(&first), Some(&last)) = (hours.keys().next(), hours.keys().next_back())
                {
                    let length = (last - first) / 3_600_000 + 1;
                    if length > parse::MAX_ITEMS as i64 {
                        return Err("earthquake hourly range exceeds bound".into());
                    }
                    samples.extend((0..length).map(|index| {
                        let observed_ms = first + index * 3_600_000;
                        Observation {
                            observed_ms,
                            value: f64::from(*hours.get(&observed_ms).unwrap_or(&0)),
                        }
                    }));
                }
            } else {
                samples.extend(decoded.items.iter().filter_map(|event| {
                    Observation::new(event.position.observed_ms?, event.magnitude?)
                }));
            }
            samples.sort_by_key(|sample| sample.observed_ms);
            Ok(DataSeries {
                samples,
                rejected: decoded.rejected,
                ..DataSeries::default()
            })
        }
    }
}

/// ECB publishes reference dates, not an intraday measurement timestamp.
/// UTC midnight is only the chart coordinate for that calendar date.
fn ecb(bytes: &[u8], quote: &str) -> Result<DataSeries, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= parse::MAX_ITEMS)
        .ok_or_else(|| "ECB response is missing a bounded rate array".to_owned())?;
    let mut samples = std::collections::BTreeMap::new();
    let mut rejected = 0;
    for row in rows {
        if row["base"].as_str() != Some("EUR") {
            rejected += 1;
            continue;
        }
        // A multi-quote capture may legitimately contain other currencies.
        let Some(row_quote) = row["quote"].as_str() else {
            rejected += 1;
            continue;
        };
        if row_quote != quote {
            continue;
        }
        let sample = row["date"]
            .as_str()
            .filter(|date| date.len() == 10)
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|date| date.and_utc().timestamp_millis())
            .zip(
                row["rate"]
                    .as_f64()
                    .filter(|rate| rate.is_finite() && *rate > 0.0),
            )
            .and_then(|(date, rate)| Observation::new(date, rate));
        match sample {
            Some(sample) => {
                samples.insert(sample.observed_ms, sample);
            }
            None => rejected += 1,
        }
    }
    if samples.is_empty() && !rows.is_empty() {
        return Err("ECB response contains no valid selected reference rates".into());
    }
    Ok(DataSeries {samples:samples.into_values().collect(),rejected,
        detail:Some(format!("ECB EUR/{quote} daily reference; calendar date plotted at UTC midnight, not intraday telemetry or trading prices")),
        ..Default::default()})
}

/// Quicknet chain info supplies genesis 1692803367 and period 3 seconds.
/// The observation time is the round's scheduled time, not claimed telemetry.
/// This chart decodes public beacon bytes; it does not validate the BLS signature.
pub fn drand(bytes: &[u8]) -> Result<DataSeries, String> {
    if bytes.len() > parse::MAX_RESPONSE_BYTES {
        return Err("provider response exceeds 8 MB".into());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let round = value["round"]
        .as_u64()
        .filter(|round| *round > 0)
        .ok_or_else(|| "drand round is invalid".to_owned())?;
    let observed_ms = round
        .checked_sub(1)
        .and_then(|round| round.checked_mul(3))
        .and_then(|seconds| seconds.checked_add(1_692_803_367))
        .and_then(|seconds| seconds.checked_mul(1000))
        .and_then(|time| i64::try_from(time).ok())
        .ok_or_else(|| "drand scheduled time exceeds bounds".to_owned())?;
    let random = value["randomness"]
        .as_str()
        .filter(|text| text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "drand randomness must contain 32 hex bytes".to_owned())?;
    // Thirteen hex digits are exactly representable as a 52-bit integer.
    let integer = u64::from_str_radix(&random[..13], 16).map_err(|error| error.to_string())?;
    Ok(DataSeries {
        samples: vec![Observation {
            observed_ms,
            value: integer as f64 / 4_503_599_627_370_496.0,
        }],
        detail: Some(format!(
            "Quicknet round {round}; scheduled round time; BLS signature unverified"
        )),
        ..Default::default()
    })
}

fn noaa(bytes: &[u8], field: &str) -> Result<DataSeries, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= parse::MAX_ITEMS)
        .ok_or_else(|| "NOAA response is missing a bounded data array".to_owned())?;
    let mut result = DataSeries::default();
    let mut latest_timestamp = None;
    let mut valid_rows = 0;
    for row in rows {
        let Some(active) = row["active"].as_bool() else {
            result.rejected += 1;
            continue;
        };
        let observed_ms = row["time_tag"]
            .as_str()
            .and_then(|text| {
                chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
            })
            .map(|time| time.and_utc().timestamp_millis())
            .filter(|time| *time >= 0);
        let Some(observed_ms) = observed_ms else {
            result.rejected += 1;
            continue;
        };
        if !active {
            valid_rows += 1;
            continue;
        }
        let Some(value) = row.get(field) else {
            result.rejected += 1;
            continue;
        };
        if value.is_null() {
            valid_rows += 1;
            result.rejected += 1;
            continue;
        }
        let Some(value) = value.as_f64().filter(|number| number.is_finite()) else {
            result.rejected += 1;
            continue;
        };
        valid_rows += 1;
        // A valid unavailable measurement is different from a malformed row.
        // Preserve empty/inactive/null/sentinel feeds without hiding bad JSON.
        if value <= -9999.0 {
            result.rejected += 1;
            continue;
        }
        let sample = Observation { observed_ms, value };
        if latest_timestamp.is_none_or(|previous| previous < sample.observed_ms) {
            latest_timestamp = Some(sample.observed_ms);
            result.detail = row["source"]
                .as_str()
                .map(|text| text.chars().filter(|c| !c.is_control()).take(120).collect());
        }
        result.samples.push(sample);
    }
    if !rows.is_empty() && valid_rows == 0 {
        return Err("NOAA response contains only malformed observations".into());
    }
    result.samples.sort_by_key(|sample| sample.observed_ms);
    result.samples.dedup_by_key(|sample| sample.observed_ms);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::catalog::find;
    use super::*;
    #[test]
    fn noaa_distinguishes_malformed_data_from_unavailable_observations_for_every_field() {
        for source in super::super::catalog::SOURCES.iter() {
            let field = match source.provider {
                Provider::SolarWind(field) | Provider::SolarMagnetometer(field) => field,
                _ => continue,
            };
            assert!(decode(source, b"[null,{}]").is_err(), "{}", source.id);
            let invalid_time = serde_json::json!([
                {"active": true, "time_tag": "invalid", field: 1.0}
            ]);
            assert!(decode(source, &serde_json::to_vec(&invalid_time).unwrap()).is_err());
            for valid_empty in [
                serde_json::json!([]),
                serde_json::json!([
                    {"active": false, "time_tag": "2026-10-02T04:31:00", field: 999}
                ]),
                serde_json::json!([
                    {"active": true, "time_tag": "2026-10-02T04:31:00", field: null}
                ]),
                serde_json::json!([
                    {"active": true, "time_tag": "2026-10-02T04:31:00", field: -9999}
                ]),
            ] {
                assert!(decode(source, &serde_json::to_vec(&valid_empty).unwrap())
                    .unwrap()
                    .samples
                    .is_empty());
            }
            let partial = serde_json::json!([
                null,
                {"active": true, "time_tag": "2026-10-02T04:31:00", field: 1.0}
            ]);
            let data = decode(source, &serde_json::to_vec(&partial).unwrap()).unwrap();
            assert_eq!(data.samples.len(), 1);
            assert_eq!(data.rejected, 1);
        }
    }
    #[test]
    fn ecb_calendar_dates_are_sorted_positive_and_not_intraday_times() {
        let bytes=br#"[{"date":"2026-10-02","base":"EUR","quote":"USD","rate":1.15},{"date":"2026-09-30","base":"EUR","quote":"USD","rate":1.14},{"date":"2026-10-02","base":"EUR","quote":"USD","rate":1.16},{"date":"2026-02-30","base":"EUR","quote":"USD","rate":1.2},{"date":"2026-10-02","base":"USD","quote":"USD","rate":1.2},{"date":"2026-10-02","base":"EUR","quote":"USD","rate":0},{"date":"2026-10-02","base":"EUR","quote":"JPY","rate":150}]"#;
        let data = decode(find("eur_usd").unwrap(), bytes).unwrap();
        assert_eq!(data.samples.len(), 2);
        assert_eq!(data.samples[1].value, 1.16);
        assert!(data
            .samples
            .windows(2)
            .all(|pair| pair[0].observed_ms < pair[1].observed_ms));
        assert_eq!(data.samples[1].observed_ms, 1790899200000);
        assert_eq!(data.rejected, 3); // A valid other quote is intentionally filtered.
        let detail = data.detail.unwrap();
        assert!(detail.contains("ECB"));
        assert!(detail.contains("calendar"));
        assert!(detail.contains("not intraday"));
        assert!(data.candles.is_empty());
    }
    #[test]
    fn ecb_malformed_array_and_all_invalid_selected_rates_fail() {
        let source = find("eur_usd").unwrap();
        assert!(decode(source, br#"{}"#).is_err());
        for rate in ["-1", "null", r#""NaN""#, "1e999"] {
            let bytes =
                format!(r#"[{{"date":"2026-10-02","base":"EUR","quote":"USD","rate":{rate}}}]"#);
            assert!(decode(source, bytes.as_bytes()).is_err());
        }
        assert!(decode(
            source,
            br#"[{"date":"2026-10-02T12:00:00Z","base":"EUR","quote":"USD","rate":1.2}]"#
        )
        .is_err());
        assert!(decode(
            source,
            br#"[{"date":"2026-10-02","base":"EUR","quote":"JPY","rate":150}]"#
        )
        .is_err());
        let oversized = vec![serde_json::json!({}); parse::MAX_ITEMS + 1];
        let bytes = serde_json::to_vec(&oversized).unwrap();
        assert!(decode(source, &bytes)
            .unwrap_err()
            .contains("bounded rate array"));
    }
    #[test]
    fn noaa_selects_active_spacecraft_and_ignores_null_measurements() {
        let bytes=br#"[{"time_tag":"2026-10-02T04:31:00","active":false,"source":"ACE","proton_speed":999},{"time_tag":"2026-10-02T04:31:00","active":true,"source":"DSCOVR","proton_speed":332.47},{"time_tag":"2026-10-02T04:32:00","active":true,"source":"DSCOVR","proton_speed":null}]"#;
        let result = decode(find("solar_wind_speed").unwrap(), bytes).unwrap();
        assert_eq!(result.samples.len(), 1);
        assert_eq!(result.samples[0].value, 332.47);
        assert_eq!(result.samples[0].observed_ms, 1790915460000);
        assert_eq!(result.detail.as_deref(), Some("DSCOVR"));
    }
    #[test]
    fn iss_uses_provider_time_and_source_units() {
        let result = decode(
            find("iss_velocity").unwrap(),
            br#"{"timestamp":100,"velocity":27569.685698,"units":"kilometers"}"#,
        )
        .unwrap();
        assert_eq!(
            result.samples,
            [Observation::new(100000, 27569.685698).unwrap()]
        );
        assert!(decode(
            find("iss_velocity").unwrap(),
            br#"{"timestamp":100,"velocity":17130,"units":"miles"}"#
        )
        .is_err());
    }
}
