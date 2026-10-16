//! `ilium:host/keys` (api-keys.md): grab and release.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::ids::GrabId;
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

use crate::common::*;

const GRANTS: &[&str] = &["keys.grab", "keys.intercept"];

fn spec(chords: &[&str]) -> GrabSpec {
    GrabSpec { chords: chords.iter().map(|c| (*c).to_owned()).collect(), pane: None, until_release: true }
}

fn chords_n(count: usize) -> GrabSpec {
    GrabSpec { chords: (0..count).map(|n| format!("ctrl+alt+f{}", n % 12 + 1) + &"x".repeat(n / 12)).collect(), pane: None, until_release: true }
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_returns_a_grab_id_and_registers_it() {
    let mut rig = rig(GRANTS);
    let grab = rig.host.keys().grab(spec(&["ctrl+k"])).expect("grabbed");
    assert!(rig.kernel.active_grabs().contains(&grab.0));
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_pane_scope_requires_an_existing_pane() {
    let mut rig = rig(GRANTS);
    let mut missing = spec(&["ctrl+k"]);
    missing.pane = Some(MISSING_PANE);
    assert_eq!(code_of(rig.host.keys().grab(missing)), ErrorCode::NotFound);
    let mut present = spec(&["ctrl+k"]);
    present.pane = Some(PANE);
    assert!(rig.host.keys().grab(present).is_ok());
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_chord_count_boundaries_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(code_of(rig.host.keys().grab(spec(&[]))), ErrorCode::InvalidArgument);
    let one = rig.host.keys().grab(spec(&["ctrl+k"])).expect("1 chord");
    rig.host.keys().release(one);
    let thirty_two = rig.host.keys().grab(chords_n(32)).expect("32 chords");
    rig.host.keys().release(thirty_two);
    assert_eq!(code_of(rig.host.keys().grab(chords_n(33))), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_bad_chord_grammar_invalid_argument() {
    let mut rig = rig(GRANTS);
    assert_eq!(code_of(rig.host.keys().grab(spec(&["ctrl+"]))), ErrorCode::InvalidArgument);
    assert_eq!(code_of(rig.host.keys().grab(spec(&["hyper+k"]))), ErrorCode::InvalidArgument);
    assert_eq!(code_of(rig.host.keys().grab(spec(&[""]))), ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_ninth_grab_quota_exceeded() {
    let mut rig = rig(GRANTS);
    for n in 0..8 {
        rig.host.keys().grab(spec(&[&format!("ctrl+f{}", n + 1)])).expect("within quota");
    }
    assert_eq!(code_of(rig.host.keys().grab(spec(&["ctrl+f9"]))), ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_chord_held_by_another_plugin_conflict() {
    let mut rig = rig(GRANTS);
    rig.kernel.hold_chord_for("other.plugin", "ctrl+k");
    assert_eq!(code_of(rig.host.keys().grab(spec(&["ctrl+k"]))), ErrorCode::Conflict);
}

/// REQ: WIT:ilium:host/keys.grab
#[test]
fn wit_keys_grab_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["keys.grab"]);
    assert_eq!(code_of(rig.host.keys().grab(spec(&["ctrl+k"]))), ErrorCode::NotPermitted);
}

proptest! {
    // REQ: WIT:ilium:host/keys.grab
    #[test]
    fn wit_keys_grab_accepts_every_well_formed_chord(
        modifiers in proptest::sample::subsequence(vec!["ctrl", "alt", "shift"], 0..=3),
        key in "[a-z]|f[1-9]|f1[0-2]"
    ) {
        let mut rig = rig(GRANTS);
        let chord = modifiers.iter().map(|m| m.to_string()).chain(std::iter::once(key)).collect::<Vec<_>>().join("+");
        prop_assert!(rig.host.keys().grab(spec(&[&chord])).is_ok(), "{chord}");
    }
}

/// REQ: WIT:ilium:host/keys.release
#[test]
fn wit_keys_release_frees_the_chord_and_the_quota_slot() {
    let mut rig = rig(GRANTS);
    let grab = rig.host.keys().grab(spec(&["ctrl+k"])).expect("grabbed");
    rig.host.keys().release(grab);
    assert!(!rig.kernel.active_grabs().contains(&grab.0));
    assert!(rig.host.keys().grab(spec(&["ctrl+k"])).is_ok());
}

/// REQ: WIT:ilium:host/keys.release
#[test]
fn wit_keys_release_unknown_grab_is_a_no_op() {
    let mut rig = rig(GRANTS);
    rig.host.keys().release(GrabId(31_337));
    assert!(rig.host.failures().is_empty());
}

/// REQ: WIT:ilium:host/keys.release
#[test]
fn wit_keys_release_twice_is_a_no_op() {
    let mut rig = rig(GRANTS);
    let grab = rig.host.keys().grab(spec(&["ctrl+k"])).expect("grabbed");
    rig.host.keys().release(grab);
    rig.host.keys().release(grab);
    assert!(rig.kernel.active_grabs().is_empty());
}
