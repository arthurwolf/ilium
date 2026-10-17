//! Editor primitive (primitives-editor.md, primitives-editor-viewport.md): buffers, lines,
//! decorations, gutter, minimap, viewport, edit batches, annotator rate. Requirement ids
//! PRI-020..024 and the 13 WIT functions of `ilium:primitives/editor`.

use ilium_plugin_api::generated::primitives::editor::{
    GutterMark, HostEditor, MinimapMark, RangeDecoration,
};
use ilium_plugin_api::generated::types::editing::{TextEdit, TextPosition, TextRange};
use ilium_plugin_api::generated::types::ids::{BufferId, RequestId};
use ilium_plugin_api::generated::types::intents::{EditorApplyEditsSpec, Intent};
use ilium_plugin_api::generated::types::paths::FsPath;
use ilium_plugin_api::generated::types::text::{Color, ColorRole, Style};
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use crate::common::*;

const READ: &[&str] = &["editor.read"];
const EDIT: &[&str] = &["editor.read", "editor.edit", "fs.read", "fs.write"];

fn text_of(lines: usize) -> String {
    (0..lines).map(|index| format!("line {index}\n")).collect()
}

fn open_buffer(rig: &Rig, lines: usize) -> BufferId {
    rig.host
        .editor()
        .create_buffer(text_of(lines), None)
        .expect("buffer")
}

fn decoration(line: u32) -> RangeDecoration {
    RangeDecoration {
        range: TextRange {
            start: TextPosition { line, column: 0 },
            end: TextPosition { line, column: 3 },
        },
        style: Style::default(),
        hover: None,
    }
}

fn mark(line: u32) -> GutterMark {
    GutterMark {
        line,
        icon: "alert-circle".to_owned(),
        tooltip: None,
        command: None,
    }
}

fn edit(line: u32) -> TextEdit {
    TextEdit {
        range: TextRange {
            start: TextPosition { line, column: 0 },
            end: TextPosition { line, column: 0 },
        },
        replacement: "x".to_owned(),
    }
}

// ---------------------------------------------------------------- PRI-020

/// REQ: PRI-020, WIT:ilium:primitives/editor.read-lines
#[test]
fn pri_020_read_10000_lines_accepted() {
    let rig = rig(READ);
    let buffer = open_buffer(&rig, 12_000);
    let lines = rig
        .host
        .editor()
        .read_lines(buffer, 0, 10_000)
        .expect("lines");
    assert_eq!(lines.len(), 10_000);
}

/// REQ: PRI-020
#[test]
fn pri_020_read_9999_lines_accepted() {
    let rig = rig(READ);
    let buffer = open_buffer(&rig, 12_000);
    assert_eq!(
        rig.host
            .editor()
            .read_lines(buffer, 0, 9_999)
            .expect("lines")
            .len(),
        9_999
    );
}

