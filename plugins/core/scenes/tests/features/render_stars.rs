//! Stars overhead (page sample 4, F-100). Formula of the page at J2000.
use crate::common::*;
use crate::feature_support::*;

fn stars_at(
    columns: u16,
    rows: u16,
    latitude: f64,
    longitude: f64,
    time_millis: u64,
) -> ilium_plugin_sdk::testing::Screen {
    fake_scene_at("stars-overhead", columns, rows, latitude, longitude).frame(braille_frame(
        columns,
        rows,
        time_millis,
    ))
}

/// REQ: F-100
#[test]
fn stars_100_without_an_observer_the_sky_is_still_drawn_for_greenwich() {
    let screen = fake_scene("stars-overhead", 40, 20).frame(braille_frame(40, 20, 0));
    assert!(!lit_cells(&screen).is_empty());
}

/// REQ: F-100
#[test]
fn stars_100_polaris_is_visible_from_the_north() {
    let screen = stars_at(40, 20, 89.9, 0.0, 0);
    let lit = lit_cells(&screen);
    assert!(!lit.is_empty());
    assert!(lit.iter().all(|&(row, column)| row < 20 && column < 40));
}

/// REQ: F-100
#[test]
fn stars_100_the_sky_shows_many_stars_not_a_handful() {
    let screen = stars_at(80, 40, 45.0, 10.0, 0);
    assert!(lit_cells(&screen).len() > 6);
}

/// REQ: F-100
#[test]
fn stars_100_polaris_stays_near_the_centre_at_the_north_pole() {
    let screen = stars_at(41, 21, 89.9, 0.0, 0);
    let lit = lit_cells(&screen);
    assert!(
        lit.iter()
            .any(|&(row, column)| row.abs_diff(10) <= 2 && column.abs_diff(20) <= 2),
        "{lit:?}"
    );
}

/// REQ: F-100
#[test]
fn stars_100_the_sky_is_nearer_to_itself_after_one_sidereal_day_than_after_six_hours() {
    let start = stars_at(40, 20, 48.0, 2.0, 0);
    let day = stars_at(40, 20, 48.0, 2.0, SIDEREAL_DAY_MILLIS);
    let quarter = stars_at(40, 20, 48.0, 2.0, 6 * 3_600_000);
    assert!(equal_share(&start, &day) > equal_share(&start, &quarter));
}

/// REQ: F-100
#[test]
fn stars_100_the_sky_moves_within_six_hours() {
    let a = stars_at(40, 20, 48.0, 2.0, 0);
    let b = stars_at(40, 20, 48.0, 2.0, 6 * 3_600_000);
    assert_ne!(a.text(), b.text());
}

/// REQ: F-100
#[test]
fn stars_100_extreme_latitudes_render_without_failure() {
    for latitude in [90.0, -90.0, 89.999, -89.999] {
        let screen = stars_at(40, 20, latitude, 180.0, 0);
        assert_eq!((screen.columns, screen.rows), (40, 20));
    }
}

/// REQ: F-100
#[test]
fn stars_100_every_width_draws_a_sky() {
    for (columns, rows) in [(40u16, 10u16), (80, 24), (120, 30)] {
        let screen = stars_at(columns, rows, 48.0, 2.0, 0);
        assert_eq!((screen.columns, screen.rows), (columns, rows));
        assert!(!lit_cells(&screen).is_empty(), "{columns}x{rows}");
    }
}

/// REQ: F-100
#[test]
fn stars_100_same_input_gives_the_same_sky() {
    assert_eq!(
        stars_at(40, 20, 10.0, 20.0, 5000).text(),
        stars_at(40, 20, 10.0, 20.0, 5000).text()
    );
}
