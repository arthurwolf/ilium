//! Gap tests for the public `ilium_server` configuration surface that the
//! untested-item inventory names: `config::agent_detection_settings`.
//!
//! Most other inventoried items are `pub(crate)` or live in private modules,
//! so they cannot be reached from an integration test; they are listed as
//! skipped in the crate report rather than tested through internals.

use std::borrow::Cow;
use std::time::Duration;

use ilium_core::AgentClass;
use ilium_detect::AgentSignature;
use ilium_ipc::CustomAgentSignature;
use ilium_server::config::agent_detection_settings;
use ilium_server::config::DetectionConfig;

fn detection_with_intervals(working: Duration, idle: Duration) -> DetectionConfig {
    DetectionConfig {
        working_poll_interval: working,
        idle_poll_interval: idle,
        auto_answer_interstitial_prompts: true,
    }
}

fn signature(name: &'static str, class_of: fn(&str) -> AgentClass) -> AgentSignature {
    AgentSignature {
        name_substring: Cow::Borrowed(name),
        class_of,
    }
}

fn claude_class(_matched_name: &str) -> AgentClass {
    AgentClass::Claude
}

fn codex_class(_matched_name: &str) -> AgentClass {
    AgentClass::Codex
}

fn antigravity_class(_matched_name: &str) -> AgentClass {
    AgentClass::Antigravity
}

fn other_class_named_after_match(matched_name: &str) -> AgentClass {
    AgentClass::Other(matched_name.to_string())
}

#[test]
fn returns_configured_poll_seconds_for_working_and_idle_tiers() {
    let detection = detection_with_intervals(Duration::from_secs(7), Duration::from_secs(33));

    let settings = agent_detection_settings(&detection, &[]);

    assert_eq!(
        settings.working_poll_seconds, 7,
        "the working tier must report its configured seconds"
    );
    assert_eq!(
        settings.idle_poll_seconds, 33,
        "the idle tier must report its configured seconds"
    );
}

#[test]
fn default_detection_config_reports_default_poll_seconds_and_no_signatures() {
    let settings = agent_detection_settings(&DetectionConfig::default(), &[]);

    assert_eq!(
        settings.working_poll_seconds, 10,
        "a missing configuration must fall back to the 10 second working default"
    );
    assert_eq!(
        settings.idle_poll_seconds, 45,
        "a missing configuration must fall back to the 45 second idle default"
    );
    assert!(
        settings.custom_signatures.is_empty(),
        "no custom signatures were supplied, so the IPC list must be empty"
    );
}

#[test]
fn sub_second_working_interval_is_reported_as_zero_seconds() {
    let detection = detection_with_intervals(Duration::from_millis(500), Duration::from_secs(45));

    let settings = agent_detection_settings(&detection, &[]);

    assert_eq!(
        settings.working_poll_seconds, 0,
        "the 500 ms minimum must round-trip as the zero-second sentinel"
    );
    assert_eq!(
        settings.idle_poll_seconds, 45,
        "an unaffected idle interval must keep its seconds"
    );
}

#[test]
fn sub_second_remainder_is_truncated_not_rounded_up() {
    let detection =
        detection_with_intervals(Duration::from_millis(1_999), Duration::from_millis(1_500));

    let settings = agent_detection_settings(&detection, &[]);

    assert_eq!(
        settings.working_poll_seconds, 1,
        "1999 ms must truncate to 1 whole second"
    );
    assert_eq!(
        settings.idle_poll_seconds, 1,
        "1500 ms must truncate to 1 whole second"
    );
}

#[test]
fn custom_signature_list_is_empty_when_no_signatures_are_given() {
    let settings = agent_detection_settings(&DetectionConfig::default(), &[]);

    assert_eq!(
        settings.custom_signatures,
        Vec::<CustomAgentSignature>::new(),
        "an empty signature slice must produce an empty IPC list"
    );
}

#[test]
fn custom_signatures_keep_their_input_order_and_count() {
    let signatures = [
        signature("alpha", claude_class),
        signature("beta", codex_class),
        signature("gamma", antigravity_class),
    ];

    let settings = agent_detection_settings(&DetectionConfig::default(), &signatures);

    let names: Vec<&str> = settings
        .custom_signatures
        .iter()
        .map(|entry| entry.name_substring.as_str())
        .collect();
    assert_eq!(
        names,
        ["alpha", "beta", "gamma"],
        "every signature must appear once, in the supplied order"
    );
}

#[test]
fn custom_signature_name_substring_is_copied_verbatim() {
    let signatures = [signature("MixedCase Agent", claude_class)];

    let settings = agent_detection_settings(&DetectionConfig::default(), &signatures);

    assert_eq!(
        settings.custom_signatures[0].name_substring, "MixedCase Agent",
        "the name must be copied without changing case or whitespace"
    );
}

#[test]
fn custom_signature_class_comes_from_its_class_of_function() {
    let signatures = [
        signature("claudeish", claude_class),
        signature("codexish", codex_class),
        signature("agyish", antigravity_class),
    ];

    let settings = agent_detection_settings(&DetectionConfig::default(), &signatures);

    let classes: Vec<AgentClass> = settings
        .custom_signatures
        .iter()
        .map(|entry| entry.class.clone())
        .collect();
    assert_eq!(
        classes,
        [
            AgentClass::Claude,
            AgentClass::Codex,
            AgentClass::Antigravity
        ],
        "each signature's class must be whatever its class_of function returns"
    );
}

#[test]
fn custom_signature_class_of_receives_the_name_substring_as_matched_name() {
    let signatures = [signature("agentx", other_class_named_after_match)];

    let settings = agent_detection_settings(&DetectionConfig::default(), &signatures);

    assert_eq!(
        settings.custom_signatures,
        vec![CustomAgentSignature {
            name_substring: "agentx".to_string(),
            class: AgentClass::Other("agentx".to_string()),
        }],
        "class_of must be called with the name substring so Other carries that exact name"
    );
}
