//! Production-gap doc checks: one test per requirement ID added for phased rollout,
//! release and registry key compromise, host verification, ABI majors, SDK deprecation,
//! host aggregate limits, soak testing, CLI completion and output stability, privacy
//! backups, core text localisation and operator upgrades. Each test reads the manual page
//! that states the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: PKG-686
#[test]
fn pkg_686_staged_version_withheld_outside_cohort() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "offered only to clients whose cohort is inside that percentage",
        "MUST NOT choose that version for a new install or an update",
        "keep the staged version in",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-687
#[test]
fn pkg_687_cohort_value_random_and_kept_stable() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "generate one random value on first use",
        "MUST keep the same value across metadata refreshes",
        "MUST NOT send the value, the cohort or any value derived from them",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-688
#[test]
fn pkg_688_zero_percent_halts_new_offers() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "A rollout percentage of 0 MUST stop new offers",
        "MUST NOT activate it once it has seen the halt",
        "keeps it running",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-689
#[test]
fn pkg_689_rollout_change_only_by_publisher() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "MUST be made by the publisher of the plugin",
        "published as a new signed version of",
        "A change by a publisher who does not own the namespace",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-424
#[test]
fn sec_424_compromised_release_key_stops_signing() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "stop signing release blocklists with that key",
        "treat it as compromised",
        "endorse a replacement key",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-425
#[test]
fn sec_425_binary_keeps_last_good_blocklist_on_wrong_key() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "compiled into that binary",
        "keeps its last good copy",
        "refuse",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-426
#[test]
fn sec_426_new_release_key_only_in_new_binary() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "inside a new Ilium release that the user installs",
        "MUST NOT learn a new release key from a blocklist",
        "policy file",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-427
#[test]
fn sec_427_advisory_names_compromised_key() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "publish an advisory that names the compromised key",
        "plugin trust does not change",
        "replacement release",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-690
#[test]
fn pkg_690_root_compromise_rotation_and_repin() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "MUST NOT rely on the rotation chain",
        "Each client MUST re-pin the registry",
        "threshold of root keys is compromised",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-691
#[test]
fn pkg_691_new_root_json_higher_version_and_resign() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "MUST publish a new `root.json` with a higher `version`",
        "MUST refuse a document whose signature uses a key id",
        "re-sign `timestamp.json`, `snapshot.json`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-692
#[test]
fn pkg_692_root_key_change_announced_out_of_band() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "MUST announce a root key change outside the registry files",
        "MUST NOT learn a new root key from the mirror",
        "on a channel that names the new root `key_id`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-693
#[test]
fn pkg_693_user_repins_changed_root_key() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "A user MUST re-pin a registry when a root key was replaced",
        "MUST NOT accept a changed root key without that explicit re-pin",
        "`registry add` without `--root-key` fails with `registry.root-key-missing`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-428
#[test]
fn sec_428_release_archives_listed_with_sha256() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "lists every release archive with its SHA-256 hash",
        "one line per archive",
        "MUST publish a",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-429
#[test]
fn sec_429_user_checks_hash_and_attestation() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "compare the archive's SHA-256 hash with the line in",
        "run the attestation check before running the archive",
        "gh attestation verify",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-430
#[test]
fn sec_430_hash_mismatch_stops_before_unpack() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "before it decompresses or unpacks the archive",
        "Archive SHA-256 mismatch",
        "stop",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-055
#[test]
fn abi_055_frozen_copy_and_adapter_across_major() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "frozen copy and adapter",
        "host that ships",
        "MUST NOT load a package",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-056
#[test]
fn abi_056_unsupported_api_error_names_version() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "package.api-unsupported",
        "the message MUST name the",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-057
#[test]
fn abi_057_deprecated_record_then_stops_loading() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "log one `deprecated` record per plugin",
        "stops loading",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-058
#[test]
fn abi_058_limit_removal_only_in_major_release() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "lower a limit or remove a feature",
        "take effect in that major release only",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-082
#[test]
fn api_082_deprecation_marked_in_code() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "`@deprecated` JSDoc tag",
        "`#[deprecated(since",
        "A changelog entry alone MUST NOT mark an item",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-083
#[test]
fn api_083_deprecated_sdk_item_kept_until_removal_minor() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "until the removal minor named in its mark",
        "A patch release MUST NOT remove it",
        "two minors after the release that marked it",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-084
#[test]
fn api_084_changelog_deprecated_and_removed_headings() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "`Deprecated` heading",
        "`Removed` heading",
        "Each entry MUST name the item, its successor and its removal minor",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-500
#[test]
fn hst_500_aggregate_ceiling_counts_all_processes() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "aggregate ceiling on the memory of every session server, helper and job on one machine",
        "counted against that ceiling",
        "same way that",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-501
