//! Permissions explained (user-permissions-explained.md): USR-020, USR-021, USR-081, USR-083 to
//! USR-088 (host behaviour; the dialog and tab renders live in the ilium-client lane).

use crate::support::*;

// ---- USR-020 ----

/// REQ: USR-020
#[test]
fn usr_020_grant_to_the_client_side_is_not_permitted_from_the_server_side() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.sides", "1.0.0", &[]).grants("client", &["clipboard.write"]).grants("server", &["clipboard.write"]));
    let granted = rig.cli(&["grants", "com.example.sides", "--side", "client", "--grant", "clipboard.write"]);
    assert_eq!(result_exit_code(&granted), 0);
    rig.start_server();
    assert_eq!(rig.call_needing("com.example.sides", "client", "clipboard.write"), CallOutcome::Ok);
    match rig.call_needing("com.example.sides", "server", "clipboard.write") {
        CallOutcome::Refused { code, .. } => assert_eq!(code, "not-permitted"),
        other => panic!("expected not-permitted, got {other:?}"),
    }
}

/// REQ: USR-020
#[test]
fn usr_020_grant_belongs_to_one_plugin_only() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.a", "1.0.0", &["clipboard.write"]));
    install_ok(&mut rig, &spec("com.example.b", "1.0.0", &["clipboard.write"]));
    let _ = rig.cli(&["grants", "com.example.a", "--grant", "clipboard.write"]);
    rig.start_server();
    assert_eq!(rig.call_needing("com.example.a", "server", "clipboard.write"), CallOutcome::Ok);
    assert!(matches!(rig.call_needing("com.example.b", "server", "clipboard.write"), CallOutcome::Refused { .. }));
}

/// REQ: USR-020
#[test]
fn usr_020_grants_are_stored_in_the_user_state_folder_not_the_project() {
    let mut rig = UserRig::new();
    let project = rig.open_project("alpha");
    install_ok(&mut rig, &spec("com.example.a", "1.0.0", &["fs.read:project"]));
    let _ = rig.cli(&["grants", "com.example.a", "--grant", "fs.read:project"]);
    assert!(rig.state_dir().join("ilium/grants.json").exists() || rig.state_dir().join("grants.json").exists());
    assert!(tree_snapshot(&project.join(".ilium")).keys().all(|name| !name.contains("grant")));
}

// ---- USR-021 ----

/// REQ: USR-021
#[test]
fn usr_021_call_without_manifest_entry_traps_with_capability_not_requested() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.small", "1.0.0", &["tree.read"]));
    rig.start_server();
    match rig.call_needing("com.example.small", "server", "clipboard.read") {
        CallOutcome::Trapped { code } => assert_eq!(code, "host.capability-not-requested"),
        other => panic!("expected trap, got {other:?}"),
    }
}

/// REQ: USR-021
#[test]
fn usr_021_user_cannot_grant_a_permission_the_manifest_does_not_list() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.small", "1.0.0", &["tree.read"]));
    let output = rig.cli(&["grants", "com.example.small", "--grant", "clipboard.read"]);
    assert_ne!(result_exit_code(&output), 0);
    assert!(rig.grant_entries("com.example.small").iter().all(|entry| entry.grant != "clipboard.read" || entry.state != "granted"));
}

// ---- USR-081 ----

/// REQ: USR-081
#[test]
fn usr_081_bundled_only_grant_in_a_t2_package_fails_with_grant_tier() {
    let mut rig = UserRig::new();
    let key = KeyPair::generate("Example Publisher");
    rig.trust(&key);
    let output = rig.install_file(&spec("com.example.greedy", "1.0.0", &["plugins.manage"]).signed_with(&key), &["--yes"]);
    assert_eq!(error_codes(&output), vec!["manifest.grant-tier".to_string()]);
    assert!(rig.list().iter().all(|row| row.id != "com.example.greedy"));
}

/// REQ: USR-081
#[test]
fn usr_081_same_grant_in_a_t3_package_also_fails_and_a_normal_grant_does_not() {
    let mut rig = UserRig::new();
    let refused = rig.install_file(&spec("com.example.greedy", "1.0.0", &["plugins.manage"]), &["--allow-unsigned", "--yes"]);
    assert_eq!(error_codes(&refused), vec!["manifest.grant-tier".to_string()]);
    let allowed = rig.install_file(&spec("com.example.modest", "1.0.0", &["tree.read"]), &["--allow-unsigned", "--yes"]);
    assert_eq!(result_exit_code(&allowed), 0);
}

// ---- USR-083 ----

/// REQ: USR-083
#[test]
fn usr_083_two_links_to_one_site_and_one_to_another_prompt_twice() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.links", "1.0.0", &["links.open"]));
    let _ = rig.cli(&["grants", "com.example.links", "--grant", "links.open"]);
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::LinkAllow, DialogAnswer::LinkAllow]);
    rig.plugin_opens_link("com.example.links", "https://docs.example.com/a");
    rig.plugin_opens_link("com.example.links", "https://docs.example.com/b");
    rig.plugin_opens_link("com.example.links", "https://other.example.org/");
    assert_eq!(rig.link_prompts().len(), 2);
    assert_eq!(rig.opened_links().len(), 3);
}

