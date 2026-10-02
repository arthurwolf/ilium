//! Bounded inflation of embedded, provenance-recorded OSM JSON.
use std::io::Read;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub fn decode_bundle(bytes: &[u8]) -> Result<Vec<u8>, String> {
    decode_with_limit(bytes, MAX_JSON_BYTES)
}
fn decode_with_limit(bytes: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(limit as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|error| format!("Invalid compressed OSM extract: {error}"))?;
    if raw.len() > limit {
        return Err(format!("OSM extract exceeds {limit} bytes"));
    }
    Ok(raw)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_world_place_contains_its_recorded_real_osm_elements() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/openstreetmap/catalogue.json"))
                .unwrap();
        for (place, record) in super::super::catalogue::PLACES
            .iter()
            .zip(manifest["records"].as_array().unwrap())
        {
            let raw = decode_bundle(place.data).unwrap();
            let data: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(
                data["elements"].as_array().unwrap().len() as u64,
                record["element_count"].as_u64().unwrap()
            );
            assert_eq!(place.name, record["name"].as_str().unwrap());
            assert!(!data["elements"].as_array().unwrap().is_empty());
            assert!(data["remark"].is_null());
        }
    }
    #[test]
    fn compressed_input_cannot_exceed_the_expanded_byte_budget() {
        let mut compressed = Vec::new();
        flate2::read::GzEncoder::new(&b"0123456789"[..], flate2::Compression::fast())
            .read_to_end(&mut compressed)
            .unwrap();
        assert!(decode_with_limit(&compressed, 8)
            .unwrap_err()
            .contains("exceeds 8 bytes"));
        assert_eq!(decode_with_limit(&compressed, 10).unwrap(), b"0123456789");
    }
    #[test]
    fn corrupt_compression_is_reported_without_accepting_partial_bytes() {
        assert!(decode_bundle(b"broken").is_err());
        let bytes = super::super::catalogue::PLACES[0].data;
        assert!(decode_bundle(&bytes[..bytes.len() / 2]).is_err());
    }
}