/// REQ: PRI-020
#[test]
fn pri_020_read_10001_lines_invalid_argument() {
    let rig = rig(READ);
    let buffer = open_buffer(&rig, 12_000);
    assert_eq!(
        error_of(rig.host.editor().read_lines(buffer, 0, 10_001)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-020, WIT:ilium:primitives/editor.read-lines
#[test]
fn pri_020_count_past_the_last_line_returns_the_lines_that_exist() {
    let rig = rig(READ);
    let buffer = open_buffer(&rig, 5);
    assert_eq!(
        rig.host
            .editor()
            .read_lines(buffer, 3, 100)
            .expect("lines")
            .len(),
        2
    );
}

/// REQ: WIT:ilium:primitives/editor.read-lines
#[test]
fn wit_editor_read_lines_first_line_past_the_end_invalid_argument() {
    let rig = rig(READ);
    let buffer = open_buffer(&rig, 5);
    assert_eq!(
        error_of(rig.host.editor().read_lines(buffer, 6, 1)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/editor.read-lines
#[test]
fn wit_editor_read_lines_unknown_buffer_not_found() {
    let rig = rig(READ);
    assert_eq!(
        error_of(rig.host.editor().read_lines(BufferId(9_999), 0, 1)),
        Some(ErrorCode::NotFound)
    );
}

/// REQ: WIT:ilium:primitives/editor.read-lines
#[test]
fn wit_editor_read_lines_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert_eq!(
        error_of(rig.host.editor().read_lines(BufferId(1), 0, 1)),
        Some(ErrorCode::NotPermitted)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    // REQ: PRI-020
    #[test]
    fn pri_020_read_lines_count_valid_up_to_10000(count in 1u32..12_000) {
        let rig = rig(READ);
        let buffer = open_buffer(&rig, 11_000);
        prop_assert_eq!(rig.host.editor().read_lines(buffer, 0, count).is_ok(), count <= 10_000);
    }
}

// ---------------------------------------------------------------- PRI-021

/// REQ: PRI-021, WIT:ilium:primitives/editor.create-buffer
#[test]
fn pri_021_64_buffers_accepted() {
    let rig = rig(EDIT);
    for _ in 0..64 {
        rig.host
            .editor()
            .create_buffer(String::new(), None)
            .expect("buffer");
    }
}

/// REQ: PRI-021
#[test]
fn pri_021_65th_buffer_quota_exceeded() {
    let rig = rig(EDIT);
    for _ in 0..64 {
        rig.host
            .editor()
            .create_buffer(String::new(), None)
            .expect("buffer");
    }
    assert_eq!(
        error_of(rig.host.editor().create_buffer(String::new(), None)),
        Some(ErrorCode::QuotaExceeded)
    );
}

/// REQ: PRI-021, WIT:ilium:primitives/editor.close-buffer
#[test]
fn pri_021_closing_a_buffer_frees_its_slot() {
    let rig = rig(EDIT);
    let first = rig
        .host
        .editor()
        .create_buffer(String::new(), None)
        .expect("buffer");
    for _ in 0..63 {
        rig.host
            .editor()
            .create_buffer(String::new(), None)
            .expect("buffer");
    }
    rig.host.editor().close_buffer(first);
    assert!(rig.host.editor().create_buffer(String::new(), None).is_ok());
}

/// REQ: PRI-021
#[test]
fn pri_021_text_of_16_mib_accepted() {
    let rig = rig(EDIT);
    assert!(rig
        .host
        .editor()
        .create_buffer("a".repeat(16 * MIB), None)
        .is_ok());
}

/// REQ: PRI-021
#[test]
fn pri_021_text_of_16_mib_plus_one_invalid_argument() {
    let rig = rig(EDIT);
    let result = rig
        .host
        .editor()
        .create_buffer("a".repeat(16 * MIB + 1), None);
    assert_eq!(error_of(result), Some(ErrorCode::InvalidArgument));
}

/// REQ: WIT:ilium:primitives/editor.create-buffer
#[test]
fn wit_editor_create_buffer_without_grant_not_permitted() {
    let rig = rig(READ);
    assert_eq!(
        error_of(rig.host.editor().create_buffer(String::new(), None)),
        Some(ErrorCode::NotPermitted)
    );
}

/// REQ: WIT:ilium:primitives/editor.close-buffer
#[test]
fn wit_editor_close_buffer_makes_the_id_unknown() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 2);
    rig.host.editor().close_buffer(buffer);
    assert_eq!(
        error_of(rig.host.editor().read_lines(buffer, 0, 1)),
        Some(ErrorCode::NotFound)
    );
}

// ---------------------------------------------------------------- PRI-022

fn apply(
    rig: &mut Rig,
    buffer: BufferId,
    base_revision: u64,
    edits: Vec<TextEdit>,
) -> ilium_plugin_api::generated::types::requests::Outcome {
    rig.submit(Intent::EditorApplyEdits(EditorApplyEditsSpec {
        buffer,
        base_revision,
        edits,
    }))
}

fn revision_of(rig: &Rig, buffer: BufferId) -> u64 {
    rig.host
        .editor()
        .buffers()
        .into_iter()
        .find(|b| b.buffer == buffer)
        .expect("buffer listed")
        .revision
}

/// REQ: PRI-022
#[test]
fn pri_022_10000_edits_accepted() {
    let mut rig = rig(EDIT);
    let buffer = open_buffer(&rig, 10_001);
    let revision = revision_of(&rig, buffer);
    let edits = (0..10_000).map(edit).collect();
    assert!(accepted(&apply(&mut rig, buffer, revision, edits)));
}

/// REQ: PRI-022
#[test]
fn pri_022_10001_edits_refused_invalid_argument_and_buffer_unchanged() {
    let mut rig = rig(EDIT);
    let buffer = open_buffer(&rig, 10_002);
    let revision = revision_of(&rig, buffer);
    let before = rig
        .host
        .editor()
        .read_lines(buffer, 0, 10_000)
        .expect("lines");
    let edits = (0..10_001).map(edit).collect();
    let outcome = apply(&mut rig, buffer, revision, edits);
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::InvalidArgument));
    assert_eq!(
        rig.host
            .editor()
            .read_lines(buffer, 0, 10_000)
            .expect("lines"),
        before
    );
    assert_eq!(revision_of(&rig, buffer), revision);
}

/// REQ: PRI-022
#[test]
fn pri_022_stale_base_revision_refused_conflict() {
    let mut rig = rig(EDIT);
    let buffer = open_buffer(&rig, 3);
    let revision = revision_of(&rig, buffer);
    let outcome = apply(&mut rig, buffer, revision + 5, vec![edit(0)]);
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::Conflict));
}

