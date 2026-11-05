//! Requirement tests for the round-2 runtime rows: the SDK host-info wrappers (API-217, API-218),
//! the telemetry counters (API-219 .. API-221), the plugin-to-plugin documentation and examples
//! (API-222, API-223), the self-limits and self-describe imports (API-224 .. API-227), the codec
//! imports (API-228, API-229), the pack and two-minor guide rows (AUT-956 .. AUT-958), and the
//! service consumer version rows (SVC-031, SVC-032).
//!
//! No entry point for these behaviours exists in the plugin host crate yet (no `telemetry`,
//! `limits`, `describe`, `hmac-sha256`, `constant-time-equal`, `host_info` SDK wrapper, service
//! provider version read, or `pack.requires-untested` check). Each test therefore fails with a
//! message that starts with its requirement ID and states what the requirement needs. No
//! assertion is ignored, gated or weakened.

/// REQ: API-217
#[test]
fn api_217_sdk_host_info_wrappers_return_host_errors() {
    panic!(
        "API-217 not implemented: the Rust SDK host_info() must return Result<HostInfo, PluginError> \
         with the api version, build id, enabled unstable features and limits, and the TypeScript \
         hostInfo() must return the same fields; neither may panic or throw when the host returns \
         an error for the import, and each must return that error as its result"
    );
}

/// REQ: API-218
#[test]
fn api_218_unstable_feature_fallback_runs_when_host_does_not_list_it() {
    panic!(
        "API-218 not implemented: a plugin must check the enabled-feature list from host_info() or \
         hostInfo() before the first call of an unstable feature and take the documented fallback \
         when the feature is absent; the SDK pages must show this check in Rust and TypeScript, \
         with a fallback test against a fake host whose host_info() does not list the feature"
    );
}

/// REQ: API-219
#[test]
fn api_219_telemetry_count_appears_in_stats_under_plugin_prefix() {
    panic!(
        "API-219 not implemented: the telemetry import must provide count(name: string, delta: s64) \
         -> result<_, plugin-error>, and a counter must appear in the stats output under the \
         plugin. prefix after a valid call"
    );
}

/// REQ: API-220
#[test]
fn api_220_telemetry_count_refuses_bad_name_overflow_and_seventeenth_name() {
    panic!(
        "API-220 not implemented: count must refuse a name not matching ^[a-z][a-z0-9_]{{0,31}}$ \
         with invalid-argument, refuse a delta that overflows the running s64 total with \
         invalid-argument leaving the counter unchanged, and refuse a 17th distinct name on one \
         plugin side with quota-exceeded"
    );
}

/// REQ: API-221
#[test]
fn api_221_custom_counter_survives_plugin_restart_and_resets_on_host_restart() {
    panic!(
        "API-221 not implemented: a custom counter must keep its total across a plugin restart, \
         start again at zero when the host process restarts, and the api-telemetry page must state \
         the same rule as the reference host telemetry page"
    );
}

/// REQ: API-222
#[test]
fn api_222_plugin_to_plugin_page_has_row_per_path() {
    panic!(
        "API-222 not implemented: the api-plugin-to-plugin page must contain a table with one row \
         for each of service call, command invoke, extension run-hook and topic message, giving its \
         deadline, its error codes, whether it can be cancelled, and a link to its owner page"
    );
}

/// REQ: API-223
#[test]
fn api_223_plugin_to_plugin_examples_handle_not_permitted_and_timed_out() {
    panic!(
        "API-223 not implemented: the api-plugin-to-plugin page must give one Rust and one \
         TypeScript example of plugin A calling plugin B that handles a not-permitted refusal and a \
         timed-out completion; each example must be run against a fake plugin B that refuses with \
         not-permitted and then times out, and must handle both results"
    );
}

