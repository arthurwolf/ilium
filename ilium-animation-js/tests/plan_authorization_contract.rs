use ilium_animation_js::{
    manifest::{AnimationMode, Manifest},
    permissions::{Ceiling, PackageIdentity, PermissionBroker, UserChoice},
    plan::{AnimationPlan, PlanBudget},
    plan_authorization::AuthorizationProjection,
};
use serde_json::json;
use std::collections::BTreeMap;

fn plan(pointer: bool, permission: bool) -> AnimationPlan {
    let inputs = if pointer {
        json!({"pointer":{"max_hz":10}})
    } else {
        json!({})
    };
    let permissions = if permission {
        json!([{
            "id":"input.pointer", "scope":"animation_viewport", "request_id":"pointer",
            "required":false, "reason":"Follow the pointer inside the animation."
        }])
    } else {
        json!([])
    };
    AnimationPlan::parse(
        &json!({"format":"gray32", "fps":20,
        "inputs":inputs, "permissions":permissions}),
        AnimationMode::Live,
        PlanBudget::default(),
    )
    .unwrap()
}
fn manifest() -> Manifest {
    serde_json::from_value(
        json!({"api_version":1,"id":"projection-fixture", "name":"Fixture",
        "version":"1.0.0", "entry":"entry.mjs", "modes":["live"], "files":[],
        "capabilities":[{"id":"input.pointer","scope":"animation_viewport"}]}),
    )
    .unwrap()
}
#[test]
fn undeclared_pointer_cannot_create_a_subscription() {
    assert!(AuthorizationProjection::build(&manifest(), &plan(true, false)).is_err());
}
#[test]
fn optional_denial_removes_the_input_and_revocation_fences_even_pure_plans() {
    let requested = plan(true, true);
    let projection = AuthorizationProjection::build(&manifest(), &requested).unwrap();
    let mut broker = PermissionBroker::new(
        PackageIdentity::unverified("projection-fixture".into(), b"fixture").unwrap(),
        projection.manifest_ceiling.clone(),
        Ceiling {
            permissions: projection.manifest_ceiling.permissions.clone(),
        },
    )
    .unwrap();
    let review = broker
        .prepare(1, 1, projection.permission_plan.clone(), BTreeMap::new())
        .unwrap();
    let resolution = broker
        .resolve(
            review,
            BTreeMap::from([("pointer".into(), UserChoice::DenySession)]),
        )
        .unwrap();
    let active = resolution.activation.unwrap();
    let accepted = projection
        .prune_inputs(&broker, &active.channel, &requested)
        .unwrap();
    assert!(accepted.inputs.pointer.is_none());
    let _retired = broker.retire(1);
    assert!(projection
        .prune_inputs(&broker, &active.channel, &plan(false, false))
        .is_err());
}