/// REQ: USR-083
#[test]
fn usr_083_answer_is_remembered_per_plugin_across_a_restart() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.links", "1.0.0", &["links.open"]));
    install_ok(&mut rig, &spec("com.example.links2", "1.0.0", &["links.open"]));
    for id in ["com.example.links", "com.example.links2"] {
        let _ = rig.cli(&["grants", id, "--grant", "links.open"]);
    }
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::LinkAllow, DialogAnswer::LinkAllow]);
    rig.plugin_opens_link("com.example.links", "https://docs.example.com/a");
    rig.restart_server();
    rig.plugin_opens_link("com.example.links", "https://docs.example.com/b");
    assert_eq!(rig.link_prompts().len(), 1, "remembered for the same plugin and site");
    rig.plugin_opens_link("com.example.links2", "https://docs.example.com/c");
    assert_eq!(rig.link_prompts().len(), 2, "another plugin asks again");
}

/// REQ: USR-083
#[test]
fn usr_083_a_declined_link_is_not_opened() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.links", "1.0.0", &["links.open"]));
    let _ = rig.cli(&["grants", "com.example.links", "--grant", "links.open"]);
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::LinkDecline]);
    rig.plugin_opens_link("com.example.links", "https://docs.example.com/a");
    assert!(rig.opened_links().is_empty());
}

// ---- USR-084 ----

/// REQ: USR-084
#[test]
fn usr_084_no_connection_to_a_new_setting_host_before_the_user_answers() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.net", "1.0.0", &["http:setting:server_url"]));
    let _ = rig.cli(&["grants", "com.example.net", "--grant", "http:setting:server_url"]);
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::HostPause]);
    rig.set_plugin_setting("com.example.net", "server_url", "https://new.example.net");
    rig.plugin_requests("com.example.net", "https://new.example.net/ping");
    assert!(rig.connections_attempted().is_empty(), "{:?}", rig.connections_attempted());
}

/// REQ: USR-084
#[test]
fn usr_084_after_a_yes_the_host_is_contacted_and_after_a_no_it_is_not() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.net", "1.0.0", &["http:setting:server_url"]));
    let _ = rig.cli(&["grants", "com.example.net", "--grant", "http:setting:server_url"]);
    rig.start_server();
    rig.set_plugin_setting("com.example.net", "server_url", "https://yes.example.net");
    rig.attach_client(vec![DialogAnswer::HostAllow]);
    rig.plugin_requests("com.example.net", "https://yes.example.net/ping");
    assert_eq!(rig.connections_attempted(), vec!["yes.example.net".to_string()]);
    rig.set_plugin_setting("com.example.net", "server_url", "https://no.example.net");
    rig.attach_client(vec![DialogAnswer::HostDecline]);
    rig.plugin_requests("com.example.net", "https://no.example.net/ping");
    assert_eq!(rig.connections_attempted().len(), 1);
}

// ---- USR-085 ----

/// REQ: USR-085
#[test]
fn usr_085_revoke_makes_inflight_calls_revoked_and_restarts_with_fewer_grants() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.rev", "1.0.0", &["clipboard.read", "tree.read"]));
    let _ = rig.cli(&["grants", "com.example.rev", "--grant", "clipboard.read", "--grant", "tree.read"]);
    rig.start_server();
    let call = rig.start_long_call("com.example.rev", "server", "clipboard.read");
    let revoke = rig.cli(&["grants", "com.example.rev", "--revoke", "clipboard.read"]);
    assert_eq!(result_exit_code(&revoke), 0);
    assert_eq!(rig.finish_call(call), CallOutcome::Revoked);
    rig.advance_ms(2_000);
    assert!(rig.log_lines("com.example.rev").iter().any(|line| line.contains("restart")), "{:?}", rig.log_lines("com.example.rev"));
    assert_eq!(rig.granted_list_seen_by_instance("com.example.rev", "server"), vec!["tree.read".to_string()]);
}

/// REQ: USR-085
#[test]
fn usr_085_calls_started_after_the_revoke_are_not_permitted() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.rev", "1.0.0", &["clipboard.read"]));
    let _ = rig.cli(&["grants", "com.example.rev", "--grant", "clipboard.read"]);
    rig.start_server();
    let _ = rig.cli(&["grants", "com.example.rev", "--revoke", "clipboard.read"]);
    assert!(matches!(
        rig.call_needing("com.example.rev", "server", "clipboard.read"),
        CallOutcome::Refused { ref code, .. } if code == "not-permitted"
    ));
}

// ---- USR-086 ----

