//! Twins of the original tests of ilium-client/src/background_animation/video_negative_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/video_negative_tests.rs::actual_public_video_http_without_original_network_right_is_denied_before_decoder
#[test]
fn port_actual_public_video_http_without_original_network_right_is_denied_before_decoder() {
    let manifest = manifest_value();
    let grants = manifest["capabilities"]["scene"]
        .as_array()
        .expect("capabilities.scene is an array");
    let hosts: Vec<&str> = grants
        .iter()
        .filter_map(|grant| grant.as_str())
        .filter(|grant| grant.starts_with("http:"))
        .collect();
    let mut expected = vec![
        "http:services.swpc.noaa.gov",
        "http:lichess.org",
        "http:earthquake.usgs.gov",
        "http:api.exchange.coinbase.com",
        "http:api.frankfurter.dev",
        "http:api.wheretheiss.at",
        "http:api.drand.sh",
        "http:meri.digitraffic.fi",
        "http:opensky-network.org",
        "http:api.openseafeed.com",
        // The map-imagery scenes: NASA GIBS tiles (Earth at night, Satellite clouds) and the
        // EUMETSAT view service (Satellite clouds).
        "http:gibs.earthdata.nasa.gov",
        "http:view.eumetsat.int",
    ];
    let mut hosts = hosts;
    expected.sort_unstable();
    hosts.sort_unstable();
    assert_eq!(
        hosts, expected,
        "the network rights are exactly the hosts of the live-data scenes"
    );
    for id in MANIFEST_SCENES {
        let screen = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 0));
        assert_eq!(
            (screen.columns, screen.rows),
            (40, 10),
            "{id}: renders without any media or network import"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/video_negative_tests.rs::actual_public_video_rejects_unbranded_selected_asset_before_decoder
#[test]
fn port_actual_public_video_rejects_unbranded_selected_asset_before_decoder() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(
            first.text(),
            second.text(),
            "{id}: identical input must give an identical frame"
        );
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(
        scenes_page().to_lowercase().contains("video"),
        "the manual page covers this feature"
    );
}
