//! capability-agent-config-propose.md: whole-file proposals to an agent's configuration folder.

use ilium_plugin_api::generated::host::fs::ConfigChange;
use ilium_plugin_api::Tier;

use crate::support::*;

const PROPOSE: [&str; 1] = ["agent-config.propose:gemini"];

/// One whole-file change to `settings.json` with no revision (the file must not exist yet).
fn create_settings_change() -> ConfigChange {
    ConfigChange {
        path: "settings.json".to_owned(),
        content: Some(b"{}".to_vec()),
        revision: None,
        summary: "Create the settings file".to_owned(),
    }
}

/// REQ: WIT:ilium:host/fs.propose-agent-config
#[test]
fn propose_agent_config_with_the_grant_returns_a_request_that_completes() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &PROPOSE);
    let request = expect_ok(fixture.fs().propose_agent_config(
        "gemini".to_owned(),
        "Update Gemini settings".to_owned(),
        vec![create_settings_change()],
    ));
    assert!(is_ok(&complete(&mut fixture, request)));
}

/// REQ: WIT:ilium:host/fs.take-proposal
#[test]
fn take_proposal_returns_the_result_once_and_every_path_is_accounted_for() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture_in(dir.path(), Tier::T2, &PROPOSE);
    let request = expect_ok(fixture.fs().propose_agent_config(
        "gemini".to_owned(),
        "Update Gemini settings".to_owned(),
        vec![create_settings_change()],
    ));
    complete(&mut fixture, request);
    let result = fixture
        .fs()
        .take_proposal(request)
        .expect("proposal result");
    assert_eq!(result.applied.len() + result.rejected.len(), 1);
    assert!(fixture.fs().take_proposal(request).is_none());
}
