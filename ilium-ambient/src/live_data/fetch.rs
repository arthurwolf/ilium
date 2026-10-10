//! Public snapshot adapters. All blocking work stays inside owned pollers.
use super::{
    catalog::{GraphSource, Provider},
    model::{Earthquake, Position},
    parse,
    poll::Poller,
    series::{self, DataSeries},
};
use crate::{resources::AmbientResources, source::http_get_stoppable};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const USGS: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson";
const WIND: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json";
const MAGNETOMETER: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json";
const ISS: &str = "https://api.wheretheiss.at/v1/satellites/25544";
const BOATS: &str = "https://meri.digitraffic.fi/api/ais/v1/locations";
const AIRCRAFT: &str = "https://opensky-network.org/api/states/all";

pub const MIN_WINDOW_MINUTES: i32 = 1;
pub const MAX_WINDOW_MINUTES: i32 = 525600;
const CANDLE_INTERVALS: [u64; 6] = [60, 300, 900, 3600, 21600, 86400];
/// Smallest supported interval needing at most 300 inclusive observations.
/// A year exceeds even 300 daily candles; that case uses two bounded requests.
pub fn candle_granularity(window_minutes: i32) -> u64 {
    let seconds = window_minutes.clamp(MIN_WINDOW_MINUTES, MAX_WINDOW_MINUTES) as u64 * 60;
    CANDLE_INTERVALS
        .into_iter()
        .find(|interval| seconds <= interval * 299)
        .unwrap_or(86400)
}

/// Deterministic URL plan. The supplied clock belongs to the worker, never
/// the renderer. Each Coinbase request spans at most 299 intervals, including
/// both boundary buckets within the provider's 300-candle limit.
pub fn graph_urls(
    source: &GraphSource,
    window_minutes: i32,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<String> {
    match source.provider {
        Provider::Coinbase(product)=>{
            let granularity=candle_granularity(window_minutes);
            let mut start=now.checked_sub_signed(chrono::Duration::minutes(i64::from(window_minutes.clamp(MIN_WINDOW_MINUTES,MAX_WINDOW_MINUTES)))).unwrap_or(now);
            let mut urls=Vec::new();
            while start<now {
                let end=start.checked_add_signed(chrono::Duration::seconds((299*granularity) as i64)).unwrap_or(now).min(now);
                urls.push(format!("https://api.exchange.coinbase.com/products/{product}/candles?granularity={granularity}&start={}&end={}",
                    start.to_rfc3339_opts(chrono::SecondsFormat::Secs,true),end.to_rfc3339_opts(chrono::SecondsFormat::Secs,true)));
                start=end;
            }
            urls
        }
        Provider::EcbReference(quote)=>{
            let from=now.date_naive().checked_sub_days(chrono::Days::new(366)).unwrap_or(now.date_naive());
            vec![format!("https://api.frankfurter.dev/v2/providers/ecb/rates?base=EUR&quotes={quote}&from={from}")]
        }
        Provider::SolarWind(_)=>vec![WIND.into()],
        Provider::SolarMagnetometer(_)=>vec![MAGNETOMETER.into()],
        Provider::Iss(_)=>vec![ISS.into()],
        Provider::Wikipedia(_)=>vec!["https://stream.wikimedia.org/v2/stream/recentchange".into()],
        Provider::DrandRandom=>vec!["https://api.drand.sh/52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971/public/latest".into()],
        Provider::EarthquakeCount|Provider::EarthquakeMagnitude=>vec![USGS.into()],
    }
}

fn get(url: &str, stop: &AtomicBool) -> Result<Vec<u8>, String> {
    if stop.load(Ordering::Relaxed) {
        return Err("source request cancelled".into());
    }
    if url == AIRCRAFT {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let path = crate::source::default_cache_dir().join("live-data/opensky-request.time");
        super::rate::reserve(&path, now_ms, 900_000)?;
    }
    http_get_stoppable(
        url,
        parse::MAX_RESPONSE_BYTES,
        Duration::from_secs(15),
        stop,
    )
    .map_err(|error| error.to_string())
}

pub fn graph(
    resources: &AmbientResources,
    source: &'static GraphSource,
    requested_seconds: u64,
    window_minutes: i32,
) -> Result<Poller<DataSeries>, String> {
    Poller::start(
        resources,
        "live-graph",
        Duration::from_secs(requested_seconds),
        Duration::from_secs(source.minimum_poll_seconds),
        move |stop| {
            let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
            let data = fetch_graph_plan(
                source,
                window_minutes,
                now,
                &crate::source::default_cache_dir().join("live-data"),
                stop,
                get,
            )?;
            let observed = data.samples.last().map(|sample| sample.observed_ms);
            Ok((data, observed))
        },
    )
}

/// Execute one bounded history plan on the poller worker. The transport seam
/// permits admission tests without contacting public providers.
fn fetch_graph_plan(
    source: &GraphSource,
    window_minutes: i32,
    now: chrono::DateTime<chrono::Utc>,
    reservation_root: &std::path::Path,
    stop: &AtomicBool,
    mut download: impl FnMut(&str, &AtomicBool) -> Result<Vec<u8>, String>,
) -> Result<DataSeries, String> {
    if stop.load(Ordering::Acquire) {
        return Err("graph request cancelled".into());
    }
    // This is one client history-plan reservation (at most two HTTP pages),
    // not a claim about Coinbase's provider-enforced numeric quota.
    if let Provider::Coinbase(product) = source.provider {
        if product.is_empty()
            || !product
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("invalid historical-candle product identity".into());
        }
        super::rate::reserve(
            &reservation_root.join(format!("coinbase-{product}-request.time")),
            now.timestamp_millis(),
            60_000,
        )?;
    }
    let mut data = DataSeries::default();
    for url in graph_urls(source, window_minutes, now) {
        let decoded = series::decode(source, &download(&url, stop)?)?;
        data.samples.extend(decoded.samples);
        data.candles.extend(decoded.candles);
        data.rejected = data.rejected.saturating_add(decoded.rejected);
        if decoded.detail.is_some() {
            data.detail = decoded.detail;
        }
    }
    if matches!(source.provider, Provider::Coinbase(_)) {
        let start = now.timestamp_millis().saturating_sub(
            i64::from(window_minutes.clamp(MIN_WINDOW_MINUTES, MAX_WINDOW_MINUTES)) * 60000,
        );
        merge_candles(&mut data, start, now.timestamp_millis());
        data.detail = Some(format!(
            "Coinbase genuine {}s candle interval; provider-time window",
            candle_granularity(window_minutes)
        ));
    }
    Ok(data)
}