/// REQ: PRI-022
#[test]
fn pri_022_out_of_range_edit_refused_invalid_argument() {
    let mut rig = rig(EDIT);
    let buffer = open_buffer(&rig, 3);
    let revision = revision_of(&rig, buffer);
    let outcome = apply(&mut rig, buffer, revision, vec![edit(50)]);
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::InvalidArgument));
}

/// REQ: PRI-022
#[test]
fn pri_022_apply_without_grant_refused_not_permitted() {
    let mut rig = rig(READ);
    let outcome = apply(&mut rig, BufferId(1), 0, vec![edit(0)]);
    assert_eq!(refusal_of(&outcome), Some(ErrorCode::NotPermitted));
}

// ---------------------------------------------------------------- PRI-023

/// REQ: PRI-023
#[test]
fn pri_023_20_edits_in_100_ms_call_the_annotator_once() {
    let wasm = fixture_wasm("annotator-counter");
    let mut rig = rig_with_wasm(&wasm, EDIT);
    let buffer = open_buffer(&rig, 40);
    for index in 0..20 {
        rig.kernel.edit_buffer(buffer, index);
        advance_ms(&mut rig.host, 4);
        rig.host.pump(2);
    }
    assert_eq!(rig.kernel.annotator_calls(buffer), 1);
}

/// REQ: PRI-023
#[test]
fn pri_023_an_edit_after_100_ms_calls_the_annotator_again() {
    let wasm = fixture_wasm("annotator-counter");
    let mut rig = rig_with_wasm(&wasm, EDIT);
    let buffer = open_buffer(&rig, 40);
    rig.kernel.edit_buffer(buffer, 0);
    rig.host.pump(2);
    advance_ms(&mut rig.host, 100);
    rig.kernel.edit_buffer(buffer, 1);
    rig.host.pump(2);
    assert_eq!(rig.kernel.annotator_calls(buffer), 2);
}

