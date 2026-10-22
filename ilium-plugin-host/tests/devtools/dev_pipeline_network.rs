//! Network rules of the `ilium plugin` commands (DEV-894, DEV-895). Every request goes through the
//! injected fetcher, so these tests record the requests and assert that none were made offline.

use std::path::Path;
use std::sync::{Arc, Mutex};

use ilium_plugin_host::cli::CliEnv;
use ilium_plugin_host::registry_client::{FetchError, Fetcher};

use crate::common::*;

/// A fetcher that records every URL it is asked for and answers that the network is unreachable.
#[derive(Default)]
struct RecordingFetcher {
    urls: Mutex<Vec<String>>,
}

impl Fetcher for RecordingFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        self.urls
            .lock()
            .expect("fetch log lock")
            .push(url.to_owned());
        Err(FetchError::Unreachable)
    }
}

/// The URLs the recorder was asked for, in call order.
fn requested(recorder: &RecordingFetcher) -> Vec<String> {
    recorder.urls.lock().expect("fetch log lock").clone()
}

/// An environment rooted in `root` whose registry access goes through `recorder`.
fn recording_env(root: &Path, recorder: Arc<RecordingFetcher>) -> CliEnv {
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("create project dir");
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        project,
        fixed_now(),
        true,
        recorder,
    )
}

/// REQ: DEV-894
#[test]
fn check_and_pack_make_no_network_connection() {
    let root = scratch();
    let recorder = Arc::new(RecordingFetcher::default());
    let env = recording_env(root.path(), recorder.clone());
    let dir = project_dir(&env);
    project_with(&dir, |_| {});
    let _ = run_in_project(&env, "check", &[]);
    let _ = run_in_project(&env, "pack", &[]);
    assert!(
        requested(&recorder).is_empty(),
        "build, check, test and pack connect to nothing: {:?}",
        requested(&recorder)
    );
}

/// REQ: DEV-895
#[test]
fn offline_flag_fails_network_install_before_any_connection() {
    let root = scratch();
    let recorder = Arc::new(RecordingFetcher::default());
    let env = recording_env(root.path(), recorder.clone());
    let output = run(
        &env,
        &["install", "example.agent-tools", "--offline", "--json"],
    );
    assert_eq!(output.exit_code, 1, "{:?}", output.stdout_jsonl);
    assert_eq!(error_code(&output).as_deref(), Some("net.offline"));
    assert!(requested(&recorder).is_empty(), "no connection is attempted");
}

/// REQ: DEV-895
#[test]
fn offline_environment_variable_fails_network_install_before_any_connection() {
    let root = scratch();
    let recorder = Arc::new(RecordingFetcher::default());
    let env = recording_env(root.path(), recorder.clone()).with_var("ILIUM_OFFLINE", "1");
    let output = run(&env, &["install", "example.agent-tools", "--json"]);
    assert_eq!(output.exit_code, 1, "{:?}", output.stdout_jsonl);
    assert_eq!(error_code(&output).as_deref(), Some("net.offline"));
    assert!(requested(&recorder).is_empty(), "no connection is attempted");
}
