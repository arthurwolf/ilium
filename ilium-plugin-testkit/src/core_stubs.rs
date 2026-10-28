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
    STUB_ROWS
        .iter()
        .map(|(name, points, services)| CoreStub {
            name: (*name).to_string(),
            extension_points: points.iter().map(|point| (*point).to_string()).collect(),
            services: services.iter().map(|service| (*service).to_string()).collect(),
        })
        .collect()
}

/// One row: name, extension points, services.
type StubRow = (&'static str, &'static [&'static str], &'static [&'static str]);

/// The "Core stubs" table of the harness page, in manual order.
const STUB_ROWS: &[StubRow] = &[
    ("ilium.core.tree", &["before-move", "before-close", "node-sorts"], &["bookmarks"]),
    ("ilium.core.layout", &["layout-presets", "before-split"], &["presets"]),
    ("ilium.core.terminal", &["before-send-input", "open-link", "link-actions"], &["transfer"]),
    ("ilium.core.smart-copy", &["recognisers", "after-copy"], &[]),
    ("ilium.core.editor", &["before-save", "file-openers", "line-badges"], &["buffers"]),
    ("ilium.core.kanban", &["card-actions", "card-moved"], &["boards"]),
    ("ilium.core.agent-detect", &["activity-rules", "classify"], &["state"]),
    ("ilium.core.agent-toolbar", &["toolbar-actions", "banner-segments"], &["last-prompt"]),
    ("ilium.core.fleet", &["selectors", "before-broadcast"], &["broadcast", "panes"]),
    ("ilium.core.cost", &["price-sources", "quota-sources"], &["usage"]),
    ("ilium.core.titles", &["before-apply-title", "title-styles"], &["suggest"]),
    ("ilium.core.inference", &["before-request", "after-response", "provider-profiles"], &["complete"]),
    ("ilium.core.notifications", &["before-notify", "channels"], &["notify"]),
    ("ilium.core.voice", &["transcript", "voice-intents"], &["say"]),
    ("ilium.core.scheduler", &["schedule-sources", "before-send-scheduled"], &["queue"]),
    ("ilium.core.text-triggers", &["rule-packs", "before-fire"], &["test-rule"]),
    ("ilium.core.progress", &["footer-segments", "probe-kinds"], &["report"]),
    ("ilium.core.chatroom", &["before-post", "message-renderers"], &["post", "tail"]),
    ("ilium.core.worktrees", &["before-create", "post-create-steps", "before-remove"], &["create"]),
    ("ilium.core.sessions", &["snapshot-sections", "before-restore"], &["snapshots"]),
    ("ilium.core.compaction", &["techniques", "before-compact"], &["compact"]),
    ("ilium.core.settings", &["config-importers", "onboarding-steps"], &["get", "set"]),
    ("ilium.core.keys-and-help", &["key-hints", "help-sources"], &["bindings"]),
    ("ilium.core.search", &["sources", "rank"], &["query"]),
    ("ilium.core.ambient", &["palettes", "style-presets"], &["look"]),
    ("ilium.core.scenes", &["scene-packs", "scene-data-sources"], &["catalog"]),
    ("ilium.core.recap", &["fact-sources", "before-recap"], &["recap"]),
    ("ilium.core.session-convert", &["formats", "after-convert"], &["convert"]),
    ("ilium.core.agent-setup", &["instruction-blocks", "targets", "before-install"], &["install"]),
    ("ilium.core.diagnostics", &["log-sinks", "report-sections"], &["report"]),
];

/// Looks up one stub by name; `None` for an unknown name (then `install_core` fails with `harness.core-unknown`).
pub fn find(name: &str) -> Option<CoreStub> {
    table().into_iter().find(|stub| stub.name == name)
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
        assert_eq!(
            stubs.last().expect("last row").name,
            "ilium.core.diagnostics"
        );
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_stub_names_are_unique_core_plugin_names() {
        let stubs = table();
        let names: HashSet<&str> = stubs.iter().map(|stub| stub.name.as_str()).collect();
        assert_eq!(names.len(), stubs.len());
        assert!(stubs
            .iter()
            .all(|stub| stub.name.starts_with("ilium.core.")));
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_tree_stub_exposes_its_points_and_bookmarks_service() {
        let stub = find("ilium.core.tree").expect("tree stub is in the table");
        assert_eq!(
            stub.extension_points,
            vec!["before-move", "before-close", "node-sorts"]
        );
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
        assert_eq!(
            stub.extension_points,
            vec!["price-sources", "quota-sources"]
        );
        assert_eq!(stub.services, vec!["usage"]);
    }

    /// REQ: DEV-483
    #[test]
    fn dev_483_find_returns_the_same_row_as_table_for_every_stub() {
        for stub in table() {
            assert!(!stub.extension_points.is_empty(), "points of {}", stub.name);
            assert_eq!(
                find(&stub.name),
                Some(stub.clone()),
                "find of {}",
                stub.name
            );
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