fn merge_candles(data: &mut DataSeries, start_ms: i64, end_ms: i64) {
    // Requests share their inclusive boundary. Keep one candle per provider
    // timestamp and use its close for the line/bar projection of this source.
    let candles: std::collections::BTreeMap<_, _> = data
        .candles
        .drain(..)
        .filter(|c| c.observed_ms >= start_ms && c.observed_ms <= end_ms)
        .map(|c| (c.observed_ms, c))
        .collect();
    data.candles = candles.into_values().collect();
    data.samples = data
        .candles
        .iter()
        .map(|c| super::model::Observation {
            observed_ms: c.observed_ms,
            value: c.close,
        })
        .collect();
}

pub fn earthquakes(
    resources: &AmbientResources,
    requested_seconds: u64,
) -> Result<Poller<Vec<Earthquake>>, String> {
    Poller::start(
        resources,
        "live-earthquakes",
        Duration::from_secs(requested_seconds),
        Duration::from_secs(60),
        |stop| {
            let data = parse::usgs(&get(USGS, stop)?)?.items;
            let observed = data
                .iter()
                .filter_map(|event| event.position.observed_ms)
                .max();
            Ok((data, observed))
        },
    )
}

/// Digitraffic reports received AIS positions around Finnish waters; it is
/// not a global ship inventory. ureq's default gzip feature supplies the
/// mandatory compressed transfer and bounds decompressed bytes in http_get.
pub fn boats(
    resources: &AmbientResources,
    requested_seconds: u64,
) -> Result<Poller<Vec<Position>>, String> {
    positions(
        resources,
        "live-boats",
        BOATS,
        requested_seconds,
        30,
        parse::digitraffic,
    )
}

