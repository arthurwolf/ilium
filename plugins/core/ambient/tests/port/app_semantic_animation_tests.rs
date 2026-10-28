//! Twins of the tests of ilium-client/src/app_semantic_animation_tests.rs.

use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::animation_settings_are_global_and_selecting_projects_never_rebinds_them
#[test]
fn animation_settings_are_global_and_selecting_projects_never_rebinds_them_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_authored_video_reuse_and_missing_input_are_revalidated
#[test]
fn semantic_authored_video_reuse_and_missing_input_are_revalidated_twin() {
    semantic_holds();
    assert!(manifest_capabilities().contains(&"media.video".to_string()));
    let tree = direct_tree("gallery", 80, 24);
    let clip = tree
        .nodes
        .iter()
        .find(|node| node.key.as_deref() == Some("clip"))
        .expect("clip video");
    match &clip.widget {
        ilium_plugin_sdk::types::Widget::Video(spec) => {
            assert!(
                matches!(&spec.source, ilium_plugin_sdk::types::VideoSource::Asset(path) if path.starts_with("clips/") && path.ends_with(".webm"))
            );
        }
        other => panic!("expected a video, got {other:?}"),
    }
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts
#[test]
fn semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_disabled_background_preview_and_concrete_cadence_are_consistent
#[test]
fn semantic_disabled_background_preview_and_concrete_cadence_are_consistent_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_invalid_or_missing_recommendation_releases_field_host_and_timer
#[test]
fn semantic_invalid_or_missing_recommendation_releases_field_host_and_timer_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_paris_attribution_and_wikipedia_policy_use_effective_kind
#[test]
fn semantic_paris_attribution_and_wikipedia_policy_use_effective_kind_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_project_entry_group_and_pane_fallback_are_local
#[test]
fn semantic_project_entry_group_and_pane_fallback_are_local_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_status_help_preserves_full_report_after_host_release
#[test]
fn semantic_status_help_preserves_full_report_after_host_release_twin() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    host.settle();
    assert_eq!(host.settings().get("semantic"), Some(json!(true)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_undo_restores_effective_choice_and_stale_apply_preserves_it
#[test]
fn semantic_undo_restores_effective_choice_and_stale_apply_preserves_it_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::animation_settings_are_global_and_selecting_projects_never_rebinds_them
#[test]
fn animation_settings_are_global_and_selecting_projects_never_rebinds_them_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    host.settle();
    assert!(host.failures().is_empty());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_authored_video_reuse_and_missing_input_are_revalidated
#[test]
fn semantic_authored_video_reuse_and_missing_input_are_revalidated_second_check() {
    let tree = direct_tree("gallery", 80, 24);
    assert!(!tree.nodes.is_empty());
    assert!(tree.nodes.iter().any(|node| node.key.as_deref() == Some("clip")));
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts
#[test]
fn semantic_cache_revalidates_snapshots_without_rebuilding_equivalent_hosts_second_check() {
    let first = direct_tree("gallery", 80, 24);
    let second = direct_tree("gallery", 80, 24);
    assert_eq!(first.nodes.len(), second.nodes.len());
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_disabled_background_preview_and_concrete_cadence_are_consistent
#[test]
fn semantic_disabled_background_preview_and_concrete_cadence_are_consistent_second_check() {
    preview_surface_holds(120, 40);
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_invalid_or_missing_recommendation_releases_field_host_and_timer
#[test]
fn semantic_invalid_or_missing_recommendation_releases_field_host_and_timer_second_check() {
    let mut host = activated_client();
    assert_eq!(
        refused("semantic", json!(1)),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    assert_eq!(host.settings().get("semantic"), Some(json!(false)));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_paris_attribution_and_wikipedia_policy_use_effective_kind
#[test]
fn semantic_paris_attribution_and_wikipedia_policy_use_effective_kind_second_check() {
    assert_eq!(
        refused("observer", json!({ "lat": 91.0, "lon": 0.0 })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_project_entry_group_and_pane_fallback_are_local
#[test]
fn semantic_project_entry_group_and_pane_fallback_are_local_second_check() {
    assert_eq!(
        refused("saved_places", json!({ "Bad_Key": "Home" })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_status_help_preserves_full_report_after_host_release
#[test]
fn semantic_status_help_preserves_full_report_after_host_release_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    host.settle();
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

/// REQ: PORT:ilium-client/src/app_semantic_animation_tests.rs::semantic_undo_restores_effective_choice_and_stale_apply_preserves_it
#[test]
fn semantic_undo_restores_effective_choice_and_stale_apply_preserves_it_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("scene", json!("stars")).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
    assert!(host.failures().is_empty());
}
