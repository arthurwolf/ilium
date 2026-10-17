//! F-026 agent identity from the process tree, through the `detector` export `identify`
//! (page "Identify an agent CLI", Agent monitoring "Supported agent CLIs").

use proptest::prelude::*;

use crate::common::{identify, identify_name, process};

/// REQ: F-026
#[test]
fn feature_026_claude_process_is_identified_as_claude() {
    assert_eq!(identify_name("claude").as_deref(), Some("claude"));
}

/// REQ: F-026
#[test]
fn feature_026_codex_process_is_identified_as_codex() {
    assert_eq!(identify_name("codex").as_deref(), Some("codex"));
}

/// REQ: F-026
#[test]
fn feature_026_agy_process_is_identified_as_antigravity() {
    assert_eq!(identify_name("agy").as_deref(), Some("antigravity"));
}

/// REQ: F-026
#[test]
fn feature_026_antimatter_is_a_process_name_alias_of_antigravity() {
    assert_eq!(identify_name("antimatter").as_deref(), Some("antigravity"));
}

/// REQ: F-026
#[test]
fn feature_026_opencode_is_identified_with_its_own_name_as_class_other() {
    assert_eq!(identify_name("opencode").as_deref(), Some("opencode"));
}

/// REQ: F-026
#[test]
fn feature_026_aider_is_identified_with_its_own_name_as_class_other() {
    assert_eq!(identify_name("aider").as_deref(), Some("aider"));
}

/// REQ: F-026
#[test]
fn feature_026_the_manifest_signature_example_agent_is_identified() {
    assert_eq!(
        identify_name("example-agent").as_deref(),
        Some("example-agent")
    );
}

/// REQ: F-026
#[test]
fn feature_026_the_alias_example_resolves_to_the_example_agent_signature() {
    assert_eq!(identify_name("example").as_deref(), Some("example-agent"));
}

/// REQ: F-026
#[test]
fn feature_026_a_plain_shell_is_not_an_agent() {
    assert_eq!(identify_name("bash"), None);
    assert_eq!(identify_name("zsh"), None);
    assert_eq!(identify_name("fish"), None);
}

/// REQ: F-026
#[test]
fn feature_026_an_empty_process_list_identifies_nothing() {
    assert_eq!(identify(Vec::new()), None);
}

/// REQ: F-026
#[test]
fn feature_026_the_match_is_a_case_insensitive_substring_of_the_process_name() {
    assert_eq!(identify_name("Codex").as_deref(), Some("codex"));
    assert_eq!(identify_name("CLAUDE").as_deref(), Some("claude"));
}

/// REQ: F-026
#[test]
fn feature_026_a_name_that_contains_the_signature_keeps_the_registry_signature() {
    // The retained signature is the registry entry, not the whole process name.
    assert_eq!(identify_name("codex-host").as_deref(), Some("codex"));
}

/// REQ: F-026
#[test]
fn feature_026_an_agent_below_a_shell_is_found_in_the_list() {
    let processes = vec![
        process("bash", &["bash"]),
        process("claude", &["claude", "--resume"]),
    ];
    assert_eq!(identify(processes).as_deref(), Some("claude"));
}

/// REQ: F-026
#[test]
fn feature_026_an_interpreter_wrapper_is_unwrapped_through_its_argument_vector() {
    let processes = vec![process("node", &["node", "/home/user/.bun/bin/codex"])];
    assert_eq!(identify(processes).as_deref(), Some("codex"));
}

/// REQ: F-026
#[test]
fn feature_026_the_option_terminator_keeps_the_next_operand_literal() {
    for interpreter in ["sh", "bash", "fish", "node", "bun", "env", "python3"] {
        let processes = vec![process(
            interpreter,
            &[
                interpreter,
                "--",
                "/tmp/path with spaces/codex",
                "-c",
                "ignored config",
            ],
        )];
        assert_eq!(
            identify(processes).as_deref(),
            Some("codex"),
            "{interpreter}"
        );
    }
}

/// REQ: F-026
#[test]
fn feature_026_a_shell_option_operand_cannot_impersonate_an_agent() {
    for argv in [
        vec!["fish", "--command", "codex; printf returned"],
        vec!["fish", "--command", "codex $(printf argument)"],
        vec!["bash", "--rcfile", "/tmp/codex"],
        vec!["bash", "--rcfile", "/tmp/codex", "-c", "echo unrelated"],
        vec!["bash", "--init-file", "/tmp/claude"],
        vec!["bash", "-o", "codex"],
        vec!["bash", "--command", "codex"],
    ] {
        let name = argv[0];
        assert_eq!(identify(vec![process(name, &argv)]), None, "{argv:?}");
    }
}

/// REQ: F-026
#[test]
fn feature_026_a_simple_quoted_fish_command_names_the_agent() {
    let processes = vec![process(
        "fish",
        &["fish", "--command", "'/tmp/codex' --prompt 'literal ;'"],
    )];
    assert_eq!(identify(processes).as_deref(), Some("codex"));
}

/// REQ: F-026
#[test]
fn feature_026_a_dash_c_operand_after_the_terminator_is_not_an_agent() {
    let processes = vec![process("sh", &["sh", "--", "-c", "codex; echo unrelated"])];
    assert_eq!(identify(processes), None);
}

/// REQ: F-026
#[test]
fn feature_026_a_missing_argument_vector_still_matches_by_process_name() {
    assert_eq!(
        identify(vec![process("codex", &[])]).as_deref(),
        Some("codex")
    );
}

/// REQ: F-026
#[test]
fn feature_026_identity_is_stable_across_repeated_calls() {
    let first = identify_name("claude");
    let second = identify_name("claude");
    assert_eq!(first, second);
}

proptest! {
    // REQ: F-026
    #[test]
    fn feature_026_names_without_any_agent_substring_are_never_identified(name in "[b-z]{1,12}") {
        prop_assume!(!["claude", "codex", "agy", "antimatter", "opencode", "aider", "example"].iter().any(|signature| name.contains(signature)));
        prop_assert_eq!(identify_name(&name), None);
    }

    // REQ: F-026
    #[test]
    fn feature_026_a_prefix_or_suffix_around_codex_keeps_the_codex_signature(prefix in "[a-z]{0,6}", suffix in "[0-9]{0,4}") {
        let name = format!("{prefix}codex{suffix}");
        prop_assume!(!["claude", "agy", "antimatter", "opencode", "aider", "example"].iter().any(|signature| name.contains(signature)));
        let identified = identify_name(&name);
        prop_assert_eq!(identified.as_deref(), Some("codex"));
    }
}
