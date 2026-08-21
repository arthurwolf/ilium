//! Reproducible, opt-in microbenchmarks for the client event-loop hot paths.
//!
//! These stay as ignored tests so ordinary correctness runs remain fast. The
//! process-level tmux workloads remain the source of truth for end-to-end CPU;
//! these measurements attribute that cost to private rendering/cache stages.

use std::hint::black_box;
use std::time::Instant;

use ilium_core::{AgentActivity, AgentClass, NodeId, PaneContentKind, PaneStatus, ROOT_ID};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::StatefulWidget;
use ratatui::Terminal;
use tempfile::TempDir;
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};

use crate::app::{App, PaneRuntime};
use crate::config::{SidebarDensity, TreeOrder};
use crate::terminal_view::TerminalView;

/// Enough independent samples to report a median without making an ignored
/// development benchmark needlessly slow on the user's interactive machine.
const SAMPLE_COUNT: usize = 7;

/// Runs one warmed operation in fixed-size batches and prints its median.
///
/// Fixed iteration counts keep before/after runs directly comparable. The
/// caller chooses a count large enough that clock resolution is negligible.
fn measure_median_nanoseconds(
    label: &str,
    iterations_per_sample: usize,
    mut operation: impl FnMut(),
) {
    // One unmeasured call pays lazy initialization costs that the steady TUI
    // does not pay on every frame.
    operation();

    let mut sample_nanoseconds = Vec::with_capacity(SAMPLE_COUNT);
    for _sample_number in 0..SAMPLE_COUNT {
        let started_at = Instant::now();
        for _iteration_number in 0..iterations_per_sample {
            operation();
        }
        let elapsed_nanoseconds = started_at.elapsed().as_nanos();
        sample_nanoseconds.push(elapsed_nanoseconds / iterations_per_sample as u128);
    }

    sample_nanoseconds.sort_unstable();
    let median_nanoseconds = sample_nanoseconds[SAMPLE_COUNT / 2];
    println!("PERF {label} median_ns={median_nanoseconds} samples_ns={sample_nanoseconds:?}");
}

/// Owns the temporary project because the tree stores its canonical path for
/// chatroom/project rendering during every measured frame.
struct ManyPaneFixture {
    _project_directory: TempDir,
    app: App,
    terminal: Terminal<TestBackend>,
}

