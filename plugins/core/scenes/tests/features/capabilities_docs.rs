//! Grants of the catalogue, the manifest capabilities and the page samples 3 and 5 (F-099, F-100, F-101, COR-010, COR-011).
use crate::common::*;
use crate::feature_support::*;
use serde_json::json;

fn row(family: &str) -> Vec<String> {
    table_after(&scenes_page(), "| Family | Scenes")
        .into_iter()
        .find(|r| r[0] == family)
        .unwrap_or_else(|| panic!("row {family}"))
}

/// REQ: F-101
#[test]
fn grants_101_video_needs_media_video_and_fs_pick() {
    let grants = &row("Video")[4];
    assert!(
        grants.contains("`media.video`") && grants.contains("`fs.pick`"),
        "{grants}"
    );
}

/// REQ: F-101
#[test]
fn grants_101_spectrum_needs_audio_loopback_and_images_need_codec() {
    assert!(row("Audio spectrum")[4].contains("`audio.loopback`"));
    let images = &row("Images")[4];
    assert!(
        images.contains("`fs.pick`") && images.contains("`codec`"),
        "{images}"
    );
}

/// REQ: F-099, F-100
#[test]
fn grants_099_live_and_wikipedia_scenes_need_host_scoped_http() {
    assert!(row("Live data")[4].contains("`http:<host>`"));
    assert!(row("Wikipedia")[4].contains("`http:*.wikipedia.org`"));
    assert!(
        row("Maps and space")[4].contains("`http`") && row("Maps and space")[4].contains("`cache`")
    );
}

/// REQ: F-098
#[test]
fn grants_098_lichess_tv_needs_http_lichess_and_games_need_nothing() {
    assert!(row("Carpet simulations")[4].contains("`http:lichess.org`"));
    assert_eq!(row("Games")[4], "none");
}

/// REQ: F-100
#[test]
fn grants_100_voxel_landscape_needs_saves_scope_inflate_and_jobs() {
    let grants = &row("Saved worlds")[4];
    for needle in ["`minecraft-saves` scope", "`codec.inflate`", "jobs"] {
        assert!(grants.contains(needle), "{needle} in {grants}");
    }
}

/// REQ: F-095, F-096, F-097
#[test]
fn grants_095_natural_quiet_geometry_dithered_machine_rows_need_none() {
    for family in [
        "Natural cached loops",
        "Quiet procedural",
        "Geometry and mathematics",
        "Pipes, wind, growth",
        "Machines and clocks",
    ] {
        assert_eq!(row(family)[4], "none", "{family}");
    }
}

/// REQ: COR-010, F-100
#[test]
fn caps_010_manifest_capabilities_are_scene_side_grants_for_the_samples() {
    let caps = manifest_value()["capabilities"]["scene"]
        .as_array()
        .unwrap()
        .clone();
    for needle in [
        "media.video",
        "location.observer",
        "gpu.kernels",
        "http:services.swpc.noaa.gov",
    ] {
        assert!(caps.iter().any(|c| c == needle), "{needle}");
    }
}

/// REQ: F-101
#[test]
fn video_101_clip_limit_of_16_mib_is_documented_on_both_pages() {
    assert!(scenes_page().contains("at most 16 MiB"));
    assert!(page("api-media").contains("quota-exceeded") && page("api-media").contains("16 MiB"));
}

/// REQ: F-101
#[test]
fn video_101_poster_size_comes_from_codec_without_decoding() {
    let text = scenes_page();
    assert!(text.contains("`codec` import reads the poster image size without decoding it"));
    assert!(text.contains("codec::image_info(POSTER)"));
}

/// REQ: COR-011
#[test]
fn gpu_011_page_requires_a_cpu_path_and_falls_back_on_unsupported() {
    let text = scenes_page();
    assert!(text.contains("every call needs a CPU path"));
    assert!(text.contains("ErrorCode::Unsupported"));
    assert!(page("api-gpu").contains("A kernel the host does not offer returns `unsupported`"));
}

/// REQ: COR-010
#[test]
fn point_010_page_example_pack_entry_matches_the_extension_contract() {
    let example = json_block_after(&scenes_page(), "#### Example: Bundles a slow cloud scene");
    let ext = &example["contributes"]["extensions"][0];
    assert_eq!(ext["point"], POINT_PACKS);
    assert_eq!(ext["priority"], 50);
    assert_eq!(
        ext["entry"]["scenes"][0],
        json!({"scene":"clouds","controls":{"speed":0.2}})
    );
    assert_eq!(
        example["capabilities"]["scene"][0],
        format!("extensions.contribute:{POINT_PACKS}")
    );
}

/// REQ: COR-010
#[test]
fn point_010_page_table_lists_exactly_two_collect_points() {
    let rows = table_after(&scenes_page(), "| Point | Kind");
    let points: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(points, ["`scene-packs`", "`scene-data-sources`"]);
    assert!(rows.iter().all(|r| r[1] == "collect" && r[4] == "standard"));
}
