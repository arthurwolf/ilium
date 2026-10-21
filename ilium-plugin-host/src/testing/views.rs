//! A harness over the view and popup lifecycle of plugin UI trees (reference-host-rendering.md).

use ilium_plugin_api::PluginError;

/// Opens views, their popups and commits trees, recording the close events the host emits.
#[derive(Debug, Default)]
pub struct ViewHarness;

impl ViewHarness {
    /// An empty harness.
    pub fn new() -> ViewHarness {
        unimplemented!("host::testing::views::ViewHarness::new")
    }

    /// Opens a view declaring `popups`.
    pub fn open_view(&mut self, _view: &str, _popups: &[String]) {
        unimplemented!("host::testing::views::ViewHarness::open_view")
    }

    /// Opens a declared popup; `not-found` for an unknown id.
    pub fn open_popup(&mut self, _popup: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::views::ViewHarness::open_popup")
    }

    /// Closes an open popup.
    pub fn close_popup(&mut self, _popup: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::views::ViewHarness::close_popup")
    }

    /// Commits a new tree of a view declaring `popups`; popups no longer declared are closed.
    pub fn commit_tree(&mut self, _view: &str, _popups: &[String]) {
        unimplemented!("host::testing::views::ViewHarness::commit_tree")
    }

    /// Whether a popup is open.
    pub fn is_open(&self, _popup: &str) -> bool {
        unimplemented!("host::testing::views::ViewHarness::is_open")
    }

    /// The popup-closed events emitted so far, in order.
    pub fn closed_events(&self) -> Vec<String> {
        unimplemented!("host::testing::views::ViewHarness::closed_events")
    }
}

#[cfg(test)]
mod tests {
    use super::ViewHarness;
    use ilium_plugin_api::ErrorCode;

    /// A harness with one view `main` that declares `count` popups named `pop0`, `pop1`, ...
    fn harness_with_popups(count: usize) -> ViewHarness {
        let mut harness = ViewHarness::new();
        let popups: Vec<String> = (0..count).map(|index| format!("pop{index}")).collect();
        harness.open_view("main", &popups);
        harness
    }

    #[test]
    fn a_new_harness_has_no_open_popups_and_no_events() {
        let harness = ViewHarness::new();
        assert!(!harness.is_open("pop0"));
        assert!(harness.closed_events().is_empty());
    }

