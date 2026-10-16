//! reference-test-harness.md "FakeSession" and "Core stubs": DEV-480 to DEV-489.
//!
//! Routing between installed plugins (DEV-480, 481, 482, 487 via a session, the plugin half of
//! DEV-486 and DEV-488) needs a way to bind a native plugin type to an installed package; that is
//! listed in API_GAPS.md. The tests here cover everything reachable without it.

use ilium_plugin_sdk_core::exports::HookCall;
use ilium_plugin_sdk_core::types::{ErrorCode, PluginId};
use ilium_plugin_testkit::core_stubs;
use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::session::FakeSession;

use crate::common::{assert_fails_with, with_clean_env};

/// Name, extension points, services: the "Core stubs" table of the harness page, row by row.
const STUB_TABLE: [(&str, &[&str], &[&str]); 30] = [
    ("ilium.core.tree", &["before-move", "before-close", "node-sorts"], &[]),
    ("ilium.core.layout", &["layout-presets", "before-split"], &[]),
    ("ilium.core.terminal", &["before-send-input", "open-link", "link-actions"], &[]),
    ("ilium.core.smart-copy", &["recognisers", "after-copy"], &[]),
    ("ilium.core.editor", &["before-save", "file-openers", "line-badges"], &[]),
    ("ilium.core.kanban", &["card-actions", "card-moved"], &[]),
    ("ilium.core.agent-detect", &["activity-rules", "classify"], &[]),
    ("ilium.core.agent-toolbar", &["toolbar-actions", "banner-segments"], &[]),
    ("ilium.core.fleet", &["selectors", "before-broadcast"], &[]),
    ("ilium.core.cost", &["price-sources", "quota-sources"], &["usage"]),
    ("ilium.core.titles", &["before-apply-title", "title-styles"], &[]),
    ("ilium.core.inference", &["before-request", "after-response", "provider-profiles"], &[]),
    ("ilium.core.notifications", &["before-notify", "channels"], &[]),
    ("ilium.core.voice", &["transcript", "voice-intents"], &[]),
    ("ilium.core.scheduler", &["schedule-sources", "before-send-scheduled"], &[]),
    ("ilium.core.text-triggers", &["rule-packs", "before-fire"], &[]),
    ("ilium.core.progress", &["footer-segments", "probe-kinds"], &[]),
    ("ilium.core.chatroom", &["before-post", "message-renderers"], &[]),
    ("ilium.core.worktrees", &["before-create", "post-create-steps", "before-remove"], &[]),
    ("ilium.core.sessions", &["snapshot-sections", "before-restore"], &[]),
    ("ilium.core.compaction", &["techniques", "before-compact"], &[]),
    ("ilium.core.settings", &["config-importers", "onboarding-steps"], &[]),
    ("ilium.core.keys-and-help", &["key-hints", "help-sources"], &[]),
    ("ilium.core.search", &["sources", "rank"], &[]),
    ("ilium.core.ambient", &["palettes", "style-presets"], &[]),
    ("ilium.core.scenes", &["scene-packs", "scene-data-sources"], &[]),
    ("ilium.core.recap", &["fact-sources", "before-recap"], &["recap"]),
    ("ilium.core.session-convert", &["formats", "after-convert"], &["convert"]),
    ("ilium.core.agent-setup", &["instruction-blocks", "targets", "before-install"], &["install"]),
    ("ilium.core.diagnostics", &["log-sinks", "report-sections"], &["report"]),
];

// ---- DEV-483, DEV-489 ------------------------------------------------------------------------

/// REQ: DEV-483
#[test]
fn dev_483_the_stub_table_has_thirty_rows_in_manual_order() {
    let table = core_stubs::table();
    assert_eq!(table.len(), 30);
    let names: Vec<&str> = table.iter().map(|stub| stub.name.as_str()).collect();
    let expected: Vec<&str> = STUB_TABLE.iter().map(|row| row.0).collect();
    assert_eq!(names, expected);
}

/// REQ: DEV-483
#[test]
fn dev_483_every_stub_exposes_exactly_the_documented_points_and_services() {
    let table = core_stubs::table();
    for (stub, row) in table.iter().zip(STUB_TABLE.iter()) {
        let points: Vec<&str> = stub.extension_points.iter().map(String::as_str).collect();
        let services: Vec<&str> = stub.services.iter().map(String::as_str).collect();
        assert_eq!(points, row.1, "extension points of {}", row.0);
        assert_eq!(services, row.2, "services of {}", row.0);
    }
}

