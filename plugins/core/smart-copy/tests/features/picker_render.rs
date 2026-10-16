//! F-065/F-066: golden screen text of the Smart Copy picker popup (page figure, 48 columns) and
//! width sweeps at 40, 80 and 120 columns.

use crate::common::{candidates_message, client_host, page_figure, picker_target};

const CANDIDATES: [&str; 3] = ["src/server/routes.rs", "cargo test -p ilium-server", "9f3c2ab"];

fn render(width: u16, height: u16) -> Vec<String> {
    let mut host = client_host();
    host.deliver_one(candidates_message(&CANDIDATES));
    host.render_view(&picker_target(), width, height)
        .into_iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// REQ: F-065, F-066
#[test]
fn feature_065_picker_at_48_columns_matches_the_page_figure() {
    let figure = page_figure(1);
    let rows: Vec<String> = render(48, 7).into_iter().filter(|row| !row.is_empty()).collect();
    assert_eq!(rows, figure);
}

/// REQ: F-065
#[test]
fn feature_065_picker_lists_the_three_candidates_numbered() {
    let text = render(80, 10).join("\n");
    for (index, candidate) in CANDIDATES.iter().enumerate() {
        assert!(text.contains(&format!("{} {}", index + 1, candidate)), "{candidate}\n{text}");
    }
}

/// REQ: F-065
#[test]
fn feature_065_picker_shows_title_and_key_hints_at_every_width() {
    for width in [40u16, 80, 120] {
        let text = render(width, 10).join("\n");
        assert!(text.contains("Smart Copy"), "{width}");
        assert!(text.contains("copy"), "{width}");
        assert!(text.contains("Esc"), "{width}");
    }
}

/// REQ: F-065
#[test]
fn feature_065_picker_never_exceeds_the_screen_width() {
    for width in [40u16, 80, 120] {
        for row in render(width, 10) {
            assert!(row.chars().count() <= width as usize, "{width}: {row:?}");
        }
    }
}

/// REQ: F-065
#[test]
fn feature_065_picker_keeps_numbers_visible_when_a_candidate_is_wider_than_the_screen() {
    let mut host = client_host();
    host.deliver_one(candidates_message(&["a/very/long/path/that/keeps/going/and/going/and/going/forever.rs"]));
    let rows = host.render_view(&picker_target(), 40, 8);
    let text = rows.join("\n");
    assert!(text.contains("1 a/very"), "{text}");
    assert!(rows.iter().all(|row| row.trim_end().chars().count() <= 40));
}

/// REQ: F-065
#[test]
fn feature_065_picker_is_empty_before_any_candidates_arrive() {
    let mut host = client_host();
    let rows = host.render_view(&picker_target(), 80, 8);
    assert!(rows.iter().all(|row| row.trim().is_empty()));
}

/// REQ: F-066
#[test]
fn feature_066_picker_shows_at_most_three_regions() {
    let mut host = client_host();
    host.deliver_one(candidates_message(&["one/a.rs", "two/b.rs", "three/c.rs", "four/d.rs"]));
    let text = host.render_view(&picker_target(), 80, 12).join("\n");
    assert!(text.contains("3 three/c.rs"), "{text}");
    assert!(!text.contains("four/d.rs"), "{text}");
}
