//! Worker-only loading. The caller owns scheduling, transport and cancellation.
use super::{
    bundle::{decode_bundle, MAX_JSON_BYTES},
    catalogue::PLACES,
    geometry::{parse_map, GeometryMap},
    request::MapRequest,
};
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct LoadedMap {
    pub request: MapRequest,
    pub label: String,
    pub map: GeometryMap,
}
/// The fetch callback is invoked only for an explicitly configured service.
/// All bytes, including cached/custom bytes, pass the same strict decoder.
pub fn load_map(
    request: MapRequest,
    stop: &AtomicBool,
    fetch: impl FnOnce(&str, &AtomicBool) -> Result<Vec<u8>, String>,
) -> Result<LoadedMap, String> {
    cancelled(stop)?;
    let center = request.center()?;
    let (bytes, label) = match &request {
        MapRequest::Catalogue(index) => {
            let place = PLACES.get(*index).ok_or("Unknown catalogue place")?;
            (decode_bundle(place.data)?, place.name.to_owned())
        }
        MapRequest::Local { path, .. } => {
            let metadata =
                std::fs::metadata(path).map_err(|error| format!("OSM file metadata: {error}"))?;
            if !metadata.is_file() {
                return Err("OSM source must be a regular JSON file".into());
            }
            if metadata.len() > MAX_JSON_BYTES as u64 {
                return Err("OSM file exceeds 16 MiB limit".into());
            }
            let file =
                std::fs::File::open(path).map_err(|error| format!("OSM file open: {error}"))?;
            (
                read_bounded(file, stop)?,
                format!("Local extract at {:.4}, {:.4}", center[0], center[1]),
            )
        }
        MapRequest::Custom { url, .. } => (
            fetch(url, stop)?,
            format!("Custom extract at {:.4}, {:.4}", center[0], center[1]),
        ),
    };
    cancelled(stop)?;
    let map = parse_map(&bytes, center)?;
    cancelled(stop)?;
    Ok(LoadedMap {
        request,
        label,
        map,
    })
}
fn cancelled(stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::Relaxed) {
        Err("OSM load cancelled".into())
    } else {
        Ok(())
    }
}
fn read_bounded(mut reader: impl Read, stop: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut block = [0u8; 64 * 1024];
    loop {
        cancelled(stop)?;
        let count = reader
            .read(&mut block)
            .map_err(|error| format!("OSM file read: {error}"))?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > MAX_JSON_BYTES {
            return Err("OSM file exceeds 16 MiB limit".into());
        }
        bytes.extend_from_slice(&block[..count]);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_and_local_sources_never_invoke_network() {
        let never_fetch =
            |_: &str, _: &AtomicBool| -> Result<Vec<u8>, String> { panic!("unexpected network") };
        let stop = AtomicBool::new(false);
        let offline = load_map(MapRequest::Catalogue(0), &stop, never_fetch).unwrap();
        assert_eq!(offline.label, "Paris");
        assert!(offline.map.features.len() > 100);
        let fixture = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fixture.path(), include_bytes!("fixtures/paris-layers.json")).unwrap();
        let local = MapRequest::Local {
            path: fixture.path().to_owned(),
            center: [48.8584, 2.2945],
        };
        assert_eq!(
            load_map(local, &stop, never_fetch)
                .unwrap()
                .map
                .sources
                .len(),
            6
        );
    }
    #[test]
    fn cancellation_and_oversize_reads_fail_before_loading_or_fetching() {
        let stop = AtomicBool::new(true);
        assert!(load_map(MapRequest::Catalogue(0), &stop, |_, _| panic!(
            "unexpected fetch"
        ))
        .is_err());
        assert!(read_bounded(std::io::Cursor::new(b"{}"), &stop).is_err());
        let stop = AtomicBool::new(false);
        assert!(
            read_bounded(std::io::repeat(b' ').take(MAX_JSON_BYTES as u64 + 1), &stop).is_err()
        );
    }
    #[test]
    fn explicit_custom_source_uses_callback_once_and_rejects_partial_data() {
        let request = MapRequest::Custom {
            url: "https://example.org/api?data=test".into(),
            center: [0., 0.],
        };
        let stop = AtomicBool::new(false);
        let loaded = load_map(request.clone(), &stop, |url, _| {
            assert_eq!(url, "https://example.org/api?data=test");
            Ok(br#"{"elements":[]}"#.to_vec())
        })
        .unwrap();
        assert_eq!(loaded.request, request);
        assert!(load_map(request, &stop, |_, _| Ok(
            br#"{"elements":[],"remark":"timeout"}"#.to_vec()
        ))
        .is_err());
    }
    #[test]
    fn invalid_request_identity_fails_before_io() {
        let stop = AtomicBool::new(false);
        assert!(
            load_map(MapRequest::Catalogue(usize::MAX), &stop, |_, _| panic!(
                "unexpected network"
            ))
            .is_err()
        );
        let invalid = MapRequest::Custom {
            url: "https://example.org/api".into(),
            center: [f64::NAN, 0.],
        };
        assert!(load_map(invalid, &stop, |_, _| panic!("unexpected network")).is_err());
    }
}
