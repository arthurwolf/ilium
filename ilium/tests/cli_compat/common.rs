//! Shared helpers of the cli_compat tests: an isolated sandbox, a bounded binary run and page readers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A finished `ilium` run.
#[derive(Debug, Clone)]
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// stdout lines that parse as JSON objects; panics on a line that does not (every JSONL line is one object).
    pub fn records(&self) -> Vec<serde_json::Value> {
        self.stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("not JSON {line:?}: {error}")))
            .collect()
    }
}

/// Isolated XDG directories and a project directory (the `--cwd`).
pub struct Sandbox {
    root: tempfile::TempDir,
    pub project: PathBuf,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        // Short prefix: the derived Unix socket path must fit sockaddr_un.
        let root = tempfile::Builder::new().prefix("ic").tempdir().expect("sandbox dir");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("project dir");
        Sandbox { root, project }
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// Runs `ilium --cwd <project> <args...>` with a hard 20 s bound.
    pub fn ilium(&self, args: &[&str]) -> Run {
        self.ilium_env(args, &[])
    }

    /// Same with extra environment variables.
    pub fn ilium_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ilium"));
        command
            .arg("--cwd")
            .arg(&self.project)
            .args(args)
            .current_dir(&self.project)
            .env("XDG_DATA_HOME", self.root.path().join("data"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("ILIUM_SOCKET_DIR", self.root.path().join("sock"))
            .env_remove("ILIUM_SAFE_PLUGINS")
            .env_remove("ILIUM_NO_PLUGINS")
            .env_remove("ILIUM_PANE_ID")
            .env_remove("ILIUM_SESSION_SOCKET");
        for (key, value) in envs {
            command.env(key, value);
        }
        run_bounded(command)
    }
}

/// Runs a prepared command with stdin closed and a 20 s deadline.
pub fn run_bounded(mut command: Command) -> Run {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ilium");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().expect("poll child").is_some() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("collect output");
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

pub fn manual_page(name: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{name}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(format!("{name}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The first line of a record list that has the given `type`.
pub fn first_of_type<'a>(records: &'a [serde_json::Value], kind: &str) -> Option<&'a serde_json::Value> {
    records.iter().find(|record| record["type"] == kind)
}

/// Command names listed under `Commands:` of a help text.
pub fn help_commands(help: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_block = false;
    for line in help.lines() {
        if line.trim_end() == "Commands:" {
            in_block = true;
            continue;
        }
        if in_block {
            if line.trim().is_empty() || !line.starts_with(' ') {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                commands.push(name.to_owned());
            }
        }
    }
    commands
}
