use ilium_animation_js::{
    manifest::AnimationMode,
    plan::{AnimationPlan, PlanBudget},
};
use serde_json::json;

#[test]
fn pure_plan_requests_no_devices_or_source_products() {
    let plan = AnimationPlan::parse(
        &json!({"format":"gray32","fps":20,"inputs":{}}),
        AnimationMode::Live,
        PlanBudget::default(),
    )
    .unwrap();
    assert!(plan.inputs.is_empty());
    assert!(plan.permissions.is_empty());
}

#[test]
fn audio_products_are_independent_and_bounded() {
    let parse = |audio| {
        AnimationPlan::parse(
            &json!({"format":"gray8","fps":30,"inputs":{"audio":audio}}),
            AnimationMode::Live,
            PlanBudget::default(),
        )
    };
    let plan = parse(json!({"max_hz":20,"products":["waveform"],"waveform_samples":256})).unwrap();
    assert!(!plan.inputs.audio.as_ref().unwrap().needs_fft());
    assert!(parse(json!({"max_hz":20,"products":["bands"],"band_count":100000})).is_err());
    assert!(parse(json!({"max_hz":20,"products":["surprise"]})).is_err());
}

#[test]
fn pre_rendered_plan_can_bind_recording_created_during_create() {
    let plan = json!({"format":"gray8","fps":30,"inputs":{"pointer":{"max_hz":30}},"replay":{"seed":0,"duration_seconds":10,"seamless":false}});
    let parsed =
        AnimationPlan::parse(&plan, AnimationMode::PreRendered, PlanBudget::default()).unwrap();
    assert_eq!(parsed.inputs.pointer.as_ref().unwrap().max_hz, 30.0);
    let replay = parsed.replay.unwrap();
    assert_eq!(replay.duration_seconds, 10.0);
    assert!(replay.input_recording.is_none());
}

#[test]
fn unknown_fields_and_unbounded_replay_are_rejected() {
    let parse =
        |value| AnimationPlan::parse(&value, AnimationMode::PreRendered, PlanBudget::default());
    assert!(parse(json!({"format":"gray8","fps":30,"inputs":{"processes":{}}})).is_err());
    assert!(parse(json!({"format":"gray8","fps":30,"inputs":{},"replay":{"seed":0,"duration_seconds":10000,"seamless":false}})).is_err());
}