/// REQ: PRI-023
#[test]
fn pri_023_an_edit_at_99_ms_makes_no_second_call() {
    let wasm = fixture_wasm("annotator-counter");
    let mut rig = rig_with_wasm(&wasm, EDIT);
    let buffer = open_buffer(&rig, 40);
    rig.kernel.edit_buffer(buffer, 0);
    rig.host.pump(2);
    advance_ms(&mut rig.host, 99);
    rig.kernel.edit_buffer(buffer, 1);
    rig.host.pump(2);
    assert_eq!(rig.kernel.annotator_calls(buffer), 1);
}

/// REQ: PRI-023
#[test]
fn pri_023_the_rate_is_per_buffer() {
    let wasm = fixture_wasm("annotator-counter");
    let mut rig = rig_with_wasm(&wasm, EDIT);
    let first = open_buffer(&rig, 10);
    let second = open_buffer(&rig, 10);
    rig.kernel.edit_buffer(first, 0);
    rig.kernel.edit_buffer(second, 0);
    rig.host.pump(2);
    assert_eq!(rig.kernel.annotator_calls(first), 1);
    assert_eq!(rig.kernel.annotator_calls(second), 1);
}

// ---------------------------------------------------------------- PRI-024

/// REQ: PRI-024, WIT:ilium:primitives/editor.set-decorations
#[test]
fn pri_024_10000_decorations_in_a_layer_accepted() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    let items = (0..10_000).map(decoration).collect();
    assert!(rig
        .host
        .editor()
        .set_decorations(buffer, "layer".into(), items)
        .is_ok());
}

/// REQ: PRI-024
#[test]
fn pri_024_10001_decorations_in_a_layer_invalid_argument() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    let items = (0..10_001).map(decoration).collect();
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_decorations(buffer, "layer".into(), items)
        ),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-024, WIT:ilium:primitives/editor.set-gutter
#[test]
fn pri_024_10000_gutter_marks_in_a_layer_accepted() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    let marks = (0..10_000).map(mark).collect();
    assert!(rig
        .host
        .editor()
        .set_gutter(buffer, "layer".into(), marks)
        .is_ok());
}

/// REQ: PRI-024
#[test]
fn pri_024_10001_gutter_marks_in_a_layer_invalid_argument() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    let marks = (0..10_001).map(mark).collect();
    assert_eq!(
        error_of(rig.host.editor().set_gutter(buffer, "layer".into(), marks)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: PRI-024
#[test]
fn pri_024_session_total_of_65536_accepted_and_one_more_refused() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    for layer in 0..6 {
        let items = (0..10_000).map(decoration).collect();
        rig.host
            .editor()
            .set_decorations(buffer, format!("layer-{layer}"), items)
            .expect("layer");
    }
    let items = (0..5_536).map(decoration).collect();
    assert!(rig
        .host
        .editor()
        .set_decorations(buffer, "layer-6".into(), items)
        .is_ok());
    let one_more = rig
        .host
        .editor()
        .set_decorations(buffer, "layer-7".into(), vec![decoration(0)]);
    assert!(one_more.is_err(), "the 65,537th item must be refused");
}

/// REQ: PRI-024
#[test]
fn pri_024_replacing_a_layer_releases_its_share_of_the_total() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 20_000);
    for layer in 0..6 {
        let items = (0..10_000).map(decoration).collect();
        rig.host
            .editor()
            .set_decorations(buffer, format!("layer-{layer}"), items)
            .expect("layer");
    }
    rig.host
        .editor()
        .set_decorations(buffer, "layer-0".into(), Vec::new())
        .expect("clear one layer");
    let items = (0..10_000).map(decoration).collect();
    assert!(rig
        .host
        .editor()
        .set_decorations(buffer, "layer-new".into(), items)
        .is_ok());
}

/// REQ: WIT:ilium:primitives/editor.set-decorations
#[test]
fn wit_editor_set_decorations_unknown_buffer_not_found() {
    let rig = rig(EDIT);
    let result = rig
        .host
        .editor()
        .set_decorations(BufferId(404), "x".into(), vec![decoration(0)]);
    assert_eq!(error_of(result), Some(ErrorCode::NotFound));
}