#[test]
fn hst_501_admission_passes_aggregate_ceiling() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "MUST pass the aggregate ceiling",
        "every session key that applies to it",
        "MUST NOT skip another",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-502
#[test]
fn hst_502_waiting_requests_admitted_in_order() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "core T1 plugins at server start first",
        "in the order they were made across all sessions",
        "admit waiting requests against the aggregate ceiling",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-503
#[test]
fn hst_503_aggregate_refusal_uses_host_capacity() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "the reason `host-capacity`",
        "MUST NOT count as a failure",
        "MUST be logged as an aggregate refusal",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-504
#[test]
fn hst_504_aggregate_thread_ceiling_refusal() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "aggregate thread ceiling",
        "would exceed that ceiling",
        "MUST NOT count as a failure",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-505
#[test]
fn hst_505_soak_repeats_all_four_steps() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "MUST repeat the four steps above",
        "the number of live instances, the number of helper processes",
        "A run MUST NOT skip a step",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-506
#[test]
fn hst_506_steady_state_counts_stay_flat() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "MUST equal the steady-state counts",
        "The steady state MUST be measured after the first full cycle",
        "MUST NOT grow over the run beyond the tolerance",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-507
#[test]
fn hst_507_soak_fails_on_leaking_instance() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "The soak run MUST fail when a live instance",
        "The failure MUST name the resource and the cycle",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-508
#[test]
fn hst_508_soak_on_reference_machine_release_build() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "MUST run on the reference machine with a release build",
        "MUST be reported as incomplete",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-855
#[test]
fn dev_855_completion_for_four_shells() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "bash, zsh, fish and PowerShell",
        "same command table that `ilium --help` reads",
        "every subcommand that `ilium --help` lists",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-856
#[test]
fn dev_856_plugin_cli_names_checked() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "`contributes.cli`",
        "`manifest.cli-name-taken`",
        "`aliases` list",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-857
#[test]
fn dev_857_man_pages_generated_offline() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "generated offline",
        "one page for each kernel command",
        "MUST NOT contact a registry, a mirror or a running server",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-858
#[test]
fn dev_858_cli_deprecated_before_it_stops_working() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST first be marked deprecated",
        "the release in which it stops working",
        "MUST keep working as before",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-859
#[test]
fn dev_859_deprecated_cli_notice_once_per_invocation() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST print a notice",
        "once per invocation",
        "MUST name the item",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-860
#[test]
fn dev_860_output_change_only_in_contract_release() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST happen only in a release whose contract version",
        "MUST be listed in that release's notes",
        "change in the type or meaning of a field",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-861
#[test]
fn dev_861_exit_codes_stable_and_listed() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "MUST NOT change in any release",
        "MUST be added to the exit code table",
        "no existing code MUST be given a new meaning",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-236
#[test]
fn usr_236_crash_records_and_backups_listed() {
    let page = manual_page("user-privacy");
    for phrase in [
        "30 days",
        "200 records",
        "server state folder",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-237
#[test]
fn usr_237_keep_newest_backup_purge_all() {
    let page = manual_page("user-privacy");
    for phrase in [
        "newest pre-migration backup",
        "uninstall <id> --purge",
        "backups/<id>/",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-238
#[test]
fn usr_238_uninstall_without_purge_keeps_backups() {
    let page = manual_page("user-privacy");
    for phrase in [
        "without `--purge` MUST keep the pre-migration backups",
        "so that a reinstall finds them",
        "this rule matches the uninstall rules of the install and uninstall reference",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-239
#[test]
fn usr_239_core_text_keeps_ids_and_surfaces() {
    let page = manual_page("guide-internationalisation");
    for phrase in [
        "keep every command id",
        "settings tab id",
        "manifest.replaces-surface-incomplete",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-240
#[test]
fn usr_240_replacement_supplies_title_and_notice() {
    let page = manual_page("guide-internationalisation");
    for phrase in [
        "supply the title and notice text",
        "returns only when the replacement is turned off",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-295
#[test]
fn lif_295_new_binary_applies_at_next_start() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "takes effect only at the next server start",
        "keep its loaded instances unchanged",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-296
#[test]
fn lif_296_clean_stop_runs_deactivate() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "clean stop, not with a kill signal",
        "skips `deactivate`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-297
#[test]
fn lif_297_host_rollback_not_plugin_rollback() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "MUST NOT change the Ilium binary",
        "ilium plugin rollback",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-298
#[test]
fn lif_298_upgrade_checks_then_restart() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "ilium plugin list --json",
        "only after its cause is fixed",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
