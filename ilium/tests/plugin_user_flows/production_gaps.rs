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
    for phrase in ["package.api-unsupported", "the message MUST name the"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-057
#[test]
fn abi_057_deprecated_record_then_stops_loading() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in ["log one `deprecated` record per plugin", "stops loading"] {
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
    for phrase in ["30 days", "200 records", "server state folder"] {
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
    for phrase in ["clean stop, not with a kill signal", "skips `deactivate`"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-297
#[test]
fn lif_297_host_rollback_not_plugin_rollback() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in ["MUST NOT change the Ilium binary", "ilium plugin rollback"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-298
#[test]
fn lif_298_upgrade_checks_then_restart() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in ["ilium plugin list --json", "only after its cause is fixed"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-686
#[test]
fn pkg_686_staged_version_falls_back_to_the_choice_without_it_and_full_is_plain() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "It MUST choose the version it would have chosen if the staged version did not exist.",
        "A version at 100 is offered to every client, the same as a version with no rollout.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-687
#[test]
fn pkg_687_cohort_is_decided_on_the_machine_and_stored_locally() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "store that value locally, and compare it with the rollout percentage.",
        "so that it does not move in and out of a rollout between checks.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-688
#[test]
fn pkg_688_halt_reaches_every_client_and_removal_is_a_takedown() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "stop new offers of that version to every client at its next metadata refresh, including clients that are inside the cohort.",
        "A version that must be removed from installed copies is handled by a takedown in the registry reference, not by a halt.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-689
#[test]
fn pkg_689_rollout_change_rollback_protected_and_token_refusals() {
    let page = manual_page("reference-release-rollout");
    for phrase in [
        "so that rollback protection stops an older percentage from being served again.",
        "A missing or refused token MUST be refused with `registry.auth`.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-424
#[test]
fn sec_424_compromised_release_key_signs_no_other_artefact() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "When the release key is lost or may be exposed,",
        "it MUST NOT sign any other release artefact with it.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-425
#[test]
fn sec_425_wrong_key_blocklist_is_refused_with_an_audit_line() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "writes an audit line, and does not apply the new entries.",
        "A release blocklist signed by any other key MUST be refused",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-426
#[test]
fn sec_426_new_release_key_found_in_other_files_is_not_accepted() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "A new release key found in any file other than a new release binary",
        "Not accepted ([SEC-426])",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-427
#[test]
fn sec_427_advisory_fingerprint_and_trust_store_wording() {
    let page = manual_page("reference-release-key-compromise");
    for phrase in [
        "names the compromised key by its fingerprint, states that binaries using it no longer receive blocklist updates",
        "so users who trust publishers keep the same trust store.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-690
#[test]
fn pkg_690_stolen_root_threshold_is_not_recovered_by_a_new_set() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "A new root key set that the attacker could also sign is not a recovery.",
        "[PKG-693](/pages/reference-registry-key-compromise/)",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-691
#[test]
fn pkg_691_new_root_json_signed_by_current_threshold_and_error_code() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "signed by the threshold of the current root keys, whose `roles` entry for that role names the new key ids.",
        "with `registry.signature-invalid`.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-692
#[test]
fn pkg_692_root_key_announced_before_old_key_retires_and_not_from_mirror() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "before it retires the old root key.",
        "or from the registry response that carries the change.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: PKG-693
#[test]
fn pkg_693_changed_root_key_stops_with_registry_changed_and_remove_add_steps() {
    let page = manual_page("reference-registry-key-compromise");
    for phrase in [
        "and the client stops with `registry.changed`.",
        "The user runs `ilium plugin registry remove <url>` and then `ilium plugin registry add <url> --root-key <new_key_id>`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-428
#[test]
fn sec_428_verification_table_lists_hashes_and_attestation() {
    let page = manual_page("reference-host-release-verification");
    for phrase in ["Hashes and attestation, as in [SEC-428] and [SEC-429]"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-429
#[test]
fn sec_429_attestation_needs_github_and_offline_sites_cannot_run_it() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "that carries the same archive name",
        "The attestation check of [SEC-429] needs GitHub, so an offline site cannot run it itself.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: SEC-430
#[test]
fn sec_430_installer_compares_hash_before_unpacking() {
    let page = manual_page("reference-host-release-verification");
    for phrase in [
        "The installer MUST compare the archive hash with `SHA256SUMS`",
        "when the two hashes differ.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-055
#[test]
fn abi_055_only_the_final_zero_x_minor_is_retained_on_a_one_point_zero_host() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "Retained 0.x minors on a `1.0.0` host: exactly one (the final minor) [ABI-055].",
        "whose `api` is any other `0.x` minor.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-056
#[test]
fn abi_056_unsupported_major_refused_with_package_api_unsupported() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "whose `api` is not the one retained under [ABI-055] MUST be refused with `package.api-unsupported`",
        "the versions the host supports.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-057
#[test]
fn abi_057_deprecated_record_names_the_major_where_loading_stops() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "when it loads a package whose `api` is the final `0.x` minor",
        "and the record MUST name the major version in which that `0.x` minor stops loading.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: ABI-058
#[test]
fn abi_058_limit_lowered_only_when_marked_in_an_earlier_release() {
    let page = manual_page("reference-abi-major-versions");
    for phrase in [
        "unless the change was marked in an earlier release under [ABI-052] and [ABI-053]",
        "the change MUST take effect in that major release only.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-082
#[test]
fn api_082_deprecation_note_names_successor_and_removal_minor() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "The note MUST name the successor and the removal minor",
        "on the item itself.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-083
#[test]
fn api_083_deprecated_item_keeps_signature_and_patch_removes_nothing() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "A deprecated SDK item MUST keep its signature and its behaviour",
        "Patch releases remove nothing ([API-083]).",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: API-084
#[test]
fn api_084_changelog_headings_follow_keep_a_changelog_names() {
    let page = manual_page("reference-sdk-deprecation");
    for phrase in [
        "The headings use the Keep a Changelog section names.",
        "Each SDK release MUST list every item it marks",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-500
#[test]
fn hst_500_helper_memory_counted_like_session_memory_mib() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in ["counts a helper against `session_memory_mib`."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-501
#[test]
fn hst_501_passing_one_admission_check_never_skips_another() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in ["Passing one check MUST NOT skip another."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-502
#[test]
fn hst_502_waiting_requests_admitted_in_the_stated_order() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in ["The host MUST admit waiting requests against the aggregate ceiling in this order:"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-503
#[test]
fn hst_503_capacity_refusal_leaves_side_inactive_and_retries_in_order() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "MUST leave the side `inactive`",
        "MUST be retried when capacity returns, in the order of HST-502.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-504
#[test]
fn hst_504_helper_and_job_threads_count_against_thread_ceiling() {
    let page = manual_page("reference-host-aggregate-limits");
    for phrase in [
        "The host MUST count the threads of every helper and job against an aggregate thread ceiling.",
        "It MUST refuse a new helper or job with `host-capacity`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-505
#[test]
fn hst_505_soak_records_resident_memory_each_cycle() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in ["and the resident memory of the server and of each helper."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-506
#[test]
fn hst_506_resident_memory_stays_within_release_tolerance() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "at the end of every cycle",
        "tolerance set for the release.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-507
#[test]
fn hst_507_soak_fails_on_unreplaced_helper_or_self_restart() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "when a helper that was restarted is not replaced by a new process",
        "or when the server records a restart of itself during the run.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: HST-508
#[test]
fn hst_508_soak_run_record_names_commit_wasmtime_and_cycles() {
    let page = manual_page("reference-host-soak-and-load-testing");
    for phrase in [
        "the run record MUST name the commit, the Wasmtime version and the number of cycles completed.",
        "MUST NOT count as a pass.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-855
#[test]
fn dev_855_each_completion_script_completes_every_subcommand() {
    let page = manual_page("reference-cli-completion");
    for phrase in ["Each script MUST complete every subcommand"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-856
#[test]
fn dev_856_refused_cli_name_never_appears_in_completion() {
    let page = manual_page("reference-cli-completion");
    for phrase in ["A name the install refused with `manifest.cli-name-reserved`"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-857
#[test]
fn dev_857_man_pages_cover_core_and_contributed_commands() {
    let page = manual_page("reference-cli-completion");
    for phrase in ["each core plugin command and each contributed subcommand."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-858
#[test]
fn dev_858_removal_of_a_flag_is_marked_first_and_keeps_working_until_then() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "A flag, field or record type that a release removes or renames",
        "Until that release, the item MUST keep working as before.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-859
#[test]
fn dev_859_deprecation_notice_shape_in_jsonl_and_text() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "In JSONL output the notice is one `warning` record with the code `deprecated`.",
        "In text output it is one line on standard error.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-860
#[test]
fn dev_860_breaking_change_includes_rename_governed_by_deprecation() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in ["A breaking change is a removal or rename that [DEV-858] governs"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-861
#[test]
fn dev_861_new_exit_code_listed_in_release_notes() {
    let page = manual_page("reference-cli-output-stability");
    for phrase in [
        "The number and the meaning of an exit code",
        "listed in the release notes",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-236
#[test]
fn usr_236_oldest_crash_records_removed_first_down_to_two_hundred() {
    let page = manual_page("user-privacy");
    for phrase in ["the oldest records MUST be removed first until 200 remain."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-237
#[test]
fn usr_237_purge_deletes_every_pre_migration_backup() {
    let page = manual_page("user-privacy");
    for phrase in [
        "`ilium plugin uninstall <id> --purge` MUST delete every pre-migration backup of that plugin.",
        "Only the newest one per plugin, kept after each committed update",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-238
#[test]
fn usr_238_uninstall_without_purge_keeps_backups_in_the_delete_table() {
    let page = manual_page("user-privacy");
    for phrase in ["An uninstall without `--purge` keeps them ([USR-238])."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-239
#[test]
fn usr_239_replacement_keeps_ids_spelled_as_they_are() {
    let page = manual_page("guide-internationalisation");
    for phrase in [
        "with the same spelling, and MUST NOT translate them.",
        "A replacement MAY give its tab and command titles in another language.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: USR-240
#[test]
fn usr_240_replacement_keeps_stored_tab_settings_and_translates_own_text() {
    let page = manual_page("guide-internationalisation");
    for phrase in [
        "the stored settings of the tab stay as they were.",
        "Third-party replacements always translate their own text, as [USR-240] states.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-295
#[test]
fn lif_295_binary_replacement_does_not_touch_running_server_until_restart() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "When the Ilium binary on disk is replaced, the running server",
        "the new binary MUST take effect only at the next server start.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-296
#[test]
fn lif_296_kill_signal_loses_restore_bytes_and_plugin_state() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in ["so the server side loses its restore bytes and every plugin loses its in-memory state."] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-297
#[test]
fn lif_297_host_rollback_needs_a_host_release_undo_not_plugin_rollback() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in ["or the host version, so an operator who needs to undo a host release"] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: LIF-298
#[test]
fn lif_298_failure_window_cleared_by_enabling_so_fix_cause_first() {
    let page = manual_page("guide-operator-upgrades");
    for phrase in [
        "`ilium plugin logs <id> --tail 100`",
        "because enabling clears its failure window.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
