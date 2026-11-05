/// REQ: PDV-008
#[test]
fn pdv_008_export_writes_toml_version_1_mode_0600_with_plugin_ids_and_versions() {
    // Observable: Export on a machine with two plugins and inspect the file: it starts with `version = 1`, `stat` shows mode 0600, and it contains both plugin ids with their exact versions.
    panic!(
        "PDV-008 not implemented: `ilium plugin config export --out <file>` MUST write a TOML file with `version = 1`, mode 0600, listing plugin ids, exact versions and each plugin's non-secret settings."
    );
}

/// REQ: PDV-009
#[test]
fn pdv_009_export_excludes_secrets_keyring_grants_consent_and_instance_id() {
    // Observable: Store a secret and hold grants for a plugin, then export: the file contains no key named `secret`, `grants`, `consent` or `instance_id` and none of the secret's bytes.
    panic!(
        "PDV-009 not implemented: The export MUST NOT contain secrets, keyring entries, grants, consent records or the per-plugin random identifier."
    );
}

/// REQ: PDV-010
#[test]
fn pdv_010_import_validates_whole_file_before_applying_and_exits_2_on_error() {
    // Observable: Import a file with one valid plugin section and one setting of the wrong type: the exit code is 2, the output names the invalid key, and neither plugin's settings changed.
    panic!(
        "PDV-010 not implemented: `ilium plugin config import <file>` MUST validate the whole file against every plugin's settings schema before applying anything, and on any error MUST apply nothing and exit with code 2."
    );
}

/// REQ: PDV-011
#[test]
fn pdv_011_import_skips_policy_locked_keys_and_leaves_policy_file_unmodified() {
    // Observable: Lock `example.sync.interval` in the system policy and import a file setting it: the output lists the key with `locked`, the setting keeps the policy value, and the policy file hash is unchanged.
    panic!(
        "PDV-011 not implemented: Import MUST skip any key the admin policy locks, list it as `locked` in its output, and MUST NOT modify the policy file."
    );
}

/// REQ: PDV-012
#[test]
fn pdv_012_dry_run_prints_each_change_as_jsonl_and_writes_nothing() {
    // Observable: Run the dry run against a file that changes three settings: stdout has three JSON lines with `plugin_id`, `key`, `old` and `new`, and the settings store hash is unchanged.
    panic!(
        "PDV-012 not implemented: `ilium plugin config import --dry-run <file>` MUST print each change as one JSONL line and MUST write nothing."
    );
}

/// REQ: PDV-013
#[test]
fn pdv_013_import_higher_version_fails_with_config_version_unsupported_exit_2() {
    // Observable: Import a file with `version = 2` on a client supporting version 1: the exit code is 2 and the output contains `config-version-unsupported`.
    panic!(
        "PDV-013 not implemented: Import of a file whose `version` is higher than the supported version MUST fail with reason `config-version-unsupported` and exit code 2."
    );
}

/// REQ: PDV-014
#[test]
fn pdv_014_export_and_import_write_audit_records_without_setting_values() {
    // Observable: Export then import, and read the audit log: one `config-export` and one `config-import` line exist, each with `detail.bytes` and `detail.files`, and neither contains any exported setting value.
    panic!(
        "PDV-014 not implemented: Each export and import MUST write one audit record of event `config-export` or `config-import` with the byte count and file count in `detail` and no setting values."
    );
}