/// REQ: WIT:ilium:primitives/editor.set-decorations
#[test]
fn wit_editor_set_decorations_without_grant_not_permitted() {
    let rig = rig(&[]);
    let result = rig
        .host
        .editor()
        .set_decorations(BufferId(1), "x".into(), Vec::new());
    assert_eq!(error_of(result), Some(ErrorCode::NotPermitted));
}

/// REQ: WIT:ilium:primitives/editor.set-gutter
#[test]
fn wit_editor_set_gutter_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_gutter(BufferId(1), "x".into(), Vec::new())
        ),
        Some(ErrorCode::NotPermitted)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    // REQ: PRI-024
    #[test]
    fn pri_024_a_layer_is_valid_iff_it_holds_at_most_10000_marks(count in 9_990usize..10_010) {
        let rig = rig(EDIT);
        let buffer = open_buffer(&rig, 11_000);
        let marks = (0..count as u32).map(mark).collect();
        prop_assert_eq!(rig.host.editor().set_gutter(buffer, "l".into(), marks).is_ok(), count <= 10_000);
    }
}

// ---------------------------------------------------------------- other WIT functions

/// REQ: WIT:ilium:primitives/editor.buffers
#[test]
fn wit_editor_buffers_lists_the_plugins_buffers() {
    let rig = rig(EDIT);
    let first = open_buffer(&rig, 1);
    let second = open_buffer(&rig, 2);
    let listed: Vec<BufferId> = rig
        .host
        .editor()
        .buffers()
        .into_iter()
        .map(|b| b.buffer)
        .collect();
    assert!(listed.contains(&first) && listed.contains(&second));
}

/// REQ: WIT:ilium:primitives/editor.buffers
#[test]
fn wit_editor_buffers_without_grant_is_empty_and_logs_a_refusal() {
    let rig = rig(&[]);
    assert!(rig.host.editor().buffers().is_empty());
    assert!(
        log_count(&rig.host, "editor.read") >= 1,
        "{}",
        log_text(&rig.host)
    );
}

/// REQ: WIT:ilium:primitives/editor.buffer-for-pane
#[test]
fn wit_editor_buffer_for_pane_returns_the_pane_buffer() {
    let rig = rig(READ);
    let buffer = rig.kernel.attach_buffer(EDITOR_PANE, "hello\n");
    assert_eq!(
        rig.host
            .editor()
            .buffer_for_pane(EDITOR_PANE)
            .map(|b| b.buffer),
        Some(buffer)
    );
}

/// REQ: WIT:ilium:primitives/editor.buffer-for-pane
#[test]
fn wit_editor_buffer_for_pane_of_a_terminal_is_none() {
    let rig = rig(READ);
    assert!(rig.host.editor().buffer_for_pane(PANE).is_none());
}

/// REQ: WIT:ilium:primitives/editor.buffer-for-pane
#[test]
fn wit_editor_buffer_for_pane_without_grant_is_none() {
    let rig = rig(&[]);
    rig.kernel.attach_buffer(EDITOR_PANE, "hello\n");
    assert!(rig.host.editor().buffer_for_pane(EDITOR_PANE).is_none());
}

/// REQ: WIT:ilium:primitives/editor.selections
#[test]
fn wit_editor_selections_returns_the_ranges() {
    let rig = rig(READ);
    let buffer = rig.kernel.attach_buffer(EDITOR_PANE, "hello\n");
    let range = TextRange {
        start: TextPosition { line: 0, column: 1 },
        end: TextPosition { line: 0, column: 4 },
    };
    rig.kernel.set_selections(buffer, vec![range.clone()]);
    assert_eq!(rig.host.editor().selections(buffer), vec![range]);
}

