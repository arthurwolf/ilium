//! The catalogue must describe exactly the crates under `plugins/core/`, and every component
//! it holds must be a stamped WebAssembly component. These tests hold in every build mode; in
//! `skip` mode (no component built) the component checks have nothing to look at.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ilium_plugin_bundle::{find, plugins, BundledPlugin, COMPLETE, MISSING, MODE, PENDING};

const SIDES: [&str; 3] = ["server", "client", "scene"];
/// `\0asm`, then the component-model layer: version 13, layer 1.
const COMPONENT_HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
/// The stamped tail: custom section 0, size 24, name length 17, the name, version 0.1.0.
const API_VERSION_TAIL: &[u8] = b"\x00\x18\x11ilium:api-version\x00\x00\x00\x01\x00\x00";

fn core_directories() -> BTreeSet<String> {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/core");
    std::fs::read_dir(&core)
        .expect("plugins/core is listable")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .map(|path| {
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn declared_sides(plugin: &BundledPlugin) -> Vec<&'static str> {
    let manifest: serde_json::Value =
        serde_json::from_str(plugin.manifest).expect("manifest is JSON");
    SIDES
        .into_iter()
        .filter(|side| manifest.get(side).is_some_and(serde_json::Value::is_object))
        .collect()
}

#[test]
fn catalogue_lists_every_core_plugin_crate_once() {
    let listed: Vec<&str> = plugins().iter().map(|plugin| plugin.directory).collect();
    let unique: BTreeSet<String> = listed.iter().map(|name| (*name).to_owned()).collect();

    assert_eq!(
        unique.len(),
        listed.len(),
        "a plugin directory is listed twice"
    );
    assert_eq!(unique, core_directories());
}

#[test]
fn ids_are_unique_match_the_manifest_and_resolve() {
    let mut seen = BTreeSet::new();
    for plugin in plugins() {
        let manifest: serde_json::Value =
            serde_json::from_str(plugin.manifest).expect("manifest is JSON");

        assert_eq!(manifest["id"].as_str(), Some(plugin.id));
        assert!(seen.insert(plugin.id), "{} is listed twice", plugin.id);
        assert_eq!(
            find(plugin.id).map(|found| found.directory),
            Some(plugin.directory)
        );
    }
    assert!(find("ilium.core.no-such-plugin").is_none());
}

#[test]
fn every_declared_side_is_a_component_or_accounted_for() {
    for plugin in plugins() {
        let declared = declared_sides(plugin);
        for side in SIDES {
            let entry = (plugin.directory, side);
            let absent = MISSING.contains(&entry) || PENDING.contains(&entry);
            match plugin.component(side) {
                Some(_) => {
                    assert!(
                        declared.contains(&side),
                        "{entry:?} is built but not declared"
                    );
                    assert!(!absent, "{entry:?} is both built and listed as absent");
                }
                None => assert_eq!(
                    absent,
                    declared.contains(&side),
                    "{entry:?}: absent components must be exactly the declared, unbuilt ones"
                ),
            }
        }
    }
}

#[test]
fn every_component_is_a_stamped_webassembly_component() {
    for plugin in plugins() {
        for side in SIDES {
            let Some(bytes) = plugin.component(side) else {
                continue;
            };
            assert!(
                bytes.starts_with(&COMPONENT_HEADER),
                "{}/{side} is not a WebAssembly component",
                plugin.directory
            );
            assert!(
                bytes.ends_with(API_VERSION_TAIL),
                "{}/{side} lacks the ilium:api-version section",
                plugin.directory
            );
        }
    }
}

#[test]
fn completeness_agrees_with_the_mode_and_the_missing_list() {
    assert!(["skip", "lenient", "strict"].contains(&MODE));
    assert_eq!(COMPLETE, MODE != "skip" && MISSING.is_empty());
    if MODE == "strict" {
        assert!(MISSING.is_empty(), "a strict build cannot miss a component");
    }
    if MODE == "skip" {
        assert!(plugins()
            .iter()
            .all(|plugin| SIDES.iter().all(|side| plugin.component(side).is_none())));
    }
}

#[test]
fn service_schemas_are_embedded_as_json_of_the_provided_service() {
    let agent_setup = ilium_plugin_bundle::find("ilium.core.agent-setup").expect("agent-setup");
    let install = agent_setup
        .schemas
        .iter()
        .find(|schema| schema.text.contains("\"install\""))
        .expect("agent-setup embeds schemas/install.json");
    assert!(install.service.starts_with("ilium.core.agent-setup/"));
    for plugin in plugins() {
        for schema in plugin.schemas {
            let parsed: serde_json::Value =
                serde_json::from_str(schema.text).expect("schema files are JSON");
            assert!(parsed.is_object(), "{} schema root", schema.service);
        }
    }
}
