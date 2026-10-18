//! Extensions tab (user-extensions-tab.md): USR-050 to USR-054 (host behaviour; the screen
//! text is in the ilium-client lane).

use crate::support::*;

fn owner_and_two_contributors(rig: &mut UserRig) {
    install_ok(
        rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    install_ok(
        rig,
        &spec("com.example.first", "1.0.0", &[]).contributes_with_priority(
            "com.example.owner",
            "owner.point",
            10,
        ),
    );
    install_ok(
        rig,
        &spec("com.example.second", "1.0.0", &[]).contributes_with_priority(
            "com.example.owner",
            "owner.point",
            20,
        ),
    );
}

// ---- USR-050 ----

/// REQ: USR-050
#[test]
fn usr_050_default_run_order_follows_priority() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    assert_eq!(
        rig.call_point_order("com.example.owner", "owner.point"),
        vec!["com.example.first", "com.example.second"]
    );
}

/// REQ: USR-050
#[test]
fn usr_050_user_order_wins_over_priority() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    rig.extensions_move_up("com.example.owner", "owner.point", "com.example.second");
    assert_eq!(
        rig.call_point_order("com.example.owner", "owner.point"),
        vec!["com.example.second", "com.example.first"]
    );
}

/// REQ: USR-050
#[test]
fn usr_050_move_down_restores_the_original_order_and_edges_do_nothing() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    rig.extensions_move_up("com.example.owner", "owner.point", "com.example.first");
    assert_eq!(
        rig.call_point_order("com.example.owner", "owner.point"),
        vec!["com.example.first", "com.example.second"],
        "top row cannot move up"
    );
    rig.extensions_move_down("com.example.owner", "owner.point", "com.example.first");
    assert_eq!(
        rig.call_point_order("com.example.owner", "owner.point"),
        vec!["com.example.second", "com.example.first"]
    );
}

// ---- USR-051 ----

/// REQ: USR-051
#[test]
fn usr_051_order_and_toggle_are_kept_across_a_restart_in_the_host_config() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    rig.extensions_move_up("com.example.owner", "owner.point", "com.example.second");
    rig.extensions_toggle(
        "com.example.owner",
        "owner.point",
        "com.example.first",
        false,
    );
    rig.restart_server();
    assert_eq!(
        rig.call_point_order("com.example.owner", "owner.point"),
        vec!["com.example.second"]
    );
    let config =
        std::fs::read_to_string(rig.config_dir().join("config.toml")).expect("config.toml");
    assert!(
        config.contains("[plugins.extensions"),
        "stored by the host in the extensions table"
    );
}

/// REQ: USR-051
#[test]
fn usr_051_no_plugin_folder_holds_the_order() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    rig.extensions_move_up("com.example.owner", "owner.point", "com.example.second");
    for id in [
        "com.example.owner",
        "com.example.first",
        "com.example.second",
    ] {
        let data = tree_snapshot(&rig.data_dir().join("plugins/data").join(id));
        assert!(
            data.is_empty(),
            "{id} wrote {:?}",
            data.keys().collect::<Vec<_>>()
        );
    }
}

// ---- USR-052 ----

/// REQ: USR-052
#[test]
fn usr_052_a_veto_handler_past_its_deadline_does_not_block_the_action() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_veto_point("owner.before-close"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.slow", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.before-close",
            HookBehaviour::SleepMs(500),
        ),
    );
    rig.start_server();
    let outcome = rig.run_veto_point("com.example.owner", "owner.before-close");
    assert!(outcome.action_went_ahead);
    assert!(!outcome.vetoed);
}

/// REQ: USR-052
#[test]
fn usr_052_trap_error_and_timeout_all_count_as_no_change() {
    for behaviour in [
        HookBehaviour::Trap,
        HookBehaviour::ReturnError,
        HookBehaviour::SleepMs(500),
    ] {
        let mut rig = UserRig::new();
        install_ok(
            &mut rig,
            &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
        );
        install_ok(
            &mut rig,
            &spec("com.example.bad", "1.0.0", &[]).hook(
                "com.example.owner",
                "owner.filter",
                behaviour.clone(),
            ),
        );
        rig.start_server();
        let result = rig.run_filter_point("com.example.owner", "owner.filter", "input");
        assert_eq!(result, "input", "{behaviour:?}");
    }
}

