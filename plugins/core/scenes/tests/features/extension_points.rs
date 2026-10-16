//! Extension points and the catalog service (page "Extension points").
use crate::common::*;
use serde_json::json;

/// REQ: COR-010
#[test]
fn point_010_scene_packs_collects_a_contributed_pack() {
    let mut session = session_with_plugin();
    let entry = json!({"title":"Calm","scenes":[{"scene":"clouds","controls":{"speed":0.2}}]});
    contribute(&mut session, POINT_PACKS, "calm", 50, &entry);
    let collected = session.plugin(PLUGIN_ID).extensions().collect(POINT_PACKS);
    assert!(collected.iter().any(|e| e["title"] == "Calm"), "{collected:?}");
}

/// REQ: COR-010
#[test]
fn point_010_scene_data_sources_collects_a_contributed_source() {
    let mut session = session_with_plugin();
    contribute(&mut session, POINT_SOURCES, "feed", 50, &json!({"title":"Feed","service":"x.y/feed"}));
    let collected = session.plugin(PLUGIN_ID).extensions().collect(POINT_SOURCES);
    assert_eq!(collected.len(), 1);
}

/// REQ: COR-010
#[test]
fn point_010_higher_priority_collects_first() {
    let mut session = session_with_plugin();
    contribute(&mut session, POINT_PACKS, "low", 10, &json!({"title":"Low","scenes":[]}));
    contribute(&mut session, POINT_PACKS, "high", 90, &json!({"title":"High","scenes":[]}));
    let collected = session.plugin(PLUGIN_ID).extensions().collect(POINT_PACKS);
    assert_eq!(collected[0]["title"], "High");
}

/// REQ: COR-010
#[test]
fn point_010_empty_point_collects_nothing() {
    let mut session = session_with_plugin();
    assert!(session.plugin(PLUGIN_ID).extensions().collect(POINT_PACKS).is_empty());
}

/// REQ: F-095, F-096, F-100
#[test]
fn catalog_095_list_returns_the_three_manifest_scenes() {
    let mut session = session_with_plugin();
    let ids: Vec<String> = catalog_scenes(&mut session).iter().map(|s| s["id"].as_str().unwrap().to_string()).collect();
    for id in MANIFEST_SCENES {
        assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
    }
}

/// REQ: F-096
#[test]
fn catalog_096_entry_carries_title_and_family() {
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let waves = scenes.iter().find(|s| s["id"] == "waves").unwrap();
    assert_eq!(waves["title"], "Waves");
    assert_eq!(waves["family"], "procedural");
}

/// REQ: COR-010
#[test]
fn catalog_010_unknown_method_is_not_found() {
    let mut session = session_with_plugin();
    let result = catalog_call(&mut session, "nope", json!({}));
    let error = result.expect_err("unknown method"); assert!(format!("{:?}", error.code).to_lowercase().contains("notfound"), "{error:?}");
}