/// REQ: DEV-483
#[test]
fn dev_483_find_returns_the_row_for_a_known_name_and_none_otherwise() {
    let stub = core_stubs::find("ilium.core.cost").expect("known stub");
    assert_eq!(stub.services, vec!["usage".to_string()]);
    assert!(core_stubs::find("ilium.core.nope").is_none());
    assert!(core_stubs::find("").is_none());
}

/// REQ: DEV-483
#[test]
fn dev_483_install_core_of_an_unknown_name_fails_with_core_unknown() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        assert_fails_with(codes::CORE_UNKNOWN, || {
            session.install_core("ilium.core.nope");
        });
    });
}

/// REQ: DEV-483
#[test]
fn dev_483_install_core_of_a_known_name_succeeds_for_every_row() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        for row in STUB_TABLE {
            session.install_core(row.0);
        }
        session.start();
    });
}

/// REQ: DEV-489
#[test]
fn dev_489_a_hook_on_a_stub_yields_pass() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.tree").start();
        let call = HookCall { call: 1, point: "before-move".to_string(), payload_json: "{}".to_string() };
        let reply = session.run_hook("ilium.core.tree", call).expect("stub answers");
        assert_eq!(format!("{reply:?}"), "Pass");
    });
}

/// REQ: DEV-489
#[test]
fn dev_489_a_service_on_a_stub_yields_ok_true_for_every_method() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.cost").start();
        for request in [r#"{"method":"a"}"#, r#"{"method":"b"}"#, "{}"] {
            let reply = session
                .call_service("ilium.core.cost", "usage", "*", request)
                .expect("provided service");
            assert_eq!(format!("{reply:?}"), r#"Done("{\"ok\":true}")"#);
        }
    });
}

// ---- DEV-484 ---------------------------------------------------------------------------------

/// REQ: DEV-484
#[test]
fn dev_484_pump_on_an_empty_session_returns_ok() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.start();
        session.pump().expect("nothing queued");
    });
}

// ---- DEV-486 ---------------------------------------------------------------------------------

/// REQ: DEV-486
#[test]
fn dev_486_server_of_an_uninstalled_id_fails_with_plugin_unknown() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.start();
        assert_fails_with(codes::PLUGIN_UNKNOWN, || {
            session.server("example.absent");
        });
    });
}

/// REQ: DEV-486
#[test]
fn dev_486_client_of_an_uninstalled_id_fails_with_plugin_unknown() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.start();
        assert_fails_with(codes::PLUGIN_UNKNOWN, || {
            session.client(PluginId("example.absent".to_string()));
        });
    });
}

/// REQ: DEV-486
#[test]
fn dev_486_a_stub_has_no_client_side_so_asking_for_it_fails_with_plugin_unknown() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.cost").start();
        assert_fails_with(codes::PLUGIN_UNKNOWN, || {
            session.client("ilium.core.cost");
        });
    });
}

// ---- DEV-487, DEV-488 ------------------------------------------------------------------------

/// REQ: DEV-488
#[test]
fn dev_488_a_service_nobody_provides_returns_not_found() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.cost").start();
        let error = session
            .call_service("ilium.core.cost", "example.missing", "*", "{}")
            .expect_err("no provider");
        assert_eq!(error.code, ErrorCode::NotFound);
    });
}

/// REQ: DEV-488
#[test]
fn dev_488_a_version_range_the_provider_does_not_satisfy_returns_not_found() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.install_core("ilium.core.cost").start();
        let error = session
            .call_service("ilium.core.cost", "usage", ">=99.0.0", "{}")
            .expect_err("range excludes the provider");
        assert_eq!(error.code, ErrorCode::NotFound);
    });
}

/// REQ: DEV-487
#[test]
fn dev_487_run_hook_for_an_uninstalled_plugin_fails_with_plugin_unknown() {
    with_clean_env(|| {
        let mut session = FakeSession::new();
        session.start();
        let call = HookCall { call: 1, point: "x".to_string(), payload_json: "{}".to_string() };
        let error = session.run_hook("example.absent", call).expect_err("unknown plugin");
        assert_eq!(error.code, codes::PLUGIN_UNKNOWN);
    });
}