/// REQ: USR-052
#[test]
fn usr_052_default_deadline_is_twenty_milliseconds_boundaries() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.edge", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.filter",
            HookBehaviour::AppendAfterMs(19, "!"),
        ),
    );
    rig.start_server();
    assert_eq!(
        rig.run_filter_point("com.example.owner", "owner.filter", "x"),
        "x!",
        "19 ms is inside the deadline"
    );
    let mut slow = UserRig::new();
    install_ok(
        &mut slow,
        &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
    );
    install_ok(
        &mut slow,
        &spec("com.example.edge", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.filter",
            HookBehaviour::AppendAfterMs(21, "!"),
        ),
    );
    slow.start_server();
    assert_eq!(
        slow.run_filter_point("com.example.owner", "owner.filter", "x"),
        "x",
        "21 ms is past it"
    );
}

// ---- USR-053 ----

/// REQ: USR-053
#[test]
fn usr_053_three_breaches_stop_the_extension_with_a_notice_and_reset() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.bad", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.filter",
            HookBehaviour::Trap,
        ),
    );
    rig.start_server();
    for _ in 0..2 {
        let _ = rig.run_filter_point("com.example.owner", "owner.filter", "x");
    }
    assert!(
        rig.extension_active("com.example.owner", "owner.filter", "com.example.bad"),
        "two breaches (n-1)"
    );
    let _ = rig.run_filter_point("com.example.owner", "owner.filter", "x");
    assert!(
        !rig.extension_active("com.example.owner", "owner.filter", "com.example.bad"),
        "three breaches (n)"
    );
    assert!(
        rig.notices()
            .iter()
            .any(|text| text.contains("stopped after 3 failures") || text.contains("stopped")),
        "{:?}",
        rig.notices()
    );
    assert!(rig.extension_reset_offered("com.example.owner", "owner.filter", "com.example.bad"));
}

/// REQ: USR-053
#[test]
fn usr_053_the_stop_is_not_written_to_disk_and_a_restart_clears_it() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.bad", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.filter",
            HookBehaviour::Trap,
        ),
    );
    rig.start_server();
    for _ in 0..3 {
        let _ = rig.run_filter_point("com.example.owner", "owner.filter", "x");
    }
    let config_before = tree_snapshot(&rig.config_dir());
    let state_snapshot = tree_snapshot(&rig.data_dir().join("plugins"));
    rig.restart_server();
    assert!(rig.extension_active("com.example.owner", "owner.filter", "com.example.bad"));
    assert_eq!(tree_snapshot(&rig.config_dir()), config_before);
    assert_eq!(
        tree_snapshot(&rig.data_dir().join("plugins"))
            .keys()
            .collect::<Vec<_>>(),
        state_snapshot.keys().collect::<Vec<_>>()
    );
}

/// REQ: USR-053
#[test]
fn usr_053_reset_makes_the_extension_active_again() {
    let mut rig = UserRig::new();
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_filter_point("owner.filter"),
    );
    install_ok(
        &mut rig,
        &spec("com.example.bad", "1.0.0", &[]).hook(
            "com.example.owner",
            "owner.filter",
            HookBehaviour::Trap,
        ),
    );
    rig.start_server();
    for _ in 0..3 {
        let _ = rig.run_filter_point("com.example.owner", "owner.filter", "x");
    }
    rig.extension_reset("com.example.owner", "owner.filter", "com.example.bad");
    assert!(rig.extension_active("com.example.owner", "owner.filter", "com.example.bad"));
}

// ---- USR-054 ----

/// REQ: USR-054
#[test]
fn usr_054_missing_owner_shows_waiting_and_reinstall_activates() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    let _ = rig.cli(&["uninstall", "com.example.owner", "--yes"]);
    assert_eq!(
        rig.extension_label("com.example.owner", "owner.point", "com.example.first"),
        "waiting for com.example.owner"
    );
    install_ok(
        &mut rig,
        &spec("com.example.owner", "1.0.0", &[]).defines_point("owner.point"),
    );
    rig.start_server();
    assert!(rig.extension_active("com.example.owner", "owner.point", "com.example.first"));
}

/// REQ: USR-054
#[test]
fn usr_054_owner_turned_off_shows_waiting_and_keeps_the_toggle() {
    let mut rig = UserRig::new();
    owner_and_two_contributors(&mut rig);
    rig.start_server();
    rig.extensions_toggle(
        "com.example.owner",
        "owner.point",
        "com.example.first",
        false,
    );
    let _ = rig.cli(&["disable", "com.example.owner"]);
    assert_eq!(
        rig.extension_label("com.example.owner", "owner.point", "com.example.second"),
        "waiting for com.example.owner"
    );
    let _ = rig.cli(&["enable", "com.example.owner"]);
    assert!(rig.extension_active("com.example.owner", "owner.point", "com.example.second"));
    assert!(
        !rig.extension_active("com.example.owner", "owner.point", "com.example.first"),
        "the user's switch kept its setting"
    );
}