/// REQ: USR-086
#[test]
fn usr_086_high_grant_through_the_cli_asks_the_attached_client() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.high", "1.0.0", &["terminal.write"]));
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::ConfirmNo]);
    let output = rig.cli(&["grants", "com.example.high", "--grant", "terminal.write"]);
    assert_eq!(rig.confirmations_shown().len(), 1);
    assert_ne!(result_exit_code(&output), 0);
    assert!(rig.grant_entries("com.example.high").iter().all(|entry| entry.state != "granted"));
}

/// REQ: USR-086
#[test]
fn usr_086_high_grant_after_a_yes_is_recorded() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.high", "1.0.0", &["terminal.write"]));
    rig.start_server();
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let output = rig.cli(&["grants", "com.example.high", "--grant", "terminal.write"]);
    assert_eq!(result_exit_code(&output), 0);
    assert!(rig.grant_entries("com.example.high").iter().any(|entry| entry.grant == "terminal.write" && entry.state == "granted"));
}

/// REQ: USR-086
#[test]
fn usr_086_normal_grant_needs_no_confirmation() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.mild", "1.0.0", &["tree.read"]));
    rig.start_server();
    rig.attach_client(vec![]);
    let output = rig.cli(&["grants", "com.example.mild", "--grant", "tree.read"]);
    assert_eq!(result_exit_code(&output), 0);
    assert!(rig.confirmations_shown().is_empty());
}

// ---- USR-087 ----

/// REQ: USR-087
#[test]
fn usr_087_footer_indicator_shows_for_three_seconds_after_a_clipboard_read() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.clip", "1.0.0", &["clipboard.read"]));
    let _ = rig.cli(&["grants", "com.example.clip", "--grant", "clipboard.read"]);
    rig.start_server();
    assert_eq!(rig.call_needing("com.example.clip", "server", "clipboard.read"), CallOutcome::Ok);
    assert!(rig.clipboard_indicator_visible());
    rig.advance_ms(2_999);
    assert!(rig.clipboard_indicator_visible(), "n-1");
    rig.advance_ms(2);
    assert!(!rig.clipboard_indicator_visible(), "n+1");
}

/// REQ: USR-087
#[test]
fn usr_087_a_clipboard_write_also_shows_the_indicator() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.clip", "1.0.0", &["clipboard.write"]));
    let _ = rig.cli(&["grants", "com.example.clip", "--grant", "clipboard.write"]);
    rig.start_server();
    assert_eq!(rig.call_needing("com.example.clip", "server", "clipboard.write"), CallOutcome::Ok);
    assert!(rig.clipboard_indicator_visible());
}

// ---- USR-088 ----

/// REQ: USR-088
#[test]
fn usr_088_ten_toasts_at_once_show_three() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.toast", "1.0.0", &["notify.toast"]));
    let _ = rig.cli(&["grants", "com.example.toast", "--grant", "notify.toast"]);
    rig.start_server();
    for index in 0..10 {
        rig.plugin_toast("com.example.toast", &format!("message {index}"));
    }
    assert_eq!(rig.visible_toasts().len(), 3);
}

/// REQ: USR-088
#[test]
fn usr_088_toast_limit_boundaries_two_three_four() {
    for (sent, expected) in [(2usize, 2usize), (3, 3), (4, 3)] {
        let mut rig = UserRig::new();
        install_ok(&mut rig, &spec("com.example.toast", "1.0.0", &["notify.toast"]));
        let _ = rig.cli(&["grants", "com.example.toast", "--grant", "notify.toast"]);
        rig.start_server();
        for index in 0..sent {
            rig.plugin_toast("com.example.toast", &format!("message {index}"));
        }
        assert_eq!(rig.visible_toasts().len(), expected, "sent {sent}");
    }
}

/// REQ: USR-088
#[test]
fn usr_088_after_ten_seconds_the_plugin_may_toast_again() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.toast", "1.0.0", &["notify.toast"]));
    let _ = rig.cli(&["grants", "com.example.toast", "--grant", "notify.toast"]);
    rig.start_server();
    for index in 0..4 {
        rig.plugin_toast("com.example.toast", &format!("message {index}"));
    }
    rig.advance_ms(10_001);
    rig.clear_toasts();
    rig.plugin_toast("com.example.toast", "again");
    assert_eq!(rig.visible_toasts().len(), 1);
}

/// REQ: USR-088
#[test]
fn usr_088_input_lock_ends_after_two_seconds() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.lock", "1.0.0", &["terminal.input-lock"]));
    let _ = rig.cli(&["grants", "com.example.lock", "--grant", "terminal.input-lock"]);
    rig.start_server();
    rig.plugin_locks_input("com.example.lock");
    assert!(rig.input_locked());
    rig.advance_ms(1_999);
    assert!(rig.input_locked(), "n-1");
    rig.advance_ms(2);
    assert!(!rig.input_locked(), "n+1");
}
