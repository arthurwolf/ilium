// Round-seven production requirements (PRU IDs), red until implemented.

/// REQ: PRU-001
#[test]
fn pru_001_the_scheduler_must_not_deliver_a() {
    // Observable: A test schedules the text y plus Enter for a fixture agent pane, drives the pane into awaiting-approval before the deadline, and asserts no byte reaches the PTY until the activity changes or Send now is chosen, with one notice raised.
    panic!("PRU-001 not implemented: The Scheduler MUST NOT deliver a scheduled input or queued prompt to a pane whose agent activity is awaiting-approval at delivery time; it MUST hold the entry, raise one notice naming the pane, and deliver only when the activity leaves awaiting-approval or the user chooses Send now.");
}

/// REQ: PRU-002
#[test]
fn pru_002_the_editor_must_save_by_writing() {
    // Observable: A test saves a buffer opened through a symlink to a mode 0600 file after a simulated write failure midway, and asserts the link still exists, the target keeps its mode and original bytes, and no partial file remains.
    panic!("PRU-002 not implemented: The Editor MUST save by writing a temporary file in the target directory and renaming it over the target, MUST resolve a symlinked path and replace its target rather than the link, and MUST preserve the file mode and owner where the platform allows; if any of these cannot be kept it MUST refuse the save with a visible reason and leave the original file unchanged.");
}

/// REQ: PRU-003
#[test]
fn pru_003_the_worktrees_plugin_must_refuse_to() {
    // Observable: A test creates a repository whose config sets core.fsmonitor to a script that writes a marker file, requests a worktree, and asserts the marker is never written; a repository owned by another uid yields refused(unsafe-repository) before any git command runs.
    panic!("PRU-003 not implemented: The Worktrees plugin MUST refuse to run git in a repository that is not owned by the current user, with refused(unsafe-repository), and MUST run every git command with core.fsmonitor and core.pager disabled and without honouring repository-local program settings, so that repository configuration cannot start a program outside the process.run grant.");
}

/// REQ: PRU-004
#[test]
fn pru_004_the_notifications_plugin_must_remove_every() {
    // Observable: A test sets a pane title containing an ESC color sequence, a newline and markup tags, triggers a done notification, and asserts the desktop notifier body and the toast contain only escaped printable text of at most 256 cells.
    panic!("PRU-004 not implemented: The Notifications plugin MUST remove every C0 and C1 control character and every ESC-initiated sequence from pane titles, process names and agent text before placing them in a toast, notice or desktop notification, MUST escape markup for the platform notifier, and MUST truncate the body to 256 cells.");
}

/// REQ: PRU-005
#[test]
fn pru_005_a_restore_of_an_archive_older() {
    // Observable: Back up, then revoke a publisher key and a HIGH grant, restore the older archive on the same machine, and assert the key is still revoked, its plugins stay blocked, the grant is absent and the audit log names both.
    panic!("PRU-005 not implemented: A restore of an archive older than the current revocation state MUST NOT weaken it: after restore the host MUST re-apply every key revocation, blocklist entry, recall notice and yank recorded before the restore, MUST drop any grant revoked after the archive was written, and MUST write one audit record naming each item re-applied.");
}

/// REQ: PRU-006
#[test]
fn pru_006_install_must_read_the_package_bytes() {
    // Observable: Serve a URL package that returns different bytes on a second request, or edit a local file while the review dialog is open, and assert the installed content hash equals the reviewed hash or the install is refused with package.changed-after-review.
    panic!("PRU-006 not implemented: Install MUST read the package bytes exactly once into staging, compute the SHA-256 over that staged copy, show the review dialog for that hash, and record the hash with the grant decision. Activation MUST use only the staged copy and MUST refuse with package.changed-after-review if the hash differs.");
}

/// REQ: PRU-007
#[test]
fn pru_007_a_dependency_id_with_no_installed() {
    // Observable: Configure a private and a public registry, require an id owned on the private one, publish a higher version of that id on the public one, and assert the install refuses with dependency.source-mismatch and installs nothing from the public registry.
    panic!("PRU-007 not implemented: A dependency id with no installed copy MUST resolve only from the source and publisher namespace of the requiring plugin, or from a source the administrator lists for that id. A higher version of the same id from another configured source MUST NOT win the tie-break, and the host MUST refuse with dependency.source-mismatch naming both sources.");
}

/// REQ: PRU-008
#[test]
fn pru_008_when_the_operating_system_reports_that() {
    // Observable: Create a watch, flood the watched folder with more events than the OS queue holds in the test harness, and assert that the plugin receives an fs-changed event with resync set for that watch id.
    panic!("PRU-008 not implemented: When the operating system reports that it dropped file change events for a watch (kernel queue overflow, or a watch on a file system that cannot report changes), the host MUST deliver one fs-changed event for that watch with the flag resync set, and the plugin MUST treat it as a request to rescan the watched folder; the host MUST NOT silently continue as if no event was lost.");
}

/// REQ: PRU-009
#[test]
fn pru_009_when_the_host_truncates_a_label() {
    // Observable: Render a row whose text is wider than its box with screen_reader enabled and assert that the screen reader text stream contains the full string while the visual cell ends with an ellipsis.
    panic!("PRU-009 not implemented: When the host truncates a label, title or row text with an ellipsis because it exceeds its box, the full untruncated text MUST remain available to the screen reader mode text output and to a reveal action (focus or hover detail), so no information is lost by truncation alone.");
}

/// REQ: PRU-010
#[test]
fn pru_010_server_start_must_scale_with_the() {
    // Observable: Install 500 stub plugins with lazy activation, start the server, and assert that startup time is within 20 percent of the time with 5 installed and that no component file of a non-activated plugin was opened.
    panic!("PRU-010 not implemented: Server start MUST scale with the number of enabled plugins that activate at startup, not with the number installed: the host MUST read only installed.json and the manifests of installed plugins at start, MUST NOT read or compile the component of a plugin that has not activated, and with 500 installed plugins of which 5 activate at startup the server MUST be ready for input within the same budget as with 5 installed.");
}

/// REQ: PRU-011
#[test]
fn pru_011_a_peer_message_that_the_host() {
    // Observable: Send a message to a plugin whose queue holds a pending message, stop the server before the receiver handler runs, restart it, and assert the handler never receives the message, the discard is logged with the receiver id and reason, and the sender completion text said queued and not delivered.
    panic!("PRU-011 not implemented: A peer message that the host accepted but has not yet delivered MUST NOT be kept across a server stop, a receiver disable, an uninstall or a receiver upgrade or restart: the host MUST discard it, count it in the receiver message-dropped counter with the reason, and report the sender intent outcome as accepted-for-queue only, so that a sender needing delivery must use a reply or a dedup-key acknowledgement from the receiver.");
}
