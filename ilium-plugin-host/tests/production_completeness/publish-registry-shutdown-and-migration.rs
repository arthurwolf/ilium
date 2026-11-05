/// REQ: PDW-001
#[test]
fn pdw_001_signed_sunset_notice_at_least_180_days_before_expiry() {
    // Observable: Verify a snapshot carrying `sunset.ends_at` less than 180 days before the final timestamp expiry with `ilium plugin registry doctor`: it reports `registry.sunset-too-short`.
    panic!(
        "PDW-001 not implemented: An operator ending service MUST publish a signed `sunset` notice in `snapshot.json` at least 180 days before the last signed `timestamp.json` expires."
    );
}

/// REQ: PDW-002
#[test]
fn pdw_002_read_only_freeze_refuses_writes_with_read_only_error() {
    // Observable: Run `ilium plugin publish` against a registry in freeze: the exit code is non-zero and the output contains `registry.read-only`, while `ilium plugin install` of an existing package still succeeds.
    panic!(
        "PDW-002 not implemented: During the freeze the registry MUST serve existing metadata and packages read-only and MUST refuse publish, yank and owner changes with `registry.read-only`."
    );
}

/// REQ: PDW-003
#[test]
fn pdw_003_verified_sunset_shows_ending_date_without_blocking() {
    // Observable: Point a client at a registry with a verified sunset notice and run `ilium plugin list`: the output contains `registry ending on` followed by the date, and `ilium plugin install` of a listed package exits 0.
    panic!(
        "PDW-003 not implemented: Once a sunset notice is verified, a client MUST show `registry ending on <date>` in `ilium plugin list` and `ilium plugin registry doctor`, and MUST NOT block install, update or removal."
    );
}

/// REQ: PDW-004
#[test]
fn pdw_004_handover_root_must_be_signed_by_previous_threshold() {
    // Observable: Serve a root.json version 3 signed only by new keys: `ilium plugin registry doctor` reports `registry.root-invalid` and the pinned root stays at version 2.
    panic!(
        "PDW-004 not implemented: A handover to a new operator MUST be published as a `root.json` version signed by the threshold of the previous root keys, and a client MUST refuse a new root not so signed with `registry.root-invalid`."
    );
}

/// REQ: PDW-005
#[test]
fn pdw_005_expired_metadata_blocks_updates_keeps_plugins_running() {
    // Observable: Advance the clock past the final timestamp expiry: `ilium plugin update` exits non-zero with `registry.metadata-expired` and `ilium plugin list` still shows the installed plugin as `enabled`.
    panic!(
        "PDW-005 not implemented: After the last timestamp expires, a client MUST report `registry.metadata-expired`, refuse new installs and updates from that registry, and keep installed plugins running without deleting them."
    );
}

/// REQ: PDW-006
#[test]
fn pdw_006_url_move_requires_signed_root_and_user_confirmation() {
    // Observable: Serve an HTTP 301 from the old registry URL to a new host: the client keeps the old pinned URL and `registry doctor` reports `registry.redirect-ignored`; with a signed `url` change the client prompts and re-pins only after `y`.
    panic!(
        "PDW-006 not implemented: A URL move MUST be a `url` field in a `root.json` version signed by the current root, and the client MUST re-pin the new URL only after the user confirms in a prompt, never from an HTTP redirect."
    );
}

/// REQ: PDW-007
#[test]
fn pdw_007_mirror_keeps_last_verified_snapshot_labelled_frozen() {
    // Observable: Run `ilium plugin mirror sync --json` against an upstream that has shut down: the output line has `status: "frozen"` and the mirror still serves the last snapshot's packages.
    panic!(
        "PDW-007 not implemented: A mirror MUST keep serving the last verified snapshot after the upstream shutdown and MUST label it `frozen` in its sync report."
    );
}
