/// REQ: PDV-015
#[test]
fn pdv_015_status_json_fields() {
    // Observable: Run the command on a machine with a system policy and a stricter user file: the JSON has `policy_sha256`, `loaded_at`, `schema_version`, an `origins` map with `system` or `user` per key, and `ignored_files` as an integer.
    panic!(
        "PDV-015 not implemented: `ilium plugin policy status --json` MUST print `policy_sha256` of the system file, `loaded_at`, `schema_version`, the origin (`system` or `user`) of each effective key, and the count of ignored files."
    );
}

/// REQ: PDV-016
#[test]
fn pdv_016_policy_sha256_matches_audit() {
    // Observable: Compute `sha256sum` of the system policy file right after start: it equals `policy_sha256` from the status command and the `after` field of the last `policy-reloaded` audit line.
    panic!(
        "PDV-016 not implemented: `policy_sha256` MUST be the SHA-256 of the system policy file bytes as read at start, and MUST equal the `after` hash of the latest `policy-reloaded` audit record."
    );
}

/// REQ: PDV-017
#[test]
fn pdv_017_pending_restart_on_file_change() {
    // Observable: Append a comment line to the system policy file while the server runs and run the status command: the JSON has `pending_restart: true` and the exit code is 4.
    panic!(
        "PDV-017 not implemented: When the file on disk differs from the loaded hash, the command MUST report `pending_restart: true` and exit with code 4."
    );
}

/// REQ: PDV-018
#[test]
fn pdv_018_ignored_policy_reports_reason() {
    // Observable: Make the system policy file world-writable and start the server: the status command prints `policy: "ignored"` with `reason` naming the permission problem and exits 3.
    panic!(
        "PDV-018 not implemented: If the system file was ignored under [SEC-182], the command MUST report `policy: 'ignored'` with the reason and exit with code 3."
    );
}

/// REQ: PDV-019
#[test]
fn pdv_019_status_reads_only_local_files() {
    // Observable: Run the command under a network namespace with no interfaces: it exits with the same output and code as with networking available.
    panic!(
        "PDV-019 not implemented: The status command MUST read only local files and MUST NOT open any network connection."
    );
}

/// REQ: PDV-020
#[test]
fn pdv_020_no_notice_for_looser_user_values() {
    // Observable: Set a looser value for a key in the user file and run the status command: the JSON contains the system value only and no `notice` field.
    panic!(
        "PDV-020 not implemented: The output MUST NOT include notice text or user-file values that are not stricter than the system file."
    );
}
