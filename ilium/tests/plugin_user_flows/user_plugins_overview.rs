//! Plugins for users (user-plugins-overview.md): USR-001 to USR-003.

use crate::support::*;

/// REQ: USR-001
#[test]
fn doc_usr_001_overview_states_30_core_plugins_ship_with_ilium() {
    let page = manual_page("user-plugins-overview");
    assert!(
        page.contains("ship with Ilium"),
        "phrase 'ship with Ilium' missing"
    );
    assert!(page.contains("30"), "the count 30 is missing");
    let sentence = page
        .lines()
        .find(|line| line.contains("ship with Ilium"))
        .expect("a line with the phrase");
    assert!(
        sentence.contains("30"),
        "the count and the phrase must be in the same statement: {sentence}"
    );
}

/// REQ: USR-001
#[test]
fn doc_usr_001_core_plugin_page_lists_exactly_30_ids() {
    assert_eq!(core_plugin_ids_from_manual().len(), 30);
}

/// REQ: USR-001
#[test]
fn usr_001_fresh_profile_lists_30_core_plugins_without_any_install() {
    let mut rig = UserRig::new();
    let rows = rig.list();
    let core: Vec<&PluginRow> = rows
        .iter()
        .filter(|row| row.id.starts_with("ilium.core."))
        .collect();
    assert_eq!(core.len(), 30);
    assert!(
        rows.iter().all(|row| row.id.starts_with("ilium.core.")),
        "a fresh profile lists no third-party plugin"
    );
}

/// REQ: USR-002
#[test]
fn usr_002_no_plugin_request_in_one_hour_with_two_plugins_installed() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.one", "1.0.0", &["tree.read"]));
    install_ok(&mut rig, &spec("com.example.two", "1.0.0", &["tree.read"]));
    rig.start_server();
    rig.advance_ms(3_600_000);
    assert!(
        rig.network_requests().is_empty(),
        "requests: {:?}",
        rig.network_requests()
    );
}

/// REQ: USR-002
#[test]
fn usr_002_cli_commands_never_use_the_fetcher() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.one", "1.0.0", &["tree.read"]));
    for arguments in [
        ["list"].as_slice(),
        &["grants", "com.example.one"],
        &["info", "com.example.one"],
    ] {
        let _ = rig.cli(arguments);
    }
    assert_eq!(rig.fetch_calls(), 0);
}

/// REQ: USR-002
#[test]
fn usr_002_hour_after_restart_still_no_plugin_list_or_package_request() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.one", "1.0.0", &[]));
    rig.start_server();
    rig.restart_server();
    rig.advance_ms(3_600_000);
    let requests = rig.network_requests();
    assert!(
        requests
            .iter()
            .all(|url| !url.contains(".iliumpkg") && !url.contains("plugins")),
        "{requests:?}"
    );
}

/// REQ: USR-003
#[test]
fn usr_003_review_shows_all_five_grants_before_the_plugin_runs() {
    let mut rig = UserRig::new();
    let package = spec(
        "com.example.five",
        "1.0.0",
        &[
            "tree.read",
            "tree.write",
            "panes.read",
            "clipboard.write",
            "notify.toast",
        ],
    )
    .start_writes_file("ran.txt");
    rig.attach_client(vec![DialogAnswer::ReviewDecline]);
    let _ = rig.install_file(&package, &["--allow-unsigned"]);
    let dialogs = rig.dialogs_shown();
    assert_eq!(dialogs.len(), 1);
    assert_eq!(dialogs[0].rows.len(), 5);
    assert!(
        !rig.data_file_exists("com.example.five", "ran.txt"),
        "plugin code ran before the review was answered"
    );
}

/// REQ: USR-003
#[test]
fn usr_003_unchecked_grants_stay_locked_not_permitted() {
    let mut rig = UserRig::new();
    let grants = [
        "tree.read",
        "tree.write",
        "panes.read",
        "clipboard.write",
        "notify.toast",
    ];
    rig.attach_client(vec![DialogAnswer::Review(vec![
        "tree.read".into(),
        "tree.write".into(),
        "panes.read".into(),
    ])]);
    let output = rig.install_file(
        &spec("com.example.five", "1.0.0", &grants),
        &["--allow-unsigned"],
    );
    assert_eq!(result_exit_code(&output), 0);
    rig.start_server();
    for given in ["tree.read", "tree.write", "panes.read"] {
        assert_eq!(
            rig.call_needing("com.example.five", "server", given),
            CallOutcome::Ok,
            "{given}"
        );
    }
    for locked in ["clipboard.write", "notify.toast"] {
        match rig.call_needing("com.example.five", "server", locked) {
            CallOutcome::Refused { code, .. } => assert_eq!(code, "not-permitted", "{locked}"),
            other => panic!("{locked}: expected not-permitted, got {other:?}"),
        }
    }
}

/// REQ: USR-003
#[test]
fn usr_003_declined_grant_stays_locked_after_a_server_restart() {
    let mut rig = UserRig::new();
    rig.attach_client(vec![DialogAnswer::Review(vec!["tree.read".into()])]);
    let _ = rig.install_file(
        &spec("com.example.two", "1.0.0", &["tree.read", "tree.write"]),
        &["--allow-unsigned"],
    );
    rig.start_server();
    rig.restart_server();
    assert!(matches!(
        rig.call_needing("com.example.two", "server", "tree.write"),
        CallOutcome::Refused { ref code, .. } if code == "not-permitted"
    ));
}
