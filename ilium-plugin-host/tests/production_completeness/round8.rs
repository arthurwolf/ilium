// Round-eight production requirements (PRT IDs), red until implemented.

/// REQ: PRT-001
#[test]
fn prt_001_the_sessions_plugin_must_create_snapshot() {
    // Observable: A test saves a session with a secret in a pane environment variable and asserts the snapshot file mode is 0600, the secret value is absent from the file, and a snapshot made group-writable is refused on load with a visible reason.
    panic!("PRT-001 not implemented: The Sessions plugin MUST create snapshot files, rolling backups and recovery copies readable by the owning user only (mode 0600 in a 0700 folder or the platform equivalent), MUST NOT write environment variable values or pane screen contents into a snapshot unless the user has enabled scrollback saving, and MUST refuse to load a snapshot that is group or world writable.");
}

/// REQ: PRT-002
#[test]
fn prt_002_when_copying_files_matched_by_worktreeinclude() {
    // Observable: A test lists a symlink to a file outside the checkout in .worktreeinclude, creates a worktree, and asserts the outside file is not copied or linked into the new worktree and the summary names the skipped entry.
    panic!("PRT-002 not implemented: When copying files matched by .worktreeinclude, the Worktrees plugin MUST skip any entry that is a symbolic link or whose resolved path lies outside the source checkout root, MUST NOT follow links while copying, and MUST report each skipped entry in the creation summary.");
}

/// REQ: PRT-003
#[test]
fn prt_003_the_inference_plugin_must_refuse_to() {
    // Observable: A test configures a provider with an http URL on a non-loopback address and then an https endpoint that redirects to another host, and asserts both requests complete with refused and that no authorization header reaches either host.
    panic!("PRT-003 not implemented: The Inference plugin MUST refuse to send a request, and MUST refuse to send a credential, to a non-loopback provider endpoint that is not https, MUST validate the server certificate without a user-skippable bypass, and MUST NOT follow a redirect to a different host or to http.");
}

/// REQ: PRT-004
#[test]
fn prt_004_raw_captured_audio_must_stay_in() {
    // Observable: A test records a phrase with retention off, ends transcription by success and by error, and asserts no audio file remains, no network connection was opened, and the file mode of any temporary file was 0600.
    panic!("PRT-004 not implemented: Raw captured audio MUST stay in memory or in an owner-only temporary file, MUST be sent only to a speech model running on the machine unless the user has enabled a named remote provider in the same session, and MUST be deleted when transcription ends or fails, unless the user has enabled recording retention.");
}

/// REQ: PRT-005
#[test]
fn prt_005_when_a_reader_closes_standard_output() {
    // Observable: Run `ilium plugin list --json / head -c 1`, and a restore of a large archive piped through `head -c 1`, and assert no panic or backtrace text on standard error, the documented exit status for a closed pipe, and a restore that is either fully applied or fully rolled back.
    panic!("PRT-005 not implemented: When a reader closes standard output early (a broken pipe, for example `ilium plugin list --json / head -1`), every ilium command MUST stop writing, exit quietly without a panic, backtrace or error text on standard error, and MUST NOT leave an install, update, restore or publish half done, because a state-changing command finishes or rolls back its transaction before it exits.");
}

/// REQ: PRT-006
#[test]
fn prt_006_ilium_plugin_restore_and_dry_run() {
    // Observable: Restore archives holding `../../x`, an absolute path, a symlink entry pointing outside the data root, a hard link and a setuid file, and assert `restore.unsafe-entry`, a non-zero exit and no file created outside or inside the data root.
    panic!("PRT-006 not implemented: `ilium plugin restore` and `--dry-run` MUST refuse, with `restore.unsafe-entry` and before writing any file, an archive entry whose path is absolute, contains a `..` segment or a drive prefix, or whose type is a symbolic link, hard link, device, FIFO or socket, and MUST create every restored file with the owner-only mode of the live data root and never with the mode, owner or setuid bits recorded in the archive.");
}

/// REQ: PRT-007
#[test]
fn prt_007_the_json_and_jsonl_output_of() {
    // Observable: Run the same `--json` command with LC_ALL=de_DE.UTF-8 and with LC_ALL=C and assert the two outputs are byte-identical except for the `message` fields.
    panic!("PRT-007 not implemented: The `--json` and JSONL output of every ilium command MUST be independent of the user locale, language setting and the LANG, LC_ALL and LC_NUMERIC variables: numbers use a dot decimal mark and no grouping, timestamps are UTC in the documented form, enumerated values and `code` fields are never translated, and only a `message` field MAY follow the UI language.");
}

/// REQ: PRT-008
#[test]
fn prt_008_when_a_grant_is_revoked_or() {
    // Observable: With terminal.read-screen granted and ten screen-changed events queued for a plugin whose on-events is blocked, revoke the grant, unblock the plugin, and assert that it receives grant-set-changed and none of the ten queued events, and that the same holds for events redelivered after a forced trap.
    panic!("PRT-008 not implemented: When a grant is revoked or expires, the host MUST remove from that plugin side every already queued event, and every event held for redelivery after a trap, whose payload exists only because of that grant (screen cells, prompt text, transcript, editor text, file data), before it delivers grant-set-changed, so that no data gated by the grant reaches the plugin after the epoch bump.");
}

/// REQ: PRT-009
#[test]
fn prt_009_every_ui_action_must_carry_the() {
    // Observable: Render a list of three rows, re-render it with the first row removed before the user input is processed, send a selected action with index 2 for the old tree, and assert the plugin receives no selected action and receives one view-changed notice naming the view.
    panic!("PRT-009 not implemented: Every ui-action MUST carry the revision of the rendered tree that the user acted on, and the host MUST drop, and MUST NOT deliver to the plugin, an action whose key is absent from the current tree or that carries an index or drop value, when the view was re-rendered between the paint the user saw and the input; the plugin sees a view-changed notice instead of the action.");
}

/// REQ: PRT-010
#[test]
fn prt_010_compilation_of_a_t2_or_t3() {
    // Observable: Load a fixture component that stays within the size, function and nesting bounds of SEC-707 but is built to compile slowly, with the harness deadline lowered to 2 seconds: the helper is killed, the load is refused with host.compile-timeout, and the server keeps serving other plugins.
    panic!("PRT-010 not implemented: Compilation of a T2 or T3 component in the sandbox helper MUST have a wall-clock deadline of 60 seconds, and when it passes the host MUST kill the helper, refuse the load with the code host.compile-timeout, and count no more than one failure in the three-failure window.");
}

/// REQ: PRT-011
#[test]
fn prt_011_a_client_downloading_a_package_blob() {
    // Observable: Serve a blob whose index entry declares 523044 bytes but streams an endless body from a test mirror: the client stops at 523045 bytes, prints registry.size, leaves no partial file in the downloads cache, and a second test mirror that sends nothing for 31 seconds yields registry.stalled.
    panic!("PRT-011 not implemented: A client downloading a package blob MUST abort the transfer with registry.size as soon as the received bytes exceed the length field of the signed index entry or the 64 MiB package limit, and MUST abort with registry.stalled when no bytes arrive for 30 seconds, and MUST delete the partial staging file in both cases.");
}
