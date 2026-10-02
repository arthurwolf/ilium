//! Stable graph source identifiers and client/provider cadence policies.
//! Cadence is a request floor, not a promise of a new observation each poll.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Coinbase(&'static str),
    EcbReference(&'static str),
    SolarWind(&'static str),
    SolarMagnetometer(&'static str),
    Iss(&'static str),
    EarthquakeCount,
    EarthquakeMagnitude,
    DrandRandom,
    Wikipedia(super::events::WikiMetric),
}

#[derive(Debug, Clone, Copy)]
pub struct GraphSource {
    pub id: &'static str,
    pub label: &'static str,
    pub units: &'static str,
    pub provider: Provider,
    pub minimum_poll_seconds: u64,
    pub has_ohlc: bool,
    pub attribution: &'static str,
    pub documentation: &'static str,
}

const COINBASE_DOCS:&str="https://docs.cdp.coinbase.com/api-reference/exchange-api/rest-api/products/get-product-candles";
const NOAA_DOCS:&str="https://www.weather.gov/media/notification/pdf_2026/scn26-21_Data_Format_Changes_Impacting_SWPC_Products.pdf";
const ISS_DOCS: &str = "https://wheretheiss.at/w/developer";
const USGS_DOCS: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php";

const fn crypto(id: &'static str, label: &'static str, product: &'static str) -> GraphSource {
    GraphSource {
        id,
        label,
        units: "USD",
        provider: Provider::Coinbase(product),
        // Coinbase discourages frequent historical-candle requests. This is
        // our client polling policy, not a provider-enforced numeric quota.
        minimum_poll_seconds: 60,
        has_ohlc: true,
        attribution: "Coinbase Exchange",
        documentation: COINBASE_DOCS,
    }
}
const fn wind(
    id: &'static str,
    label: &'static str,
    field: &'static str,
    units: &'static str,
) -> GraphSource {
    GraphSource {
        id,
        label,
        units,
        provider: Provider::SolarWind(field),
        minimum_poll_seconds: 60,
        has_ohlc: false,
        attribution: "NOAA SWPC — active RTSW spacecraft",
        documentation: NOAA_DOCS,
    }
}
const fn magnetic(id: &'static str, label: &'static str, field: &'static str) -> GraphSource {
    GraphSource {
        id,
        label,
        units: "nT",
        provider: Provider::SolarMagnetometer(field),
        minimum_poll_seconds: 60,
        has_ohlc: false,
        attribution: "NOAA SWPC — active RTSW spacecraft",
        documentation: NOAA_DOCS,
    }
}
const fn iss(
    id: &'static str,
    label: &'static str,
    field: &'static str,
    units: &'static str,
) -> GraphSource {
    GraphSource {
        id,
        label,
        units,
        provider: Provider::Iss(field),
        minimum_poll_seconds: 5,
        has_ohlc: false,
        attribution: "Where the ISS at? — orbital estimate",
        documentation: ISS_DOCS,
    }
}

const fn ecb(id: &'static str, label: &'static str, quote: &'static str) -> GraphSource {
    GraphSource {
        id,
        label,
        units: quote,
        provider: Provider::EcbReference(quote),
        minimum_poll_seconds: 3600,
        has_ohlc: false,
        attribution: "ECB via Frankfurter — daily EUR reference, not intraday or trading prices",
        documentation: "https://frankfurter.dev/",
    }
}

/// Initial verified-schema catalogue. Further event streams and market
/// indicators are additive; these IDs do not imply the full feature is done.
pub const SOURCES: [GraphSource; 32] = [
    crypto("btc_usd", "Bitcoin / USD", "BTC-USD"),
    crypto("eth_usd", "Ethereum / USD", "ETH-USD"),
    crypto("sol_usd", "Solana / USD", "SOL-USD"),
    crypto("ltc_usd", "Litecoin / USD", "LTC-USD"),
    crypto("doge_usd", "Dogecoin / USD", "DOGE-USD"),
    crypto("ada_usd", "Cardano / USD", "ADA-USD"),
    crypto("xrp_usd", "XRP / USD", "XRP-USD"),
    crypto("avax_usd", "Avalanche / USD", "AVAX-USD"),
    ecb("eur_usd", "EUR / USD (ECB daily)", "USD"),
    ecb("eur_gbp", "EUR / GBP (ECB daily)", "GBP"),
    ecb("eur_jpy", "EUR / JPY (ECB daily)", "JPY"),
    ecb("eur_chf", "EUR / CHF (ECB daily)", "CHF"),
    ecb("eur_cad", "EUR / CAD (ECB daily)", "CAD"),
    ecb("eur_aud", "EUR / AUD (ECB daily)", "AUD"),
    ecb("eur_sek", "EUR / SEK (ECB daily)", "SEK"),
    ecb("eur_nok", "EUR / NOK (ECB daily)", "NOK"),
    wind(
        "solar_wind_speed",
        "Solar wind speed",
        "proton_speed",
        "km/s",
    ),
    wind(
        "solar_wind_density",
        "Solar wind density",
        "proton_density",
        "protons/cm³",
    ),
    wind(
        "solar_wind_temperature",
        "Solar wind temperature",
        "proton_temperature",
        "K",
    ),
    magnetic(
        "interplanetary_field",
        "Interplanetary magnetic field",
        "bt",
    ),
    magnetic("interplanetary_bx", "Magnetic field Bx (GSM)", "bx_gsm"),
    magnetic("interplanetary_by", "Magnetic field By (GSM)", "by_gsm"),
    magnetic("interplanetary_bz", "Magnetic field Bz (GSM)", "bz_gsm"),
    iss("iss_altitude", "ISS altitude", "altitude", "km"),
    iss("iss_velocity", "ISS orbital speed", "velocity", "km/h"),
    iss("iss_latitude", "ISS latitude", "latitude", "°"),
    iss("iss_longitude", "ISS longitude", "longitude", "°"),
    GraphSource {
        id: "earthquake_count",
        label: "Earthquake activity",
        units: "events/hour",
        provider: Provider::EarthquakeCount,
        minimum_poll_seconds: 60,
        has_ohlc: false,
        attribution: "USGS — all reported magnitudes",
        documentation: USGS_DOCS,
    },
    GraphSource {
        id: "earthquake_magnitude",
        label: "Earthquake magnitudes",
        units: "magnitude",
        provider: Provider::EarthquakeMagnitude,
        minimum_poll_seconds: 60,
        has_ohlc: false,
        attribution: "USGS — all reported magnitudes",
        documentation: USGS_DOCS,
    },
    GraphSource {
        id: "drand_randomness",
        label: "Public randomness (Quicknet)",
        units: "0–1",
        provider: Provider::DrandRandom,
        minimum_poll_seconds: 3,
        has_ohlc: false,
        attribution: "drand Quicknet — public beacon, signature not verified",
        documentation: "https://docs.drand.love/developer/API-v1/drand-http-api/",
    },
    GraphSource {
        id: "wikipedia_edit_rate",
        label: "Wikipedia edit activity",
        units: "edits/s",
        provider: Provider::Wikipedia(super::events::WikiMetric::EditRate),
        minimum_poll_seconds: 5,
        has_ohlc: false,
        attribution: "Wikimedia EventStreams — Wikipedia edits across languages",
        documentation:
            "https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams_HTTP_Service",
    },
    GraphSource {
        id: "wikipedia_bot_share",
        label: "Wikipedia bot share",
        units: "% of known edits",
        provider: Provider::Wikipedia(super::events::WikiMetric::BotShare),
        minimum_poll_seconds: 5,
        has_ohlc: false,
        attribution: "Wikimedia EventStreams — edits with known bot classification",
        documentation:
            "https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams_HTTP_Service",
    },
];

pub fn find(id: &str) -> Option<&'static GraphSource> {
    SOURCES.iter().find(|source| source.id == id)
}