/// REQ: API-224
#[test]
fn api_224_limits_import_reports_effective_remaining_and_reset_per_budget() {
    panic!(
        "API-224 not implemented: the host must provide a read-only limits() import returning, for \
         the calling plugin side, the effective value and remaining amount of each reachable budget \
         (filesystem read and write caps, daily inference tokens and requests, live handles, memory \
         bytes, event queue bytes), with the reset time where a budget resets"
    );
}

/// REQ: API-225
#[test]
fn api_225_limits_hides_other_budgets_and_is_stable_without_spend() {
    panic!(
        "API-225 not implemented: limits() must not reveal another plugin's budget or the user's \
         account totals, and two calls with no intervening spend must return the same values"
    );
}

/// REQ: API-226
#[test]
fn api_226_describe_returns_identity_versions_and_granted_grants() {
    panic!(
        "API-226 not implemented: the host must provide a read-only describe() import returning the \
         calling plugin's id, its version, its manifest api string, the host version, and the list of \
         grants currently granted to the calling side"
    );
}

/// REQ: API-227
#[test]
fn api_227_describe_grants_follow_grant_changes_within_one_event() {
    panic!(
        "API-227 not implemented: the grant list returned by describe() must change within one event \
         delivery after a grant is granted or revoked, and must never include a refused grant"
    );
}

/// REQ: API-228
#[test]
fn api_228_codec_hmac_sha256_matches_vector_and_refuses_long_key() {
    panic!(
        "API-228 not implemented: the codec import must provide hmac-sha256(key: list<u8>, data: \
         list<u8>) -> result<list<u8>, plugin-error> returning the 32-byte MAC, and must refuse a key \
         longer than 4 KiB with invalid-argument"
    );
}

/// REQ: API-229
#[test]
fn api_229_codec_constant_time_equal_rejects_length_mismatch_and_has_stable_timing() {
    panic!(
        "API-229 not implemented: the codec import must provide constant-time-equal(a: list<u8>, b: \
         list<u8>) -> bool, returning false when the lengths differ and taking the same time for \
         equal-length inputs whatever their content"
    );
}

/// REQ: AUT-956
#[test]
fn aut_956_pack_refuses_untested_host_versions_listing_each_missing_one() {
    panic!(
        "AUT-956 not implemented: pack must refuse with pack.requires-untested and exit 1 when any \
         host contract version admitted by the manifest requires range is missing from tested_hosts \
         in dist/build.json, and the message must list every missing version"
    );
}

/// REQ: AUT-957
#[test]
fn aut_957_two_minor_worked_page_commands_and_golden_layouts_hold() {
    panic!(
        "AUT-957 not implemented: the manual must contain one worked page taking a plugin from a \
         single-minor requires range to a two-minor range, with the commands, the golden directory \
         layout per minor and the expected output of each step; following it in a scratch copy of \
         the examples repository must give the stated exit codes and golden folder paths"
    );
}

/// REQ: AUT-958
#[test]
fn aut_958_two_minor_worked_page_steps_run_as_examples_ci_job() {
    panic!(
        "AUT-958 not implemented: the worked two-minor page steps must run as a CI job in the \
         examples repository, and that job must fail when the page's commands or expected output \
         change"
    );
}

/// REQ: SVC-031
#[test]
fn svc_031_consumer_waits_for_compatible_provider_and_calls_return_not_found() {
    panic!(
        "SVC-031 not implemented: a consumer's requires.services range must be checked against the \
         registered provider's version; while no provider version satisfies it the consumer must stay \
         waiting with no provider method run, and a call made in that state must complete as \
         not-found with no partial reply"
    );
}

/// REQ: SVC-032
#[test]
fn svc_032_consumer_reads_provider_version_and_fallback_uses_matching_payload() {
    panic!(
        "SVC-032 not implemented: a consumer must be able to read the version of the registered \
         provider of a name (no such call exists in the 0.1 service surface of the reference WIT), \
         and the Tutorial 22 fallback path must use that version to choose the 1.0 or 1.1 payload; \
         registering a provider at 1.1.0 must read back as 1.1.0 and send the 1.1 payload"
    );
}