/// REQ: WIT:ilium:primitives/editor.selections
#[test]
fn wit_editor_selections_without_grant_is_empty() {
    let rig = rig(&[]);
    assert!(rig.host.editor().selections(BufferId(1)).is_empty());
}

/// REQ: WIT:ilium:primitives/editor.load-file, WIT:ilium:primitives/editor.take-buffer
#[test]
fn wit_editor_load_file_completes_with_a_buffer() {
    let mut rig = rig(EDIT);
    rig.kernel
        .put_file("project", "notes.txt", b"alpha\nbeta\n");
    let request = rig
        .host
        .editor()
        .load_file(FsPath {
            scope: "project".into(),
            path: "notes.txt".into(),
        })
        .expect("request");
    rig.host.pump(8);
    let buffer = rig
        .host
        .editor()
        .take_buffer(request)
        .expect("buffer loaded");
    assert_eq!(
        rig.host
            .editor()
            .read_lines(buffer, 0, 10)
            .expect("lines")
            .len(),
        2
    );
}

/// REQ: WIT:ilium:primitives/editor.load-file
#[test]
fn wit_editor_load_file_missing_file_fails_not_found() {
    let mut rig = rig(EDIT);
    let request = rig
        .host
        .editor()
        .load_file(FsPath {
            scope: "project".into(),
            path: "absent.txt".into(),
        })
        .expect("request");
    rig.host.pump(8);
    assert!(rig.host.editor().take_buffer(request).is_none());
    assert_eq!(
        rig.host.last_completion_failure(request),
        Some(ErrorCode::NotFound)
    );
}

