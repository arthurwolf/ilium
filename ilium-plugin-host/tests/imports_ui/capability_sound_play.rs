//! `ilium:host/sounds` (capability-sound-play.md).

use ilium_plugin_api::generated::host::sounds::HostSounds;

use crate::common::*;

/// REQ: WIT:ilium:host/sounds.catalog
#[test]
fn wit_sounds_catalog_lists_the_bundled_id() {
    let rig = rig(&["sound.play"]);
    assert!(rig.host.sounds().catalog().contains(&"chirping".to_owned()));
}

/// REQ: WIT:ilium:host/sounds.catalog
#[test]
fn wit_sounds_catalog_adds_the_plugins_own_ids() {
    let rig = rig(&["sound.play"]);
    rig.kernel.set_plugin_sounds(&["done"]);
    let catalog = rig.host.sounds().catalog();
    assert!(catalog.contains(&"done".to_owned()) && catalog.contains(&"chirping".to_owned()));
}

/// REQ: WIT:ilium:host/sounds.catalog
#[test]
fn wit_sounds_catalog_hides_other_plugins_sounds() {
    let rig = rig(&["sound.play"]);
    rig.kernel
        .set_other_plugin_sounds("other.plugin", &["theirs"]);
    assert!(!rig.host.sounds().catalog().contains(&"theirs".to_owned()));
}

/// REQ: WIT:ilium:host/sounds.catalog
#[test]
fn wit_sounds_catalog_is_always_granted() {
    let rig = rig(&[]);
    assert!(!rig.host.sounds().catalog().is_empty());
}
