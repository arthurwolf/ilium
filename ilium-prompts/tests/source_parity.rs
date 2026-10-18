use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn extracted_sources_match_frozen_original_bytes_and_format_skeletons() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/source-hashes.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = ilium_prompts::catalog()
            .find(|&&(catalog_name, _)| catalog_name == name)
            .unwrap()
            .1;
        // Semantic intentionally extends the restructure contract. Retain the
        // original extraction hashes as history; its current behavior is
        // checked by semantic_restructure_contract and the client parser tests.
        if matches!(
            name,
            "naming/restructure"
                | "naming/restructure/restructure-prompt-exceeded-the-maximum-restructure-prompt-characters-character-safet"
                | "naming/restructure-label-example"
                | "naming/restructure-summary-example"
        ) {
            continue;
        }
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(baseline_source(name, source).as_bytes())
            ),
            case["sha256"].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn semantic_restructure_contract_keeps_catalog_and_required_pointers() {
    let context = serde_json::json!({
        "animation_catalog": "COMPLETE_CATALOG_SENTINEL",
        "fixed_groups": "FIXED_GROUP_SENTINEL",
        "resource_capabilities": "AUTHORED_CAPABILITY_SENTINEL",
    });
    let prompt = ilium_prompts::render("naming/restructure", &context).unwrap();
    for value in [
        "COMPLETE_CATALOG_SENTINEL",
        "FIXED_GROUP_SENTINEL",
        "AUTHORED_CAPABILITY_SENTINEL",
        "<animation-catalog>",
        "\"animations\"",
        "\"project\"",
        "name-fixed",
        "Protected splits",
    ] {
        assert!(prompt.contains(value), "missing {value}");
    }
    for name in [
        "naming/restructure-label-example",
        "naming/restructure-summary-example",
    ] {
        let example = ilium_prompts::render(name, &serde_json::json!({})).unwrap();
        let value: Value = serde_json::from_str(&example).unwrap();
        assert!(value["animations"]["definitions"].is_array());
        assert!(value["animations"]["project"].is_string());
        fn check_nodes(nodes: &[Value]) {
            for node in nodes {
                assert!(node["animation"].is_string());
                if let Some(children) = node["children"].as_array() {
                    check_nodes(children);
                }
            }
        }
        check_nodes(value["children"].as_array().unwrap());
    }
}

// Keep the immutable extraction hashes: only the explicitly authorized additive
// instruction blocks may differ from the frozen baseline source.
fn baseline_source(name: &str, source: &str) -> String {
    // Onboarding deliberately expands the sound-source registry. Normalize
    // only its exact authorized message, retaining the immutable extraction
    // hash and still rejecting any other unexpected text change.
    if name == "voice/settings/sound-source-must-be-system-beep-or"
        && source
            == "sound.source must be system_beep, sound_file, bundled_chirping, generated, or muted"
    {
        return "sound source must be system_beep or sound_file".to_owned();
    }
    // The progress example message changed from a release-build example to a
    // unit-test example in version 11. Normalize only that example sentence so
    // the immutable extraction hash still guards every other byte.
    if name == "agent/progress-instructions" {
        let source = source.replace(r#"A successful check is only a heartbeat: it proves the probe ran and its report was accepted, not that the job advanced or remains alive. Keep the one-line message stable within a phase; do not add a changing clock or heartbeat counter to reset the no-reported-progress warning. State queued, compiling, linking or waiting explicitly in the message/details. When work has no measurable total, say that its progress is indeterminate in details and keep the last defensible percentage; never invent incremental percentages or an ETA. Use real completed/total counters when available. A drop in percent may mark a new phase. Details and ETA changes alone are not substantive progress.

Track the actual job identity and its final exit/result record, not merely a local launcher or SSH wrapper. Local process checks must include boot identity and process start/command identity to avoid PID reuse. For remote jobs, use a remote job/result source that verifies the remote worker; a live local wrapper is not remote liveness. Check for a final result before process liveness and recheck the result before reporting a vanished job. If the task cannot be observed, report an observation failure rather than inventing a successful running heartbeat or a task outcome. Never inspect or stop another owner's job.

"#, "");
        return source.replace(
            "Running the focused unit tests of the ilium-client crate - test 14 of 22",
            "Building the release binaries of the ilium terminal app - compiling crate 14 of 22",
        );
    }
    let fields: &[&str] = match name {
        "agent/ask-for-update" => &["custom_instructions"],
        "naming/project-name" => &["project_naming", "naming_and_organization"],
        "naming/session-title" | "naming/terminal-title" => {
            &["entry_naming", "naming_and_organization"]
        }
        "naming/restructure" => &["entry_naming", "organization", "naming_and_organization"],
        "naming/smart-copy-system" => &["smart_copy"],
        _ => &[],
    };
    let mut baseline = source.to_owned();
    for field in fields {
        let purpose = if *field == "custom_instructions" {
            "ask-for-update"
        } else {
            field
        };
        let block = format!("{{{{#if {field}}}}}\n<custom-instructions purpose=\"{purpose}\">\n{{{{{field}}}}}\n</custom-instructions>{{{{/if}}}}");
        baseline = baseline.replace(&block, "");
    }
    baseline
}

#[test]
fn progress_instructions_separate_heartbeat_from_work_and_require_real_job_identity() {
    let source = ilium_prompts::agent::PROGRESS_INSTRUCTIONS;
    for required in ["only a heartbeat", "progress is indeterminate", "real completed/total counters", "PID reuse", "remote worker", "recheck the result", "observation failure"] {
        assert!(source.contains(required), "missing {required}");
    }
}
