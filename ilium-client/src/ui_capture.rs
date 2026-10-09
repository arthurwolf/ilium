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
    let directory = if let Some(directory) = std::env::var_os(directory_variable) {
        std::path::PathBuf::from(directory)
    } else if let Ok(job) = std::env::var("NI_BUILD_REMOTE_JOB") {
        assert!(
            job.len() == 32 && job.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid ni-build capture job identity"
        );
        let expected =
            std::path::PathBuf::from(format!("/data/ni-build-service/jobs/{job}/target"));
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .expect("ni-build capture target");
        assert_eq!(
            target, expected,
            "capture target must belong to the ni-build job"
        );
        target.join("ui-test-captures")
    } else {
        return;
    };
    assert!(
        directory.is_absolute(),
        "capture directory must be absolute"
    );
    assert!(name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-'));
    std::fs::create_dir_all(&directory).expect("create task-owned capture directory");
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
