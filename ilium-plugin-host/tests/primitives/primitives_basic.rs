//! Basic primitive (primitives-basic.md): a plugin-drawn pane body, title, status, focus, size,
//! activity and dirty state. Requirement ids PRI-030, PRI-031 and the six WIT functions of
//! `ilium:primitives/basic`.

use std::sync::{Arc, Mutex};

use ilium_plugin_api::generated::primitives::basic::HostBasic;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::{Intent, Submission};
use ilium_plugin_api::generated::types::layout::Area;
use ilium_plugin_api::generated::types::text::{Span, Style};
use ilium_plugin_api::generated::types::tree::{AgentActivity, PaneInfo, PrimitiveKind};
use ilium_plugin_api::generated::types::ui::{UiTree, ViewTarget};
use ilium_plugin_api::{ErrorCode, ProcessKind, Side, Tier};
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::testing::{FakeKernel, HostHarness, NativeGuest};

use crate::common::*;

const BASIC: &[&str] = &["panes.provide-kind", "tree.metadata"];

fn span(text: &str) -> Vec<Span> {
    vec![Span {
        text: text.to_owned(),
        style: Style::default(),
    }]
}

/// A guest that draws a fixed body and records the events it receives.
struct BodyGuest {
    seen: Arc<Mutex<Vec<String>>>,
    areas: Arc<Mutex<Vec<Area>>>,
}

impl NativeGuest for BodyGuest {
    fn on_events(&mut self, batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        let mut seen = self.seen.lock().expect("seen lock");
        seen.extend(batch.iter().map(|event| event.case_name().to_owned()));
        Ok(Vec::new())
    }

    fn render(&mut self, _view: ViewTarget, area: Area) -> Result<UiTree, Trap> {
        self.areas.lock().expect("areas lock").push(area);
        Ok(UiTree {
            version: 1,
            nodes: Vec::new(),
            root: 0,
        })
    }
}

struct BodyRig {
    host: ilium_plugin_host::testing::HarnessHandle,
    seen: Arc<Mutex<Vec<String>>>,
    areas: Arc<Mutex<Vec<Area>>>,
}

fn body_rig() -> BodyRig {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
    kernel.set_tree(standard_tree());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let areas = Arc::new(Mutex::new(Vec::new()));
    let host = HostHarness::new()
        .tier(Tier::T3)
        .side(Side::Client)
        .grants(BASIC)
        .kernel(kernel)
        .with_native_guest(Box::new(BodyGuest {
            seen: seen.clone(),
            areas: areas.clone(),
        }))
        .build()
        .expect("the harness builds a host for a native guest");
    BodyRig { host, seen, areas }
}

// ---------------------------------------------------------------- PRI-030

/// REQ: PRI-030, WIT:ilium:exports/views.render
#[test]
fn pri_030_basic_pane_body_comes_from_views_render() {
    let rig = body_rig();
    let tree = rig
        .host
        .render(
            ViewTarget::Pane(BASIC_PANE),
            Area {
                columns: 40,
                rows: 10,
            },
        )
        .expect("rendered body");
    let text = serde_json::to_string_pretty(&tree).expect("tree serializes") + "\n";
    assert_golden("basic_pane_body.json", &text);
}

/// REQ: PRI-030
#[test]
fn pri_030_render_receives_the_pane_area() {
    let rig = body_rig();
    rig.host
        .render(
            ViewTarget::Pane(BASIC_PANE),
            Area {
                columns: 40,
                rows: 10,
            },
        )
        .expect("rendered body");
    assert_eq!(
        rig.areas.lock().expect("areas lock").as_slice(),
        &[Area {
            columns: 40,
            rows: 10
        }]
    );
}

/// REQ: PRI-030
#[test]
fn pri_030_zero_size_area_is_not_an_error() {
    let rig = body_rig();
    assert!(rig
        .host
        .render(
            ViewTarget::Pane(BASIC_PANE),
            Area {
                columns: 0,
                rows: 0
            }
        )
        .is_ok());
}

/// REQ: PRI-030
#[test]
fn pri_030_basic_pane_side_receives_on_events_like_any_plugin_side() {
    let rig = body_rig();
    rig.host.deliver(vec![Event::PaneCreated(PaneInfo {
        node: BASIC_PANE,
        content_kind: "example.scratchpad.notes".to_owned(),
        primitive: PrimitiveKind::Basic,
        columns: 40,
        rows: 10,
        focused: false,
        title: "scratch".to_owned(),
        is_restored: false,
    })]);
    rig.host.pump(8);
    assert!(rig
        .seen
        .lock()
        .expect("seen lock")
        .contains(&"pane-created".to_owned()));
}

