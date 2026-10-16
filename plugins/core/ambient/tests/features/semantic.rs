//! Semantic animation (F-103) and the handler of the plugin: `settings-changed` for the `semantic`
//! key, the last suggestion kept in global storage, and the rule that the handler returns no
//! submissions (page sample 4: `Vec::new()` for every batch).

use ilium_plugin_sdk::host::storage::Bucket;
use ilium_plugin_sdk::types::{Event, ImageSource, VideoSource, ViewTarget, Widget};
use proptest::prelude::*;
use serde_json::json;

use crate::common::*;

const LAST_SCENE: &str = "last_suggestion";

fn gallery_assets(host: &mut ilium_plugin_sdk::testing::FakeHost) -> (String, String) {
    let tree = {
        let _ = host.render(ViewTarget::Panel(GALLERY.to_string()), 80, 24);
        last_tree_of(host, ViewTarget::Panel(GALLERY.to_string()))
    };
    let image = nodes_where(&tree, |widget| matches!(widget, Widget::Image(_)));
    let video = nodes_where(&tree, |widget| matches!(widget, Widget::Video(_)));
    let thumb = match &image[0].widget {
        Widget::Image(spec) => match &spec.source {
            ImageSource::Asset(path) => path.clone(),
            other => panic!("thumbnail comes from the package assets, got {other:?}"),
        },
        _ => unreachable!(),
    };
    let clip = match &video[0].widget {
        Widget::Video(spec) => match &spec.source {
            VideoSource::Asset(path) => path.clone(),
            other => panic!("clip comes from the package assets, got {other:?}"),
        },
        _ => unreachable!(),
    };
    (thumb, clip)
}

/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_on_returns_no_submissions() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(true)).expect("set");
    let submissions = host.deliver(vec![settings_changed(&["semantic"])]);
    assert!(submissions.is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_off_returns_no_submissions() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(false)).expect("set");
    let submissions = host.deliver(vec![settings_changed(&["semantic"])]);
    assert!(submissions.is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_a_change_of_other_keys_returns_no_submissions() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![settings_changed(&["scene", "observer"])]);
    assert!(submissions.is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_a_change_with_no_keys_returns_no_submissions() {
    let mut host = activated_client();
    let submissions = host.deliver(vec![settings_changed(&[])]);
    assert!(submissions.is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_an_empty_batch_returns_no_submissions() {
    let mut host = activated_client();
    assert!(host.deliver(Vec::new()).is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_a_batch_with_several_changes_is_handled_in_one_call_without_failure() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(true)).expect("set");
    let submissions = host.deliver(vec![
        settings_changed(&["semantic"]),
        settings_changed(&["scene"]),
        settings_changed(&["semantic", "saved_places"]),
    ]);
    host.settle();
    assert!(submissions.is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: F-103
#[test]
fn semantic_103_the_handler_never_raises_a_failure_record() {
    let mut host = activated_client();
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

/// With semantic off and nothing stored the gallery selects the default scene.
///
/// REQ: F-103, F-093
#[test]
fn semantic_103_gallery_defaults_to_the_stars_thumbnail_and_clip() {
    let mut host = activated_client();
    let (thumb, clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/stars.png");
    assert_eq!(clip, "clips/stars.webm");
}

/// REQ: F-093
#[test]
fn semantic_093_gallery_follows_the_scene_row() {
    let mut host = activated_client();
    host.settings().set("scene", json!("clouds")).expect("set");
    host.deliver(vec![settings_changed(&["scene"])]);
    host.settle();
    let (thumb, clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/clouds.png");
    assert_eq!(clip, "clips/clouds.webm");
}

/// Turning semantic on offers the last suggestion kept in global storage as the starting scene.
///
/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_on_offers_the_stored_last_suggestion_as_the_starting_scene() {
    let mut host = client_host();
    seed_global_storage(&mut host, LAST_SCENE, b"clouds");
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    let (thumb, _clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/clouds.png");
}

/// With nothing stored the starting scene is stars.
///
/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_on_without_a_stored_suggestion_starts_with_stars() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    let (thumb, _clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/stars.png");
}

/// Non-UTF-8 bytes in the stored suggestion fall back to stars (page sample 4: `.ok()` then default).
///
/// REQ: F-103
#[test]
fn semantic_103_a_stored_suggestion_that_is_not_text_falls_back_to_stars() {
    let mut host = client_host();
    seed_global_storage(&mut host, LAST_SCENE, &[0xff, 0xfe, 0x00]);
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    let (thumb, _clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/stars.png");
}

/// The suggestion is read when the toggle turns on; turning it off leaves storage untouched.
///
/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_off_does_not_write_global_storage() {
    let mut host = client_host();
    seed_global_storage(&mut host, LAST_SCENE, b"clouds");
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    host.settings().set("semantic", json!(false)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    assert_eq!(host.storage().get(Bucket::Global, LAST_SCENE), Some(b"clouds".to_vec()));
}

/// Reading the suggestion never changes the stored value.
///
/// REQ: F-103
#[test]
fn semantic_103_turning_semantic_on_keeps_the_stored_suggestion_unchanged() {
    let mut host = client_host();
    seed_global_storage(&mut host, LAST_SCENE, b"clouds");
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    assert_eq!(host.storage().get(Bucket::Global, LAST_SCENE), Some(b"clouds".to_vec()));
}

/// REQ: F-103
#[test]
fn semantic_103_the_plugin_writes_only_the_global_bucket() {
    let mut host = activated_client();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["semantic"])]);
    host.settle();
    for bucket in [Bucket::Project, Bucket::Session, Bucket::Shared] {
        assert!(host.storage().dump(bucket).is_empty(), "{bucket:?}");
    }
}

/// Random event streams never make the handler fail or submit (the plugin only listens to settings changes).
///
/// REQ: F-103
#[test]
fn semantic_103_fuzzed_event_streams_never_fail_the_plugin() {
    let mut host = activated_client();
    assert!(host.fuzz_run(7, 40).is_ok());
    assert!(host.failures().is_empty());
}

/// Events other than settings changes are ignored without a submission.
///
/// REQ: F-103
#[test]
fn semantic_103_events_other_than_settings_changes_return_no_submissions() {
    let mut host = activated_client();
    let events: Vec<Event> = host.fuzz_events(11, 30);
    let submissions = host.deliver(events);
    assert!(submissions.is_empty());
}

/// The semantic row is the only row whose change triggers a suggestion; a change of `semantic` with
/// any other keys alongside is still handled once.
///
/// REQ: F-103
#[test]
fn semantic_103_semantic_next_to_other_keys_still_offers_the_suggestion() {
    let mut host = client_host();
    seed_global_storage(&mut host, LAST_SCENE, b"clouds");
    host.activate::<ilium_plugin_core_ambient::AmbientClient>();
    host.settings().set("semantic", json!(true)).expect("set");
    host.deliver(vec![settings_changed(&["scene", "semantic", "observer"])]);
    host.settle();
    let (thumb, _clip) = gallery_assets(&mut host);
    assert_eq!(thumb, "thumbs/clouds.png");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // REQ: F-103
    #[test]
    fn semantic_103_property_any_key_list_is_handled_without_submissions(keys in proptest::collection::vec("[a-z_]{0,12}", 0..6)) {
        let mut host = activated_client();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let submissions = host.deliver(vec![settings_changed(&refs)]);
        prop_assert!(submissions.is_empty());
        prop_assert!(host.failures().is_empty());
    }
}
