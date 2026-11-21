use ilium_inference::decisions::{parse_polling_decision, PollingChoice};
use serde_json::json;

#[test]
fn polling_choice_and_cached_usage_are_validated() {
    let response = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"polling","confidence":0.91,"probabilities":[{"value":"polling","probability":0.91}]}],"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":300,"cache_write_tokens":100}}});
    let parsed = parse_polling_decision(&response, 0.8).unwrap();
    assert_eq!(parsed.choice, PollingChoice::Polling);
    let usage = parsed.usage.unwrap();
    assert_eq!(usage.uncached_tokens, 600);
    assert_eq!(usage.cached_tokens, 300);
    assert_eq!(usage.cache_write_tokens, 100);
}

#[test]
fn refusals_and_low_confidence_are_never_clean_results() {
    let refusal = json!({"answers":[{"type":"refusal","name":"polling_activity"}]});
    assert_eq!(
        parse_polling_decision(&refusal, 0.8).unwrap().choice,
        PollingChoice::Insufficient
    );
    let low = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"no_finding","confidence":0.3}]});
    assert_eq!(
        parse_polling_decision(&low, 0.8).unwrap().choice,
        PollingChoice::Insufficient
    );
}

#[test]
fn impossible_usage_and_unknown_choices_are_rejected() {
    let bad = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"polling","confidence":0.9}],"usage":{"input_tokens":10,"input_tokens_details":{"cached_tokens":11}}});
    assert!(parse_polling_decision(&bad, 0.8).is_err());
    let bad_choice = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"invented","confidence":0.9}]});
    assert!(parse_polling_decision(&bad_choice, 0.8).is_err());
}

#[test]
fn missing_usage_cannot_be_reported_as_a_clean_finding() {
    let response = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"no_finding","confidence":0.99}]});
    let decision = parse_polling_decision(&response, 0.8).unwrap();
    assert_eq!(decision.choice, PollingChoice::Insufficient);
    assert!(decision.usage.is_none());
    let no_details = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"polling","confidence":0.99}],"usage":{"input_tokens":10}});
    let decision = parse_polling_decision(&no_details, 0.8).unwrap();
    assert_eq!(decision.choice, PollingChoice::Insufficient);
    assert!(decision.usage.is_none());
}

#[test]
fn decision_request_uses_single_typed_question_and_separate_evidence() {
    let payload = ilium_inference::decisions::polling_request("evidence: pending");
    assert_eq!(payload["model"], "gpt-6-luna");
    assert_eq!(payload["questions"][0]["type"], "choice");
    assert_eq!(payload["questions"][0]["name"], "polling_activity");
    assert!(payload["input"]
        .as_str()
        .unwrap()
        .contains("evidence: pending"));
}

#[test]
fn evidence_sanitizer_removes_known_and_credential_shaped_values() {
    let clean = ilium_inference::decisions::sanitize_evidence(
        "key: special-test-key; OPENAI_API_KEY=sk-abcdefghijklmnopqrstuv",
        "special-test-key",
    );
    assert!(!clean.contains("special-test-key"));
    assert!(!clean.contains("sk-abcdefghijklmnopqrstuv"));
}

#[test]
fn malformed_supplied_probabilities_cannot_become_a_finding() {
    for probabilities in [
        json!([{"value":"polling","probability":1.2}]),
        json!([{"value":"invented","probability":0.9}]),
        json!([{"value":"polling","probability":"0.9"}]),
        json!([{"value":"polling","probability":0.9},{"value":"polling","probability":0.1}]),
        json!({"polling":0.9}),
    ] {
        let response = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"polling","confidence":0.9,"probabilities":probabilities}],"usage":{"input_tokens":10,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0}}});
        assert!(parse_polling_decision(&response, 0.8).is_err());
    }
}

#[test]
fn uncertain_polling_decisions_preserve_reported_choice_and_probabilities() {
    let response = json!({"answers":[{"type":"choice","name":"polling_activity","choice":"no_finding","confidence":0.6,"probabilities":[{"value":"no_finding","probability":0.6},{"value":"polling","probability":0.4}]}],"usage":{"input_tokens":10,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0}}});
    let decision = parse_polling_decision(&response, 0.8).unwrap();
    assert_eq!(decision.choice, PollingChoice::Insufficient);
    assert_eq!(decision.reported_choice, Some(PollingChoice::NoFinding));
    let probabilities = decision.probabilities.unwrap();
    assert_eq!(probabilities.len(), 2);
    assert_eq!(probabilities[0].choice, PollingChoice::NoFinding);
    assert_eq!(probabilities[0].probability, 0.6);
    assert_eq!(probabilities[1].choice, PollingChoice::Polling);
    assert_eq!(probabilities[1].probability, 0.4);
}

#[test]
fn missing_polling_probability_vector_and_refusal_remain_explicitly_absent() {
    for answer in [
        json!({"type":"refusal","name":"polling_activity"}),
        json!({"type":"choice","name":"polling_activity","choice":"polling","confidence":0.95}),
    ] {
        let decision = parse_polling_decision(&json!({"answers":[answer]}), 0.8).unwrap();
        assert!(decision.probabilities.is_none());
        assert_eq!(decision.choice, PollingChoice::Insufficient);
    }
}
