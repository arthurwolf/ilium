//! The views export: the preview panel, the scene gallery, the location picker and the renders of
//! the page at 40, 80 and 120 columns (F-093, F-094, F-103).

use ilium_plugin_sdk::types::{Constraint, Direction, ImageSource, MediaFit, PixelFormat, UiTree, VideoSource, ViewTarget, Widget};
use proptest::prelude::*;

use crate::common::*;

fn panel(id: &str) -> ViewTarget {
    ViewTarget::Panel(id.to_string())
}

fn surface_spec(tree: &UiTree) -> ilium_plugin_sdk::types::SurfaceSpec {
    match nodes_where(tree, |widget| matches!(widget, Widget::Surface(_))).remove(0).widget {
        Widget::Surface(spec) => spec,
        _ => unreachable!(),
    }
}

/// The ui-tree invariants of the view contract: root inside, children after their parent, every
/// non-root node exactly once, keys unique across the whole view.
fn assert_valid_tree(tree: &UiTree) {
    assert!(!tree.nodes.is_empty());
    assert!((tree.root as usize) < tree.nodes.len());
    let mut seen = vec![0u32; tree.nodes.len()];
    for (index, node) in tree.nodes.iter().enumerate() {
        for child in &node.children {
            assert!(*child as usize > index, "child {child} must come after parent {index}");
            assert!((*child as usize) < tree.nodes.len());
            seen[*child as usize] += 1;
        }
    }
    for (index, count) in seen.iter().enumerate() {
        let expected = if index == tree.root as usize { 0 } else { 1 };
        assert_eq!(*count, expected, "node {index}");
    }
    let mut keys: Vec<&str> = tree.nodes.iter().filter_map(|node| node.key.as_deref()).collect();
    let total = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), total, "duplicate widget key");
}

// --- preview surface (page sample 1) ----------------------------------------------------------

/// REQ: F-093, F-094
#[test]
fn views_093_scene_preview_panel_is_one_rgb_surface_keyed_frame() {
    let tree = render_direct(panel(PREVIEW_VIEW), 80, 24);
    let node = node_by_key(&tree, "frame").expect("frame surface");
    assert!(matches!(node.widget, Widget::Surface(_)));
    assert_eq!(surface_spec(&tree).format, PixelFormat::Rgb8);
}

/// The surface asks for at least 40 columns and 4 rows, the preview row height of the manifest.
///
/// REQ: F-093, F-094
#[test]
fn views_093_scene_preview_surface_asks_for_forty_columns_and_four_rows() {
    let spec = surface_spec(&render_direct(panel(PREVIEW_VIEW), 80, 24));
    assert_eq!(spec.min_columns, 40);
    assert_eq!(spec.min_rows, 4);
}

/// REQ: F-093
#[test]
fn views_093_scene_preview_surface_min_rows_equals_the_manifest_preview_height() {
    let preview = manifest_rows().into_iter().find(|row| row["type"] == "preview").expect("preview");
    let spec = surface_spec(&render_direct(panel(PREVIEW_VIEW), 80, 24));
    assert_eq!(u64::from(spec.min_rows), preview["height"].as_u64().expect("height"));
}

/// REQ: F-093
#[test]
fn views_093_scene_preview_tree_is_valid() {
    assert_valid_tree(&render_direct(panel(PREVIEW_VIEW), 80, 24));
}

// --- gallery (page sample 3) ------------------------------------------------------------------

/// REQ: F-103, F-093
#[test]
fn views_103_gallery_is_a_horizontal_layout_of_twenty_columns_and_the_rest() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    let layouts = nodes_where(&tree, |widget| matches!(widget, Widget::Layout(_)));
    assert_eq!(layouts.len(), 1);
    match &layouts[0].widget {
        Widget::Layout(spec) => {
            assert_eq!(spec.direction, Direction::Horizontal);
            assert_eq!(spec.constraints, vec![Constraint::Length(20), Constraint::Fill(1)]);
        }
        _ => unreachable!(),
    }
    assert_eq!(layouts[0].children.len(), 2);
}

/// REQ: F-103
#[test]
fn views_103_gallery_thumbnail_is_a_contained_asset_image_keyed_thumb() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    let node = node_by_key(&tree, "thumb").expect("thumb");
    match node.widget {
        Widget::Image(spec) => {
            assert_eq!(spec.fit, MediaFit::Contain);
            assert!(matches!(spec.source, ImageSource::Asset(ref path) if path.starts_with("thumbs/") && path.ends_with(".png")));
        }
        other => panic!("expected an image, got {other:?}"),
    }
}

