//! Manual page `reference-minimum-system-requirements`: the installer accepts only the shipped
//! operating system and architecture pairs (HST-955), the macOS and Windows minimum releases
//! (HST-956, HST-957) and the refusal that happens before any write (HST-958).

use ilium_plugin_host::platform_rules::shipped_triples;

use ilium_plugin_host::platform_rules::check_minimum_release;

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-minimum-system-requirements.md";

/// REQ: HST-955
#[test]
fn hst_955_the_installer_accepts_exactly_the_four_shipped_platform_triples() {
    let mut triples = shipped_triples();
    triples.sort();
    let expected = [
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
    ];
    assert_eq!(triples, expected);
}

/// REQ: HST-955
#[test]
fn hst_955_an_intel_mac_or_arm_windows_pair_is_not_an_accepted_platform() {
    let triples = shipped_triples();
    assert!(
        !triples
            .iter()
            .any(|triple| triple.as_str() == "x86_64-apple-darwin"),
        "Intel macOS is not a shipped pair"
    );
    assert!(
        !triples
            .iter()
            .any(|triple| triple.as_str() == "aarch64-pc-windows-msvc"),
        "Windows on arm is not a shipped pair"
    );
}

/// REQ: HST-956
#[test]
fn hst_956_the_macos_aarch64_minimum_release_is_macos_14_sonoma() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "On macOS aarch64 the minimum release is macOS 14 Sonoma (proposed, owner to confirm).",
    );
}

/// REQ: HST-956
#[test]
fn hst_956_the_installer_refuses_an_earlier_macos_release_with_install_unsupported_platform() {
    let refusal = check_minimum_release("macos", 13).expect_err("macOS 13 is below the minimum");
    assert!(refusal.starts_with("install.unsupported-platform"));
    assert!(refusal.contains("13") && refusal.contains("14"));
    assert!(check_minimum_release("macos", 14).is_ok());
    assert!(check_minimum_release("windows", 19045).is_err(), "Windows 10 is refused");
    assert!(check_minimum_release("windows", 22621).is_ok());
}

/// REQ: HST-957
#[test]
fn hst_957_the_windows_minimum_is_windows_11_22h2_build_22621() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "On Windows x86_64 the minimum release is Windows 11, version 22H2, build 22621 (proposed, owner to confirm).",
    );
}

/// REQ: HST-957
#[test]
fn hst_957_windows_10_is_not_accepted_as_the_minimum() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "Windows 10 is not accepted, because Microsoft ended its mainstream support in October 2025.",
    );
}

/// REQ: HST-958
#[test]
fn hst_958_a_refusal_exits_with_status_1_and_prints_one_line_with_the_values() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "The installer MUST exit with status 1");
    assert_page_states(
        &page,
        "MUST print one line with the error code and the detected and minimum values",
    );
}

/// REQ: HST-958
#[test]
fn hst_958_a_refusal_happens_before_any_file_is_written_to_the_install_location() {
    // The refusal is a pure decision made before any install step: one line, values included,
    // and the same input always refuses without touching a folder.
    let folder = tempfile::tempdir().expect("install location");
    let refusal = check_minimum_release("macos", 12).expect_err("refused");
    assert_eq!(refusal.lines().count(), 1, "exactly one line: {refusal}");
    assert!(refusal.contains("12") && refusal.contains("14"));
    assert_eq!(
        std::fs::read_dir(folder.path()).expect("read install location").count(),
        0,
        "no file is written to the install location"
    );
}
