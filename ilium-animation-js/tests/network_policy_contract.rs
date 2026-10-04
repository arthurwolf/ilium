use ilium_animation_js::network::{classify_address, validate_https_origin, NetworkAddressClass};
use std::net::IpAddr;

#[test]
fn private_and_metadata_addresses_require_separate_local_authority() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "::ffff:127.0.0.1",
    ] {
        assert_ne!(
            classify_address(address.parse::<IpAddr>().unwrap()),
            NetworkAddressClass::Public,
            "{address}"
        );
    }
    for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
        assert_eq!(
            classify_address(address.parse::<IpAddr>().unwrap()),
            NetworkAddressClass::Public,
            "{address}"
        );
    }
}
#[test]
fn origin_scope_is_exact_https_without_credentials_paths_or_fragments() {
    assert_eq!(
        validate_https_origin("https://example.org").unwrap(),
        "https://example.org"
    );
    assert_eq!(
        validate_https_origin("https://example.org:443").unwrap(),
        "https://example.org"
    );
    assert_eq!(
        validate_https_origin("https://example.org:8443").unwrap(),
        "https://example.org:8443"
    );
    for origin in [
        "http://example.org",
        "https://user:pass@example.org",
        "https://example.org/path",
        "https://example.org?x=1",
        "https://example.org#x",
        "https://*.example.org",
    ] {
        assert!(validate_https_origin(origin).is_err(), "{origin}");
    }
}
