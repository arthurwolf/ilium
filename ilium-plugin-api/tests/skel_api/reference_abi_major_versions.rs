//! ABI-950 and ABI-951 from reference-abi-major-versions.md: how long the final minor of a major
//! line stays loadable after the next major release, and the refusal after that date.
//!
//! The documentation tests read the manual page and assert the concrete numbers and codes of the
//! rule. The behaviour tests call the retention check and the refusal check of the host, which do
//! not exist yet; they fail until those items land (see API_GAPS.md).

fn manual_page() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs/plugins-manual/pages/reference-abi-major-versions.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} must be readable: {error}", path.display());
    })
}

/// The single bullet line that states a requirement, for example `- [ABI-950] The final ...`.
fn rule_line(page: &str, requirement_id: &str) -> String {
    let prefix = format!("- [{requirement_id}] ");
    page.lines()
        .find(|line| line.starts_with(&prefix))
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("the page must state {requirement_id} as a bullet"))
}

/// REQ: ABI-950
#[test]
fn abi_950_rule_states_twelve_month_window_after_2_0_0_marked_for_owner_confirmation() {
    let line = rule_line(&manual_page(), "ABI-950");
    assert!(
        line.contains(
            "The final `0.x` minor MUST stay loadable until 12 months (proposed, owner to confirm) after the release of `2.0.0`"
        ),
        "ABI-950 must keep the final 0.x minor loadable for 12 months after 2.0.0, marked as proposed"
    );
    assert!(
        line.contains("refused with `package.api-unsupported`"),
        "ABI-950 must name the refusal code after the window"
    );
}

/// REQ: ABI-950
#[test]
fn abi_950_final_0x_minor_loads_one_day_before_twelve_months_after_2_0_0() {
    unimplemented!("ilium_plugin_api::version::final_minor_loadable")
}

/// REQ: ABI-950
#[test]
fn abi_950_final_0x_minor_is_refused_with_package_api_unsupported_after_the_window() {
    unimplemented!("ilium_plugin_api::version::retention_refusal")
}

/// REQ: ABI-951
#[test]
fn abi_951_rule_covers_every_major_line_from_one_onward_with_twelve_month_window() {
    let line = rule_line(&manual_page(), "ABI-951");
    assert!(
        line.contains("For each major line `N` from `1` onward"),
        "ABI-951 must apply to every major line from 1 onward"
    );
    assert!(
        line.contains(
            "until 12 months (proposed, owner to confirm) after the release of major `N+1`"
        ),
        "ABI-951 must keep the final minor of each line for 12 months after the next major"
    );
    assert!(
        line.contains("the release notes of major `N+1` MUST state that date"),
        "ABI-951 must require the release notes of the next major to state the date"
    );
}

/// REQ: ABI-951
#[test]
fn abi_951_final_minor_of_major_one_loads_one_day_before_twelve_months_after_major_two() {
    unimplemented!("ilium_plugin_api::version::final_minor_loadable")
}

/// REQ: ABI-951
#[test]
fn abi_951_final_minor_of_major_two_loads_until_twelve_months_after_major_three() {
    unimplemented!("ilium_plugin_api::version::final_minor_loadable")
}

/// REQ: ABI-951
#[test]
fn abi_951_packages_of_major_one_are_refused_after_twelve_months_after_major_two() {
    unimplemented!("ilium_plugin_api::version::retention_refusal")
}
