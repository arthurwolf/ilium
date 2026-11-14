//! OpenSeaFeed snapshot schema pinned to 4210d4f990b6489208912ed02de98006f6a61366.
//! Decode one row at a time; never construct a full fleet JSON value tree.
//! Position age is unavailable: ts is ANY AIS update, not a coordinate fix.
use super::{model::Position, parse::optional_number}; // Reuse validated geography and optional numbers.
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor}; // Incremental object/array decoding.
use serde::Deserializer; // Dispatch the object and array visitors.
use serde_json::Value; // Only one bounded-input row is materialized at a time.
use std::{
    collections::HashSet,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
}; // Bounded identity set and cancellation.
pub const ENDPOINT: &str = "https://api.openseafeed.com/v1/snapshot"; // Public anonymous snapshot host.
pub const MAX_RESPONSE_BYTES: usize = 32_000_000; // Source-specific decompressed body admission limit.
pub const MAX_RECORDS: usize = 200_000; // Reject larger fleets rather than silently truncating.
pub const MINIMUM_POLL_SECONDS: u64 = 60; // Client floor based on snapshot generation/cache cadence.
pub const ATTRIBUTION: &str = "OpenSeaFeed contributors · CC BY 4.0"; // Required provider credit.
pub const PROVIDER_URL: &str = "https://openseafeed.com/"; // Preserve a readable source reference.
pub const LICENSE_URL: &str = "https://creativecommons.org/licenses/by/4.0/"; // Preserve a readable license reference.
pub const COVERAGE: &str = "Reported AIS positions; incomplete reception; position age unavailable"; // Never promise complete or fresh coordinates.
pub const CHANGES: &str = "Validity-filtered and projected; known non-vessel/group identities excluded; other unusual identities retained"; // Describe transformations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)] // Metadata is cheap to copy without fleet cloning.
pub struct FleetCounts {
    // Disjoint primary counts partition every advertised record.
    pub records: usize,          // All entries in the complete source array.
    pub malformed: usize,        // Invalid required identity/time/coordinates.
    pub unpositioned: usize,     // Valid metadata with missing coordinate components.
    pub sar_aircraft: usize,     // Positioned 111-prefixed SAR identities excluded.
    pub navigation_aids: usize,  // Positioned 99-prefixed aid identities excluded.
    pub distress_devices: usize, // Positioned 970/972/974 identities excluded.
    pub coast_or_group: usize,   // Positioned 00MID/0MID identities excluded.
    pub unusual_retained: usize, // Diagnostic subset of retained positions, not an additional omission.
} // End count metadata.
impl FleetCounts {
    // Derived count avoids a second mutable exclusion total.
    pub fn excluded(&self) -> usize {
        // These categories are mutually exclusive.
        self.sar_aircraft + self.navigation_aids + self.distress_devices + self.coast_or_group
        // No position sampling.
    } // End the total.
} // End count helpers.
#[derive(Debug)] // Diagnostics need not clone a large fleet.
pub struct FleetSnapshot {
    // Immutable decoded content, separate from local receipt.
    pub positions: Arc<Vec<Position>>, // Shared geometry input without vector cloning.
    pub generated_ms: i64,             // Snapshot-build time only.
    pub latest_ais_update_ms: Option<i64>, // Latest ts among structurally valid records, including valid omissions.
    pub counts: FleetCounts, // Complete-response accounting survives adapter boundaries.
} // End decoded content.
#[derive(Debug, Clone)] // Cache hits can share the identical data and original receipt.
pub struct FleetReceipt {
    // Network receipt belongs to this complete successful fetch.
    pub snapshot: Arc<FleetSnapshot>, // Cache consumers retain the same geometry identity.
    pub received_ms: i64, // Body-completion time, before JSON decoding; never refreshed by a cache hit.
} // End receipt metadata.
#[derive(Default)] // One visitor owns the bounded output and identity set.
struct VesselRows {
    // Temporary state is released on any whole-response error.
    positions: Vec<Position>, // At most MAX_RECORDS admitted positions.
    identities: HashSet<u32>, // Detect contradictory duplicate identity rows rather than choosing one silently.
    latest_ais_update_ms: Option<i64>, // Not a substitute for Position::observed_ms.
    counts: FleetCounts,      // Count every array entry exactly once.
} // End temporary decode state.
impl VesselRows {
    // Row validation is independent from JSON framing.
    fn accept(&mut self, row: &Value) -> Result<(), String> {
        // Errors here invalidate the full snapshot only for duplicates.
        self.counts.records += 1; // This includes malformed and unpositioned entries.
        let Some((mmsi, update_ms, longitude, latitude)) = row_fields(row) else {
            // Validate required and coordinate fields.
            self.counts.malformed += 1; // A malformed row is not a valid empty fleet.
            return Ok(()); // Continue inspecting other bounded rows.
        }; // Required metadata is now trustworthy as reported data, not authenticated identity.
        if !self.identities.insert(mmsi) {
            return Err("duplicate OpenSeaFeed identity".into());
        } // Reject ambiguous duplicate state.
        self.latest_ais_update_ms = Some(
            self.latest_ais_update_ms
                .map_or(update_ms, |old| old.max(update_ms)),
        ); // Preserve any-message time separately.
        let (Some(longitude), Some(latitude)) = (longitude, latitude) else {
            // Missing coordinates are a legitimate omission.
            self.counts.unpositioned += 1; // Do not count missing coordinates as malformed JSON.
            return Ok(()); // There is no location to project.
        }; // A finite in-range coordinate pair is available.
        let excluded = match mmsi {
            // ITU/NAVCEN namespace policy, not identity authentication.
            111_000_000..=111_999_999 => Some(&mut self.counts.sar_aircraft), // SAR-aircraft namespace.
            990_000_000..=999_999_999 => Some(&mut self.counts.navigation_aids), // Aid-to-navigation namespace.
            970_000_000..=970_999_999 | 972_000_000..=972_999_999 | 974_000_000..=974_999_999 => {
                Some(&mut self.counts.distress_devices)
            } // Distress transmitters.
            2_000_000..=7_999_999 | 20_000_000..=79_999_999 => {
                Some(&mut self.counts.coast_or_group)
            } // Zero-padded 00MID/0MID; MID begins 2 through 7.
            _ => None, // Other unusual programming is retained, not silently classified as non-vessel.
        }; // Categories cannot overlap.
        if let Some(count) = excluded {
            *count += 1;
            return Ok(());
        } // Exclusions count positioned records only.
        let mut position = Position::new(mmsi.to_string(), longitude, latitude, None)
            .ok_or("invalid validated fleet position")?; // Position-fix time is unavailable.
        position.heading_degrees = row["hdg"]
            .as_u64()
            .filter(|heading| *heading < 360)
            .map(|heading| heading as f64); // Never substitute COG for heading.
        position.speed_metres_per_second = row["sog"]
            .as_f64()
            .filter(|speed| speed.is_finite() && (0.0..102.3).contains(speed))
            .map(|speed| speed * 1852.0 / 3600.0); // Knots to metres per second.
        position.label = row["name"]
            .as_str()
            .map(|name| name.chars().filter(|c| !c.is_control()).take(120).collect()); // Bound and sanitize optional text.
        if !(200_000_000..=799_999_999).contains(&mmsi) && mmsi / 10_000_000 != 98 {
            self.counts.unusual_retained += 1;
        } // Diagnostic only; never drop these positions.
        self.positions.push(position); // Every non-excluded valid position and real heading is preserved.
        Ok(()) // No inferred movement or freshness.
    } // End row admission.
} // End row collector.
fn row_fields(row: &Value) -> Option<(u32, i64, Option<f64>, Option<f64>)> {
    // Validate before classifying missing coordinates.
    let mmsi = row["mmsi"]
        .as_u64()
        .filter(|id| (1..=999_999_999).contains(id))? as u32; // A reported numeric MMSI must fit nine digits and be nonzero.
    let update_ms = row["ts"]
        .as_u64()
        .and_then(|time| i64::try_from(time).ok())?; // Require representable any-message milliseconds.
    let longitude = optional_number(&row["lon"]).ok()?; // Missing/null is distinct from malformed non-null.
    let latitude = optional_number(&row["lat"]).ok()?; // Validate independently, including half-present pairs.
    if longitude.is_some_and(|x| !(-180.0..=180.0).contains(&x))
        || latitude.is_some_and(|y| !(-90.0..=90.0).contains(&y))
    {
        return None;
    } // Supplied invalid components are malformed.
    Some((mmsi, update_ms, longitude, latitude)) // No fix timestamp is inferred from ts.
} // End scalar validation.
#[derive(Clone, Copy)] // Seeds borrow cancellation, never own another worker.
struct SnapshotSeed<'a> {
    stop: &'a AtomicBool,
    limit: usize,
} // Limit is injectable only inside this private implementation.
impl<'de> DeserializeSeed<'de> for SnapshotSeed<'_> {
    // Stateful wrapper decoder.
    type Value = FleetSnapshot; // Return validated source content only.
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        // Require an object wrapper.
        deserializer.deserialize_map(self) // A bare array is not the observed production schema.
    } // End wrapper dispatch.
} // End seed implementation.
impl<'de> Visitor<'de> for SnapshotSeed<'_> {
    // Read required wrapper fields in any order.
    type Value = FleetSnapshot; // Required fields are validated before publication.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenSeaFeed snapshot object")
    } // A bounded diagnostic.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        // No fleet-sized intermediate JSON tree.
        let (mut generated_ms, mut count, mut rows) = (None, None, None); // Track presence separately from valid zero values.
        while let Some(key) = map.next_key::<String>()? {
            // Unknown wrapper additions remain forward-compatible.
            check_stop(self.stop).map_err(de::Error::custom)?; // Cooperative cancellation at each wrapper field.
            match key.as_str() {
                // Duplicate required fields must not overwrite prior values.
                "generated_at" => {
                    // Snapshot generation is not a position timestamp.
                    if generated_ms.is_some() {
                        return Err(de::Error::duplicate_field("generated_at"));
                    } // Reject ambiguity.
                    let time = map.next_value::<u64>()?; // Exact integer milliseconds.
                    generated_ms = Some(i64::try_from(time).map_err(de::Error::custom)?);
                    // Reject overflow, never clamp.
                } // End generation field.
                "count" => {
                    // Validate the advertised count, wherever it appears.
                    if count.is_some() {
                        return Err(de::Error::duplicate_field("count"));
                    } // Reject ambiguity.
                    let value = map.next_value::<usize>()?; // Negative/fractional counts are invalid.
                    if value > self.limit {
                        return Err(de::Error::custom("OpenSeaFeed record limit exceeded"));
                    } // Fail before allocation when possible.
                    count = Some(value); // Compare to the actual complete array later.
                } // End advertised count.
                "vessels" => {
                    // Decode entries incrementally.
                    if rows.is_some() {
                        return Err(de::Error::duplicate_field("vessels"));
                    } // Never concatenate duplicate arrays.
                    rows = Some(map.next_value_seed(RowSeed(self))?); // At most one row value exists alongside output.
                } // End fleet array.
                _ => {
                    let _: IgnoredAny = map.next_value()?;
                } // Skip unknown wrapper data without retaining it.
            } // End one field.
        } // End complete wrapper.
        let generated_ms = generated_ms.ok_or_else(|| de::Error::missing_field("generated_at"))?; // Required even for empty fleets.
        let count = count.ok_or_else(|| de::Error::missing_field("count"))?; // Absence is not zero.
        let rows = rows.ok_or_else(|| de::Error::missing_field("vessels"))?; // Absence is not an empty array.
        if count != rows.counts.records {
            return Err(de::Error::custom(
                "OpenSeaFeed count does not match complete array",
            ));
        } // No silent truncation.
        if count > 0 && rows.counts.malformed == count {
            return Err(de::Error::custom(
                "OpenSeaFeed contains only malformed rows",
            ));
        } // Retain last-good state on this error.
        Ok(FleetSnapshot {
            positions: Arc::new(rows.positions),
            generated_ms,
            latest_ais_update_ms: rows.latest_ais_update_ms,
            counts: rows.counts,
        }) // Preserve independent times and accounting.
    } // End wrapper validation.
} // End object visitor.
struct RowSeed<'a>(SnapshotSeed<'a>); // Pass the same bounds and stop flag into the array.
impl<'de> DeserializeSeed<'de> for RowSeed<'_> {
    // Stateful sequence dispatch.
    type Value = VesselRows; // Accumulate only bounded accepted output and counts.
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    } // Require an array.
} // End row-seed dispatch.
impl<'de> Visitor<'de> for RowSeed<'_> {
    // Enforce the record bound while consuming, not after allocation.
    type Value = VesselRows; // No partial successful publication.
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded AIS record array")
    } // A bounded diagnostic.
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        // Visit the complete bounded sequence.
        let mut rows = VesselRows::default(); // No capacity allocation from an untrusted hint.
        for _ in 0..self.0.limit {
            // Never admit more than the configured hard ceiling.
            check_stop(self.0.stop).map_err(de::Error::custom)?; // Cancel between rows; one row remains bounded by the body ceiling.
            let Some(row) = sequence.next_element::<Value>()? else {
                return Ok(rows);
            }; // Normal end, including an empty array.
            rows.accept(&row).map_err(de::Error::custom)?; // Retain rejection/exclusion counts; reject duplicates.
        } // The maximum number of entries has been consumed.
        check_stop(self.0.stop).map_err(de::Error::custom)?; // Do not continue an obsolete maximum-sized decode.
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(de::Error::custom("OpenSeaFeed record limit exceeded"));
        } // Extra entry rejects the whole snapshot without retaining its tree.
        Ok(rows) // Exactly-at-bound input is legal.
    } // End bounded sequence.
} // End row visitor.
fn check_stop(stop: &AtomicBool) -> Result<(), String> {
    // No wall or receipt timestamp is manufactured here.
    if stop.load(Ordering::Relaxed) {
        return Err("OpenSeaFeed decode cancelled".into());
    } // Discard unpublished work.
    Ok(()) // Continue the finite decode.
} // End cancellation guard.
pub fn decode(bytes: &[u8], stop: &AtomicBool) -> Result<FleetSnapshot, String> {
    // Production decoder uses fixed admission limits.
    decode_bounded(bytes, stop, MAX_RESPONSE_BYTES, MAX_RECORDS) // Source-specific limits do not widen other adapters.
} // End public decoder.
fn decode_bounded(
    bytes: &[u8],
    stop: &AtomicBool,
    byte_limit: usize,
    record_limit: usize,
) -> Result<FleetSnapshot, String> {
    // Private boundary-test seam.
    check_stop(stop)?; // Stop before allocation or scanning.
    if bytes.len() > byte_limit {
        return Err("OpenSeaFeed decompressed response limit exceeded".into());
    } // Check the complete body before JSON allocation.
    let mut deserializer = serde_json::Deserializer::from_slice(bytes); // Serde's default nesting bound remains enabled.
    let snapshot = SnapshotSeed {
        stop,
        limit: record_limit,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| format!("invalid OpenSeaFeed snapshot: {error}"))?; // Validate schema/counts.
    deserializer
        .end()
        .map_err(|error| format!("trailing OpenSeaFeed data: {error}"))?; // Reject trailing JSON or a truncated envelope.
    check_stop(stop)?; // Cancelled work must not be published as success.
    Ok(snapshot) // Receipt is supplied separately by the network adapter.
} // End complete-body validation.
#[cfg(test)] // Deterministic fixtures, never public fleet queries.
mod tests {
    // Exercise actual decoder paths with synthetic identities and coordinates.
    use super::*; // Access the private bounded-decoder seam.
    fn body(rows: &str, count: usize) -> Vec<u8> {
        format!(r#"{{"generated_at":9000,"count":{count},"vessels":[{rows}]}}"#).into_bytes()
    } // Explicit wrapper fixture.
    fn read(rows: &str, count: usize) -> Result<FleetSnapshot, String> {
        decode(&body(rows, count), &AtomicBool::new(false))
    } // No network or production data.
    #[test] // Any-message updates must not rejuvenate coordinate-fix time.
    fn updates_are_not_fixes_and_cog_is_not_heading() {
        // A newer ts with identical coordinates remains unknown-age geometry.
        let first = read(
            r#"{"mmsi":234567890,"ts":1000,"lon":2,"lat":48,"hdg":511,"cog":90,"sog":10}"#,
            1,
        )
        .unwrap(); // Sentinel heading plus known course.
        let second = read(
            r#"{"mmsi":234567890,"ts":8000,"lon":2,"lat":48,"hdg":511,"cog":90,"sog":10}"#,
            1,
        )
        .unwrap(); // Static-update simulation.
        assert_eq!(first.positions, second.positions); // No inferred coordinate change or fake fix time.
        assert_eq!(second.positions[0].observed_ms, None); // Receipt/build/update are not coordinate time.
        assert_eq!(second.positions[0].heading_degrees, None); // Course never substitutes for heading.
        assert!(
            (second.positions[0].speed_metres_per_second.unwrap() - 10.0 * 1852.0 / 3600.0).abs()
                < 1e-10
        ); // Exact source-unit conversion.
        assert_eq!(
            (second.generated_ms, second.latest_ais_update_ms),
            (9000, Some(8000))
        ); // Times remain independently inspectable.
    } // End timestamp test.
    #[test] // Legitimate empty and omitted fleets differ from malformed responses.
    fn omission_accounting_and_all_invalid_guard() {
        // Mixed validity preserves useful rows.
        assert_eq!(read("", 0).unwrap().counts.records, 0); // Genuine empty array.
        assert_eq!(
            read(r#"{"mmsi":234567890,"ts":1}"#, 1)
                .unwrap()
                .counts
                .unpositioned,
            1
        ); // Missing coordinates are legitimate.
        assert!(read("null,{}", 2).is_err()); // All malformed must retain the caller's last-good state.
        let result = read(
            r#"null,{"mmsi":234567890,"ts":1,"lat":48,"lon":2,"hdg":90},{"mmsi":234567891,"ts":2}"#,
            3,
        )
        .unwrap(); // Mixed finite fixture.
        assert_eq!(
            (
                result.positions.len(),
                result.counts.malformed,
                result.counts.unpositioned
            ),
            (1, 1, 1)
        ); // Partition every record.
        assert_eq!(result.positions[0].heading_degrees, Some(90.0)); // Preserve actual heading.
        for row in [
            r#"{"mmsi":1,"ts":-1}"#,
            r#"{"mmsi":1,"ts":9223372036854775808}"#,
            r#"{"mmsi":1,"ts":1,"lon":"bad"}"#,
            r#"{"mmsi":1,"ts":1,"lon":181}"#,
        ] {
            // Bad timestamps and half-present invalid coordinates.
            assert!(read(row, 1).is_err()); // None of these is a legitimate unpositioned record.
        } // End malformed fixtures.
    } // End accounting test.
    #[test] // Only documented non-vessel/group namespaces are excluded.
    fn namespaces_are_explicit_and_unusual_ids_are_not_silently_dropped() {
        // All locations below are synthetic.
        let ids = [
            111234567, 992345678, 970123456, 972123456, 974123456, 2345678, 23456789, 123,
            982345678, 234567890,
        ]; // Includes short unknown and associated craft.
        let rows = ids
            .iter()
            .map(|id| format!(r#"{{"mmsi":{id},"ts":1,"lat":0,"lon":0}}"#))
            .collect::<Vec<_>>()
            .join(","); // Distinct valid metadata.
        let result = read(&rows, ids.len()).unwrap(); // Apply the actual classification policy.
        assert_eq!(
            (
                result.counts.sar_aircraft,
                result.counts.navigation_aids,
                result.counts.distress_devices,
                result.counts.coast_or_group
            ),
            (1, 1, 3, 2)
        ); // Visible exclusion categories.
        assert_eq!(
            (result.positions.len(), result.counts.unusual_retained),
            (3, 1)
        ); // Retain associated craft and ambiguous short identity.
        assert_eq!(
            result.positions.len() + result.counts.excluded(),
            result.counts.records
        ); // No hidden omissions.
        assert!(read(r#"{"mmsi":111234567,"ts":1,"lat":0,"lon":0}"#, 1)
            .unwrap()
            .positions
            .is_empty()); // A valid filtered-only response is not a failure.
    } // End namespace test.
    #[test] // Wrapper ambiguity, mismatched counts and duplicate identities fail closed.
    fn wrapper_and_duplicate_guards() {
        // These cases must never publish a partial replacement.
        let row = r#"{"mmsi":234567890,"ts":1,"lat":0,"lon":0}"#; // One synthetic record.
        assert!(read(row, 0).is_err()); // Advertised count must match actual rows.
        assert!(read(&format!("{row},{row}"), 2).is_err()); // No silent choice between duplicate identities.
        for bytes in [
            b"[]".as_slice(),
            br#"{"generated_at":0,"count":0}"#,
            br#"{"generated_at":0,"count":0,"count":0,"vessels":[]}"#,
            br#"{"generated_at":0,"count":0,"vessels":[]} {}"#,
        ] {
            // Required presence, unique keys and complete framing.
            assert!(decode(bytes, &AtomicBool::new(false)).is_err()); // All four are invalid snapshots.
        } // End malformed wrappers.
        assert!(decode(
            br#"{"vessels":[],"count":0,"generated_at":0,"extra":{"nested":[1]}}"#,
            &AtomicBool::new(false)
        )
        .is_ok()); // Field order and unknown additions are allowed.
    } // End wrapper test.
    #[test] // Exercise byte and record ceilings on the production path with smaller private limits.
    fn exact_bounds_and_cancellation() {
        // No huge allocations or network dependencies in this unit test.
        let stop = AtomicBool::new(false); // Running decoder fixture.
        let bytes = body(r#"{"mmsi":123,"ts":1}"#, 1); // Valid unpositioned row.
        assert!(decode_bounded(&bytes, &stop, bytes.len(), 1).is_ok()); // Exactly at both bounds is allowed.
        assert!(decode_bounded(&bytes, &stop, bytes.len() - 1, 1).is_err()); // One byte over is rejected.
        let extra = body(r#"{"mmsi":123,"ts":1},{"mmsi":124,"ts":1}"#, 1); // Count is within bound, actual array is not.
        assert!(decode_bounded(&extra, &stop, extra.len(), 1)
            .unwrap_err()
            .contains("record limit")); // Sequence limit acts before trusting count equality.
        stop.store(true, Ordering::Relaxed); // Simulate cancellation before decoding.
        assert!(decode(&bytes, &stop).unwrap_err().contains("cancelled")); // No cancelled success.
    } // End boundary test.
} // End decoder fixtures.
