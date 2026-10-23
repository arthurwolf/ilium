//! Manual page `reference-platform-support`: release notes name the tested terminals (HST-389),
//! the 256-colour expectation and its fallback (HST-390), and the platform removal lifecycle
//! (HST-391, HST-392).

use super::common::{assert_page_states, manual_page};

const PAGE: &str = "reference-platform-support.md";

/// REQ: HST-389
#[test]
fn hst_389_release_notes_name_the_tested_terminals_for_each_shipped_platform() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The release notes MUST name, for each shipped platform, the terminal emulators from the table above that the release was tested in",
    );
}

/// REQ: HST-389
#[test]
fn hst_389_the_client_is_not_described_as_supported_in_any_other_terminal() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST NOT describe the client as supported in any other terminal",
    );
}

/// REQ: HST-390
#[test]
fn hst_390_the_client_expects_at_least_256_colours_from_the_outer_terminal() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "The client MUST expect at least 256 colours from the outer terminal (proposed, owner to confirm).",
    );
    assert_page_states(
        &page,
        "MUST map each `indexed` and `rgb` colour of a plugin to the nearest colour the terminal offers",
    );
}

/// REQ: HST-390
#[test]
fn hst_390_a_16_colour_terminal_still_starts_and_does_not_fail_the_plugin_call() {
    unimplemented!("ilium_client::render::nearest_terminal_colour")
}

/// REQ: HST-391
#[test]
fn hst_391_a_platform_leaves_the_matrix_only_after_a_minor_release_names_the_removal() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "unless the minor release before it named the removal in its release notes",
    );
}

/// REQ: HST-391
#[test]
fn hst_391_a_patch_release_never_removes_a_platform() {
    let page = manual_page(PAGE);
    assert_page_states(&page, "A patch release MUST NOT remove a platform.");
}

/// REQ: HST-392
#[test]
fn hst_392_a_removed_platform_stops_shipping_ilium_and_its_t1_bundled_plugins() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "the release MUST stop shipping Ilium and its T1 bundled plugins for that platform, including the `.cwasm` for its triple",
    );
}

/// REQ: HST-392
#[test]
fn hst_392_an_installed_plugin_for_a_removed_platform_is_refused_and_keeps_its_data() {
    let page = manual_page(PAGE);
    assert_page_states(
        &page,
        "MUST be marked `refused-at-load` with `package.platform-unsupported` and MUST keep its data",
    );
}
