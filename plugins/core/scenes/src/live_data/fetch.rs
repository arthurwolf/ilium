//! Public snapshot adapters: the request plan and the decoding of each source, as the plan and
//! decode halves of a [`Poller`]. The polling itself (host http, no thread) is `poll.rs`.
use super::{
    catalog::{GraphSource, Provider},
    fleet::{self, FleetBatch, FleetSource},
    model::Earthquake,
    parse,
    poll::{Decode, Plan, Poller, Request},
    rate::{self, HostLedger, Ledger},
    series::{self, DataSeries},
};
use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

const USGS: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson";
const WIND: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json";
const MAGNETOMETER: &str = "https://services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json";
const ISS: &str = "https://api.wheretheiss.at/v1/satellites/25544";

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

fn requests(urls: Vec<String>, limit: usize) -> Vec<Request> {
    urls.into_iter().map(|url| Request { url, limit }).collect()
}

fn date_time(now_ms: i64) -> Result<chrono::DateTime<chrono::Utc>, String> {
    chrono::DateTime::from_timestamp_millis(now_ms).ok_or_else(|| "clock out of range".to_owned())
}

/// The requests of one graph refresh at `now_ms`. A Coinbase history plan reserves the shared
/// one-minute client floor of its pair first (one reservation for at most two HTTP pages, not
/// a claim about Coinbase's provider-enforced quota).
pub fn graph_plan(
    source: &GraphSource,
    window_minutes: i32,
    now_ms: i64,
    ledger: &mut dyn Ledger,
) -> Result<Vec<Request>, String> {
    let now = date_time(now_ms)?;
    if let Provider::Coinbase(product) = source.provider {
        if product.is_empty()
            || !product
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("invalid historical-candle product identity".into());
        }
        rate::reserve(ledger, &format!("coinbase-{product}"), now_ms, 60_000)?;
    }
    Ok(requests(
        graph_urls(source, window_minutes, now),
        parse::MAX_RESPONSE_BYTES,
    ))
}

