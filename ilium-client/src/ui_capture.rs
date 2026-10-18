//! Optional synthetic buffer artifacts from production-renderer tests.
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;

pub(crate) fn save(name: &str, terminal: &Terminal<TestBackend>) {
    save_with_env(name, terminal, "ILIUM_UI_RENDER_DIR");
}

pub(crate) fn save_with_env(
    name: &str,
    terminal: &Terminal<TestBackend>,
    directory_variable: &str,
) {
    assert!(name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-'));
    if let Some(directory) = std::env::var_os(directory_variable) {
        write_json_artifact(name, terminal, &std::path::PathBuf::from(directory));
    } else if std::env::var_os("NI_BUILD_REMOTE_JOB").is_some() {
        // ni-build returns the test log, not the cache slot the test binary runs from,
        // so a remote capture is printed as text frames into that log instead.
        print_text_frame(name, terminal);
    }
}

fn write_json_artifact(name: &str, terminal: &Terminal<TestBackend>, directory: &std::path::Path) {
    assert!(
        directory.is_absolute(),
        "capture directory must be absolute"
    );
    std::fs::create_dir_all(directory).expect("create task-owned capture directory");
    let buffer = terminal.backend().buffer();
    let cells: Vec<_> = buffer
        .content
        .iter()
        .map(|cell| {
            json!({
                "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                "bg": format!("{:?}", cell.bg), "modifier": format!("{:?}", cell.modifier),
            })
        })
        .collect();
    let path = directory.join(format!("{name}.json"));
    // A reused artifact name is a harness error rather than permission to overwrite.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create unique UI capture artifact");
    serde_json::to_writer(&mut file, &json!({
        "type": "artifact", "fixture": name, "synthetic": true,
        "width": buffer.area.width, "height": buffer.area.height, "cells": cells,
        "limitation": "Production renderer buffer with synthetic state; not live terminal input or fonts",
    })).expect("write UI capture artifact");
    println!(
        "{}",
        json!({"type": "artifact", "path": path, "synthetic": true})
    );
}

fn print_text_frame(name: &str, terminal: &Terminal<TestBackend>) {
    let buffer = terminal.backend().buffer();
    let width = usize::from(buffer.area.width).max(1);
    println!(
        "ui-frame-begin {name} {}x{}",
        buffer.area.width, buffer.area.height
    );
    for row in buffer.content.chunks(width) {
        let line: String = row.iter().map(|cell| cell.symbol()).collect();
        println!("{}", line.trim_end());
    }
    println!("ui-frame-end {name}");
}
