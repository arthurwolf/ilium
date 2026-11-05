/// REQ: PDZ-007
#[test]
fn pdz_007_kill_falls_back_to_offline_path_after_2s() {
    // Observable: Stop the server loop with SIGSTOP and run the kill command: within 3 s it exits 0, prints `path: offline`, and the helper pid is gone.
    panic!(
        "PDZ-007 not implemented: `ilium plugin kill <id> --reason <text>` MUST fall back, after 2 s without a server reply, to writing the kill state file and sending `cgroup.kill` to the plugin's helper cgroup leaf, and MUST print which path (`server` or `offline`) it used."
    );
}

/// REQ: PDZ-008
#[test]
fn pdz_008_offline_kill_state_file_honoured_at_next_start() {
    // Observable: With the server stopped, run `ilium plugin kill example.sync --reason test`, start the server, and run `ilium plugin list --json`: the plugin's state is `disabled-by-admin` and no `plugin.activated` log line exists for it.
    panic!(
        "PDZ-008 not implemented: A kill state file written while the server is stopped MUST be honoured at the next start before any activation of that plugin, leaving it in state `disabled-by-admin`."
    );
}

/// REQ: PDZ-009
#[test]
fn pdz_009_helper_cgroup_leaf_path_pattern_and_counted_failure() {
    // Observable: Write `1` to `cgroup.kill` of `.../helpers/example.sync/` while the plugin runs: the helper exits, `ilium plugin health example.sync --json` shows `failures_in_window: 1`, and the leaf path matches the pattern.
    panic!(
        "PDZ-009 not implemented: Helper cgroup leaves MUST follow the documented path pattern `<ilium cgroup>/helpers/<plugin-id>/` so an operator can find them by plugin id, and `cgroup.kill` written to that leaf MUST be treated by the server as one counted helper failure."
    );
}

/// REQ: PDZ-010
#[test]
fn pdz_010_supervised_heartbeat_and_stall_exit_status_70() {
    // Observable: Run the server with `--supervised` and block its loop in the harness for 31 s: the heartbeat file stops updating and the server process exits with status 70.
    panic!(
        "PDZ-010 not implemented: When run under a supervisor (`--supervised`), the server MUST write a heartbeat file every 1 s and MUST exit with status 70 if its main loop has not ticked for 30 s, beyond the `host.loop-stalled` detection of [HST-618]."
    );
}

/// REQ: PDZ-011
#[test]
fn pdz_011_offline_kill_writes_audit_record_with_path_offline() {
    // Observable: After an offline kill and a restart, `ilium plugin audit --event plugin.killed --plugin example.sync --json` prints one line with `path: \"offline\"`.
    panic!(
        "PDZ-011 not implemented: An offline kill MUST write one `plugin.killed` audit record with the field `path` set to `offline`, written at the next start if the server was not running."
    );
}

/// REQ: PDZ-012
#[test]
fn pdz_012_kill_by_any_path_never_calls_plugin_exports() {
    // Observable: Kill a plugin whose deactivate export writes a marker file: after the kill the marker file does not exist.
    panic!(
        "PDZ-012 not implemented: A kill by any path MUST NOT call any export of the plugin, including its deactivate export."
    );
}

/// REQ: PDZ-013
#[test]
fn pdz_013_disable_all_third_party_stops_plugins_without_restart() {
    // Observable: Run the command with three third-party and two core plugins active: within 1 s the server reports all three as `disabled-by-admin`, the command output lists the two core ids under `skipped`, and the server pid is unchanged.
    panic!(
        "PDZ-013 not implemented: `ilium plugin disable --all-third-party --reason <text>` ([SEC-457]) MUST stop every non-core plugin within 1 s on the server and within 5 s on each attached client without restarting the server, and MUST print the ids of the core plugins it skipped."
    );
}

/// REQ: PDZ-014
#[test]
fn pdz_014_enable_all_third_party_stops_at_first_failed_grant_review() {
    // Observable: Disable all, then decline the grant review of the second plugin in install order during re-enable: the first is `enabled`, the second and third remain `disabled-by-admin`, and the output names the second plugin.
    panic!(
        "PDZ-014 not implemented: `ilium plugin enable --all-third-party` ([SEC-458]) MUST re-enable plugins one at a time in install order and MUST stop at the first plugin that fails its grant review, printing that plugin id and leaving later plugins disabled."
    );
}

/// REQ: PDZ-015
#[test]
fn pdz_015_both_commands_work_in_safe_mode_without_network() {
    // Observable: Start the server in safe mode with networking disabled in the harness and run `ilium plugin disable --all-third-party --reason test`: the exit code is 0 and the state file lists every third-party plugin as `disabled-by-admin`.
    panic!(
        "PDZ-015 not implemented: Both commands MUST work in safe mode and with no network available."
    );
}
