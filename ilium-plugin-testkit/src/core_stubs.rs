//! The 30 bundled core plugin stubs of `FakeSession::install_core` (reference-test-harness.md "Core stubs").

/// One core stub: its name, the extension points it exposes and the services it provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreStub {
    /// Core plugin name, for example "ilium.agent-tools".
    pub name: String,
    /// Extension point names the stub exposes.
    pub extension_points: Vec<String>,
    /// Service names the stub provides.
    pub services: Vec<String>,
}

/// The table of core stubs, parsed from the "Core stubs" table of the harness page.
pub fn table() -> Vec<CoreStub> {
    unimplemented!("harness.core-stubs-table")
}

/// Looks up one stub by name; `None` for an unknown name (then `install_core` fails with `harness.core-unknown`).
pub fn find(_name: &str) -> Option<CoreStub> {
    unimplemented!("harness.core-stubs-table")
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use std::collections::HashSet;

    /// REQ: DEV-483
    #[test]
    fn dev_483_table_has_thirty_stubs_in_manual_order() {
        let stubs = table();
        assert_eq!(stubs.len(), 30);
        assert_eq!(stubs.first().expect("first row").name, "ilium.core.tree");
        assert_eq!(stubs.last().expect("last row").name, "ilium.core.diagnostics");
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_stub_names_are_unique_core_plugin_names() {
        let stubs = table();
        let names: HashSet<&str> = stubs.iter().map(|stub| stub.name.as_str()).collect();
        assert_eq!(names.len(), stubs.len());
        assert!(stubs.iter().all(|stub| stub.name.starts_with("ilium.core.")));
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_tree_stub_exposes_its_points_and_bookmarks_service() {
        let stub = find("ilium.core.tree").expect("tree stub is in the table");
        assert_eq!(stub.extension_points, vec!["before-move", "before-close", "node-sorts"]);
        assert_eq!(stub.services, vec!["bookmarks"]);
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_smart_copy_stub_has_points_and_no_services() {
        let stub = find("ilium.core.smart-copy").expect("smart-copy stub is in the table");
        assert_eq!(stub.extension_points, vec!["recognisers", "after-copy"]);
        assert!(stub.services.is_empty());
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_cost_stub_exposes_usage_service() {
        let stub = find("ilium.core.cost").expect("cost stub is in the table");
        assert_eq!(stub.extension_points, vec!["price-sources", "quota-sources"]);
        assert_eq!(stub.services, vec!["usage"]);
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_find_returns_the_same_row_as_table_for_every_stub() {
        for stub in table() {
            assert!(!stub.extension_points.is_empty(), "points of {}", stub.name);
            assert_eq!(find(&stub.name), Some(stub.clone()), "find of {}", stub.name);
        }
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_find_of_unknown_or_empty_name_returns_none() {
        assert!(find("ilium.core.nope").is_none());
        assert!(find("").is_none());
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_find_does_not_match_a_prefix_of_a_core_name() {
        assert!(find("ilium.core.tre").is_none());
        assert!(find("ilium.core").is_none());
    }
}
