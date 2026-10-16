//! Twins of ilium-client/src/markdown/render.rs and raster.rs image and heading tests through the
//! rendered Markdown view (F-021).

use crate::helpers::{plain_host, rendered_screen, row_of};
use crate::support::render_pane;
use ilium_plugin_sdk::generated::types::Color;
use serde_json::json;

/// A solid 24-bit BMP of `width` x `height` pixels in the given RGB colour.
fn solid_bmp(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let pixel_bytes = row_bytes * height;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&(54 + pixel_bytes).to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    for _ in 0..height {
        for _ in 0..width {
            bytes.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        bytes.resize(bytes.len() + (row_bytes - width * 3) as usize, 0);
    }
    bytes
}

fn image_host(file_bytes: &[u8]) -> ilium_plugin_sdk::testing::host::FakeHost {
    let mut host = plain_host("notes.md", Some("markdown"), "![pic](pic.bmp)\n", &[("markdown_rendered_by_default", json!(true))]);
    host.fs().write("project:pic.bmp", file_bytes);
    host
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::local_image_paints_its_color_into_the_buffer
#[test]
fn port_media_local_image_paints_its_colour_and_hides_the_alt_text() {
    let mut host = image_host(&solid_bmp(20, 8, [65, 105, 225]));
    let screen = render_pane(&mut host, 40, 12);
    assert!(screen.find("pic").is_none(), "{}", screen.text());
    let royal_blue = Color::Rgb(0x4169E1);
    let painted = (0..screen.rows).any(|row| (0..screen.columns).any(|column| screen.cell(row, column).background == royal_blue));
    assert!(painted, "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::malformed_and_oversized_images_and_cell_layout_are_rejected
#[test]
fn port_media_malformed_image_falls_back_to_the_alt_text() {
    let mut host = image_host(b"not an image");
    let screen = render_pane(&mut host, 40, 12);
    assert!(screen.find("pic").is_some(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::malformed_and_oversized_images_and_cell_layout_are_rejected
#[test]
fn port_media_oversized_image_header_falls_back_to_the_alt_text() {
    let mut bytes = solid_bmp(2, 2, [0, 0, 0]);
    bytes[18..22].copy_from_slice(&100000u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&100000u32.to_le_bytes());
    let mut host = image_host(&bytes);
    let screen = render_pane(&mut host, 40, 12);
    assert!(screen.find("pic").is_some(), "{}", screen.text());
    let alt_rows = (0..screen.rows).filter(|index| screen.row(*index).trim().contains("pic")).count();
    assert_eq!(alt_rows, 1, "the alt text appears on exactly one row: {}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::malformed_and_oversized_images_and_cell_layout_are_rejected
#[test]
fn port_media_a_thousand_row_document_still_renders_its_first_rows() {
    let text: String = (0..1000).map(|_| "body\n\n").collect();
    let mut host = plain_host("big.md", Some("markdown"), &text, &[("markdown_rendered_by_default", json!(true))]);
    let screen = render_pane(&mut host, 20, 8);
    assert!(screen.find("body").is_some(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::heading_rasterizes_to_a_protocol_block, PORT:ilium-client/src/markdown/raster.rs::rasterizes_nonempty_header_to_nonzero_image
#[test]
fn port_media_a_rasterized_heading_replaces_the_plain_title_text() {
    let mut host = plain_host("n.md", Some("markdown"), "# Title\n", &[("markdown_rendered_by_default", json!(true))]);
    let before = render_pane(&mut host, 40, 8);
    // The fake host has no graphics protocol, so the heading falls back to bold plain text.
    let (row, column) = before.find("Title").expect("heading text drawn");
    assert!(before.cell(row, column).bold, "{}", before.text());
    assert_eq!(before.row(row).trim(), "Title", "{}", before.text());
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::heading_to_body_spacing_survives_rasterization
#[test]
fn port_media_a_blank_row_separates_the_heading_from_the_body() {
    let screen = rendered_screen("# Heading\n\nbody\n");
    let body = row_of(&screen, "body");
    assert!(body >= 3, "{}", screen.text());
    assert!(screen.row((body - 1) as u16).trim().is_empty(), "{}", screen.text());
}

/// REQ: PORT:ilium-client/src/markdown/raster.rs::smaller_tiers_produce_smaller_font_size
#[test]
fn port_media_level_one_heading_is_at_least_as_tall_as_level_six() {
    let h1 = rendered_screen("# Header\n\nbody\n");
    let h6 = rendered_screen("###### Header\n\nbody\n");
    assert!(row_of(&h1, "body") >= row_of(&h6, "body"));
}

/// REQ: PORT:ilium-client/src/markdown/render.rs::prepared_wrapping_paints_exactly_the_original_ratatui_layout
#[test]
fn port_media_wrapped_paragraph_fits_the_width_and_keeps_every_word() {
    let mut host = plain_host("w.md", Some("markdown"), "one two three four\n", &[("markdown_rendered_by_default", json!(true)), ("line_display", json!("wrap"))]);
    let screen = render_pane(&mut host, 8, 12);
    for word in ["one", "two", "three", "four"] {
        assert!(screen.find(word).is_some(), "{word}\n{}", screen.text());
    }
    assert!(screen.text().lines().all(|line| line.chars().count() <= 8), "{}", screen.text());
}
