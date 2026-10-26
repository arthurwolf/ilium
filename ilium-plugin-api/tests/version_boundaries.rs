//! API strings and binary sections describe the same unsigned 16-bit triplet.
use ilium_plugin_api::version::{API_VERSION_BYTES, ApiVersion, CONTRACT, CURRENT};

#[test]
fn published_identity_and_payload_agree() {
    assert_eq!(ApiVersion::parse(CONTRACT).unwrap(), CURRENT);
    assert_eq!(
        ApiVersion::from_custom_section(&API_VERSION_BYTES).unwrap(),
        CURRENT
    );
    assert_eq!(format!("ilium:plugin@{CURRENT}"), CONTRACT);
}

#[test]
fn unsigned_sixteen_bit_bounds_are_lossless() {
    let version = ApiVersion::parse("ilium:plugin@65535.256.513").unwrap();
    assert_eq!(
        version,
        ApiVersion {
            major: 65535,
            minor: 256,
            patch: 513
        }
    );
    assert_eq!(
        ApiVersion::from_custom_section(&[255, 255, 1, 0, 2, 1]).unwrap(),
        version
    );
    assert_eq!(version.to_string(), "65535.256.513");
}

#[test]
fn malformed_or_noncanonical_contracts_are_schema_errors() {
    for contract in [
        "",
        "0.1.0",
        " ilium:plugin@0.1.0",
        "ilium:plugin@0.1.0 ",
        "ilium:plugin@0.1",
        "ilium:plugin@0.1.0.0",
        "ilium:plugin@00.1.0",
        "ilium:plugin@0.01.0",
        "ilium:plugin@0.1.00",
        "ilium:plugin@+0.1.0",
        "ilium:plugin@0.-1.0",
        "ilium:plugin@0.1.0-beta",
        "ilium:plugin@0.1.0+build",
        "ilium:plugin@65536.1.0",
        "ilium:plugin@0.65536.0",
        "ilium:plugin@0.1.65536",
        "ilium:plugin@٠.1.0",
        "ilium:plugin@0..0",
    ] {
        let error = ApiVersion::parse(contract).expect_err(contract);
        assert_eq!(error.code.as_str(), "manifest.schema", "{contract}");
        assert_eq!(error.pointer, "/api");
        assert_eq!(error.stage.unwrap().0, 3);
    }
}

#[test]
fn parsing_does_not_reject_well_formed_unsupported_versions() {
    let future = ApiVersion::parse("ilium:plugin@1.0.0").unwrap();
    assert!(!ApiVersion::host_window().contains(&future));
}

#[test]
fn section_payload_is_exactly_six_bytes() {
    for length in [0, 1, 2, 3, 4, 5, 7, 8, 16] {
        let error = ApiVersion::from_custom_section(&vec![0; length]).unwrap_err();
        assert_eq!(error.code.as_str(), "package.api-section-invalid");
        assert_eq!(error.stage.unwrap().0, 6);
    }
}

#[test]
fn host_window_counts_minors_and_includes_every_patch() {
    let window = ApiVersion::host_window();
    assert_eq!(window.start().major, CURRENT.major);
    assert_eq!(window.start().minor, CURRENT.minor.saturating_sub(1));
    assert_eq!(window.start().patch, 0);
    for minor in CURRENT.minor.saturating_sub(1)..=CURRENT.minor {
        for patch in [0, CURRENT.patch, u16::MAX] {
            assert!(window.contains(&ApiVersion {
                major: CURRENT.major,
                minor,
                patch
            }));
        }
    }
    if let Some(minor) = CURRENT.minor.checked_add(1) {
        assert!(!window.contains(&ApiVersion {
            major: CURRENT.major,
            minor,
            patch: 0
        }));
    }
    if let Some(minor) = CURRENT.minor.checked_sub(2) {
        assert!(!window.contains(&ApiVersion {
            major: CURRENT.major,
            minor,
            patch: u16::MAX
        }));
    }
}
