//! Animation features that live outside the server kernel: F-093, F-094, F-102, F-104.

use crate::common::*;

/// REQ: F-093
#[test]
fn f_093_every_dither_mode_is_available_and_they_are_fifteen() {
    assert_eq!(ilium_ambient::raster::DitherMode::ALL.len(), 15);
}

/// REQ: F-093
#[test]
fn f_093_dither_modes_are_distinct() {
    let all = ilium_ambient::raster::DitherMode::ALL;
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

/// REQ: F-094
#[test]
fn f_094_the_shared_appearance_default_is_color_mode() {
    let appearance = ilium_ambient::style::Appearance::default();
    assert!(format!("{:?}", appearance.mode).contains("Color"));
}

/// REQ: F-102
#[test]
fn f_102_an_old_format_iliumanim_package_is_refused_with_package_old_format() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("legacy.iliumanim");
    std::fs::write(&path, b"ILIUMANIM\x01legacy").expect("write");
    let error = ilium_plugin_host::package::read_package(&std::fs::read(&path).expect("read"))
        .expect_err("must refuse");
    assert!(format!("{error:?}").contains("old-format"), "{error:?}");
}

/// REQ: F-104
#[test]
fn f_104_the_animation_probe_is_hidden_but_parses() {
    let run = Sandbox::new().ilium(&["release-animation-probe", "--help"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
}

/// REQ: F-104
#[test]
fn f_104_gpu_is_optional_the_probe_does_not_require_a_device() {
    let run = Sandbox::new().ilium_env(
        &["release-animation-probe", "--help"],
        &[("WGPU_BACKEND", "none")],
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
}
