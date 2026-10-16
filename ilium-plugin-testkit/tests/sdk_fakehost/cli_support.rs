//! Helpers that run the `ilium plugin test` command for the sdk-fakehost lane (part 2).
//!
//! The binary is named by `ILIUM_BIN` (an absolute path). The component under test is named by
//! `ILIUM_TEST_COMPONENT_SERVER`. A missing variable fails the test with `fixture.missing`, so these
//! tests are red until the toolchain builds the fixtures.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::{temp_dir, write_file};

/// The result of one CLI run.
pub struct CliRun {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliRun {
    /// The stdout lines that parse as JSON objects, in order.
    pub fn records(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }

    /// The raw stdout lines that start with `{`.
    pub fn record_lines(&self) -> Vec<String> {
        self.stdout.lines().filter(|line| line.starts_with('{')).map(str::to_string).collect()
    }

    /// Records whose `type` equals `kind`.
    pub fn of_type(&self, kind: &str) -> Vec<serde_json::Value> {
        self.records().into_iter().filter(|record| record["type"] == kind).collect()
    }
}

fn required_env(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => panic!("fixture.missing: set {name} (see the toolchain fixture build)"),
    }
}

/// A one-sided (server) plugin project with a built component and the given fixtures.
/// `fixtures` is a list of `(file name, body)`; a name ending in `.expect.json` is an expectation file.
pub fn project(label: &str, fixtures: &[(&str, &str)]) -> PathBuf {
    let dir = temp_dir(label);
    write_file(
        &dir,
        "ilium-plugin.json",
        r#"{
  "api": "ilium:plugin@0.1.0",
  "id": "example.cli-demo",
  "name": "CLI demo",
  "version": "0.1.0",
  "publisher": "Example Publisher",
  "server": { "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["handler"] },
  "capabilities": { "server": [] }
}
"#,
    );
    std::fs::create_dir_all(dir.join("dist")).expect("create dist");
    std::fs::copy(required_env("ILIUM_TEST_COMPONENT_SERVER"), dir.join("dist/server.wasm"))
        .expect("copy the built component fixture");
    for (name, body) in fixtures {
        write_file(&dir, &format!("tests/fixtures/{name}"), body);
    }
    dir
}

/// Runs `ilium plugin test <dir> <args...>` with stdout piped, so it is not a terminal.
pub fn run_test(dir: &Path, args: &[&str]) -> CliRun {
    run_with_env(dir, args, &[])
}

/// As `run_test`, with extra environment variables.
pub fn run_with_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> CliRun {
    let mut command = Command::new(required_env("ILIUM_BIN"));
    command.arg("plugin").arg("test").arg(dir).args(args);
    for (name, value) in envs {
        command.env(name, value);
    }
    let output = command.output().expect("start the ilium binary");
    CliRun {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

/// Runs the same command under a pseudo terminal with `script(1)`, so stdout is a terminal.
pub fn run_on_terminal(dir: &Path, args: &[&str]) -> CliRun {
    let mut line = format!("{} plugin test '{}'", required_env("ILIUM_BIN"), dir.display());
    for arg in args {
        line.push_str(&format!(" '{arg}'"));
    }
    let output = Command::new("script")
        .args(["-qec", &line, "/dev/null"])
        .output()
        .expect("start script(1) to get a terminal");
    CliRun {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}
