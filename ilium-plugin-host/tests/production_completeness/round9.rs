// Round-nine production requirements (PRS IDs), red until implemented.

/// REQ: PRS-001
#[test]
fn prs_001_for_a_pane_marked_private_or() {
    // Observable: A test marks a pane private, makes its agent finish with output containing a unique marker string, and asserts the desktop notification stub, the toast and the notice log file contain the generic label and never the marker string.
    panic!("PRS-001 not implemented: For a pane marked private, or in a private project, the Notifications plugin MUST NOT place the pane title, process name, command line or agent text in a desktop notification, a toast shown on a shared or mirrored client, or the durable notice log; it MUST show only a generic label naming the event kind and a pane number, and the full text MUST be shown only inside the focused client.");
}

/// REQ: PRS-002
#[test]
fn prs_002_when_the_session_server_receives_sigterm() {
    // Observable: Start a server with a fixture plugin that has an acknowledged pending storage write, send SIGTERM, and assert that deactivate is called, the write is on disk, the audit file ends in a complete record and the process exits with code 0 within 10 seconds.
    panic!("PRS-002 not implemented: When the session server receives SIGTERM, SIGINT or SIGHUP (or the Windows console close, logoff or shutdown event), it MUST begin the same graceful stop as a user stop request, with the same 5 second total deactivation budget, flush acknowledged storage writes and the audit log, and exit within 10 seconds, and a second signal during the stop MUST NOT skip the flush");
}
