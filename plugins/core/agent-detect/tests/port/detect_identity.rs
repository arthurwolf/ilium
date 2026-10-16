//! Twins of the identity tests of ilium-detect through the `identify` export (argv lists stand for process command lines).

use crate::common::{identify, identify_argv, identify_name, launched, process};

fn some(signature: &str) -> Option<String> {
    Some(signature.to_string())
}

/// REQ: PORT:ilium-detect/src/lib.rs::classify_process_name_matches_known_signatures
#[test]
fn port_027_known_process_names_map_to_their_signature() {
    assert_eq!(identify_name("claude"), some("claude"));
    assert_eq!(identify_name("codex"), some("codex"));
    assert_eq!(identify_name("agy"), some("antigravity"));
    assert_eq!(identify_name("vim"), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::process_matches_retain_the_exact_registry_signature
#[test]
fn port_027_a_match_retains_the_registry_signature() {
    assert_eq!(identify_name("codex-host"), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::identify_agent_returns_none_when_no_agent_descendant_exists
#[test]
fn port_027_no_agent_process_gives_no_signature() {
    assert_eq!(identify(vec![process("bash", &["bash"]), process("vim", &["vim"])]), None);
    assert_eq!(identify(vec![]), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::an_ordinary_program_is_not_treated_as_an_interpreter
#[test]
fn port_027_an_ordinary_program_is_not_an_interpreter() {
    assert_eq!(identify_argv(&["vim", "codex.md"]), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::a_direct_invocation_is_left_alone
#[test]
fn port_027_a_direct_invocation_is_identified_as_is() {
    assert_eq!(identify_argv(&["codex", "--resume", "session"]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::an_argument_is_reduced_to_its_lowercase_file_name
#[test]
fn port_027_an_argument_path_is_reduced_to_its_lowercase_file_name() {
    assert_eq!(identify(vec![process("node", &["node", "/Opt/Tools/CODEX"])]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::an_interpreter_hands_its_script_name_on
#[test]
fn port_027_an_interpreter_hands_its_script_name_on() {
    assert_eq!(identify(vec![process("node", &["node", "/usr/lib/claude/cli.js"])]), None);
    assert_eq!(identify(vec![process("node", &["node", "/usr/bin/claude"])]), some("claude"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::an_interpreter_is_followed_to_its_program_whichever_shape_it_takes
#[test]
fn port_027_interpreters_are_followed_to_their_program() {
    for argv in [
        vec!["node", "/usr/bin/codex"],
        vec!["python3", "/usr/bin/codex"],
        vec!["bun", "/usr/bin/codex"],
        vec!["env", "codex"],
    ] {
        assert_eq!(identify(vec![process(argv[0], &argv)]), some("codex"), "{argv:?}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::a_command_line_is_reduced_to_the_program_it_runs
#[test]
fn port_027_a_command_line_is_reduced_to_its_program() {
    assert_eq!(identify(vec![process("sh", &["sh", "-c", "codex --resume session"])]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::a_shell_wrapped_command_line_expands_into_the_tokens_the_program_sees
#[test]
fn port_027_a_shell_wrapped_command_line_expands_into_tokens() {
    assert_eq!(identify(vec![process("bash", &["bash", "-lc", "claude --resume session"])]), some("claude"));
    assert_eq!(identify(vec![process("sh", &["sh", "-c", "vim codex.md"])]), None);
}

/// REQ: PORT:ilium-detect/src/lib.rs::single_shell_commands_keep_literal_quoted_agent_identity
#[test]
fn port_027_single_quoted_commands_keep_the_agent_identity() {
    assert_eq!(identify(vec![process("sh", &["sh", "-c", "'/tmp/fixtures/codex' --resume session"])]), some("codex"));
    assert_eq!(
        identify(vec![process("sh", &["sh", "-c", "\"/tmp/fixture directory/claude\" --resume session"])]),
        some("claude")
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::compound_shell_commands_cannot_revive_a_stopped_agent
#[test]
fn port_027_compound_commands_give_no_signature() {
    for command in [
        "codex; printf returned",
        "claude && echo again",
        "codex || true",
        "codex | cat",
        "codex & wait",
        "codex\nprintf returned",
        "codex > output",
        "codex $(echo x)",
        "codex `echo x`",
    ] {
        assert_eq!(identify(vec![process("sh", &["sh", "-c", command])]), None, "{command:?}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::literal_shell_identity_rejects_malformed_or_dynamic_operands
#[test]
fn port_027_malformed_or_dynamic_operands_give_no_signature() {
    for command in ["'codex", "codex\\", "${dynamic} codex", "# codex", "codex;", "codex &&", "codex\0x"] {
        assert_eq!(identify(vec![process("sh", &["sh", "-c", command])]), None, "{command:?}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::shell_command_mode_is_specific_to_shells_and_combined_c_flags
#[test]
fn port_027_command_mode_is_specific_to_shells() {
    for shell in ["/bin/sh", "/bin/bash", "/bin/zsh", "sh.exe"] {
        assert_eq!(identify(vec![launched(&[shell, "-lc", "codex --resume session"])]), some("codex"), "{shell}");
    }
    for interpreter in ["python3", "node", "bun"] {
        assert_eq!(identify(vec![process(interpreter, &[interpreter, "-c", "codex; unrelated_code"])]), None, "{interpreter}");
    }
}

/// REQ: PORT:ilium-detect/src/lib.rs::shell_option_operands_cannot_impersonate_an_agent
#[test]
fn port_027_shell_option_operands_do_not_identify_an_agent() {
    for option in ["--rcfile", "--init-file", "-o", "--command"] {
        assert_eq!(identify(vec![process("bash", &["bash", option, "codex"])]), None, "{option}");
    }
    assert_eq!(identify(vec![process("fish", &["fish", "--command", "codex --resume session"])]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::interpreter_option_terminator_keeps_next_operand_literal
#[test]
fn port_027_the_option_terminator_keeps_the_next_operand_literal() {
    assert_eq!(identify(vec![process("sh", &["sh", "--", "-c", "codex; echo unrelated"])]), None);
    assert_eq!(identify(vec![process("sh", &["sh", "--", "codex"])]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::literal_script_operands_preserve_interpreter_and_agent_configuration_arguments
#[test]
fn port_027_env_split_string_keeps_configuration_arguments() {
    let command = "'/tmp/fixture directory/codex' -c code_mode";
    assert_eq!(identify(vec![process("env", &["env", "-S", command])]), some("codex"));
}

/// REQ: PORT:ilium-detect/tests/interpreter_wrapper_identity.rs::identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match
#[test]
fn port_027_a_native_agent_is_preferred_over_an_interpreter_wrapper() {
    let wrapper = process("node", &["node", "/usr/bin/codex"]);
    let native = process("claude", &["claude"]);
    assert_eq!(identify(vec![wrapper, native]), some("claude"));
}

/// REQ: PORT:ilium-detect/tests/script_agent_identity.rs::a_shebang_script_named_after_an_agent_is_identified_as_that_agent
#[test]
fn port_027_a_script_named_after_an_agent_is_that_agent() {
    assert_eq!(identify(vec![process("codex", &["/bin/sh", "/tmp/fixtures/codex"])]), some("codex"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::classify_process_name_with_extra_matches_a_caller_supplied_signature
#[test]
fn port_027_a_manifest_signature_matches_a_custom_process() {
    assert_eq!(identify_name("example-agent"), some("example-agent"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::classify_process_name_with_extra_never_shadows_a_built_in_signature
#[test]
fn port_027_a_custom_signature_never_shadows_a_built_in_one() {
    assert_eq!(identify(vec![process("claude", &["claude", "example-agent"])]), some("claude"));
}
