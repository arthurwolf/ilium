//! user-safe-mode-and-troubleshooting.md: LIF-180..184. LIF-185, LIF-186 and LIF-187 belong to part 2.

use crate::support::*;

const ESSENTIAL: [&str; 4] = [
    "ilium.core.tree",
    "ilium.core.layout",
    "ilium.core.settings",
    "ilium.core.sessions",
];

fn mixed_host() -> LifeHost {
    let mut host = LifeHost::new();
    for name in ESSENTIAL {
        host.install(PluginSpec::new(name).tier(Tier::T1).startup());
    }
    host.install(PluginSpec::new("ilium.core.extra").tier(Tier::T1).startup());
    host.install(PluginSpec::new(A).tier(Tier::T2).startup());
    host.install(PluginSpec::new(B).tier(Tier::T3).startup());
    host
}

/// REQ: LIF-180
#[test]
fn lif_180_third_party_mode_loads_no_t2_or_t3() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::ThirdParty, None, None);
    let running = host.running_plugins();
    assert!(!running.contains(&A.to_owned()) && !running.contains(&B.to_owned()));
    assert!(running.contains(&"ilium.core.extra".to_owned()));
}

/// REQ: LIF-180
#[test]
fn lif_180_third_party_mode_ignores_replaces() {
    let mut host = mixed_host();
    host.install(
        PluginSpec::new(C)
            .tier(Tier::T2)
            .replaces("ilium.core.extra")
            .startup(),
    );
    host.start_safe(SafeMode::ThirdParty, None, None);
    assert!(host
        .running_plugins()
        .contains(&"ilium.core.extra".to_owned()));
}

/// REQ: LIF-180
#[test]
fn lif_180_all_optional_mode_loads_only_the_essential_set() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::AllOptional, None, None);
    let mut running = host.running_plugins();
    running.sort();
    let mut expected: Vec<String> = ESSENTIAL.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(running, expected);
}

/// REQ: LIF-180
#[test]
fn lif_180_neither_mode_changes_files_on_disk() {
    for mode in [SafeMode::ThirdParty, SafeMode::AllOptional] {
        let mut host = mixed_host();
        let before = host.profile_snapshot();
        host.start_safe(mode, None, None);
        host.shutdown();
        assert_eq!(host.profile_snapshot(), before);
    }
}

/// REQ: LIF-181
#[test]
fn lif_181_environment_variable_behaves_like_the_flag() {
    let mut by_env = mixed_host();
    by_env.start_safe(SafeMode::Off, None, Some("third-party"));
    let mut by_flag = mixed_host();
    by_flag.start_safe(SafeMode::ThirdParty, None, None);
    assert_eq!(by_env.running_plugins(), by_flag.running_plugins());
}

/// REQ: LIF-181
#[test]
fn lif_181_flag_wins_over_the_environment_variable() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::Off, Some("all-optional"), Some("third-party"));
    assert_eq!(host.safe_mode(), SafeMode::AllOptional);
}

/// REQ: LIF-182
#[test]
fn lif_182_footer_shows_the_safe_mode_indicator() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::ThirdParty, None, None);
    assert!(host
        .footer()
        .contains("Safe mode: third-party plugins are off."));
}

/// REQ: LIF-182
#[test]
fn lif_182_no_indicator_outside_safe_mode() {
    let mut host = mixed_host();
    host.start(StartMode::Normal);
    assert!(!host.footer().contains("Safe mode"));
}

/// REQ: LIF-183
#[test]
fn lif_183_the_essential_set_is_exactly_four_plugins_and_loads_in_all_optional() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::AllOptional, None, None);
    for name in ESSENTIAL {
        assert!(host.running_plugins().contains(&name.to_owned()), "{name}");
    }
    assert_eq!(host.running_plugins().len(), 4);
}

/// REQ: LIF-183
#[test]
fn lif_183_a_fifth_core_plugin_is_not_essential() {
    let mut host = mixed_host();
    host.start_safe(SafeMode::AllOptional, None, None);
    assert!(!host
        .running_plugins()
        .contains(&"ilium.core.extra".to_owned()));
}

/// REQ: LIF-184
#[test]
fn lif_184_bisect_to_the_end_leaves_installed_json_unchanged() {
    let mut host = mixed_host();
    host.start(StartMode::Normal);
    let before = host.installed_json();
    host.cli(&["plugin", "bisect", "start"]);
    for _ in 0..8 {
        let out = host.cli(&["plugin", "bisect", "good"]);
        if out
            .stdout_jsonl
            .iter()
            .any(|l| l["type"] == "bisect" && l["found"].is_string())
        {
            break;
        }
    }
    assert_eq!(host.installed_json(), before);
}

/// REQ: LIF-184
#[test]
fn lif_184_reset_restores_the_saved_enabled_set() {
    let mut host = mixed_host();
    host.start(StartMode::Normal);
    let saved = host.running_plugins();
    host.cli(&["plugin", "bisect", "start"]);
    host.cli(&["plugin", "bisect", "bad"]);
    let out = host.cli(&["plugin", "bisect", "reset"]);
    assert_eq!(out.exit_code, 0);
    assert_eq!(host.running_plugins(), saved);
}
