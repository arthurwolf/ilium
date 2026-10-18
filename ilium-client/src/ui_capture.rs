//! Optional synthetic buffer artifacts from production-renderer tests.
use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;
use std::io::Write;

pub(crate) fn save(name: &str, terminal: &Terminal<TestBackend>) {
    save_with_env(name, terminal, "ILIUM_UI_RENDER_DIR");
}

pub(crate) fn save_with_env(
    name: &str,
    terminal: &Terminal<TestBackend>,
    directory_variable: &str,
) {
    let (directory, remote_job) = if let Some(directory) = std::env::var_os(directory_variable) {
        (std::path::PathBuf::from(directory), None)
    } else if let Some(job) = std::env::var_os("NI_BUILD_REMOTE_JOB") {
        let job = job
            .into_string()
            .expect("ni-build job identity must be UTF-8");
        assert!(
            job.len() == 32 && job.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid ni-build capture job identity"
        );
        // Cargo's real target is a leased cache slot and deliberately is not
        // exposed as CARGO_TARGET_DIR. This job-private directory holds only
        // atomic uniqueness claims; the returned, hash-verified stdout log
        // carries the complete synthetic capture.
        (
            std::path::PathBuf::from(format!(
                "/data/ni-build-service/jobs/{job}/target/ui-test-capture-claims"
            )),
            Some(job),
        )
    } else {
        return;
    };
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
    let mut capture = json!({
        "type": "artifact", "fixture": name, "synthetic": true,
        "width": buffer.area.width, "height": buffer.area.height, "cells": cells,
        "limitation": "Production renderer buffer with synthetic state; not live terminal input or fonts",
    });
    if let Some(job) = remote_job {
        // create_new is atomic across parallel test binaries in this job.
        // The empty claim is transient; the payload is retained in stdout.log.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.claim")))
            .expect("create unique UI capture claim");
        capture["ni_build_job_id"] = json!(job);
        // Write directly: libtest discards println! output from passing tests.
        // Holding stdout's lock keeps parallel captures as complete JSON lines.
        writeln!(std::io::stdout().lock(), "{capture}").expect("retain UI capture in verified log");
    } else {
        let path = directory.join(format!("{name}.json"));
        // A reused artifact name is a harness error rather than permission to overwrite.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create unique UI capture artifact");
        serde_json::to_writer(&mut file, &capture).expect("write UI capture artifact");
        println!(
            "{}",
            json!({"type": "artifact", "path": path, "synthetic": true})
        );
    }
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
