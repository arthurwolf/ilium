//! The three fleet sources (OpenSeaFeed and Digitraffic boats, OpenSky aircraft): their request
//! floors, endpoints and the decoding of a complete body into one immutable batch. Fetching is
//! the poller's job (`poll.rs`), because a scene owns no thread and no file.
use super::{model::Position, openseafeed, parse};
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FleetSource {
    OpenSeaFeed,
    Digitraffic,
    Aircraft,
}

impl FleetSource {
    /// Minimum client interval in seconds.
    pub fn floor(self) -> u64 {
        match self {
            Self::OpenSeaFeed => openseafeed::MINIMUM_POLL_SECONDS,
            Self::Digitraffic => 30,
            Self::Aircraft => 900,
        }
    }
    /// Key of the shared request ledger.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenSeaFeed => "openseafeed",
            Self::Digitraffic => "digitraffic",
            Self::Aircraft => "opensky",
        }
    }
    pub fn limit(self) -> usize {
        if self == Self::OpenSeaFeed {
            openseafeed::MAX_RESPONSE_BYTES
        } else {
            parse::MAX_RESPONSE_BYTES
        }
    }
    pub fn url(self) -> &'static str {
        match self {
            Self::OpenSeaFeed => openseafeed::ENDPOINT,
            Self::Digitraffic => "https://meri.digitraffic.fi/api/ais/v1/locations",
            Self::Aircraft => "https://opensky-network.org/api/states/all",
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct FleetMeta {
    // Source time, counts and omission reasons stay separate.
    pub generated_ms: Option<i64>, // Snapshot build, never a coordinate fix.
    pub latest_ais_ms: Option<i64>, // Latest ANY AIS message, never a coordinate fix.
    pub latest_fix_ms: Option<i64>, // Latest known fix among retained positions, not fleet freshness.
    pub known_fixes: usize,         // Quantify missing fix timestamps.
    pub counts: openseafeed::FleetCounts, // Disjoint source-row accounting.
    pub filtered: usize,            // Surface reports intentionally omitted by the aircraft parser.
} // End block.

/// One immutable decoded result; the geometry identity (the `Arc`) is stable until the next
/// successful fetch.
#[derive(Debug)]
pub(super) struct FleetBatch {
    pub positions: Arc<Vec<Position>>,
    pub meta: FleetMeta,
}

pub(super) fn decode(source: FleetSource, bytes: &[u8]) -> Result<FleetBatch, String> {
    let stop = &AtomicBool::new(false);
    // Worker-only transformation of a complete bounded body.
    if source == FleetSource::OpenSeaFeed {
        // Keep source-specific schema and timestamp semantics.
        let fleet = openseafeed::decode(bytes, stop)?; // Streaming rows, no full-fleet JSON tree.
        return Ok(FleetBatch {
            positions: fleet.positions,
            meta: FleetMeta {
                generated_ms: Some(fleet.generated_ms),
                latest_ais_ms: fleet.latest_ais_update_ms,
                counts: fleet.counts,
                ..Default::default()
            },
        }); // Every fix remains unknown.
    } // End block.
    let decoded = if source == FleetSource::Digitraffic {
        parse::digitraffic(bytes)?
    } else {
        parse::opensky(bytes)?
    }; // Aircraft parser keeps airborne-only scope.
    let meta = FleetMeta {
        latest_fix_ms: decoded.items.iter().filter_map(|p| p.observed_ms).max(),
        known_fixes: decoded
            .items
            .iter()
            .filter(|p| p.observed_ms.is_some())
            .count(),
        filtered: decoded.filtered,
        counts: openseafeed::FleetCounts {
            records: decoded.items.len()
                + decoded.rejected
                + decoded.unpositioned
                + decoded.filtered,
            malformed: decoded.rejected,
            unpositioned: decoded.unpositioned,
            ..Default::default()
        },
        ..Default::default()
    }; // Preserve every omission category.
    Ok(FleetBatch {
        positions: Arc::new(decoded.items),
        meta,
    }) // The service guards this Arc before publication.
} // End block.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_floors_and_ledger_keys_are_distinct() {
        let sources = [
            FleetSource::OpenSeaFeed,
            FleetSource::Digitraffic,
            FleetSource::Aircraft,
        ];
        assert_eq!(sources.map(FleetSource::floor), [60, 30, 900]);
        for (index, source) in sources.iter().enumerate() {
            for other in &sources[index + 1..] {
                assert_ne!(source.name(), other.name());
                assert_ne!(source.url(), other.url());
            }
        }
    }

    #[test]
    fn aircraft_decode_keeps_airborne_fixes_and_counts_the_omissions() {
        let body = br#"{"time":999,"states":[["abcdef","CALL","Country",100,120,2,48,1000,false,30,45,0,null,1000,"1200",false,0],["ground",null,"Country",101,120,2,48,0,true,0,null,0,null,0,null,false,0]]}"#;
        let batch = decode(FleetSource::Aircraft, body).unwrap();
        assert_eq!(batch.positions.len(), 1);
        assert_eq!(batch.meta.known_fixes, 1);
        assert_eq!(batch.meta.filtered, 1);
        assert_eq!(batch.meta.counts.records, 2);
    }

    #[test]
    fn malformed_body_is_an_error_not_an_empty_fleet() {
        assert!(decode(FleetSource::Digitraffic, b"not json").is_err());
        assert!(decode(FleetSource::OpenSeaFeed, b"{}").is_err());
    }
}