/// Anonymous OpenSky grants 400 daily credits. An unfiltered world query
/// costs four: a fifteen-minute request floor stays below that daily budget.
pub fn aircraft(
    resources: &AmbientResources,
    requested_seconds: u64,
) -> Result<Poller<Vec<Position>>, String> {
    positions(
        resources,
        "live-aircraft",
        AIRCRAFT,
        requested_seconds,
        900,
        parse::opensky,
    )
}

fn positions(
    resources: &AmbientResources,
    name: &str,
    url: &'static str,
    requested: u64,
    minimum: u64,
    decode: fn(&[u8]) -> Result<parse::Decoded<Position>, String>,
) -> Result<Poller<Vec<Position>>, String> {
    Poller::start(
        resources,
        name,
        Duration::from_secs(requested),
        Duration::from_secs(minimum),
        move |stop| {
            let data = decode(&get(url, stop)?)?.items;
            let observed = data
                .iter()
                .filter_map(|position| position.observed_ms)
                .max();
            Ok((data, observed))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-10-02T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }
    #[test]
    fn one_minute_plan_covers_exactly_one_minute_and_subminimum_input_clamps_coherently() {
        let source = super::super::catalog::find("btc_usd").unwrap();
        for minutes in [0, 1] {
            let urls = graph_urls(source, minutes, now());
            assert_eq!(urls.len(), 1);
            assert!(urls[0]
                .contains("granularity=60&start=2026-10-02T11:59:00Z&end=2026-10-02T12:00:00Z"));
            assert_eq!(candle_granularity(minutes), 60);
        }
    }
    #[test]
    fn plans_keep_current_schemas_and_all_sources_have_public_urls() {
        let wind = super::super::catalog::find("solar_wind_speed").unwrap();
        assert!(graph_urls(wind, 120, now())[0].ends_with("/json/rtsw/rtsw_wind_1m.json"));
        let bitcoin = super::super::catalog::find("btc_usd").unwrap();
        assert!(
            graph_urls(bitcoin, 120, now())[0].contains("/BTC-USD/candles?granularity=60&start=")
        );
        for source in &super::super::catalog::SOURCES {
            assert!(graph_urls(source, 120, now())
                .iter()
                .all(|url| url.starts_with("https://")));
        }
        let fx = super::super::catalog::find("eur_usd").unwrap();
        assert_eq!(graph_urls(fx,120,now()),["https://api.frankfurter.dev/v2/providers/ecb/rates?base=EUR&quotes=USD&from=2025-10-01"]);
    }
    #[test]
    fn every_window_fits_supported_intervals_and_bounded_inclusive_requests() {
        let source = super::super::catalog::find("btc_usd").unwrap();
        for window in [1, 5, 30, 120, 300, 1440, 10080, 43200, 129600, 525600] {
            let granularity = candle_granularity(window);
            assert!(CANDLE_INTERVALS.contains(&granularity));
            let urls = graph_urls(source, window, now());
            assert!(!urls.is_empty() && urls.len() <= 2);
            let mut spans = Vec::new();
            for url in urls {
                let start = url
                    .split("&start=")
                    .nth(1)
                    .unwrap()
                    .split("&end=")
                    .next()
                    .unwrap();
                let end = url.split("&end=").nth(1).unwrap();
                let start = chrono::DateTime::parse_from_rfc3339(start)
                    .unwrap()
                    .timestamp();
                let end = chrono::DateTime::parse_from_rfc3339(end)
                    .unwrap()
                    .timestamp();
                assert!(end > start && end - start <= (299 * granularity) as i64);
                spans.push((start, end));
            }
            assert_eq!(spans[0].0, now().timestamp() - i64::from(window) * 60);
            assert_eq!(spans.last().unwrap().1, now().timestamp());
            assert!(spans.windows(2).all(|parts| parts[0].1 == parts[1].0));
        }
        assert_eq!(candle_granularity(120), 60);
        assert_eq!(candle_granularity(1440), 300);
        assert_eq!(candle_granularity(525600), 86400);
    }
    #[test]
    fn pagination_merge_keeps_revised_boundary_once_and_discards_overfetch() {
        let candle = |time, close| {
            super::super::model::Candle::new(time, 1.0, 5.0, 0.0, close, 1.0).unwrap()
        };
        let mut data = DataSeries {
            candles: vec![
                candle(100, 2.0),
                candle(200, 2.0),
                candle(200, 3.0),
                candle(300, 4.0),
            ],
            ..Default::default()
        };
        merge_candles(&mut data, 150, 250);
        assert_eq!(data.candles.len(), 1);
        assert_eq!(data.candles[0].close, 3.0);
        assert_eq!(
            data.samples,
            [super::super::model::Observation::new(200, 3.0).unwrap()]
        );
    }
    #[test]
    fn stopped_source_does_not_reserve_or_fetch() {
        assert!(get(AIRCRAFT, &AtomicBool::new(true))
            .unwrap_err()
            .contains("cancelled"));
    }
}

#[cfg(test)]
mod shared_history_admission_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn reopen_and_second_client_share_history_floor_but_fast_inputs_are_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let bitcoin = super::super::catalog::find("btc_usd").unwrap();
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_000_000, 0).unwrap();
        let stop = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        // Synthetic candle transport; no provider request or production cache.
        let download = |_: &str, _: &AtomicBool| {
            calls.fetch_add(1, Ordering::AcqRel);
            Ok(br#"[[999960,1,3,2,2.5,1]]"#.to_vec())
        };
        fetch_graph_plan(bitcoin, 1, now, root.path(), &stop, download).unwrap();
        let reopened = fetch_graph_plan(
            bitcoin,
            5,
            now + chrono::Duration::milliseconds(1),
            root.path(),
            &stop,
            download,
        );
        assert!(
            reopened.is_err(),
            "another client/window must not bypass the history floor"
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        fetch_graph_plan(
            bitcoin,
            1,
            now + chrono::Duration::seconds(60),
            root.path(),
            &stop,
            download,
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 2);
        let ethereum = super::super::catalog::find("eth_usd").unwrap();
        fetch_graph_plan(ethereum, 1, now, root.path(), &stop, download).unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 3);
        let drand = super::super::catalog::find("drand_randomness").unwrap();
        let random = |_: &str, _: &AtomicBool| {
            Ok(br#"{"round":1,"randomness":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#.to_vec())
        };
        fetch_graph_plan(drand, 1, now, root.path(), &stop, random).unwrap();
        fetch_graph_plan(
            drand,
            1,
            now + chrono::Duration::seconds(5),
            root.path(),
            &stop,
            random,
        )
        .unwrap();
    }
    #[test]
    fn failed_cancelled_and_corrupt_history_reservations_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let bitcoin = super::super::catalog::find("btc_usd").unwrap();
        let now = chrono::DateTime::<chrono::Utc>::from_timestamp(1_000_000, 0).unwrap();
        let stop = AtomicBool::new(true);
        assert!(
            fetch_graph_plan(bitcoin, 1, now, root.path(), &stop, |_, _| panic!(
                "cancelled request contacted transport"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        stop.store(false, Ordering::Release);
        let calls = AtomicUsize::new(0);
        assert!(
            fetch_graph_plan(bitcoin, 1, now, root.path(), &stop, |_, _| {
                calls.fetch_add(1, Ordering::AcqRel);
                Err("synthetic transport failure".into())
            })
            .is_err()
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(fetch_graph_plan(
            bitcoin,
            1,
            now + chrono::Duration::seconds(1),
            root.path(),
            &stop,
            |_, _| panic!("failed attempt reset budget")
        )
        .is_err());
        let reservation = root.path().join("coinbase-BTC-USD-request.time");
        std::fs::write(&reservation, b"invalid fixture ledger").unwrap();
        assert!(fetch_graph_plan(
            bitcoin,
            1,
            now + chrono::Duration::seconds(61),
            root.path(),
            &stop,
            |_, _| panic!("corrupt ledger reset budget")
        )
        .is_err());
        assert_eq!(
            std::fs::read(reservation).unwrap(),
            b"invalid fixture ledger"
        );
    }
}