/// REQ: WIT:ilium:primitives/editor.load-file
#[test]
fn wit_editor_load_file_binary_file_fails_invalid_argument() {
    let mut rig = rig(EDIT);
    rig.kernel
        .put_file("project", "blob.bin", &[0, 159, 146, 150, 0, 0]);
    let request = rig
        .host
        .editor()
        .load_file(FsPath {
            scope: "project".into(),
            path: "blob.bin".into(),
        })
        .expect("request");
    rig.host.pump(8);
    assert_eq!(
        rig.host.last_completion_failure(request),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/editor.take-buffer
#[test]
fn wit_editor_take_buffer_unknown_request_is_none() {
    let rig = rig(EDIT);
    assert!(rig.host.editor().take_buffer(RequestId(5_150)).is_none());
}

/// REQ: WIT:ilium:primitives/editor.save-buffer
#[test]
fn wit_editor_save_buffer_without_path_on_a_pathless_buffer_invalid_argument() {
    let rig = rig(EDIT);
    let buffer = open_buffer(&rig, 2);
    assert_eq!(
        error_of(rig.host.editor().save_buffer(buffer, None)),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/editor.save-buffer
#[test]
fn wit_editor_save_buffer_writes_the_file() {
    let mut rig = rig(EDIT);
    let buffer = open_buffer(&rig, 2);
    let path = FsPath {
        scope: "project".into(),
        path: "out.txt".into(),
    };
    let request = rig
        .host
        .editor()
        .save_buffer(buffer, Some(path))
        .expect("request");
    rig.host.pump(8);
    assert!(accepted_completion(&rig, request));
    assert_eq!(
        rig.kernel.file("project", "out.txt").as_deref(),
        Some(text_of(2).as_bytes())
    );
}

/// REQ: WIT:ilium:primitives/editor.save-buffer
#[test]
fn wit_editor_save_buffer_changed_on_disk_fails_conflict() {
    let mut rig = rig(EDIT);
    rig.kernel.put_file("project", "shared.txt", b"one\n");
    let load = rig
        .host
        .editor()
        .load_file(FsPath {
            scope: "project".into(),
            path: "shared.txt".into(),
        })
        .expect("load");
    rig.host.pump(8);
    let buffer = rig.host.editor().take_buffer(load).expect("buffer");
    rig.kernel
        .put_file("project", "shared.txt", b"changed elsewhere\n");
    let request = rig
        .host
        .editor()
        .save_buffer(buffer, None)
        .expect("request");
    rig.host.pump(8);
    assert_eq!(
        rig.host.last_completion_failure(request),
        Some(ErrorCode::Conflict)
    );
}

fn accepted_completion(rig: &Rig, request: RequestId) -> bool {
    rig.host.last_completion_failure(request).is_none()
}

// ---------------------------------------------------------------- viewport and minimap

/// REQ: WIT:ilium:primitives/editor.viewport
#[test]
fn wit_editor_viewport_returns_the_visible_range() {
    let rig = rig(READ);
    rig.kernel.attach_buffer(EDITOR_PANE, &text_of(200));
    rig.kernel.set_editor_viewport(EDITOR_PANE, 10, 40, 80, 31);
    let viewport = rig.host.editor().viewport(EDITOR_PANE).expect("viewport");
    assert_eq!((viewport.first_line, viewport.last_line), (10, 40));
    assert_eq!((viewport.columns, viewport.rows), (80, 31));
}

/// REQ: WIT:ilium:primitives/editor.viewport
#[test]
fn wit_editor_viewport_of_a_terminal_pane_is_none() {
    let rig = rig(READ);
    assert!(rig.host.editor().viewport(PANE).is_none());
}

/// REQ: WIT:ilium:primitives/editor.set-minimap-marks
#[test]
fn wit_editor_set_minimap_marks_4096_marks_accepted() {
    let rig = rig(READ);
    let buffer = rig.kernel.attach_buffer(EDITOR_PANE, &text_of(5_000));
    let marks = (0..4_096)
        .map(|line| MinimapMark {
            line,
            color: Color::Role(ColorRole::Accent),
            lane: 0,
        })
        .collect();
    assert!(rig
        .host
        .editor()
        .set_minimap_marks(buffer, "marks".into(), marks)
        .is_ok());
}

/// REQ: WIT:ilium:primitives/editor.set-minimap-marks
#[test]
fn wit_editor_set_minimap_marks_4097_marks_invalid_argument() {
    let rig = rig(READ);
    let buffer = rig.kernel.attach_buffer(EDITOR_PANE, &text_of(5_000));
    let marks = (0..4_097)
        .map(|line| MinimapMark {
            line,
            color: Color::Role(ColorRole::Accent),
            lane: 0,
        })
        .collect();
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_minimap_marks(buffer, "marks".into(), marks)
        ),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/editor.set-minimap-marks
#[test]
fn wit_editor_set_minimap_marks_lane_2_accepted_lane_3_invalid_argument() {
    let rig = rig(READ);
    let buffer = rig.kernel.attach_buffer(EDITOR_PANE, &text_of(10));
    let lane = |lane| {
        vec![MinimapMark {
            line: 0,
            color: Color::Role(ColorRole::Accent),
            lane,
        }]
    };
    assert!(rig
        .host
        .editor()
        .set_minimap_marks(buffer, "m".into(), lane(2))
        .is_ok());
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_minimap_marks(buffer, "m".into(), lane(3))
        ),
        Some(ErrorCode::InvalidArgument)
    );
}

/// REQ: WIT:ilium:primitives/editor.set-minimap-marks
#[test]
fn wit_editor_set_minimap_marks_unknown_buffer_not_found() {
    let rig = rig(READ);
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_minimap_marks(BufferId(404), "m".into(), Vec::new())
        ),
        Some(ErrorCode::NotFound)
    );
}

/// REQ: WIT:ilium:primitives/editor.set-minimap-marks
#[test]
fn wit_editor_set_minimap_marks_without_grant_not_permitted() {
    let rig = rig(&[]);
    assert_eq!(
        error_of(
            rig.host
                .editor()
                .set_minimap_marks(BufferId(1), "m".into(), Vec::new())
        ),
        Some(ErrorCode::NotPermitted)
    );
}