/// REQ: PRI-030
#[test]
fn pri_030_pane_not_ready_never_applies_to_a_basic_pane() {
    let rig = rig(BASIC);
    assert!(rig
        .host
        .basic()
        .set_title(BASIC_PANE, "ready at once".into())
        .is_ok());
}

// ---------------------------------------------------------------- PRI-031 and WIT

/// REQ: PRI-031, WIT:ilium:primitives/basic.set-dirty
#[test]
fn pri_031_set_dirty_true_marks_the_pane() {
    let rig = rig(BASIC);
    rig.host
        .basic()
        .set_dirty(BASIC_PANE, true)
        .expect("marked");
    assert!(rig.kernel.pane_is_dirty(BASIC_PANE));
}

/// REQ: PRI-031
#[test]
fn pri_031_set_dirty_false_clears_the_mark() {
    let rig = rig(BASIC);
    rig.host
        .basic()
        .set_dirty(BASIC_PANE, true)
        .expect("marked");
    rig.host
        .basic()
        .set_dirty(BASIC_PANE, false)
        .expect("cleared");
    assert!(!rig.kernel.pane_is_dirty(BASIC_PANE));
}

/// REQ: PRI-031
#[test]
fn pri_031_closing_a_dirty_pane_asks_before_discarding() {
    let mut rig = rig(BASIC);
    rig.host
        .basic()
        .set_dirty(BASIC_PANE, true)
        .expect("marked");
    let _ = rig.submit(Intent::CloseNode(BASIC_PANE));
    assert!(
        rig.kernel.pending_confirmation(BASIC_PANE),
        "a confirmation must be pending"
    );
    assert!(
        rig.kernel.node_exists(BASIC_PANE),
        "the pane stays until the user confirms"
    );
}

/// REQ: PRI-031
#[test]
fn pri_031_closing_a_clean_pane_asks_nothing() {
    let mut rig = rig(BASIC);
    let _ = rig.submit(Intent::CloseNode(BASIC_PANE));
    assert!(!rig.kernel.pending_confirmation(BASIC_PANE));
}

