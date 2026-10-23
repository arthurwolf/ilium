//! Symbolication of trap backtraces (DEV-878). Page: reference-plugin-debugging.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::common::*;
use crate::{new_rust_project, utf8};

/// The path of the `dist/<name>.debug.json` record written by a debug build.
fn debug_record_path(dir: &Path) -> PathBuf {
    let name = tree_of(&dir.join("dist"))
        .into_iter()
        .find(|path| path.ends_with(".debug.json"))
        .expect("dist/<name>.debug.json is written");
    dir.join("dist").join(name)
}

/// The parsed debug record of a debug build.
fn debug_record(dir: &Path) -> Value {
    let text = std::fs::read_to_string(debug_record_path(dir)).expect("read debug record");
    serde_json::from_str(&text).expect("debug record is JSON")
}

/// Writes a trap record that carries `input_hash` and the given backtrace frames.
fn write_trap(dir: &Path, input_hash: &str, frames: Value) {
    let trap = json!({ "input_hash": input_hash, "backtrace": frames });
    write_text(dir, "trap.json", &trap.to_string());
}

/// REQ: DEV-878
#[test]
fn symbolize_maps_each_frame_to_file_line_and_function_when_hash_matches() {
    let (_root, env, dir) = new_rust_project();
    let built = run_in_project(&env, "build", &[]);
    assert_eq!(built.exit_code, 0, "{:?}", built.stdout_jsonl);
    let hash = debug_record(&dir)["input_hash"]
        .as_str()
        .expect("debug record names input_hash as a string")
        .to_owned();
    write_trap(&dir, &hash, json!([{ "pc": 4096 }, { "pc": 8192 }]));
    let trap = dir.join("trap.json");
    let debug = debug_record_path(&dir);
    let out = run(
        &env,
        &[
            "symbolize",
            utf8(&trap),
            "--debug-info",
            utf8(&debug),
            "--json",
        ],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let frames = records_of(&out, "frame");
    assert_eq!(frames.len(), 2, "one frame record per trap frame");
    for frame in frames {
        assert!(
            frame.get("file").and_then(Value::as_str).is_some(),
            "frame names a source file: {frame}"
        );
        assert!(
            frame.get("line").and_then(Value::as_u64).is_some(),
            "frame names a line: {frame}"
        );
        assert!(
            frame.get("function").and_then(Value::as_str).is_some(),
            "frame names a function: {frame}"
        );
    }
}

/// REQ: DEV-878
#[test]
fn symbolize_fails_with_hash_mismatch_and_prints_no_lines_after_rebuild() {
    let (_root, env, dir) = new_rust_project();
    assert_eq!(
        run_in_project(&env, "build", &[]).exit_code,
        0,
        "first build"
    );
    let old_hash = debug_record(&dir)["input_hash"]
        .as_str()
        .expect("input_hash string")
        .to_owned();
    write_trap(&dir, &old_hash, json!([{ "pc": 4096 }]));
    write_text(&dir, "src/lib.rs", "// edited after the trap was taken\n");
    let rebuilt = run_in_project(&env, "build", &[]);
    assert_eq!(rebuilt.exit_code, 0, "rebuild after the edit");
    let trap = dir.join("trap.json");
    let debug = debug_record_path(&dir);
    let out = run(
        &env,
        &[
            "symbolize",
            utf8(&trap),
            "--debug-info",
            utf8(&debug),
            "--json",
        ],
    );
    assert_ne!(out.exit_code, 0, "a hash mismatch must fail");
    assert_eq!(
        error_code(&out).as_deref(),
        Some("debug.hash-mismatch"),
        "{:?}",
        out.stdout_jsonl
    );
    assert!(
        records_of(&out, "frame")
            .iter()
            .all(|frame| frame.get("line").is_none()),
        "no guessed lines after a hash mismatch"
    );
}

/// REQ: DEV-878
#[test]
fn symbolize_prints_function_name_only_for_frame_without_debug_info() {
    let (_root, env, dir) = new_rust_project();
    assert_eq!(run_in_project(&env, "build", &[]).exit_code, 0, "build");
    let hash = debug_record(&dir)["input_hash"]
        .as_str()
        .expect("input_hash string")
        .to_owned();
    write_trap(
        &dir,
        &hash,
        json!([{ "pc": 4096 }, { "pc": 1, "function": "wasm-function[7]" }]),
    );
    let trap = dir.join("trap.json");
    let debug = debug_record_path(&dir);
    let out = run(
        &env,
        &[
            "symbolize",
            utf8(&trap),
            "--debug-info",
            utf8(&debug),
            "--json",
        ],
    );
    assert_eq!(out.exit_code, 0, "{:?}", out.stdout_jsonl);
    let bare = records_of(&out, "frame")
        .into_iter()
        .find(|frame| frame.get("function").and_then(Value::as_str) == Some("wasm-function[7]"))
        .expect("the frame without debug information is printed");
    assert!(bare.get("file").is_none(), "no guessed file: {bare}");
    assert!(bare.get("line").is_none(), "no guessed line: {bare}");
}