/// Decodes the bodies of a graph plan, in request order, into one series.
pub fn graph_decode(
    source: &GraphSource,
    window_minutes: i32,
    now_ms: i64,
    bodies: &[Vec<u8>],
) -> Result<DataSeries, String> {
    let mut data = DataSeries::default();
    for body in bodies {
        let decoded = series::decode(source, body)?;
        data.samples.extend(decoded.samples);
        data.candles.extend(decoded.candles);
        data.rejected = data.rejected.saturating_add(decoded.rejected);
        if decoded.detail.is_some() {
            data.detail = decoded.detail;
        }
    }
    if matches!(source.provider, Provider::Coinbase(_)) {
        let start = now_ms.saturating_sub(
            i64::from(window_minutes.clamp(MIN_WINDOW_MINUTES, MAX_WINDOW_MINUTES)) * 60000,
        );
        merge_candles(&mut data, start, now_ms);
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

/// The poller of a graph source over the given transport and ledger.
pub fn graph_with(
    transport: Box<dyn super::poll::Transport>,
    mut ledger: Box<dyn Ledger>,
    source: &'static GraphSource,
    requested_seconds: u64,
    window_minutes: i32,
) -> Poller<DataSeries> {
    let planned_at = Arc::new(AtomicI64::new(0));
    let plan_clock = Arc::clone(&planned_at);
    let plan: Plan = Box::new(move |now_ms| {
        let requests = graph_plan(source, window_minutes, now_ms, ledger.as_mut())?;
        plan_clock.store(now_ms, Ordering::Release);
        Ok(requests)
    });
    let decode: Decode<DataSeries> = Box::new(move |bodies, _| {
        let data = graph_decode(
            source,
            window_minutes,
            planned_at.load(Ordering::Acquire),
            bodies,
        )?;
        let observed = data.samples.last().map(|sample| sample.observed_ms);
        Ok((data, observed))
    });
    Poller::with_transport(
        transport,
        Duration::from_secs(requested_seconds),
        Duration::from_secs(source.minimum_poll_seconds),
        plan,
        decode,
    )
}

/// The graph poller over the host's http import and shared storage.
pub fn graph(
    source: &'static GraphSource,
    requested_seconds: u64,
    window_minutes: i32,
) -> Poller<DataSeries> {
    graph_with(
        Box::new(super::poll::HostTransport),
        Box::new(HostLedger),
        source,
        requested_seconds,
        window_minutes,
    )
}

pub fn earthquakes(requested_seconds: u64) -> Poller<Vec<Earthquake>> {
    Poller::new(
        Duration::from_secs(requested_seconds),
        Duration::from_secs(60),
        Box::new(|_| {
            Ok(vec![Request {
                url: USGS.into(),
                limit: parse::MAX_RESPONSE_BYTES,
            }])
        }),
        Box::new(|bodies, _| {
            let data = parse::usgs(&bodies[0])?.items;
            let observed = data
                .iter()
                .filter_map(|event| event.position.observed_ms)
                .max();
            Ok((data, observed))
        }),
    )
}

/// The fleet poller of `source` over the given transport and ledger: one request per refresh,
/// reserved against the source's shared floor.
pub fn fleet_with(
    transport: Box<dyn super::poll::Transport>,
    ledger: Box<dyn Ledger>,
    source: FleetSource,
    requested_seconds: u64,
) -> Poller<FleetBatch> {
    let ledger = Arc::new(Mutex::new(ledger));
    Poller::with_transport(
        transport,
        Duration::from_secs(requested_seconds),
        Duration::from_secs(source.floor()),
        Box::new(move |now_ms| {
            let mut ledger = ledger
                .lock()
                .map_err(|_| "request ledger unavailable".to_owned())?;
            rate::reserve(
                ledger.as_mut(),
                &format!("{}-fleet", source.name()),
                now_ms,
                source.floor() as i64 * 1000,
            )?;
            Ok(vec![Request {
                url: source.url().into(),
                limit: source.limit(),
            }])
        }),
        Box::new(move |bodies, _| {
            let batch = fleet::decode(source, &bodies[0])?;
            let observed = batch.meta.latest_fix_ms.or(batch.meta.latest_ais_ms);
            Ok((batch, observed))
        }),
    )
}

/// The fleet poller over the host's http import and shared storage.
pub fn fleet_poller(source: FleetSource, requested_seconds: u64) -> Poller<FleetBatch> {
    fleet_with(
        Box::new(super::poll::HostTransport),
        Box::new(HostLedger),
        source,
        requested_seconds,
    )
}

#[cfg(test)]
mod tests {
    use super::super::rate::MemoryLedger;
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
    // A synthetic transport and ledger: no provider request, no storage.
    fn poller_requests(
        ledger: &mut MemoryLedger,
        id: &str,
        window: i32,
        now_ms: i64,
    ) -> Result<usize, String> {
        let source = super::super::catalog::find(id).unwrap();
        graph_plan(source, window, now_ms, ledger).map(|requests| requests.len())
    }

    #[test]
    fn reopen_and_second_client_share_history_floor_but_fast_inputs_are_unchanged() {
        let mut ledger = MemoryLedger::default();
        let now = 1_000_000_000;
        assert_eq!(poller_requests(&mut ledger, "btc_usd", 1, now), Ok(1));
        assert!(
            poller_requests(&mut ledger, "btc_usd", 5, now + 1).is_err(),
            "another client/window must not bypass the history floor"
        );
        assert_eq!(
            poller_requests(&mut ledger, "btc_usd", 1, now + 60_000),
            Ok(1)
        );
        assert!(poller_requests(&mut ledger, "eth_usd", 1, now).is_ok());
        assert!(poller_requests(&mut ledger, "drand_randomness", 1, now).is_ok());
        assert!(poller_requests(&mut ledger, "drand_randomness", 1, now + 5_000).is_ok());
    }

    #[test]
    fn a_failed_attempt_keeps_its_slot_and_a_corrupt_ledger_fails_closed() {
        let mut ledger = MemoryLedger::default();
        let now = 1_000_000_000;
        assert!(poller_requests(&mut ledger, "btc_usd", 1, now).is_ok());
        // The attempt may have failed on the network: the slot stays consumed.
        assert!(poller_requests(&mut ledger, "btc_usd", 1, now + 1_000).is_err());
        ledger
            .write("coinbase-BTC-USD", b"invalid fixture ledger".to_vec())
            .unwrap();
        assert!(poller_requests(&mut ledger, "btc_usd", 1, now + 61_000).is_err());
        assert_eq!(
            ledger.read("coinbase-BTC-USD").unwrap(),
            b"invalid fixture ledger"
        );
    }

    #[test]
    fn a_graph_poller_runs_plan_and_decode_over_a_scripted_transport() {
        struct Scripted(Vec<String>);
        impl super::super::poll::Transport for Scripted {
            fn start(&mut self, url: &str) -> Result<u64, String> {
                self.0.push(url.to_owned());
                Ok(1)
            }
            fn take(&mut self, _: u64) -> Option<super::super::poll::Answer> {
                Some(super::super::poll::Answer::Body(
                    br#"[[999960,1,3,2,2.5,1]]"#.to_vec(),
                ))
            }
        }
        let source = super::super::catalog::find("btc_usd").unwrap();
        let mut poller = graph_with(
            Box::new(Scripted(Vec::new())),
            Box::new(MemoryLedger::default()),
            source,
            60,
            1,
        );
        poller.poll(1_000_000_000);
        let snapshot = poller.snapshot();
        let data = snapshot.data.as_ref().expect("decoded series");
        assert_eq!(data.candles.len(), 1);
        assert_eq!(snapshot.state.received_ms, Some(1_000_000_000));
    }

    #[test]
    fn fleet_poller_reserves_the_source_floor_and_publishes_the_batch() {
        struct Scripted(&'static [u8]);
        impl super::super::poll::Transport for Scripted {
            fn start(&mut self, _: &str) -> Result<u64, String> {
                Ok(1)
            }
            fn take(&mut self, _: u64) -> Option<super::super::poll::Answer> {
                Some(super::super::poll::Answer::Body(self.0.to_vec()))
            }
        }
        let body: &'static [u8] = br#"{"time":999,"states":[["abcdef","CALL","Country",100,120,2,48,1000,false,30,45,0,null,1000,"1200",false,0]]}"#;
        let mut poller = fleet_with(
            Box::new(Scripted(body)),
            Box::new(MemoryLedger::default()),
            FleetSource::Aircraft,
            900,
        );
        poller.poll(5_000_000);
        let snapshot = poller.snapshot();
        assert_eq!(snapshot.data.as_ref().unwrap().positions.len(), 1);
        assert!(snapshot.state.error.is_none());
    }
}