/// REQ: PRI-031, WIT:ilium:primitives/basic.set-dirty
#[test]
fn pri_031_set_dirty_on_a_node_of_another_kind_not_permitted() {
    let rig = rig(BASIC);
    assert_eq!(
        error_of(rig.host.basic().set_dirty(PANE, true)),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/basic.set-dirty
#[test]
fn wit_basic_set_dirty_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert_eq!(
        error_of(rig.host.basic().set_dirty(BASIC_PANE, true)),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/basic.set-title
#[test]
fn wit_basic_set_title_changes_the_pane_title() {
    let rig = rig(BASIC);
    rig.host
        .basic()
        .set_title(BASIC_PANE, "Notes (3)".into())
        .expect("title");
    assert_eq!(
        rig.kernel.pane_title(BASIC_PANE).as_deref(),
        Some("Notes (3)")
    );
}

/// REQ: WIT:ilium:primitives/basic.set-title
#[test]
fn wit_basic_set_title_256_chars_accepted_257_invalid_argument() {
    let rig = rig(BASIC);
    assert!(rig
        .host
        .basic()
        .set_title(BASIC_PANE, "t".repeat(256))
        .is_ok());
    assert_eq!(
        error_of(rig.host.basic().set_title(BASIC_PANE, "t".repeat(257))),
        Some(ErrorCode::InvalidArgument)
    );
    assert_eq!(
        rig.kernel.pane_title(BASIC_PANE).map(|t| t.len()),
        Some(256)
    );
}

/// REQ: WIT:ilium:primitives/basic.set-title
#[test]
fn wit_basic_set_title_over_a_manual_title_conflict() {
    let rig = rig(BASIC);
    rig.kernel.set_manual_title(BASIC_PANE, "mine");
    assert_eq!(
        error_of(rig.host.basic().set_title(BASIC_PANE, "plugin".into())),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(rig.kernel.pane_title(BASIC_PANE).as_deref(), Some("mine"));
}

/// REQ: WIT:ilium:primitives/basic.set-title
#[test]
fn wit_basic_set_title_on_a_foreign_node_not_permitted_and_logged_as_warning() {
    let rig = rig(BASIC);
    assert_eq!(
        error_of(rig.host.basic().set_title(PANE, "x".into())),
        Some(ErrorCode::NotPermitted)
    );
    let warned = rig.host.logs().iter().any(|line| {
        line.level == "warn"
            && line
                .message
                .contains("refused basic.set-title: not-permitted")
    });
    assert!(warned, "{}", log_text(&rig.host));
}

/// REQ: WIT:ilium:primitives/basic.set-title
#[test]
fn wit_basic_set_title_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert_eq!(
        error_of(rig.host.basic().set_title(BASIC_PANE, "x".into())),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/basic.set-status
#[test]
fn wit_basic_set_status_changes_the_footer() {
    let rig = rig(BASIC);
    rig.host
        .basic()
        .set_status(BASIC_PANE, span("Saved"))
        .expect("status");
    assert_eq!(
        rig.kernel.pane_status_text(BASIC_PANE).as_deref(),
        Some("Saved")
    );
}

/// REQ: WIT:ilium:primitives/basic.set-status
#[test]
fn wit_basic_set_status_256_chars_accepted_257_invalid_argument() {
    let rig = rig(BASIC);
    assert!(rig
        .host
        .basic()
        .set_status(BASIC_PANE, span(&"s".repeat(256)))
        .is_ok());
    assert_eq!(
        error_of(
            rig.host
                .basic()
                .set_status(BASIC_PANE, span(&"s".repeat(257)))
        ),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/basic.set-status
#[test]
fn wit_basic_set_status_with_a_line_break_invalid_argument() {
    let rig = rig(BASIC);
    assert_eq!(
        error_of(rig.host.basic().set_status(BASIC_PANE, span("a\nb"))),
        Some(ErrorCode::InvalidArgument)
    );
    assert_ne!(
        rig.kernel.pane_status_text(BASIC_PANE).as_deref(),
        Some("a\nb")
    );
}

/// REQ: WIT:ilium:primitives/basic.set-status
#[test]
fn wit_basic_set_status_on_a_foreign_node_not_permitted() {
    let rig = rig(BASIC);
    assert_eq!(
        error_of(rig.host.basic().set_status(PANE, span("x"))),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/basic.request-focus
#[test]
fn wit_basic_request_focus_moves_focus_to_the_pane() {
    let rig = rig(BASIC);
    rig.host.basic().request_focus(BASIC_PANE).expect("focus");
    assert_eq!(rig.kernel.focused_pane(), Some(BASIC_PANE));
}

/// REQ: WIT:ilium:primitives/basic.request-focus
#[test]
fn wit_basic_request_focus_on_a_foreign_node_not_permitted() {
    let rig = rig(BASIC);
    assert_eq!(
        error_of(rig.host.basic().request_focus(PANE)),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/basic.size
#[test]
fn wit_basic_size_returns_columns_and_rows() {
    let rig = rig(BASIC);
    rig.kernel.set_pane_size(BASIC_PANE, 40, 12);
    assert_eq!(rig.host.basic().size(BASIC_PANE), Some((40, 12)));
}

/// REQ: WIT:ilium:primitives/basic.size
#[test]
fn wit_basic_size_of_a_foreign_node_is_none_without_an_error() {
    let rig = rig(BASIC);
    assert_eq!(rig.host.basic().size(PANE), None);
    assert_eq!(rig.host.basic().size(MISSING_PANE), None);
}

/// REQ: WIT:ilium:primitives/basic.report-activity
#[test]
fn wit_basic_report_activity_working_shows_on_the_row() {
    let rig = rig(BASIC);
    rig.host
        .basic()
        .report_activity(BASIC_PANE, AgentActivity::Working)
        .expect("activity");
    assert_eq!(
        rig.kernel.pane_activity(BASIC_PANE),
        Some(AgentActivity::Working)
    );
}

/// REQ: WIT:ilium:primitives/basic.report-activity
#[test]
fn wit_basic_report_activity_on_a_foreign_node_not_permitted() {
    let rig = rig(BASIC);
    let result = rig
        .host
        .basic()
        .report_activity(PANE, AgentActivity::Working);
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}

/// REQ: WIT:ilium:primitives/basic.report-activity
#[test]
fn wit_basic_report_activity_without_grant_not_permitted() {
    let rig = rig(&[]);
    let result = rig
        .host
        .basic()
        .report_activity(BASIC_PANE, AgentActivity::Idle);
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}
