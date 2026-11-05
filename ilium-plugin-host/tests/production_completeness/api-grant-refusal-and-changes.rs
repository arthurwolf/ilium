/// REQ: PDY-007
#[test]
fn pdy_007_refused_call_returns_not_permitted_result() {
    // Observable: Call `fs.read` from a plugin whose `fs.read` grant was revoked: the result is `not-permitted` with `grant: "fs.read"` and `reason: "revoked"` and no other fields.
    panic!(
        "PDY-007 not implemented: A refused call MUST return the `not-permitted` result with the fields `grant` and `reason`, and `reason` MUST be exactly one of `declined`, `revoked`, `not-granted` or `policy`."
    );
}

/// REQ: PDY-008
#[test]
fn pdy_008_policy_refusal_hides_policy_details() {
    // Observable: Deny `net.fetch` for the plugin in the system policy file and call it: the result carries `reason: "policy"` and the serialized result contains no file path, key name or rule id.
    panic!(
        "PDY-008 not implemented: A refusal with reason `policy` MUST NOT reveal which policy file, key or rule matched."
    );
}

/// REQ: PDY-009
#[test]
fn pdy_009_refused_call_returns_fast_without_prompt() {
    // Observable: Call an un-granted capability without first-use consent 100 times in the harness: every call returns `not-permitted` in under 50 ms and the harness records zero prompts.
    panic!(
        "PDY-009 not implemented: A refused call MUST return within 50 ms and MUST NOT open a prompt unless the grant is defined with first-use consent."
    );
}

/// REQ: PDY-010
#[test]
fn pdy_010_grant_set_changed_event_delivered_within_one_second() {
    // Observable: Revoke `fs.read` with the CLI while the plugin runs: within 1 s the plugin's event handler receives `grant-set-changed` with `removed: ["fs.read"]`, `added: []` and the new epoch, and no other keys.
    panic!(
        "PDY-010 not implemented: The host MUST deliver the plugin event `grant-set-changed` within 1 s of each epoch bump, carrying only the `added` and `removed` grant names and the new `epoch`; it is distinct from the server-to-client `grants-changed` control record."
    );
}

/// REQ: PDY-011
#[test]
fn pdy_011_grants_current_lists_only_manifest_grants() {
    // Observable: Call `grants.current` from a plugin whose manifest requests `fs.read` and `clipboard.read` while the user holds other plugins' grants: the result contains at most those two names with their states.
    panic!(
        "PDY-011 not implemented: A plugin MUST be able to read its current grant set through `grants.current`, and the result MUST list only grants its manifest requests, never grants it was not offered."
    );
}

/// REQ: PDY-012
#[test]
fn pdy_012_no_retry_refused_call_until_grant_set_changed() {
    // Observable: Retry a call refused as `declined` ten times without an intervening grant event: every retry returns `not-permitted` with reason `declined` and the harness records zero prompts.
    panic!(
        "PDY-012 not implemented: A plugin MUST NOT retry a call refused with reason `declined`, `revoked` or `policy` until it receives a `grant-set-changed` event that adds the grant, and the host MUST return `not-permitted` without a prompt on such retries."
    );
}
