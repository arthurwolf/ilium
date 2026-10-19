//! reference-host-rendering.md (HST-495): popup ids.

use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::testing::views::ViewHarness;

fn harness_with_popups(count: usize) -> ViewHarness {
    let mut harness = ViewHarness::new();
    harness.open_view(
        "main",
        &(0..count)
            .map(|index| format!("pop{index}"))
            .collect::<Vec<_>>(),
    );
    harness
}

/// REQ: HST-495
#[test]
fn hst_495_a_unique_popup_id_opens() {
    let mut harness = harness_with_popups(1);
    assert!(harness.open_popup("pop0").is_ok());
    assert!(harness.is_open("pop0"));
}

/// REQ: HST-495
#[test]
fn hst_495_a_missing_popup_id_returns_not_found() {
    let mut harness = harness_with_popups(1);
    assert_eq!(
        harness.open_popup("nope").expect_err("missing").code,
        ErrorCode::NotFound
    );
}

/// REQ: HST-495
#[test]
fn hst_495_two_matching_popup_ids_return_invalid_argument() {
    let mut harness = ViewHarness::new();
    harness.open_view("one", &["dup".to_owned()]);
    harness.open_view("two", &["dup".to_owned()]);
    assert_eq!(
        harness.open_popup("dup").expect_err("ambiguous").code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: HST-495
#[test]
fn hst_495_the_fourth_open_popup_is_accepted() {
    let mut harness = harness_with_popups(5);
    for index in 0..4 {
        assert!(
            harness.open_popup(&format!("pop{index}")).is_ok(),
            "popup {index}"
        );
        assert!(
            harness.is_open(&format!("pop{index}")),
            "popup {index} open"
        );
    }
}

/// REQ: HST-495
#[test]
fn hst_495_the_fifth_open_popup_returns_quota_exceeded() {
    let mut harness = harness_with_popups(6);
    for index in 0..4 {
        harness
            .open_popup(&format!("pop{index}"))
            .expect("within limit");
    }
    assert_eq!(
        harness.open_popup("pop4").expect_err("fifth").code,
        ErrorCode::QuotaExceeded
    );
}

/// REQ: HST-495
#[test]
fn hst_495_close_popup_on_an_unopened_id_is_ok() {
    let mut harness = harness_with_popups(1);
    assert!(harness.close_popup("pop0").is_ok());
    assert!(harness.close_popup("never-existed").is_ok());
    assert!(!harness.is_open("pop0"));
    assert!(!harness.is_open("never-existed"));
    assert!(harness.closed_events().is_empty());
}

/// REQ: HST-495
#[test]
fn hst_495_a_popup_missing_from_the_next_tree_closes_and_sends_closed() {
    let mut harness = harness_with_popups(2);
    harness.open_popup("pop0").expect("open");
    harness.commit_tree("main", &["pop1".to_owned()]);
    assert!(!harness.is_open("pop0"));
    assert_eq!(harness.closed_events(), vec!["pop0".to_owned()]);
}
