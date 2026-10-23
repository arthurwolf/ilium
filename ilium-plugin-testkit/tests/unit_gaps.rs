//! Unit-gap tests for `ilium-plugin-testkit`, written against the behaviour documented in
//! `docs/plugins-manual/pages/reference-test-harness.md`. Many fakes are still
//! `unimplemented!()` stubs, so most of these tests are expected to fail (red) until the stubs
//! get real bodies. They must not be weakened to match the stubs.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ilium_plugin_sdk_core::anim::FrameInput;
use ilium_plugin_sdk_core::exports::{HookCall, HookReply, MenuContext, ServiceReply};
use ilium_plugin_sdk_core::host::{Bucket, HttpResponse, ProcessExit};
use ilium_plugin_sdk_core::plugin::NativeSurface;
use ilium_plugin_sdk_core::types::ui::ViewTarget;
use ilium_plugin_sdk_core::types::{
    Activation, AgentActivity, DeactivationReason, ErrorCode, Event, PluginError,
    PluginId, Submission,
};
use ilium_plugin_sdk_core::{NativeEntry, NativeRenderer};
use ilium_plugin_testkit::fakes::fs::parse_shorthand;
use ilium_plugin_testkit::fixture::{Fixture, FixtureStatus};
use ilium_plugin_testkit::host::{FakeHost, Mode};
use ilium_plugin_testkit::{codes, FakeScene, FakeSession, HarnessError};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde_json::json;

/// Serialises every test that changes process environment variables.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Plugin that accepts every activation and produces no submissions.
macro_rules! idle_plugin {
    ($name:ident) => {
        #[derive(Default)]
        struct $name;

        impl NativeEntry for $name {
            const SIDE: &'static str = "server";
            const EXPORTS: &'static [&'static str] = &[];

            fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
                Ok(())
            }

            fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
                None
            }

            fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
                Vec::new()
            }
        }
    };
}

idle_plugin!(IdlePlugin);
idle_plugin!(OtherPlugin);

/// Plugin whose activation always fails, used for conformance reporting.
#[derive(Default)]
struct FailingPlugin;

impl NativeEntry for FailingPlugin {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &[];

    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Err(PluginError::new(
            ErrorCode::Internal,
            "activation refused on purpose",
        ))
    }

    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }

    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// Scene renderer that paints every cell with a dot.
#[derive(Default)]
struct BlankRenderer;

impl NativeRenderer for BlankRenderer {
    fn new(_spec_json: &str) -> Self {
        Self
    }

    fn render(&mut self, _frame: &FrameInput, surface: &mut NativeSurface) {
        for cell in surface.cells.iter_mut() {
            cell.0 = '.';
        }
    }

    fn set_palette(&mut self, _palette_rgb: Vec<u32>) {}

    fn frames_per_second(&self) -> u32 {
        30
    }
}

/// Scene renderer that writes its palette length into the first cell.
#[derive(Default)]
struct PaletteRenderer {
    palette_length: usize,
}

impl NativeRenderer for PaletteRenderer {
    fn new(_spec_json: &str) -> Self {
        Self::default()
    }

    fn render(&mut self, _frame: &FrameInput, surface: &mut NativeSurface) {
        for cell in surface.cells.iter_mut() {
            cell.0 = '.';
        }
        if let Some(first_cell) = surface.cells.first_mut() {
            first_cell.0 = char::from_digit(self.palette_length as u32, 10).unwrap_or('?');
        }
    }

    fn set_palette(&mut self, palette_rgb: Vec<u32>) {
        self.palette_length = palette_rgb.len();
    }

    fn frames_per_second(&self) -> u32 {
        30
    }
}

fn panic_text(payload: Box<dyn Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    String::from("<non-string panic payload>")
}

/// Runs `run` and returns the panic text when it panicked.
fn panic_message(run: impl FnOnce()) -> Option<String> {
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(()) => None,
        Err(payload) => Some(panic_text(payload)),
    }
}

/// Asserts that `run` panics and that the panic text names `code`.
fn assert_panic_mentions(code: &str, run: impl FnOnce()) {
    let message = panic_message(run)
        .unwrap_or_else(|| panic!("expected a panic that mentions {code}, but nothing panicked"));
    assert!(
        message.contains(code),
        "panic text {message:?} does not mention {code}"
    );
}

/// Unwraps a harness result, failing the test with the error text on `Err`.
fn harness_ok<T>(result: Result<T, HarnessError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("unexpected harness error: {error}"),
    }
}

/// Returns the code of a harness error, failing the test when the call succeeded.
fn harness_code<T>(result: Result<T, HarnessError>) -> String {
    match result {
        Ok(_) => panic!("expected a harness error, got Ok"),
        Err(error) => error.code,
    }
}

/// Returns the plugin error of a failed call, failing the test when the call succeeded.
fn plugin_error<T: std::fmt::Debug>(result: Result<T, PluginError>) -> PluginError {
    match result {
        Ok(value) => panic!("expected a plugin error, got Ok({value:?})"),
        Err(error) => error,
    }
}

/// Sets environment variables for the duration of `run`, restoring them afterwards.
fn with_env<T>(pairs: &[(&str, Option<&str>)], run: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous: Vec<(String, Option<String>)> = pairs
        .iter()
        .map(|(key, _)| (key.to_string(), std::env::var(key).ok()))
        .collect();
    for (key, value) in pairs {
        match value {
            Some(text) => std::env::set_var(key, text),
            None => std::env::remove_var(key),
        }
    }
    let outcome = catch_unwind(AssertUnwindSafe(run));
    for (key, value) in previous {
        match value {
            Some(text) => std::env::set_var(&key, text),
            None => std::env::remove_var(&key),
        }
    }
    match outcome {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// A file path inside cargo's per-target temporary directory, unique per test name.
fn scratch_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("unit_gaps_{name}"))
}

fn write_scratch(name: &str, text: &str) -> PathBuf {
    let path = scratch_path(name);
    std::fs::write(&path, text).expect("scratch file must be writable");
    path
}

fn started_session() -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("ilium.core.tree").start();
    session
}

fn replay_header(version: u32) -> String {
    format!(
        "{{\"type\":\"header\",\"version\":{version},\"api\":\"ilium:plugin@0.1.0\",\
         \"plugin\":\"test.plugin\",\"side\":\"server\",\"seed\":1,\"start\":0}}"
    )
}

fn fixture_outcome_text(path: &Path) -> String {
    match ilium_plugin_testkit::fixture::run::<IdlePlugin>(path) {
        Ok(FixtureStatus::Pass) => String::from("pass"),
        Ok(FixtureStatus::Fail(message)) => message,
        Ok(FixtureStatus::Trap(code)) => code,
        Err(error) => error.code,
    }
}

fn coverage_summary(percent: f64) -> ilium_plugin_testkit::coverage::CoverageSummary {
    ilium_plugin_testkit::coverage::CoverageSummary {
        lines_total: 100,
        lines_covered: percent as u64,
        percent,
    }
}

// ---------------------------------------------------------------------------
// host.rs: FakeHost lifecycle, rendering, clock, grants and hooks
// ---------------------------------------------------------------------------

