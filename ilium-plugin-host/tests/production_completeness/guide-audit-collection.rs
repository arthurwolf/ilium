/// REQ: PDV-021
#[test]
fn pdv_021_key_records_by_machine_id_seq_and_flag_tampering() {
    // Observable: Replay a record with the same machine id and `seq` but a changed `detail`: the reference collector raises a `tamper` alert naming the machine id and `seq`.
    panic!(
        "PDV-021 not implemented: A collector MUST key records by machine id plus `seq` and MUST treat a repeated pair arriving with a different record hash as tampering."
    );
}

/// REQ: PDV-022
#[test]
fn pdv_022_raise_gap_alert_on_seq_skip_or_prev_hash_mismatch() {
    // Observable: Drop one record from a forwarded stream: the reference collector raises a `gap` alert with the expected and received `seq`.
    panic!(
        "PDV-022 not implemented: A collector MUST raise a gap alert when `seq` skips a value or when `prev_hash` does not equal the hash of the previous accepted record."
    );
}

/// REQ: PDV-023
#[test]
fn pdv_023_central_retention_at_least_host_audit_keep_days() {
    // Observable: Configure the reference collector with retention 30 days against a host with `audit.keep_days = 365`: its configuration check fails with `retention-below-host`.
    panic!(
        "PDV-023 not implemented: Central retention MUST be at least the host `audit.keep_days` value (default 365)."
    );
}

/// REQ: PDV-024
#[test]
fn pdv_024_syslog_message_one_json_object_max_8192_bytes_truncate_detail() {
    // Observable: Write an audit record with a 20 KiB `detail` and capture the syslog message: it is one JSON object of at most 8192 bytes, `detail` is shortened, `truncated` is `true`, and every other field is intact.
    panic!(
        "PDV-024 not implemented: Each syslog message MUST be one JSON object of at most 8192 bytes, and a record exceeding that MUST be truncated in `detail` only, with `truncated: true` added."
    );
}

/// REQ: PDV-025
#[test]
fn pdv_025_minimum_alert_set_covers_four_integrity_events() {
    // Observable: Load the published alert rule file into the reference collector and forward one record of each of the four events: four alerts fire, one per event.
    panic!(
        "PDV-025 not implemented: The documented minimum alert set MUST include records with event `policy-violation`, `signature-failure`, `blocklist-hit` and `integrity-failure`."
    );
}

/// REQ: PDV-026
#[test]
fn pdv_026_collectors_order_records_by_seq_not_ts() {
    // Observable: Forward records whose `ts` decreases while `seq` increases: the reference collector stores them in `seq` order and raises no reorder alert.
    panic!(
        "PDV-026 not implemented: Collectors MUST order records by `seq`, never by `ts`, because the host clock may move backwards."
    );
}