/// REQ: F-103
#[test]
fn views_103_gallery_clip_is_a_looping_autoplay_asset_video_covering_its_area() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    let node = node_by_key(&tree, "clip").expect("clip");
    match node.widget {
        Widget::Video(spec) => {
            assert!(spec.autoplay);
            assert!(spec.looped);
            assert_eq!(spec.fit, MediaFit::Cover);
            assert!(matches!(spec.source, VideoSource::Asset(ref path) if path.starts_with("clips/") && path.ends_with(".webm")));
        }
        other => panic!("expected a video, got {other:?}"),
    }
}

/// The thumbnail sits left of the clip: the layout lists the image first.
///
/// REQ: F-103
#[test]
fn views_103_gallery_layout_lists_the_thumbnail_before_the_clip() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    let layout = nodes_where(&tree, |widget| matches!(widget, Widget::Layout(_))).remove(0);
    let first = &tree.nodes[layout.children[0] as usize];
    let second = &tree.nodes[layout.children[1] as usize];
    assert_eq!(first.key.as_deref(), Some("thumb"));
    assert_eq!(second.key.as_deref(), Some("clip"));
}

/// REQ: F-103
#[test]
fn views_103_gallery_tree_is_valid() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    assert_valid_tree(&tree);
    assert_eq!(nodes_where(&tree, |widget| matches!(widget, Widget::Layout(_))).len(), 1);
    assert_eq!(nodes_where(&tree, |widget| matches!(widget, Widget::Image(_))).len(), 1);
    assert_eq!(nodes_where(&tree, |widget| matches!(widget, Widget::Video(_))).len(), 1);
    assert!(node_by_key(&tree, "thumb").is_some());
    assert!(node_by_key(&tree, "clip").is_some());
}

/// REQ: F-103
#[test]
fn views_103_gallery_assets_are_named_after_the_same_scene() {
    let tree = render_direct(panel(GALLERY), 80, 24);
    let thumb = match node_by_key(&tree, "thumb").expect("thumb").widget {
        Widget::Image(spec) => match spec.source {
            ImageSource::Asset(path) => path,
            _ => panic!("asset"),
        },
        _ => panic!("image"),
    };
    let clip = match node_by_key(&tree, "clip").expect("clip").widget {
        Widget::Video(spec) => match spec.source {
            VideoSource::Asset(path) => path,
            _ => panic!("asset"),
        },
        _ => panic!("video"),
    };
    let scene_of_thumb = thumb.trim_start_matches("thumbs/").trim_end_matches(".png");
    let scene_of_clip = clip.trim_start_matches("clips/").trim_end_matches(".webm");
    assert_eq!(scene_of_thumb, scene_of_clip);
}

// --- location picker (page sample 2) ----------------------------------------------------------

/// The picker binds to the `observer` row, is keyed `place` and offers search.
///
/// REQ: F-093, F-094
#[test]
fn views_093_a_panel_renders_the_location_picker_bound_to_the_observer_row() {
    let found = ["location", PREVIEW_VIEW, GALLERY]
        .iter()
        .map(|id| render_direct(panel(id), 80, 24))
        .flat_map(|tree| nodes_where(&tree, |widget| matches!(widget, Widget::LocationPicker(_))))
        .next()
        .expect("one panel renders a location-picker");
    assert_eq!(found.key.as_deref(), Some("place"));
    match found.widget {
        Widget::LocationPicker(spec) => {
            assert_eq!(spec.bind.as_deref(), Some("observer"));
            assert!(spec.search);
        }
        _ => unreachable!(),
    }
}

// --- the host draws the views at 40, 80 and 120 columns ----------------------------------------

fn drawn(id: &str, columns: u16, rows: u16) -> ilium_plugin_sdk::testing::Screen {
    let mut host = activated_client();
    host.render(panel(id), columns, rows)
}

/// REQ: F-093, F-103
#[test]
fn views_103_gallery_screen_has_the_requested_size_at_forty_columns() {
    let screen = drawn(GALLERY, 40, 12);
    assert_eq!((screen.columns, screen.rows), (40, 12));
}

/// REQ: F-093, F-103
#[test]
fn views_103_gallery_screen_has_the_requested_size_at_eighty_columns() {
    let screen = drawn(GALLERY, 80, 24);
    assert_eq!((screen.columns, screen.rows), (80, 24));
}

/// REQ: F-093, F-103
#[test]
fn views_103_gallery_screen_has_the_requested_size_at_one_hundred_twenty_columns() {
    let screen = drawn(GALLERY, 120, 30);
    assert_eq!((screen.columns, screen.rows), (120, 30));
}