#[test]
fn host_activate_idle_plugin_without_grants_succeeds() {
    let mut host = FakeHost::new();
    let _handle = host.activate::<IdlePlugin>();
    assert!(matches!(host.current_mode(), Mode::Native));
}

#[test]
fn host_try_activate_twice_reports_already_active() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let error = match host.try_activate::<IdlePlugin>() {
        Ok(_) => panic!("a second activation of a live plugin must fail"),
        Err(error) => error,
    };
    assert!(
        error.message.contains(codes::ALREADY_ACTIVE),
        "{}",
        error.message
    );
}

#[test]
fn host_deactivate_without_active_plugin_reports_not_active() {
    let mut host = FakeHost::new();
    assert_panic_mentions(codes::NOT_ACTIVE, || {
        host.deactivate(DeactivationReason::Shutdown);
    });
}

#[test]
fn host_deactivate_active_plugin_without_state_returns_none() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    assert_eq!(host.deactivate(DeactivationReason::Shutdown), None);
}

#[test]
fn host_deliver_empty_batch_returns_no_submissions() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    assert!(host.deliver(Vec::new()).is_empty());
}

#[test]
fn host_settle_with_no_pending_work_leaves_no_outcomes() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.settle();
    assert!(host.outcomes().is_empty());
    assert!(host.submitted().is_empty());
}

#[test]
fn host_render_known_panel_returns_requested_row_count() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let screen = host.render(ViewTarget::Panel(String::from("settings")), 20, 5);
    assert_eq!(screen.lines().len(), 5);
}

#[test]
fn host_render_zero_columns_reports_render_size() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let target = ViewTarget::Panel(String::from("settings"));
    assert_panic_mentions(codes::RENDER_SIZE, || {
        host.render(target, 0, 10);
    });
}

#[test]
fn host_render_unknown_panel_reports_view_target_unknown() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let target = ViewTarget::Panel(String::from("no-such-panel"));
    assert_panic_mentions(codes::VIEW_TARGET_UNKNOWN, || {
        host.render(target, 20, 5);
    });
}

#[test]
fn host_advance_clock_by_zero_keeps_instant() {
    let mut host = FakeHost::new();
    let before = host.clock().now();
    host.advance_clock(0);
    assert_eq!(host.clock().now(), before);
}

#[test]
fn host_set_clock_backwards_reports_clock_backwards() {
    let mut host = FakeHost::new();
    host.set_clock(2_000);
    assert_panic_mentions(codes::CLOCK_BACKWARDS, || {
        host.set_clock(1_000);
    });
}

#[test]
fn host_advance_clock_overflow_reports_clock_argument() {
    let mut host = FakeHost::new();
    assert_panic_mentions(codes::CLOCK_ARGUMENT, || {
        host.advance_clock(u64::MAX);
    });
}

#[test]
fn host_grant_now_unknown_name_reports_grant_unknown() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    assert_eq!(
        harness_code(host.grant_now("no.such.grant")),
        codes::GRANT_UNKNOWN
    );
}

#[test]
fn host_run_hook_without_scripted_reply_passes() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let call = HookCall {
        point: String::from("ilium.test.point"),
        ..Default::default()
    };
    assert_eq!(host.run_hook(call), HookReply::Pass);
}

#[test]
fn host_hook_calls_empty_before_any_run() {
    let host = FakeHost::new();
    assert!(host.all_hook_calls().is_empty());
    assert!(host.hook_calls("ilium.test.point").is_empty());
    assert!(host.hook_runs("ilium.test.point").is_empty());
}

#[test]
fn host_hook_calls_keep_order_across_points() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let first = HookCall {
        point: String::from("first"),
        ..Default::default()
    };
    let second = HookCall {
        point: String::from("second"),
        ..Default::default()
    };
    host.run_hook(first);
    host.run_hook(second);
    let all = host.all_hook_calls();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].point, "first");
    assert_eq!(all[1].point, "second");
    assert!(host.hook_calls("no.calls.here").is_empty());
}

#[test]
fn host_set_hook_reply_is_returned_for_matching_point() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.set_hook_reply("ilium.test.point", HookReply::Refuse(String::from("no")));
    let call = HookCall {
        point: String::from("ilium.test.point"),
        ..Default::default()
    };
    assert_eq!(host.run_hook(call), HookReply::Refuse(String::from("no")));
}

#[test]
fn host_set_hook_reply_when_falls_through_when_predicate_is_false() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.set_hook_reply_when(
        "ilium.test.point",
        json!({"kind": "a"}),
        HookReply::Answer(String::from("matched")),
    );
    let call = HookCall {
        point: String::from("ilium.test.point"),
        payload_json: String::from(r#"{"kind":"b"}"#),
        ..Default::default()
    };
    assert_eq!(host.run_hook(call), HookReply::Pass);
}

#[test]
fn host_set_hook_trap_makes_the_hook_raise_unexpected_call() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.set_hook_trap("ilium.test.point");
    let call = HookCall {
        point: String::from("ilium.test.point"),
        ..Default::default()
    };
    assert_panic_mentions(codes::UNEXPECTED_CALL, || {
        host.run_hook(call);
    });
}

#[test]
fn host_fail_hook_produces_a_non_pass_reply() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.fail_hook("ilium.test.point");
    let call = HookCall {
        point: String::from("ilium.test.point"),
        ..Default::default()
    };
    assert_ne!(host.run_hook(call), HookReply::Pass);
}

#[test]
fn host_run_job_returns_bytes_once_finished_with_ok() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let result = host.run_job("demo.kind", b"payload");
    assert!(
        result.is_ok(),
        "job finished with Ok bytes must return them: {result:?}"
    );
}

#[test]
fn host_menu_items_empty_for_menu_without_entries() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let context = MenuContext {
        menu: String::from("no-such-menu"),
        ..Default::default()
    };
    assert!(host.menu_items(context).is_empty());
}

#[test]
fn host_stub_service_reply_is_returned_to_caller() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.stub_service("bookmarks", "list", r#"{"items":[]}"#);
    assert_eq!(
        host.call_service("bookmarks", "list", "{}"),
        Ok(String::from(r#"{"items":[]}"#))
    );
}

#[test]
fn host_stub_service_failure_message_is_returned_to_caller() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.stub_service_failure("bookmarks", "list", "boom");
    let error = plugin_error(host.call_service("bookmarks", "list", "{}"));
    assert!(error.message.contains("boom"), "{}", error.message);
}

#[test]
fn host_service_calls_record_arguments_in_order() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    host.stub_service("bookmarks", "list", "{}");
    host.stub_service("bookmarks", "get", "{}");
    let _ = host.call_service("bookmarks", "list", r#"{"a":1}"#);
    let _ = host.call_service("bookmarks", "get", r#"{"b":2}"#);
    let calls = host.service_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, "list");
    assert_eq!(calls[1].1, "get");
}

#[test]
fn host_own_service_without_grant_reports_grant_not_declared() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    let error = plugin_error(host.call_own_service("own", "{}"));
    assert!(
        error.message.contains(codes::GRANT_NOT_DECLARED),
        "{}",
        error.message
    );
}

