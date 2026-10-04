//! All seven series families use the native catalogue and pure schema decoders.
use super::*;
use ilium_ambient::live_data::{
    catalog::{self, GraphSource, Provider},
    model::Candle,
    series,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn source(options: &SeriesOptions) -> Result<&'static GraphSource> {
    let default = match options.provider {
        SeriesProvider::Crypto => "btc_usd",
        SeriesProvider::Ecb => "eur_usd",
        SeriesProvider::NoaaSolarWind => "solar_wind_speed",
        SeriesProvider::Iss => "iss_altitude",
        SeriesProvider::Usgs => "earthquake_count",
        SeriesProvider::Wikipedia => "wikipedia_edit_rate",
        SeriesProvider::DrandQuicknet => "drand_randomness",
    };
    let source = catalog::find(options.source_id.as_deref().unwrap_or(default))
        .ok_or_else(|| AnimationError::Runtime("unknown native series source".into()))?;
    let family = match source.provider {
        Provider::Coinbase(_) => SeriesProvider::Crypto,
        Provider::EcbReference(_) => SeriesProvider::Ecb,
        Provider::SolarWind(_) | Provider::SolarMagnetometer(_) => SeriesProvider::NoaaSolarWind,
        Provider::Iss(_) => SeriesProvider::Iss,
        Provider::EarthquakeCount | Provider::EarthquakeMagnitude => SeriesProvider::Usgs,
        Provider::Wikipedia(_) => SeriesProvider::Wikipedia,
        Provider::DrandRandom => SeriesProvider::DrandQuicknet,
    };
    if family != options.provider {
        return types::fail("series source belongs to another provider family");
    }
    Ok(source)
}

pub fn fetch<C: BrokerSourceClient>(
    client: &mut C,
    options: &SeriesOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<SeriesSnapshot> {
    let source = source(options)?;
    let now = chrono::DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| AnimationError::Runtime("invalid source clock".into()))?;
    if matches!(source.provider, Provider::Wikipedia(_)) {
        return wikipedia(client, source, options, now_ms, revision, stop);
    }
    let urls = ilium_ambient::live_data::fetch::graph_urls(source, options.window_minutes, now);
    if urls.is_empty() || urls.len() > 2 {
        return types::fail("native series request plan exceeds two pages");
    }

    let mut samples = BTreeMap::new();
    let mut candles = BTreeMap::new();
    let mut detail = None;
    for url in urls {
        let body = bytes(client, http_options(url, 8_000_000, "text"), stop)?;
        let decoded = series::decode(source, &body)
            .map_err(|error| AnimationError::Runtime(format!("series {}: {error}", source.id)))?;
        for sample in decoded.samples {
            samples.insert(sample.observed_ms, sample.value);
        }
        for candle in decoded.candles {
            candles.insert(candle.observed_ms, candle);
        }
        if decoded.detail.is_some() {
            detail = decoded.detail;
        }
    }
    let mut points: Vec<_> = samples
        .into_iter()
        .map(|(epoch_ms, value)| {
            let candle: Option<&Candle> = candles.get(&epoch_ms);
            SeriesPoint {
                epoch_ms,
                value,
                open: candle.map(|candle| candle.open),
                high: candle.map(|candle| candle.high),
                low: candle.map(|candle| candle.low),
                close: candle.map(|candle| candle.close),
                volume: candle.map(|candle| candle.volume),
            }
        })
        .collect();
    if points.len() > options.max_samples {
        points.drain(..points.len() - options.max_samples);
    }
    let observed = points.last().map(|point| point.epoch_ms);
    Ok(SeriesSnapshot {
        metadata: captured(revision, now_ms, observed),
        provider: options.provider,
        source_id: source.id.into(),
        points,
        verification: if options.provider == SeriesProvider::DrandQuicknet {
            "unverified"
        } else {
            "not_applicable"
        }
        .into(),
        attribution: source.attribution.into(),
        detail,
    })
}

fn wikipedia<C: BrokerSourceClient>(
    client: &mut C,
    source: &GraphSource,
    options: &SeriesOptions,
    now_ms: i64,
    revision: u64,
    stop: &AtomicBool,
) -> Result<SeriesSnapshot> {
    let mut ids = BTreeSet::new();
    let mut buckets = BTreeMap::<i64, (u32, u32)>::new();
    let request = http_options(
        "https://stream.wikimedia.org/v2/stream/recentchange".into(),
        2 * 1024 * 1024,
        "text",
    );
    let response = client.stream_lines(&request, stop, 262_144, 1024, &mut |line| {
        cancelled(stop)?;
        let Some(data) = line.strip_prefix(b"data:") else {
            return Ok(true);
        };
        let value: Value = serde_json::from_slice(data)?;
        if value["meta"]["domain"] == "canary"
            || value["type"] != "edit"
            || !value["server_name"]
                .as_str()
                .is_some_and(|domain| domain.ends_with(".wikipedia.org"))
        {
            return Ok(true);
        }
        let id = value["meta"]["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(|| AnimationError::Runtime("Wikipedia event identity missing".into()))?;
        let second = value["timestamp"]
            .as_i64()
            .filter(|second| *second >= 0 && second.checked_mul(1000).is_some())
            .ok_or_else(|| AnimationError::Runtime("Wikipedia provider time missing".into()))?;
        let bot = value["bot"].as_bool().ok_or_else(|| {
            AnimationError::Runtime("Wikipedia bot classification missing".into())
        })?;
        if ids.len() >= 512 {
            return Ok(false);
        }
        if ids.insert(id.to_owned()) {
            let bucket = buckets.entry(second).or_default();
            bucket.0 += 1;
            bucket.1 += u32::from(bot);
        }
        Ok(buckets
            .keys()
            .next()
            .zip(buckets.keys().next_back())
            .is_none_or(|(first, last)| last - first < 5))
    })?;
    if !(200..=299).contains(&response.status) {
        return types::fail("Wikipedia stream HTTP failure");
    }
    if buckets.is_empty() {
        return types::fail("Wikipedia stream delivered no validated observations");
    }
    let bot_share = matches!(
        source.provider,
        Provider::Wikipedia(ilium_ambient::live_data::events::WikiMetric::BotShare)
    );
    let mut points: Vec<_> = buckets
        .into_iter()
        .map(|(second, (count, bots))| SeriesPoint {
            epoch_ms: second * 1000,
            value: if bot_share {
                100.0 * f64::from(bots) / f64::from(count)
            } else {
                f64::from(count)
            },
            open: None,
            high: None,
            low: None,
            close: None,
            volume: None,
        })
        .collect();
    if points.len() > options.max_samples {
        points.drain(..points.len() - options.max_samples);
    }
    let observed = points.last().map(|point| point.epoch_ms);
    Ok(SeriesSnapshot {metadata:captured(revision,now_ms,observed),provider:SeriesProvider::Wikipedia,source_id:source.id.into(),points,verification:"not_applicable".into(),attribution:source.attribution.into(),detail:Some("Bounded connection window; first/last bins provisional, disconnected intervals missing, no fabricated history".into())})
}