/// Builds the same shape as the controlled process benchmark: a 266x68 client
/// with 49 working Codex panes and one real terminal viewport selected.
fn many_pane_fixture() -> ManyPaneFixture {
    let project_directory = tempfile::tempdir().expect("create benchmark project");
    let project_path = project_directory.path().to_path_buf();
    let mut app = App::new("performance".to_string(), project_path.clone());
    let project_id = app
        .tree
        .add_project(project_path)
        .expect("add benchmark project");
    let group_id = app
        .tree
        .add_group(project_id, "default")
        .expect("add benchmark group");
    let mut first_pane_id = None;

    for pane_number in 0..49 {
        let pane_id = app
            .tree
            .add_pane(
                group_id,
                format!("Codex performance pane {pane_number:02}"),
                PaneContentKind::Terminal,
            )
            .expect("add benchmark pane");
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::AgentWithGoal(AgentClass::Codex, AgentActivity::Working),
            )
            .expect("mark benchmark pane working");

        let mut terminal_view = TerminalView::new(64, 200);
        for line_number in 0..64 {
            terminal_view.feed(
                format!(
                    "\x1b[3{}mCogitating task {pane_number:02}, line {line_number:02}\x1b[0m\r\n",
                    line_number % 8
                )
                .as_bytes(),
            );
        }
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(terminal_view)));
        first_pane_id.get_or_insert(pane_id);
    }

    app.set_screen_area(Rect::new(0, 0, 266, 68));
    app.focus_pane(first_pane_id.expect("benchmark has a pane"));
    let terminal = Terminal::new(TestBackend::new(266, 68)).expect("create benchmark terminal");

    ManyPaneFixture {
        _project_directory: project_directory,
        app,
        terminal,
    }
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_full_ui_draw_with_49_working_panes() {
    let mut fixture = many_pane_fixture();

    measure_median_nanoseconds("client.full_ui_draw_49_working_266x68", 100, || {
        fixture
            .terminal
            .draw(|frame| crate::ui::draw(frame, &mut fixture.app))
            .expect("draw benchmark frame");
        black_box(fixture.terminal.backend().buffer());
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_chatroom_reconciliation_without_a_room() {
    let mut fixture = many_pane_fixture();
    let now = Instant::now();
    fixture.app.tick_chatroom_projects(now);

    measure_median_nanoseconds("client.reconcile_chatroom_no_room", 2_000, || {
        black_box(fixture.app.tick_chatroom_projects(now));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_chatroom_reconciliation_with_a_room() {
    let project_directory = tempfile::tempdir().expect("create chatroom benchmark project");
    crate::chatroom::initialize(project_directory.path()).expect("initialize benchmark chatroom");
    let mut app = App::new(
        "performance-chatroom".to_owned(),
        project_directory.path().to_path_buf(),
    );
    app.tree
        .add_project(project_directory.path().to_path_buf())
        .expect("add chatroom benchmark project");
    let now = Instant::now();
    app.tick_chatroom_projects(now);

    measure_median_nanoseconds("client.reconcile_chatroom_existing_room", 100, || {
        black_box(app.tick_chatroom_projects(now));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_many_pane_animation_scheduler() {
    let fixture = many_pane_fixture();

    measure_median_nanoseconds("client.animation_scheduler_49_working", 10_000, || {
        let now = Instant::now();
        black_box(fixture.app.maintenance_schedule(now));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_unchanged_client_surface_recording() {
    let fixture = many_pane_fixture();
    let mut last_recorded_surface = None;
    crate::record_client_surface_change(&fixture.app, &mut last_recorded_surface);

    measure_median_nanoseconds("client.unchanged_surface_recording", 100_000, || {
        crate::record_client_surface_change(&fixture.app, &mut last_recorded_surface);
        black_box(&last_recorded_surface);
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_hidden_agent_output_without_frame_damage() {
    let mut fixture = many_pane_fixture();
    // `Tree::panes()` walks a `HashMap` in unspecified, per-process-random
    // order, so it cannot reliably name "the pane that isn't focused" --
    // `pane_ids_in_tree_order()` is deterministic insertion order, and index
    // 0 is `first_pane_id` (the one `many_pane_fixture` focused), so index 1
    // is guaranteed to be a genuinely hidden pane on every run.
    let hidden_pane_id = *fixture
        .app
        .tree
        .pane_ids_in_tree_order()
        .get(1)
        .expect("benchmark has a hidden pane");
    let mut sequence = 0_u64;

    measure_median_nanoseconds("client.hidden_agent_output_no_draw", 500, || {
        sequence = sequence.saturating_add(1);
        let mut pending_update = Some((
            hidden_pane_id,
            sequence,
            sequence,
            b"\x1b[Hhidden agent output".to_vec(),
        ));
        black_box(crate::flush_pending_screen_update(
            &mut fixture.app,
            &mut pending_update,
        ));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_tree_flatten_reuse() {
    let pane_items = (0_u64..49)
        .map(|pane_id| TreeItem::new_leaf(pane_id + 2, "working pane"))
        .collect::<Vec<_>>();
    let group_item = TreeItem::new(1, "default", pane_items).expect("unique pane identifiers");
    let items = vec![group_item];
    let mut state = TreeState::default();
    state.open(vec![1]);
    let area = Rect::new(0, 0, 64, 52);
    let mut buffer = Buffer::empty(area);
    TreeWidget::new(&items)
        .expect("unique top-level identifiers")
        .render(area, &mut buffer, &mut state);

    measure_median_nanoseconds("client.tree_second_flatten_baseline", 10_000, || {
        black_box(state.flatten(&items));
    });
    measure_median_nanoseconds("client.tree_rendered_identifiers_reuse", 100_000, || {
        black_box(state.visible_identifiers());
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_tree_ordering_and_visible_ids() {
    let fixture = many_pane_fixture();
    let project_id = fixture.app.tree.children_of(ROOT_ID).unwrap()[0];
    let group_id = fixture.app.tree.children_of(project_id).unwrap()[0];

    measure_median_nanoseconds("client.tree_manual_order_49", 20_000, || {
        black_box(crate::tree_ordering::ordered_children(
            &fixture.app.tree,
            group_id,
            TreeOrder::Manual,
        ));
    });
    measure_median_nanoseconds("client.tree_name_order_49", 2_000, || {
        black_box(crate::tree_ordering::ordered_children(
            &fixture.app.tree,
            group_id,
            TreeOrder::NameAscending,
        ));
    });
    measure_median_nanoseconds("client.visible_tree_node_ids_49", 1_000, || {
        black_box(crate::tree_ui::visible_tree_node_ids(
            &fixture.app.tree,
            &fixture.app.tree_state,
            TreeOrder::Manual,
        ));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_sidebar_density_padding() {
    measure_median_nanoseconds("client.sidebar_density_padding", 100_000, || {
        black_box(crate::tree_ui::apply_sidebar_density(
            Line::raw("working pane"),
            SidebarDensity::Comfortable,
        ));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_tree_path_and_rendered_row_metadata_reuse() {
    let child_ids = (0_u64..49).map(NodeId).collect::<Vec<_>>();
    let ancestor_path = vec![NodeId(500), NodeId(501)];

    measure_median_nanoseconds("client.tree_path_per_child_clone", 10_000, || {
        for child_id in &child_ids {
            let mut path = ancestor_path.clone();
            path.push(*child_id);
            black_box(path);
        }
    });
    measure_median_nanoseconds("client.tree_path_push_pop_reuse", 10_000, || {
        let mut path = ancestor_path.clone();
        for child_id in &child_ids {
            path.push(*child_id);
            black_box(&path);
            path.pop();
        }
    });

    let identifier_paths = child_ids
        .iter()
        .map(|child_id| vec![NodeId(500), NodeId(501), *child_id])
        .collect::<Vec<_>>();
    measure_median_nanoseconds("client.rendered_row_cloned_paths", 10_000, || {
        black_box(
            identifier_paths
                .iter()
                .enumerate()
                .map(|(row, path)| (row as u16, path.clone()))
                .collect::<Vec<_>>(),
        );
    });
    measure_median_nanoseconds("client.rendered_row_indices", 10_000, || {
        black_box(
            identifier_paths
                .iter()
                .enumerate()
                .map(|(row, _path)| (row as u16, row))
                .collect::<Vec<_>>(),
        );
    });

    measure_median_nanoseconds("client.flatten_growth_from_one", 10_000, || {
        let mut rows = Vec::with_capacity(1);
        for child_id in &child_ids {
            rows.push(*child_id);
        }
        black_box(rows);
    });
    measure_median_nanoseconds("client.flatten_subtree_reserve", 10_000, || {
        let mut rows = Vec::with_capacity(1);
        rows.reserve(child_ids.len());
        for child_id in &child_ids {
            rows.push(*child_id);
        }
        black_box(rows);
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_full_history_append() {
    let mut terminal_view = TerminalView::with_scrollback_budget_mib(24, 80, 1);
    terminal_view.append_history_for_benchmark(&vec![b'x'; 1024 * 1024]);
    let chunk = vec![b'y'; 1024];

    measure_median_nanoseconds("client.full_history_append_1k", 1_000, || {
        terminal_view.append_history_for_benchmark(&chunk);
        black_box(terminal_view.searchable_history_snapshot().len());
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_append_while_search_snapshot_is_alive() {
    let retained = vec![b'x'; 1024 * 1024];
    let chunk = vec![b'y'; 1024];
    let mut sample_nanoseconds = Vec::with_capacity(SAMPLE_COUNT);

    for _sample_number in 0..SAMPLE_COUNT {
        let mut terminal_view = TerminalView::with_scrollback_budget_mib(24, 80, 1);
        terminal_view.append_history_for_benchmark(&retained);
        let snapshot = terminal_view.searchable_history_snapshot();
        let started_at = Instant::now();
        terminal_view.append_history_for_benchmark(&chunk);
        sample_nanoseconds.push(started_at.elapsed().as_nanos());
        black_box(snapshot);
    }

    sample_nanoseconds.sort_unstable();
    println!(
        "PERF client.concurrent_search_history_append median_ns={} samples_ns={sample_nanoseconds:?}",
        sample_nanoseconds[SAMPLE_COUNT / 2]
    );
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_byte_fragmented_osc8_link() {
    let mut terminal_view = TerminalView::new(24, 80);
    let sequence =
        b"noise-before-link\x1b]8;;https://example.test/docs\x1b\\documentation\x1b]8;;\x1b\\";

    measure_median_nanoseconds("client.osc8_byte_fragmented_link", 1_000, || {
        for byte in sequence {
            terminal_view.observe_osc8_for_benchmark(std::slice::from_ref(byte));
        }
        black_box(terminal_view.osc8_link_at("documentation", 3));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_agent_terminal_output_application() {
    let mut terminal_view = TerminalView::new(64, 200);
    let output = b"\x1b[HCogitating (esc to interrupt)\r\ngpt-5.6-sol xhigh - Working";
    let mut sequence = 0_u64;

    measure_median_nanoseconds("client.agent_terminal_output", 500, || {
        let first_sequence = sequence.saturating_add(1);
        sequence = first_sequence;
        black_box(terminal_view.apply_live_output(first_sequence, sequence, output, false));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_plain_terminal_output_application() {
    let mut terminal_view = TerminalView::new(64, 200);
    let output = b"\x1b[Hshell output changed";
    let mut sequence = 0_u64;

    measure_median_nanoseconds("client.plain_terminal_output", 200, || {
        let first_sequence = sequence.saturating_add(1);
        sequence = first_sequence;
        black_box(terminal_view.apply_live_output(first_sequence, sequence, output, true));
    });
}

#[test]
#[ignore = "manual performance benchmark"]
fn benchmark_displayed_pane_subscription_snapshot() {
    let fixture = many_pane_fixture();
    measure_median_nanoseconds("client.displayed_panes_vec", 100_000, || {
        black_box(fixture.app.displayed_pane_ids());
    });
    measure_median_nanoseconds("client.displayed_panes_inline", 100_000, || {
        black_box(fixture.app.displayed_pane_slots());
    });
}
