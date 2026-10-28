//! Production-gap doc checks for the CLI completion and man page reference
//! (`reference-cli-completion`): completion flags follow the command table, generation is
//! offline and deterministic, unreadable plugin manifests are skipped with one warning, and
//! generated text carries only the allowed plugin fields. Each test reads the manual page
//! that states the rule and checks the rule's key phrases.

use crate::support::manual_page;

/// REQ: DEV-869
#[test]
fn dev_869_completion_flags_follow_command_table() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "MUST complete the flags of each command that the command table lists",
        "MUST NOT complete a flag",
        "the script MUST complete `--pane` and `--all-panes`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-870
#[test]
fn dev_870_generation_offline_from_installed_manifests() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "without starting a plugin",
        "generation MUST still succeed",
        "MUST use the manifests on disk",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-871
#[test]
fn dev_871_generation_is_deterministic() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "Generation MUST be deterministic",
        "with commands sorted by name",
        "byte-identical script and page text",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-872
#[test]
fn dev_872_man_page_lists_required_fields() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "Each man page MUST give the command name",
        "the output form (`jsonl`, `text` or `mixed`)",
        "MUST have its arguments described from that schema",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-873
#[test]
fn dev_873_unsupported_shell_and_unwritable_output_fail_cleanly() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "return exit code 2 for a shell",
        "MUST write nothing",
        "MUST NOT leave a half-written page",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-874
#[test]
fn dev_874_unreadable_manifest_skipped_with_warning() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "MUST be skipped with one warning",
        "The skip MUST NOT stop the generation",
        "the plugin MUST NOT appear in the output",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-875
#[test]
fn dev_875_output_limited_to_allowed_plugin_text() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "MUST NOT contain a command name or alias",
        "MUST NOT contain text from a plugin other than",
        "`name`, `aliases`, `summary`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-869
#[test]
fn dev_869_scripts_and_help_read_one_command_table() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "The scripts and the help text are two views of one command table",
        "Each script completes the command names, the aliases and the flags that `ilium --help` lists",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-870
#[test]
fn dev_870_generation_reads_only_manifests_and_kernel_table() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "Completion and man pages do not run a plugin.",
        "They read the manifests of the installed plugins and the kernel command table.",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-871
#[test]
fn dev_871_identical_generations_produce_a_diff_of_real_changes_only() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "so that a diff of two generations shows only real changes",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-872
#[test]
fn dev_872_man_page_fields_include_summary_arguments_and_output_form() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "the `summary`, the arguments, the output form",
        "and the exit codes of the [CLI reference]",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-873
#[test]
fn dev_873_unwritable_output_directory_exits_one_and_names_the_path() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "It MUST return exit code 1 and name the path when the output directory cannot be written",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-874
#[test]
fn dev_874_skipped_manifest_warning_names_the_plugin_id() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "with one warning that names the plugin id",
        "the plugin MUST NOT appear in the output",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}

/// REQ: DEV-875
#[test]
fn dev_875_argument_description_is_the_only_schema_text_allowed() {
    let page = manual_page("reference-cli-completion");
    for phrase in [
        "the argument description from `arguments_schema`",
    ] {
        assert!(page.contains(phrase), "missing: {phrase}");
    }
}