#[test]
fn host_contribute_entry_appears_in_extension_entries() {
    let mut host = FakeHost::new();
    host.contribute_entry("ilium.point.toolbar", "save", r#"{"label":"Save"}"#);
    assert_eq!(host.extension_entries("ilium.point.toolbar").len(), 1);
}

#[test]
fn host_withdraw_missing_extension_id_is_a_no_op() {
    let mut host = FakeHost::new();
    host.withdraw_extension("ilium.point.toolbar", "missing");
    assert!(host.extension_entries("ilium.point.toolbar").is_empty());
}

#[test]
fn host_duplicate_extension_id_keeps_an_entry() {
    let mut host = FakeHost::new();
    host.contribute_entry("ilium.point.toolbar", "save", "{}");
    host.contribute_entry("ilium.point.toolbar", "save", "{}");
    assert!(!host.extension_entries("ilium.point.toolbar").is_empty());
}

#[test]
fn host_extension_entries_empty_for_unknown_point() {
    let host = FakeHost::new();
    assert!(host.extension_entries("ilium.point.never-used").is_empty());
}

#[test]
fn host_collect_point_add_is_listed_as_extension_entry() {
    let mut host = FakeHost::new();
    host.collect_point("ilium.point.list")
        .add("a", 1, json!({"x": 1}));
    assert_eq!(host.extension_entries("ilium.point.list").len(), 1);
}

#[test]
fn host_set_extension_entries_replaces_prior_entries() {
    let mut host = FakeHost::new();
    host.contribute_entry("ilium.point.list", "old", "{}");
    host.set_extension_entries(
        "ilium.point.list",
        vec![String::from("{}"), String::from("{}")],
    );
    assert_eq!(host.extension_entries("ilium.point.list").len(), 2);
}

#[test]
fn host_extension_list_calls_start_at_zero() {
    let host = FakeHost::new();
    assert_eq!(host.extension_list_calls("ilium.point.list"), 0);
}

#[test]
fn host_kernel_settings_write_is_visible_through_get() {
    let mut host = FakeHost::new();
    let written = host
        .kernel_settings()
        .set("ilium.core.tree", "density", json!("compact"));
    assert!(
        written.is_ok(),
        "kernel write must be accepted: {written:?}"
    );
    assert_eq!(
        host.kernel_settings().get("ilium.core.tree", "density"),
        Some(json!("compact"))
    );
}

#[test]
fn host_instance_mut_before_activation_reports_an_error() {
    let mut host = FakeHost::new();
    assert!(panic_message(|| {
        host.instance_mut::<IdlePlugin>();
    })
    .is_some());
}

#[test]
fn host_instance_mut_of_wrong_type_reports_an_error() {
    let mut host = FakeHost::new();
    host.activate::<IdlePlugin>();
    assert!(panic_message(|| {
        host.instance_mut::<OtherPlugin>();
    })
    .is_some());
}

#[test]
fn host_script_inference_without_calls_records_no_requests() {
    let mut host = FakeHost::new();
    host.script_inference(vec![Err(PluginError::new(ErrorCode::Network, "down"))]);
    assert!(host.inference_requests().is_empty());
}

#[test]
fn host_empty_inference_script_yields_no_requests() {
    let mut host = FakeHost::new();
    host.script_inference(Vec::new());
    assert!(host.inference_requests().is_empty());
}

#[test]
fn host_fixture_events_empty_without_fixture() {
    let host = FakeHost::new();
    assert!(host.fixture_events().is_empty());
}

#[test]
fn host_project_root_override_is_returned() {
    let host = FakeHost::new().with_project_root(PathBuf::from("/srv/demo"));
    assert_eq!(host.project_root(), PathBuf::from("/srv/demo"));
}

#[test]
fn host_set_mode_accepts_every_variant() {
    let component = FakeHost::new().mode(Mode::Component);
    assert!(matches!(component.current_mode(), Mode::Component));
    let native = FakeHost::new().mode(Mode::Native);
    assert!(matches!(native.current_mode(), Mode::Native));
}

#[test]
fn host_seed_settings_unchecked_bypasses_validation() {
    let mut host = FakeHost::new();
    host.seed_settings_unchecked("count", json!("not-a-number"));
    assert_eq!(host.settings().get("count"), Some(json!("not-a-number")));
}

#[test]
fn host_fuzz_run_with_idle_plugin_passes() {
    let mut host = FakeHost::new();
    assert!(host.fuzz_run(7, 3).is_ok());
}

// ---------------------------------------------------------------------------
// fakes/tree.rs
// ---------------------------------------------------------------------------

#[test]
fn tree_add_group_returns_unique_id_and_group_is_visible() {
    let mut host = FakeHost::new();
    let first = host.tree().add_group("Team");
    let second = host.tree().add_group("Other");
    assert_ne!(first, second);
    let node = host
        .tree()
        .get(first)
        .expect("group must be visible through get()");
    assert_eq!(node.name, "Team");
}

#[test]
fn tree_add_group_accepts_empty_name() {
    let mut host = FakeHost::new();
    let id = host.tree().add_group("");
    assert!(host.tree().get(id).is_some());
}

#[test]
fn tree_add_two_groups_with_same_name_keeps_both() {
    let mut host = FakeHost::new();
    let first = host.tree().add_group("Dup");
    let second = host.tree().add_group("Dup");
    assert!(host.tree().get(first).is_some());
    assert!(host.tree().get(second).is_some());
    assert_ne!(first, second);
}

#[test]
fn tree_add_agent_pane_has_signature_and_default_activity() {
    let mut host = FakeHost::new();
    let id = host.tree().add_agent_pane("Claude", "claude-code");
    let node = host.tree().get(id).expect("agent pane must exist");
    let view = node.agent.expect("agent pane must carry an agent view");
    assert!(matches!(view.activity, AgentActivity::None));
}

#[test]
fn tree_add_agent_pane_with_unknown_signature_keeps_the_pane() {
    let mut host = FakeHost::new();
    let id = host.tree().add_agent_pane("Mystery", "no-such-agent");
    assert!(host.tree().get(id).is_some());
}

#[test]
fn tree_move_agent_pane_into_group_updates_parent() {
    let mut host = FakeHost::new();
    let group = host.tree().add_group("Team");
    let pane = host.tree().add_agent_pane("Claude", "claude-code");
    host.tree().move_node(pane, group);
    assert_eq!(
        host.tree().get(pane).and_then(|node| node.parent),
        Some(group)
    );
}

#[test]
fn tree_set_agent_goal_round_trips_objective() {
    let mut host = FakeHost::new();
    let pane = host.tree().add_agent_pane("Claude", "claude-code");
    host.tree()
        .set_agent_goal(pane, "ship the testkit", "active");
    let node = host.tree().get(pane).expect("agent pane must exist");
    let objective = node.agent.and_then(|view| view.objective);
    assert_eq!(objective.as_deref(), Some("ship the testkit"));
}

#[test]
fn tree_set_agent_session_on_missing_id_leaves_tree_unchanged() {
    let mut host = FakeHost::new();
    host.tree()
        .set_agent_session(9_999, "session-1", vec![String::from("line")]);
    assert!(host.tree().get(9_999).is_none());
}

#[test]
fn tree_set_agent_session_accepts_empty_transcript() {
    let mut host = FakeHost::new();
    let pane = host.tree().add_agent_pane("Claude", "claude-code");
    host.tree().set_agent_session(pane, "session-1", Vec::new());
    assert!(host.tree().get(pane).is_some());
}

#[test]
fn tree_metadata_round_trips_bytes() {
    let mut host = FakeHost::new();
    let id = host.tree().add_group("Team");
    host.tree().set_metadata(id, b"abc".to_vec());
    assert_eq!(host.tree().metadata(id), Some(b"abc".to_vec()));
}

#[test]
fn tree_metadata_accepts_empty_bytes() {
    let mut host = FakeHost::new();
    let id = host.tree().add_group("Team");
    host.tree().set_metadata(id, Vec::new());
    assert_eq!(host.tree().metadata(id), Some(Vec::new()));
}

#[test]
fn tree_metadata_unknown_id_returns_none() {
    let mut host = FakeHost::new();
    assert_eq!(host.tree().metadata(4_242), None);
}

#[test]
fn tree_remove_then_set_locked_leaves_id_absent() {
    let mut host = FakeHost::new();
    let id = host.tree().add_group("Team");
    host.tree().remove(id);
    host.tree().set_locked(id, true);
    assert!(host.tree().get(id).is_none());
}

// ---------------------------------------------------------------------------
// fakes/cli.rs
// ---------------------------------------------------------------------------

#[test]
fn cli_invoke_with_empty_arguments_records_no_arguments() {
    let mut host = FakeHost::new();
    let invocation = host.cli().invoke(&[], b"");
    assert!(invocation.arguments.is_empty());
}

#[test]
fn cli_refresh_returns_the_same_command_name() {
    let mut host = FakeHost::new();
    let invocation = host.cli().invoke(&["echo", "hi"], b"");
    let refreshed = host.cli().refresh(&invocation);
    assert_eq!(refreshed.name, invocation.name);
}

#[test]
fn cli_cancel_finished_invocation_is_a_no_op() {
    let mut host = FakeHost::new();
    let invocation = host.cli().invoke(&["echo", "hi"], b"");
    host.cli().cancel(&invocation);
    host.cli().cancel(&invocation);
}

#[test]
fn cli_unset_missing_variable_is_a_no_op() {
    let mut host = FakeHost::new();
    host.cli().unset_env("ILIUM_UNIT_GAP_MISSING");
}

#[test]
fn cli_set_cwd_to_empty_path_is_accepted() {
    let mut host = FakeHost::new();
    host.cli().set_cwd("");
    let invocation = host.cli().invoke(&["pwd"], b"");
    assert_eq!(invocation.name, "pwd");
}

// ---------------------------------------------------------------------------
// fakes/fs.rs and parse_shorthand
// ---------------------------------------------------------------------------

#[test]
fn fs_released_shared_lock_leaves_no_held_locks() {
    let mut host = FakeHost::new();
    let lock = host.fs().hold_lock("project:a.txt", false);
    assert_eq!(host.fs().held_locks().len(), 1);
    lock.release();
    assert!(host.fs().held_locks().is_empty());
}

#[test]
fn fs_held_locks_empty_on_fresh_host() {
    let mut host = FakeHost::new();
    assert!(host.fs().held_locks().is_empty());
}

#[test]
fn fs_lock_log_records_calls_in_order() {
    let mut host = FakeHost::new();
    let _shared = host.fs().hold_lock("project:a.txt", false);
    let _exclusive = host.fs().hold_lock("project:b.txt", true);
    let log = host.fs().lock_log();
    assert_eq!(log.len(), 2);
    assert!(!log[0].exclusive);
    assert!(log[1].exclusive);
}

#[test]
fn fs_replace_atomically_creates_missing_file() {
    let mut host = FakeHost::new();
    host.fs().replace_atomically("project:new.txt", b"v1");
    assert_eq!(host.fs().read("project:new.txt"), Some(b"v1".to_vec()));
}

#[test]
fn fs_replace_atomically_replaces_existing_content_whole() {
    let mut host = FakeHost::new();
    host.fs()
        .seed("project:old.txt", b"a much longer old content");
    host.fs().replace_atomically("project:old.txt", b"x");
    assert_eq!(host.fs().read("project:old.txt"), Some(b"x".to_vec()));
}

#[test]
fn fs_race_next_write_is_not_visible_before_any_write() {
    let mut host = FakeHost::new();
    host.fs().race_next_write("project:r.txt", b"racer");
    assert_eq!(host.fs().read("project:r.txt"), None);
}

#[test]
fn fs_race_next_write_lands_before_the_intended_write() {
    let mut host = FakeHost::new();
    host.fs().race_next_write("project:r.txt", b"racer");
    host.fs().write("project:r.txt", b"mine");
    assert_eq!(host.fs().read("project:r.txt"), Some(b"mine".to_vec()));
}

#[test]
fn fs_race_next_write_applies_only_once() {
    let mut host = FakeHost::new();
    host.fs().race_next_write("project:r.txt", b"racer");
    host.fs().write("project:r.txt", b"first");
    host.fs().write("project:r.txt", b"second");
    assert_eq!(host.fs().read("project:r.txt"), Some(b"second".to_vec()));
}

#[test]
fn fs_mkdir_creates_directory_and_accepts_repeat() {
    let mut host = FakeHost::new();
    host.fs().mkdir("project:docs");
    host.fs().mkdir("project:docs");
    assert!(!host.fs().paths("project").is_empty());
}

#[test]
fn fs_modified_is_none_for_missing_path() {
    let mut host = FakeHost::new();
    assert_eq!(host.fs().modified("project:missing.txt"), None);
}

#[test]
fn fs_seed_sparse_zero_size_reads_back_empty() {
    let mut host = FakeHost::new();
    host.fs().seed_sparse("project:zero.bin", 0);
    assert_eq!(
        host.fs().read("project:zero.bin").map(|bytes| bytes.len()),
        Some(0)
    );
}

#[test]
fn fs_seed_sparse_large_size_reads_back_full_length() {
    let mut host = FakeHost::new();
    host.fs().seed_sparse("project:big.bin", 4_096);
    assert_eq!(
        host.fs().read("project:big.bin").map(|bytes| bytes.len()),
        Some(4_096)
    );
}

#[test]
fn fs_tail_log_and_write_log_start_empty() {
    let mut host = FakeHost::new();
    assert!(host.fs().tail_log().is_empty());
    assert!(host.fs().write_log().is_empty());
}

#[test]
fn fs_write_log_records_each_call_in_order() {
    let mut host = FakeHost::new();
    host.fs().write("project:a.txt", b"1");
    host.fs().write("project:b.txt", b"2");
    let log = host.fs().write_log();
    assert_eq!(log.len(), 2);
    assert!(!log[0].append);
    assert!(!log[1].append);
}

#[test]
fn fs_removed_file_is_no_longer_readable() {
    let mut host = FakeHost::new();
    host.fs().write("project:gone.txt", b"bye");
    host.fs().remove("project:gone.txt");
    assert_eq!(host.fs().read("project:gone.txt"), None);
}

#[test]
fn fs_parse_shorthand_valid_text_yields_fs_path() {
    assert!(parse_shorthand("project:docs/notes.md").is_ok());
}

#[test]
fn fs_parse_shorthand_empty_text_is_rejected() {
    let error = match parse_shorthand("") {
        Ok(_) => panic!("empty shorthand must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, codes::FS_PATH_INVALID);
}

#[test]
fn fs_parse_shorthand_escaping_scope_returns_fs_path_invalid() {
    let error = match parse_shorthand("project:../escape") {
        Ok(_) => panic!("a path escaping its scope must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, codes::FS_PATH_INVALID);
}

// ---------------------------------------------------------------------------
// fakes/settings.rs
// ---------------------------------------------------------------------------

#[test]
fn settings_set_valid_key_and_value_is_stored() {
    let mut host = FakeHost::new();
    let result = host.settings().set("theme", json!("dark"));
    assert!(result.is_ok(), "valid setting must be accepted: {result:?}");
    assert_eq!(host.settings().get("theme"), Some(json!("dark")));
}

#[test]
fn settings_set_unknown_key_returns_plugin_error() {
    let mut host = FakeHost::new();
    assert!(host.settings().set("no.such.key", json!(1)).is_err());
}

#[test]
fn settings_set_type_mismatch_returns_invalid_argument() {
    let mut host = FakeHost::new();
    let error = plugin_error(host.settings().set("count", json!("not a number")));
    assert!(matches!(error.code, ErrorCode::InvalidArgument));
}

#[test]
fn settings_get_missing_key_returns_none() {
    let mut host = FakeHost::new();
    assert_eq!(host.settings().get("no.such.key"), None);
}

#[test]
fn settings_info_for_known_key_is_present() {
    let mut host = FakeHost::new();
    assert!(host.settings().info("theme").is_some());
}

#[test]
fn settings_section_value_returns_nested_object() {
    let mut host = FakeHost::new();
    host.settings()
        .seed(r#"{"appearance":{"density":"compact"}}"#);
    assert_eq!(
        host.settings().section_value("appearance"),
        Some(json!({"density": "compact"}))
    );
}

#[test]
fn settings_seed_rejects_invalid_json() {
    let mut host = FakeHost::new();
    assert!(panic_message(|| host.settings().seed("{not json")).is_some());
}

#[test]
fn settings_seed_does_not_count_as_a_write() {
    let mut host = FakeHost::new();
    host.settings().seed(r#"{"theme":"dark"}"#);
    assert!(host.settings().writes().is_empty());
}

#[test]
fn settings_writes_keep_set_order() {
    let mut host = FakeHost::new();
    let _ = host.settings().set("theme", json!("light"));
    let _ = host.settings().set("theme", json!("dark"));
    let writes = host.settings().writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].0, "theme");
    assert_eq!(writes[1].1, json!("dark"));
}

// ---------------------------------------------------------------------------
// fakes/http.rs, fakes/process.rs
// ---------------------------------------------------------------------------

#[test]
fn http_calls_and_streams_start_empty() {
    let mut host = FakeHost::new();
    assert!(host.http().calls().is_empty());
    assert!(host.http().streams().is_empty());
}

#[test]
fn http_expectation_registration_records_no_call() {
    let mut host = FakeHost::new();
    host.http().expect(
        "GET",
        "https://example.test/a",
        HttpResponse {
            status: 200,
            ..Default::default()
        },
    );
    assert!(host.http().calls().is_empty());
}

#[test]
fn process_expectation_registration_records_no_call() {
    let mut host = FakeHost::new();
    host.process()
        .expect("git", &["status"])
        .returns(ProcessExit::default());
    assert!(host.process().calls().is_empty());
}

#[test]
fn process_max_concurrent_is_zero_when_no_child_ran() {
    let mut host = FakeHost::new();
    assert_eq!(host.processes().max_concurrent(), 0);
    assert!(host.processes().children().is_empty());
}

#[test]
fn process_on_program_timeout_registration_records_no_child() {
    let mut host = FakeHost::new();
    host.processes().on_program_timeout("slow-tool");
    assert!(host.processes().children().is_empty());
}

#[test]
fn process_default_exit_registration_is_accepted() {
    let mut host = FakeHost::new();
    host.processes().default_exit(ProcessExit::default());
    assert!(host.processes().calls().is_empty());
}

// ---------------------------------------------------------------------------
// fakes/jobs.rs and fakes/storage.rs
// ---------------------------------------------------------------------------

#[test]
fn jobs_list_cancelled_and_progress_start_empty() {
    let mut host = FakeHost::new();
    assert!(host.jobs().list().is_empty());
    assert!(host.jobs().cancelled().is_empty());
    assert!(host.jobs().reported_progress().is_empty());
}

#[test]
fn jobs_progress_boundaries_zero_and_hundred_are_recorded() {
    let mut host = FakeHost::new();
    host.jobs().progress(7, 0.0, "starting");
    host.jobs().progress(7, 100.0, "done");
    assert_eq!(host.jobs().reported_progress(), vec![0.0, 100.0]);
}

#[test]
fn jobs_finish_unknown_request_reports_an_error() {
    let mut host = FakeHost::new();
    assert!(panic_message(|| host.jobs().finish(999, Ok(Vec::new()))).is_some());
}

#[test]
fn jobs_request_cancel_without_jobs_cancels_nothing() {
    let mut host = FakeHost::new();
    host.jobs().request_cancel();
    assert!(host.jobs().cancelled().is_empty());
}

#[test]
fn storage_put_then_get_round_trips_bytes() {
    let mut host = FakeHost::new();
    host.storage_mut().put(Bucket::Global, "key", b"value");
    assert_eq!(
        host.storage().get(Bucket::Global, "key"),
        Some(b"value".to_vec())
    );
}

#[test]
fn storage_empty_value_is_stored() {
    let mut host = FakeHost::new();
    host.storage_mut().put(Bucket::Global, "empty", b"");
    assert_eq!(
        host.storage().get(Bucket::Global, "empty"),
        Some(Vec::new())
    );
}

#[test]
fn storage_missing_key_returns_none() {
    let host = FakeHost::new();
    assert_eq!(host.storage().get(Bucket::Global, "missing"), None);
}

#[test]
fn storage_dump_isolates_buckets() {
    let mut host = FakeHost::new();
    host.storage_mut().put(Bucket::Global, "a", b"1");
    host.storage_mut().put(Bucket::Project, "b", b"2");
    let global = host.storage().dump(Bucket::Global);
    assert!(global.contains_key("a"));
    assert!(!global.contains_key("b"));
}

// ---------------------------------------------------------------------------
// fakes/clock.rs
// ---------------------------------------------------------------------------

#[test]
fn clock_now_equals_initial_instant() {
    let clock = ilium_plugin_testkit::fakes::clock::FakeClock::at(1_000);
    assert_eq!(clock.now(), 1_000);
}

#[test]
fn clock_instant_zero_is_a_valid_boundary() {
    let clock = ilium_plugin_testkit::fakes::clock::FakeClock::at(0);
    assert_eq!(clock.now(), 0);
}

#[test]
fn clock_large_instant_round_trips() {
    let clock = ilium_plugin_testkit::fakes::clock::FakeClock::at(u64::MAX);
    assert_eq!(clock.now(), u64::MAX);
}

// ---------------------------------------------------------------------------
// session.rs
// ---------------------------------------------------------------------------

#[test]
fn session_try_install_missing_directory_returns_error() {
    let mut session = FakeSession::new();
    let result = session.try_install(PathBuf::from("/nonexistent/unit-gap-plugin"));
    assert!(result.is_err());
}

#[test]
fn session_install_core_unknown_name_reports_core_unknown() {
    assert_panic_mentions(codes::CORE_UNKNOWN, || {
        FakeSession::new().install_core("no.such.core");
    });
}

#[test]
fn session_install_core_tree_lists_its_bookmarks_service() {
    let session = started_session();
    assert!(session.services().contains(&String::from("bookmarks")));
}

#[test]
fn session_services_empty_without_installed_plugins() {
    let session = FakeSession::new();
    assert!(session.services().is_empty());
}

#[test]
fn session_unknown_plugin_id_reports_plugin_unknown() {
    let mut session = FakeSession::new();
    assert_panic_mentions(codes::PLUGIN_UNKNOWN, || {
        session.server("no.such.plugin");
    });
}

#[test]
fn session_accessors_accept_str_string_and_plugin_id() {
    let mut session = started_session();
    let owned = String::from("ilium.core.tree");
    let _ = session.server("ilium.core.tree");
    let _ = session.server(&owned);
    let _ = session.server(PluginId(owned.clone()));
}

#[test]
fn session_run_hook_core_tree_before_move_passes() {
    let mut session = started_session();
    let call = HookCall {
        point: String::from("before-move"),
        ..Default::default()
    };
    match session.run_hook("ilium.core.tree", call) {
        Ok(reply) => assert_eq!(reply, HookReply::Pass),
        Err(error) => panic!("hook on a bundled core tree must answer: {error}"),
    }
}

#[test]
fn session_run_hook_without_handler_reports_not_a_hook_handler() {
    let mut session = started_session();
    let call = HookCall {
        point: String::from("before-move"),
        ..Default::default()
    };
    let code = harness_code(session.run_hook("ilium.core.agent-toolbar", call));
    assert_eq!(code, codes::NOT_A_HOOK_HANDLER);
}

#[test]
fn session_call_bookmarks_service_returns_done_reply() {
    let mut session = started_session();
    let result = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
    assert_eq!(
        result,
        Ok(ServiceReply::Done(String::from(r#"{"ok":true}"#)))
    );
}

#[test]
fn session_call_unknown_service_is_not_found() {
    let mut session = started_session();
    let error = plugin_error(session.call_service("ilium.test.caller", "no.service", "*", "{}"));
    assert!(matches!(error.code, ErrorCode::NotFound));
}

#[test]
fn session_call_service_after_grant_succeeds() {
    let mut session = started_session();
    session.grant_service("ilium.test.caller", "bookmarks");
    let result = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
    assert!(matches!(result, Ok(ServiceReply::Done(_))), "{result:?}");
}

#[test]
fn session_pump_with_empty_queue_succeeds() {
    let mut session = started_session();
    assert!(session.pump().is_ok());
}

#[test]
fn session_disabled_plugin_hook_is_not_dispatched() {
    let mut session = started_session();
    session.disable("ilium.core.tree");
    let call = HookCall {
        point: String::from("before-move"),
        ..Default::default()
    };
    assert!(matches!(
        session.run_hook("ilium.core.tree", call),
        Ok(HookReply::Pass)
    ));
}

#[test]
fn session_stubbed_service_reply_is_returned() {
    let mut session = started_session();
    session.grant_service("ilium.test.caller", "bookmarks");
    session.stub_service(
        "ilium.core.tree",
        "bookmarks",
        "*",
        Ok(String::from(r#"{"ok":false}"#)),
    );
    let result = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
    assert_eq!(
        result,
        Ok(ServiceReply::Done(String::from(r#"{"ok":false}"#)))
    );
}

#[test]
fn session_second_stub_overwrites_the_first() {
    let mut session = started_session();
    session.grant_service("ilium.test.caller", "bookmarks");
    session.stub_service("ilium.core.tree", "bookmarks", "*", Ok(String::from("1")));
    session.stub_service("ilium.core.tree", "bookmarks", "*", Ok(String::from("2")));
    let result = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
    assert_eq!(result, Ok(ServiceReply::Done(String::from("2"))));
}

#[test]
fn session_stalled_service_defers_the_reply() {
    let mut session = started_session();
    session.grant_service("ilium.test.caller", "bookmarks");
    session.stall_service("ilium.core.tree", "bookmarks", "*");
    let result = session.call_service("ilium.test.caller", "bookmarks", "*", "{}");
    assert!(matches!(result, Ok(ServiceReply::Deferred)), "{result:?}");
}

#[test]
fn session_contribute_then_collected_returns_entry_json() {
    let mut session = started_session();
    session.contribute(
        "ilium.core.tree",
        "ilium.point.test",
        "item",
        0,
        json!({"label": "x"}),
    );
    assert_eq!(
        session.collected("ilium.point.test"),
        vec![json!({"label": "x"})]
    );
}

#[test]
fn session_collected_on_empty_point_is_empty() {
    let mut session = started_session();
    assert!(session.collected("ilium.point.nothing-here").is_empty());
}

#[test]
fn session_call_extension_unknown_point_is_an_error() {
    let mut session = started_session();
    assert!(session
        .call_extension("ilium.point.never-declared", "{}")
        .is_err());
}

#[test]
fn session_observer_priorities_are_reflected_in_hook_chains() {
    let mut session = started_session();
    session.observe_hook("ilium.core.tree", "before-move", 10);
    session.observe_hook("ilium.core.tree", "before-move", 1);
    let chains = session.hook_chains("ilium.core.tree", "before-move");
    assert!(!chains.is_empty());
    assert!(chains.iter().any(|chain| chain.len() == 2));
}

#[test]
fn session_failing_observer_does_not_abort_the_hook() {
    let mut session = started_session();
    session.observer_fails("ilium.core.tree", "before-move");
    let call = HookCall {
        point: String::from("before-move"),
        ..Default::default()
    };
    assert!(session.run_hook("ilium.core.tree", call).is_ok());
}

#[test]
fn session_observer_log_is_recorded_on_plugin_host() {
    let mut session = started_session();
    session.observer_logs("ilium.core.tree", "before-move", "info", "hello");
    assert_eq!(session.plugin("ilium.core.tree").logs().len(), 1);
}

#[test]
fn session_settings_tab_order_empty_for_empty_session() {
    let session = FakeSession::new();
    assert!(session.settings_tab_order().is_empty());
}

#[test]
fn session_kernel_settings_value_for_unknown_tab_is_none() {
    let session = FakeSession::new();
    assert_eq!(session.kernel_settings_value("no.such.tab"), None);
}

#[test]
fn session_disabled_notice_for_enabled_tab_is_empty() {
    let session = started_session();
    assert!(session.disabled_notice("ilium.core.tree").is_empty());
}

// ---------------------------------------------------------------------------
// scene.rs
// ---------------------------------------------------------------------------

#[test]
fn scene_zero_columns_reports_render_size() {
    assert_panic_mentions(codes::RENDER_SIZE, || {
        FakeScene::new::<BlankRenderer>("demo", 0, 10);
    });
}

#[test]
fn scene_zero_rows_reports_render_size() {
    assert_panic_mentions(codes::RENDER_SIZE, || {
        FakeScene::new::<BlankRenderer>("demo", 10, 0);
    });
}

#[test]
fn scene_frame_has_requested_size() {
    let mut scene = FakeScene::new::<BlankRenderer>("demo", 12, 3);
    let screen = scene.frame(FrameInput::default());
    assert_eq!(screen.lines().len(), 3);
    assert_eq!(screen.row(0).chars().count(), 12);
}

#[test]
fn scene_empty_palette_vector_is_accepted() {
    let mut scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    scene.set_palette(Vec::new());
    let screen = scene.frame(FrameInput::default());
    assert_eq!(screen.lines().len(), 2);
}

#[test]
fn scene_palette_update_is_reflected_in_next_frame() {
    let mut scene = FakeScene::new::<PaletteRenderer>("demo", 4, 2);
    scene.set_palette(vec![1, 2, 3]);
    let screen = scene.frame(FrameInput::default());
    assert!(
        screen.row(0).starts_with('3'),
        "row was {:?}",
        screen.row(0)
    );
}

#[test]
fn scene_reconfigure_with_invalid_json_returns_false() {
    let mut scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    assert!(!scene.reconfigure("{not json"));
}

#[test]
fn scene_reconfigure_with_valid_json_returns_true() {
    let mut scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    assert!(scene.reconfigure("{}"));
}

#[test]
fn scene_location_accepts_latitude_and_longitude_boundaries() {
    let mut scene = FakeScene::new::<BlankRenderer>("demo", 4, 2).with_location(90.0, 180.0);
    assert!(scene.reconfigure("{}"));
    let mut southwest = FakeScene::new::<BlankRenderer>("demo", 4, 2).with_location(-90.0, -180.0);
    assert!(southwest.reconfigure("{}"));
}

#[test]
fn scene_frames_per_second_comes_from_renderer() {
    let scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    assert_eq!(scene.frames_per_second(), 30);
}

#[test]
fn scene_flags_default_to_off_for_plain_renderer() {
    let scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    assert!(!scene.wants_pointer());
    assert!(!scene.wants_occupancy());
    assert!(!scene.uses_cell_colors());
}

#[test]
fn scene_status_is_none_before_first_frame() {
    let scene = FakeScene::new::<BlankRenderer>("demo", 4, 2);
    assert!(scene.status().is_none());
}

// ---------------------------------------------------------------------------
// fuzz.rs
// ---------------------------------------------------------------------------

#[test]
fn fuzz_same_seed_gives_identical_events() {
    let first = ilium_plugin_testkit::fuzz::fuzz_events(7, 20);
    let second = ilium_plugin_testkit::fuzz::fuzz_events(7, 20);
    assert_eq!(first, second);
}

#[test]
fn fuzz_zero_count_gives_no_events() {
    assert!(ilium_plugin_testkit::fuzz::fuzz_events(1, 0).is_empty());
}

#[test]
fn fuzz_different_seeds_give_different_events() {
    let first = ilium_plugin_testkit::fuzz::fuzz_events(1, 20);
    let second = ilium_plugin_testkit::fuzz::fuzz_events(2, 20);
    assert_ne!(first, second);
}

#[test]
fn fuzz_arbitrary_event_is_deterministic_for_one_rng_seed() {
    let mut first_rng = ChaCha8Rng::seed_from_u64(3);
    let mut second_rng = ChaCha8Rng::seed_from_u64(3);
    let first = ilium_plugin_testkit::fuzz::arbitrary_event(&mut first_rng);
    let second = ilium_plugin_testkit::fuzz::arbitrary_event(&mut second_rng);
    assert_eq!(first, second);
}

#[test]
fn fuzz_run_with_idle_plugin_completes_without_failure() {
    let result = ilium_plugin_testkit::fuzz::fuzz_run::<IdlePlugin>(1, 5);
    assert!(result.is_ok(), "idle plugin must survive fuzzing");
}

// ---------------------------------------------------------------------------
// golden.rs
// ---------------------------------------------------------------------------

#[test]
fn golden_missing_file_reports_golden_missing() {
    let path = scratch_path("golden_missing.txt");
    let _ = std::fs::remove_file(&path);
    let text = path.to_string_lossy().into_owned();
    with_env(
        &[("ILIUM_PLUGIN_UPDATE_GOLDEN", None), ("CI", None)],
        || {
            assert_panic_mentions(codes::GOLDEN_MISSING, || {
                ilium_plugin_testkit::golden::assert_golden(&text, "actual");
            });
        },
    );
}

#[test]
fn golden_update_env_writes_the_file_with_lf_endings() {
    let path = scratch_path("golden_update.txt");
    let text = path.to_string_lossy().into_owned();
    with_env(
        &[("ILIUM_PLUGIN_UPDATE_GOLDEN", Some("1")), ("CI", None)],
        || {
            ilium_plugin_testkit::golden::assert_golden(&text, "alpha\r\nbeta");
        },
    );
    let written = std::fs::read_to_string(&path).expect("update must write the golden file");
    assert!(written.contains("alpha\nbeta"), "{written:?}");
    assert!(!written.contains('\r'), "{written:?}");
}

#[test]
fn golden_update_in_ci_reports_golden_update_in_ci() {
    let path = scratch_path("golden_ci.txt");
    let text = path.to_string_lossy().into_owned();
    with_env(
        &[
            ("ILIUM_PLUGIN_UPDATE_GOLDEN", Some("1")),
            ("CI", Some("true")),
        ],
        || {
            assert_panic_mentions(codes::GOLDEN_UPDATE_IN_CI, || {
                ilium_plugin_testkit::golden::assert_golden(&text, "anything");
            });
        },
    );
}

#[test]
fn golden_mismatch_fails_the_assertion() {
    let path = write_scratch("golden_mismatch.txt", "alpha\n");
    let text = path.to_string_lossy().into_owned();
    let failed = with_env(
        &[("ILIUM_PLUGIN_UPDATE_GOLDEN", None), ("CI", None)],
        || panic_message(|| ilium_plugin_testkit::golden::assert_golden(&text, "beta\n")).is_some(),
    );
    assert!(failed, "a differing actual value must fail the assertion");
}

#[test]
fn golden_snapshot_without_stored_file_reports_golden_missing() {
    with_env(
        &[("ILIUM_PLUGIN_UPDATE_GOLDEN", None), ("CI", None)],
        || {
            assert_panic_mentions(codes::GOLDEN_MISSING, || {
                ilium_plugin_testkit::golden::assert_snapshot(
                    "unit-gap-missing-snapshot",
                    &json!({"a": 1}),
                );
            });
        },
    );
}

// ---------------------------------------------------------------------------
// replay.rs
// ---------------------------------------------------------------------------

#[test]
fn replay_recorded_log_replays_without_mismatch() {
    let path = scratch_path("replay_round_trip.jsonl");
    let mut recorder = FakeHost::new();
    recorder.record_to(&path);
    let mut replayer = FakeHost::new();
    match replayer.replay(&path) {
        Ok(()) => {}
        Err(mismatch) => panic!("a fresh recording must replay cleanly: {mismatch}"),
    }
}

#[test]
fn replay_unsupported_version_reports_version_unsupported() {
    let path = write_scratch("replay_version.jsonl", &format!("{}\n", replay_header(2)));
    let mut host = FakeHost::new();
    let mismatch = match host.replay(&path) {
        Ok(()) => panic!("a version 2 log must not replay"),
        Err(mismatch) => mismatch,
    };
    assert!(
        mismatch
            .to_string()
            .contains(codes::REPLAY_VERSION_UNSUPPORTED),
        "{mismatch}"
    );
}

#[test]
fn replay_tampered_event_reports_replay_diverged_on_its_line() {
    let body = format!(
        "{}\n{{\"type\":\"event\",\"tampered\":true}}\n",
        replay_header(1)
    );
    let path = write_scratch("replay_diverged.jsonl", &body);
    let mut host = FakeHost::new();
    let mismatch = match host.replay(&path) {
        Ok(()) => panic!("a tampered event must diverge"),
        Err(mismatch) => mismatch,
    };
    assert_eq!(mismatch.line, 2);
    assert!(
        mismatch.to_string().contains(codes::REPLAY_DIVERGED),
        "{mismatch}"
    );
}

// ---------------------------------------------------------------------------
// coverage.rs
// ---------------------------------------------------------------------------

#[test]
fn coverage_minimum_zero_accepts_zero_percent() {
    let result = ilium_plugin_testkit::coverage::check_minimum(&coverage_summary(0.0), 0.0);
    assert!(result.is_ok());
}

#[test]
fn coverage_minimum_hundred_accepts_full_coverage() {
    let result = ilium_plugin_testkit::coverage::check_minimum(&coverage_summary(100.0), 100.0);
    assert!(result.is_ok());
}

#[test]
fn coverage_below_minimum_returns_documented_message() {
    let result = ilium_plugin_testkit::coverage::check_minimum(&coverage_summary(71.2), 80.0);
    assert_eq!(
        result,
        Err(String::from("coverage 71.2 percent is below 80"))
    );
}

#[test]
fn coverage_collect_without_tool_reports_coverage_tool_missing() {
    let project = scratch_path("coverage_project");
    let _ = std::fs::create_dir_all(&project);
    let code = with_env(&[("PATH", Some(""))], || {
        harness_code(ilium_plugin_testkit::coverage::collect(&project))
    });
    assert_eq!(code, codes::COVERAGE_TOOL_MISSING);
}

// ---------------------------------------------------------------------------
// fixture.rs
// ---------------------------------------------------------------------------

#[test]
fn fixture_load_valid_document_keeps_schema_version() {
    let path = write_scratch("fixture_valid.json", r#"{"schema": 1, "tree": []}"#);
    let fixture = harness_ok(Fixture::load(&path));
    assert_eq!(fixture.document["schema"], json!(1));
}

#[test]
fn fixture_unknown_key_reports_fixture_unknown_key() {
    let path = write_scratch(
        "fixture_unknown_key.json",
        r#"{"schema": 1, "bogus": true}"#,
    );
    let outcome = fixture_outcome_text(&path);
    assert!(outcome.contains(codes::FIXTURE_UNKNOWN_KEY), "{outcome}");
}

#[test]
fn fixture_duplicate_id_reports_fixture_duplicate_id() {
    let text = r#"{"schema":1,"tree":[
        {"id":1,"parent":null,"name":"one","class":"container","content_kind":"ilium.terminal"},
        {"id":1,"parent":null,"name":"two","class":"container","content_kind":"ilium.terminal"}
    ]}"#;
    let path = write_scratch("fixture_duplicate.json", text);
    let outcome = fixture_outcome_text(&path);
    assert!(outcome.contains(codes::FIXTURE_DUPLICATE_ID), "{outcome}");
}

#[test]
fn fixture_bad_value_names_the_json_path() {
    let text = r#"{"schema":1,"tree":[
        {"id":1,"parent":"x","name":"one","class":"container","content_kind":"ilium.terminal"}
    ]}"#;
    let path = write_scratch("fixture_bad_value.json", text);
    let outcome = fixture_outcome_text(&path);
    assert!(outcome.contains(codes::FIXTURE_BAD_VALUE), "{outcome}");
    assert!(outcome.contains("$.tree[0].parent"), "{outcome}");
}

#[test]
fn fixture_empty_file_does_not_pass() {
    let path = write_scratch("fixture_empty.json", "");
    assert_ne!(fixture_outcome_text(&path), "pass");
}

// ---------------------------------------------------------------------------
// core_stubs.rs
// ---------------------------------------------------------------------------

#[test]
fn core_stubs_table_lists_thirty_stubs() {
    assert_eq!(ilium_plugin_testkit::core_stubs::table().len(), 30);
}

#[test]
fn core_stubs_find_existing_name_returns_it() {
    let found = ilium_plugin_testkit::core_stubs::find("ilium.core.tree").map(|stub| stub.name);
    assert_eq!(found, Some(String::from("ilium.core.tree")));
}

#[test]
fn core_stubs_find_unknown_name_returns_none() {
    assert!(ilium_plugin_testkit::core_stubs::find("ilium.core.no-such-stub").is_none());
}

#[test]
fn core_stubs_find_empty_name_returns_none() {
    assert!(ilium_plugin_testkit::core_stubs::find("").is_none());
}

// ---------------------------------------------------------------------------
// conformance.rs
// ---------------------------------------------------------------------------

#[test]
fn conformance_passing_plugin_has_every_case_pass() {
    let cases = harness_ok(ilium_plugin_testkit::conformance::run_suite::<IdlePlugin>());
    assert!(!cases.is_empty());
    assert!(
        cases.iter().all(|case| case.passed),
        "every conformance case must pass"
    );
}

#[test]
fn conformance_failing_case_is_reported_by_name_with_reason() {
    let cases = harness_ok(ilium_plugin_testkit::conformance::run_suite::<FailingPlugin>());
    assert!(cases
        .iter()
        .any(|case| !case.passed && !case.name.is_empty() && !case.reason.is_empty()));
}

// ---------------------------------------------------------------------------
// manual.rs
// ---------------------------------------------------------------------------

#[test]
fn manual_env_var_overrides_default_pages_dir() {
    let dir = with_env(
        &[("ILIUM_PLUGIN_MANUAL_DIR", Some("/srv/manual/pages"))],
        ilium_plugin_testkit::manual::pages_dir,
    );
    assert_eq!(dir, PathBuf::from("/srv/manual/pages"));
}

#[test]
fn manual_unset_env_var_uses_repository_default() {
    let dir = with_env(&[("ILIUM_PLUGIN_MANUAL_DIR", None)], || {
        ilium_plugin_testkit::manual::pages_dir()
    });
    assert!(
        dir.ends_with(Path::new("docs/plugins-manual/pages")),
        "{dir:?}"
    );
}

#[test]
fn manual_empty_env_var_falls_back_to_repository_default() {
    let dir = with_env(&[("ILIUM_PLUGIN_MANUAL_DIR", Some(""))], || {
        ilium_plugin_testkit::manual::pages_dir()
    });
    assert!(
        dir.ends_with(Path::new("docs/plugins-manual/pages")),
        "{dir:?}"
    );
}