/// REQ: F-093, F-094
#[test]
fn views_093_preview_screen_has_the_requested_size_at_forty_eighty_and_one_twenty_columns() {
    for columns in [40u16, 80, 120] {
        let screen = drawn(PREVIEW_VIEW, columns, 6);
        assert_eq!((screen.columns, screen.rows), (columns, 6));
    }
}

/// No drawn row is wider than the screen.
///
/// REQ: F-093, F-103
#[test]
fn views_103_no_row_of_the_gallery_screen_is_wider_than_its_width() {
    for columns in [40u16, 80, 120] {
        let screen = drawn(GALLERY, columns, 12);
        for row in 0..screen.rows {
            assert!(screen.row(row).chars().count() <= usize::from(columns), "row {row} at {columns}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-093, F-103
    #[test]
    fn views_103_property_both_panels_render_a_valid_tree_in_any_area(columns in 20u16..200, rows in 4u16..60) {
        assert_valid_tree(&render_direct(panel(GALLERY), columns, rows));
        assert_valid_tree(&render_direct(panel(PREVIEW_VIEW), columns, rows));
    }
}

// --- the renders printed on the page ------------------------------------------------------------

/// REQ: F-093, F-094
#[test]
fn doc_f_094_page_shows_two_renders_at_forty_eight_columns() {
    let blocks = term_blocks(&page_text());
    assert_eq!(blocks.len(), 2);
    for block in &blocks {
        for line in block.lines() {
            assert_eq!(line.chars().count(), 48, "{line:?}");
        }
    }
}

/// REQ: F-093, F-094
#[test]
fn doc_f_094_animations_render_lists_scene_row_preview_and_four_shared_controls() {
    let block = &term_blocks(&page_text())[0];
    assert!(block.starts_with("╭─ Animations "));
    for text in ["Scene  ▸ Stars   Clouds   Pipes   Video", "Preview, live", "Colour mode", "Palette", "Dithering", "Frame-rate cap"] {
        assert!(block.contains(text), "{text}");
    }
    assert!(block.contains("‹ Color ›") && block.contains("‹ Dusk ›") && block.contains("‹ Bayer 4 ›") && block.contains("‹ 24 fps ›"));
}

/// Form row leaders: `…` with one blank cell on each side (CLAUDE.md "Form row leaders").
///
/// REQ: F-094
#[test]
fn doc_f_094_shared_control_rows_connect_label_and_value_with_grey_dotted_leaders() {
    let block = &term_blocks(&page_text())[0];
    let mut leader_rows = 0;
    for line in block.lines().filter(|line| line.contains('…')) {
        leader_rows += 1;
        let cells: Vec<char> = line.chars().collect();
        let first = cells.iter().position(|symbol| *symbol == '…').expect("first");
        let last = cells.iter().rposition(|symbol| *symbol == '…').expect("last");
        assert_eq!(cells[first - 1], ' ', "{line}");
        assert_eq!(cells[last + 1], ' ', "{line}");
        assert!(cells[first..=last].iter().all(|symbol| *symbol == '…'), "{line}");
    }
    assert_eq!(leader_rows, 4);
}

/// REQ: F-093
#[test]
fn doc_f_093_location_render_shows_the_geocoded_label_and_both_coordinates_with_four_decimals() {
    let block = &term_blocks(&page_text())[1];
    assert!(block.starts_with("╭─ Location "));
    assert!(block.contains("● Lisbon, Portugal"));
    let latitude = block.lines().find(|line| line.contains("Latitude")).expect("latitude");
    let longitude = block.lines().find(|line| line.contains("Longitude")).expect("longitude");
    assert!(latitude.contains("38.7223│") && longitude.contains("-9.1393│"));
}

/// The coordinates of the render equal the observer default of the manifest.
///
/// REQ: F-093
#[test]
fn doc_f_093_location_render_coordinates_equal_the_observer_default() {
    let observer = row("observer");
    assert_eq!(observer["default"]["lat"].as_f64(), Some(38.7223));
    assert_eq!(observer["default"]["lon"].as_f64(), Some(-9.1393));
}

/// The Latitude and Longitude rows of the location render follow the leader rule too.
///
/// REQ: F-093
#[test]
fn doc_f_093_location_rows_use_blank_padded_leaders() {
    let block = &term_blocks(&page_text())[1];
    for label in ["Latitude", "Longitude"] {
        let line = block.lines().find(|line| line.contains(label)).expect("row");
        assert!(line.contains(&format!("{label} …")), "{line}");
        assert!(line.contains("… "), "{line}");
    }
}