    #[test]
    fn a_default_harness_matches_a_new_one() {
        let harness = ViewHarness::default();
        assert!(!harness.is_open("pop0"));
        assert!(harness.closed_events().is_empty());
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_on_an_empty_harness_returns_not_found() {
        let mut harness = ViewHarness::new();
        assert_eq!(
            harness.open_popup("pop0").expect_err("no view").code,
            ErrorCode::NotFound
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_on_a_view_declaring_no_popups_returns_not_found() {
        let mut harness = harness_with_popups(0);
        assert_eq!(
            harness.open_popup("pop0").expect_err("none declared").code,
            ErrorCode::NotFound
        );
        assert!(!harness.is_open("pop0"));
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_with_an_empty_id_returns_not_found() {
        let mut harness = harness_with_popups(1);
        assert_eq!(
            harness.open_popup("").expect_err("empty id").code,
            ErrorCode::NotFound
        );
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_of_a_declared_id_opens_it() {
        let mut harness = harness_with_popups(1);
        assert!(harness.open_popup("pop0").is_ok());
        assert!(harness.is_open("pop0"));
    }

    /// REQ: HST-495
    #[test]
    fn open_popup_with_a_popup_id_that_matches_a_second_view_returns_invalid_argument() {
        let mut harness = ViewHarness::new();
        harness.open_view("one", &["dup".to_owned()]);
        harness.open_view("two", &["dup".to_owned()]);
        assert_eq!(
            harness.open_popup("dup").expect_err("two matches").code,
            ErrorCode::InvalidArgument
        );
        assert!(!harness.is_open("dup"));
    }

    /// REQ: HST-495
    #[test]
    fn the_fourth_open_popup_in_one_view_is_accepted() {
        let mut harness = harness_with_popups(5);
        for index in 0..4 {
            let popup = format!("pop{index}");
            assert!(harness.open_popup(&popup).is_ok(), "{popup} opens");
            assert!(harness.is_open(&popup), "{popup} is open");
        }
    }

    /// REQ: HST-495
    #[test]
    fn the_fifth_open_popup_in_one_view_returns_quota_exceeded() {
        let mut harness = harness_with_popups(5);
        for index in 0..4 {
            harness
                .open_popup(&format!("pop{index}"))
                .expect("within the limit of four");
        }
        assert_eq!(
            harness.open_popup("pop4").expect_err("fifth").code,
            ErrorCode::QuotaExceeded
        );
        assert!(!harness.is_open("pop4"));
    }

    /// REQ: HST-495
    #[test]
    fn the_popup_limit_is_per_view_so_four_in_each_of_two_views_are_accepted() {
        let mut harness = ViewHarness::new();
        let first_view: Vec<String> = (0..4).map(|index| format!("a{index}")).collect();
        let second_view: Vec<String> = (0..4).map(|index| format!("b{index}")).collect();
        harness.open_view("first", &first_view);
        harness.open_view("second", &second_view);
        for popup in first_view.iter().chain(second_view.iter()) {
            assert!(harness.open_popup(popup).is_ok(), "{popup} opens");
        }
        assert!(harness.is_open("a3"));
        assert!(harness.is_open("b3"));
    }

    /// REQ: HST-495
    #[test]
    fn opening_a_fifth_popup_succeeds_once_one_of_the_four_is_closed() {
        let mut harness = harness_with_popups(5);
        for index in 0..4 {
            harness
                .open_popup(&format!("pop{index}"))
                .expect("within the limit of four");
        }
        harness.close_popup("pop0").expect("close an open popup");
        assert!(harness.open_popup("pop4").is_ok());
        assert!(harness.is_open("pop4"));
    }

    /// REQ: HST-495
    #[test]
    fn close_popup_on_an_open_popup_returns_ok_and_hides_it() {
        let mut harness = harness_with_popups(1);
        harness.open_popup("pop0").expect("open");
        assert!(harness.close_popup("pop0").is_ok());
        assert!(!harness.is_open("pop0"));
    }

    /// REQ: HST-495
    #[test]
    fn close_popup_on_an_id_that_is_not_open_is_a_no_op_that_returns_ok() {
        let mut harness = harness_with_popups(1);
        assert!(harness.close_popup("pop0").is_ok());
        assert!(harness.close_popup("never-declared").is_ok());
        assert!(!harness.is_open("pop0"));
        assert!(harness.closed_events().is_empty());
    }

    /// REQ: HST-495
    #[test]
    fn commit_tree_keeping_an_open_popup_leaves_it_open_without_a_closed_event() {
        let mut harness = harness_with_popups(2);
        harness.open_popup("pop0").expect("open");
        harness.commit_tree("main", &["pop0".to_owned(), "pop1".to_owned()]);
        assert!(harness.is_open("pop0"));
        assert!(harness.closed_events().is_empty());
    }

    /// REQ: HST-495
    #[test]
    fn commit_tree_dropping_an_open_popup_closes_it_and_sends_closed() {
        let mut harness = harness_with_popups(2);
        harness.open_popup("pop0").expect("open");
        harness.commit_tree("main", &["pop1".to_owned()]);
        assert!(!harness.is_open("pop0"));
        assert_eq!(harness.closed_events(), vec!["pop0".to_owned()]);
    }

    /// REQ: HST-495
    #[test]
    fn commit_tree_with_an_empty_popup_list_closes_every_open_popup_of_the_view() {
        let mut harness = harness_with_popups(3);
        harness.open_popup("pop0").expect("open");
        harness.open_popup("pop2").expect("open");
        harness.commit_tree("main", &[]);
        assert!(!harness.is_open("pop0"));
        assert!(!harness.is_open("pop2"));
        let mut closed = harness.closed_events();
        closed.sort();
        assert_eq!(closed, vec!["pop0".to_owned(), "pop2".to_owned()]);
    }

    /// REQ: HST-495
    #[test]
    fn commit_tree_does_not_send_closed_for_a_popup_that_was_never_open() {
        let mut harness = harness_with_popups(2);
        harness.commit_tree("main", &[]);
        assert!(harness.closed_events().is_empty());
    }

    #[test]
    fn commit_tree_on_an_empty_harness_emits_no_events() {
        let mut harness = ViewHarness::new();
        harness.commit_tree("main", &[]);
        assert!(harness.closed_events().is_empty());
    }
}
