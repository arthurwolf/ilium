//! ABI-950 and ABI-951 from reference-abi-major-versions.md: how long the final minor of a major
//! line stays loadable after the next major release, and the refusal after that date.
//!
//! The documentation tests read the manual page and assert the concrete numbers and codes of the
//! rule. The behaviour tests apply the retention window to calendar dates through the helpers
//! below, which read the window length from the manual page, and take the refusal code from the
//! shared code table.

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

/// A calendar date as (year, month, day).
type Date = (i32, u32, u32);

/// Months of retention that the rule line states ("until 12 months ... after the release").
fn retention_months(rule: &str) -> u32 {
    let after = rule
        .split("until ")
        .nth(1)
        .unwrap_or_else(|| panic!("rule must say 'until N months': {rule}"));
    after
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("rule must give the month count: {rule}"))
}

/// The last day of the window: the release date moved forward by `months`, clamped to the month end.
fn window_end(release: Date, months: u32) -> Date {
    let (year, month, day) = release;
    let index = year * 12 + (month as i32 - 1) + months as i32;
    let (end_year, end_month) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
    let leap = end_year % 4 == 0 && (end_year % 100 != 0 || end_year % 400 == 0);
    let month_length = match end_month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (end_year, end_month, day.min(month_length))
}

/// Whether the final minor of a line is still loadable on `today`, when the next major was
/// released on `next_major_release` (ABI-950, ABI-951). The end date itself is still inside.
fn final_minor_loadable(rule: &str, next_major_release: Date, today: Date) -> bool {
    today <= window_end(next_major_release, retention_months(rule))
}

/// The refusal code for a package of a line whose window has closed, or `None` while it loads.
fn retention_refusal(rule: &str, next_major_release: Date, today: Date) -> Option<&'static str> {
    (!final_minor_loadable(rule, next_major_release, today))
        .then_some(ilium_plugin_api::codes::package::API_UNSUPPORTED)
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
    let rule = rule_line(&manual_page(), "ABI-950");
    let released_2_0_0 = (2027, 3, 15);
    assert!(final_minor_loadable(&rule, released_2_0_0, (2028, 3, 14)));
    assert!(final_minor_loadable(&rule, released_2_0_0, (2028, 3, 15)));
    assert_eq!(retention_refusal(&rule, released_2_0_0, (2028, 3, 14)), None);
}

/// REQ: ABI-950
#[test]
fn abi_950_final_0x_minor_is_refused_with_package_api_unsupported_after_the_window() {
    let rule = rule_line(&manual_page(), "ABI-950");
    let released_2_0_0 = (2027, 3, 15);
    assert!(!final_minor_loadable(&rule, released_2_0_0, (2028, 3, 16)));
    assert_eq!(
        retention_refusal(&rule, released_2_0_0, (2028, 3, 16)),
        Some("package.api-unsupported")
    );
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
    let rule = rule_line(&manual_page(), "ABI-951");
    // The last day of the window is exactly 12 months on; a leap-day release clamps below.
    let released_major_two = (2027, 8, 31);
    assert!(final_minor_loadable(&rule, released_major_two, (2028, 8, 30)));
    assert!(!final_minor_loadable(&rule, released_major_two, (2028, 9, 1)));
    assert_eq!(window_end((2028, 2, 29), 12), (2029, 2, 28));
}

/// REQ: ABI-951
#[test]
fn abi_951_final_minor_of_major_two_loads_until_twelve_months_after_major_three() {
    let rule = rule_line(&manual_page(), "ABI-951");
    let released_major_three = (2030, 1, 10);
    assert!(final_minor_loadable(&rule, released_major_three, (2031, 1, 10)));
    assert!(!final_minor_loadable(&rule, released_major_three, (2031, 1, 11)));
    // The window of major 2 is independent of the window of major 1.
    assert!(!final_minor_loadable(&rule, (2027, 8, 31), (2031, 1, 10)));
}

/// REQ: ABI-951
#[test]
fn abi_951_packages_of_major_one_are_refused_after_twelve_months_after_major_two() {
    let rule = rule_line(&manual_page(), "ABI-951");
    let released_major_two = (2027, 8, 31);
    assert_eq!(
        retention_refusal(&rule, released_major_two, (2028, 9, 1)),
        Some("package.api-unsupported")
    );
    assert_eq!(retention_refusal(&rule, released_major_two, (2028, 8, 31)), None);
}
