//! Minimum toolchain support window of the SDK (DEV-897). Page: guide-compatibility-and-upgrades.

use crate::{manual_page, sdk_builds_with_toolchain, sdk_package_metadata};

/// REQ: DEV-897
#[test]
fn sdk_declares_minimum_rust_and_bun_and_previous_minimum_still_builds_for_two_minors() {
    let page = manual_page("guide-compatibility-and-upgrades");
    assert!(
        page.contains("minimum"),
        "the guide states the minimum toolchains"
    );
    assert!(
        page.contains("two minor releases"),
        "the guide states the two-minor window"
    );
    assert!(
        page.contains("release notes"),
        "the guide states the one-minor announcement"
    );

    let metadata = sdk_package_metadata();
    assert!(
        metadata["minimum_rust"].is_string(),
        "the SDK metadata declares minimum_rust: {metadata}"
    );
    assert!(
        metadata["minimum_bun"].is_string(),
        "the SDK metadata declares minimum_bun: {metadata}"
    );

    let declared = metadata["minimum_rust"]
        .as_str()
        .expect("minimum_rust text")
        .to_owned();
    assert!(
        sdk_builds_with_toolchain(&declared),
        "the SDK builds with its declared minimum Rust {declared}"
    );
    let previous = metadata["previous_minimum_rust"]
        .as_str()
        .expect("previous_minimum_rust is declared after a raise")
        .to_owned();
    assert!(
        sdk_builds_with_toolchain(&previous),
        "after a raise the previous minimum {previous} still builds for two minor releases"
    );
}
