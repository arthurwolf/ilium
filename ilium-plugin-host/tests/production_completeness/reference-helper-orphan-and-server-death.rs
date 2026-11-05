/// REQ: PDZ-001
#[test]
fn pdz_001_helpers_set_pdeathsig_before_reading_plugin_bytes() {
    // Observable: Start a server with two active plugins, send SIGKILL to the server pid, and list processes 1 s later: no process whose command line names a helper for that server pid remains.
    panic!(
        "PDZ-001 not implemented: Each helper MUST set `PR_SET_PDEATHSIG` to SIGKILL before it reads any plugin bytes, so that SIGKILL of the server ends every helper within 1 s."
    );
}

/// REQ: PDZ-002
#[test]
fn pdz_002_helper_exits_on_dispatcher_eof_fallback() {
    // Observable: Close the dispatcher socket of a running helper from the server side without killing the server: the helper pid is gone within 2 s and its exit status is 0.
    panic!(
        "PDZ-002 not implemented: A helper MUST also exit within 2 s when its dispatcher channel reaches end-of-file, as a fallback for hosts without parent-death signals."
    );
}

/// REQ: PDZ-003
#[test]
fn pdz_003_start_reaps_stale_cgroup_leaves_before_activation() {
    // Observable: Create a stale leaf containing a sleeping process under the helper cgroup path pattern and start the server: the process is gone and the leaf directory is removed before the first `plugin.activated` log line.
    panic!(
        "PDZ-003 not implemented: At start the server MUST scan for helper cgroup leaves left by a previous boot id or server pid, kill their processes and remove the leaves before activating any plugin."
    );
}

/// REQ: PDZ-004
#[test]
fn pdz_004_stale_helper_logged_and_written_to_audit() {
    // Observable: After the stale-leaf start, the log contains one `host.helper-orphan-reaped` line and `ilium plugin audit --event helper-orphan-reaped --json` prints one line with `plugin_id` and `previous_server_pid`.
    panic!(
        "PDZ-004 not implemented: Each stale helper found at start MUST be logged with the code `host.helper-orphan-reaped` and written to the audit log as one `helper-orphan-reaped` record with the plugin id and the previous server pid."
    );
}

/// REQ: PDZ-005
#[test]
fn pdz_005_helper_gone_within_5s_after_server_pid_exit() {
    // Observable: Kill the server with SIGKILL while a plugin runs a busy loop, wait 5 s, and check: no helper pid remains and the helper cgroup leaf is empty.
    panic!(
        "PDZ-005 not implemented: A helper MUST NOT survive more than 5 s after the server pid is gone, whatever the cause of the server's exit."
    );
}

/// REQ: PDZ-006
#[test]
fn pdz_006_next_start_does_not_count_reaped_orphans_as_failures() {
    // Observable: Reap one orphan at start, then run `ilium plugin health example.sync --json`: the `failures_in_window` field is 0.
    panic!(
        "PDZ-006 not implemented: The next start MUST NOT count reaped orphans as plugin failures in the three-failure window."
    );
}
