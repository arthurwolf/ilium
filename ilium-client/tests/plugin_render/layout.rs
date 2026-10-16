use crate::support::*;

/// REQ: WID-005
#[test]
fn wid_005_three_children_two_constraints_traps_constraint_mismatch() {
    let mut rig = WidgetRig::new(40, 5);
    let err = rig.show_layout(3, &[Constraint::Length(10), Constraint::Fill(1)]).unwrap_err();
    assert_eq!(err, "ui.invalid-tree");
}

/// REQ: WID-005
#[test]
fn wid_005_matching_counts_are_accepted() {
    let mut rig = WidgetRig::new(40, 5);
    assert!(rig.show_layout(2, &[Constraint::Length(10), Constraint::Fill(1)]).is_ok());
}

/// REQ: WID-005
#[test]
fn wid_005_more_constraints_than_children_traps() {
    let mut rig = WidgetRig::new(40, 5);
    assert!(rig.show_layout(1, &[Constraint::Length(10), Constraint::Fill(1)]).is_err());
}

/// REQ: WID-006
#[test]
fn wid_006_percentage_150_clamped_to_100() {
    let widths = solve_row(40, &[Constraint::Percentage(150)]);
    assert_eq!(widths, vec![40]);
}

/// REQ: WID-006
#[test]
fn wid_006_percentage_100_gets_whole_row() {
    assert_eq!(solve_row(40, &[Constraint::Percentage(100)]), vec![40]);
}

/// REQ: WID-007
#[test]
fn wid_007_fixed_10_and_fill_in_40_columns() {
    assert_eq!(solve_row(40, &[Constraint::Length(10), Constraint::Fill(1)]), vec![10, 30]);
}

/// REQ: WID-007
#[test]
fn wid_007_ratio_and_percentage_share_the_row() {
    let widths = solve_row(40, &[Constraint::Percentage(50), Constraint::Ratio(1, 4), Constraint::Fill(1)]);
    assert_eq!(widths, vec![20, 10, 10]);
}

/// REQ: WID-007
#[test]
fn wid_007_min_and_max_respected_in_order() {
    let widths = solve_row(40, &[Constraint::Min(30), Constraint::Max(5), Constraint::Fill(1)]);
    assert!(widths[0] >= 30 && widths[1] <= 5);
    assert_eq!(widths.iter().sum::<u16>(), 40);
}

/// REQ: WID-007
#[test]
fn wid_007_two_fills_split_by_weight() {
    assert_eq!(solve_row(40, &[Constraint::Fill(1), Constraint::Fill(3)]), vec![10, 30]);
}

/// REQ: WID-007
#[test]
fn wid_007_zero_width_area_gives_zero_widths() {
    assert_eq!(solve_row(0, &[Constraint::Length(5), Constraint::Fill(1)]), vec![0, 0]);
}

/// REQ: WID-007
#[test]
fn wid_007_cells_always_sum_to_area_width() {
    proptest::proptest!(|(width in 1u16..300, a in 0u16..100, b in 0u16..100, w in 1u16..5)| {
        let widths = solve_row(width, &[Constraint::Length(a), Constraint::Percentage(b), Constraint::Fill(w)]);
        proptest::prop_assert_eq!(widths.iter().sum::<u16>(), width);
    });
}

/// REQ: WID-006, WID-007
#[test]
fn wid_006_percentage_never_exceeds_area() {
    proptest::proptest!(|(width in 1u16..300, pct in 0u16..1000)| {
        let widths = solve_row(width, &[Constraint::Percentage(pct)]);
        proptest::prop_assert!(widths[0] <= width);
    });
}

/// REQ: WID-023
#[test]
fn wid_023_gauge_filled_cells_are_monotonic_in_ratio() {
    proptest::proptest!(|(a in -2.0f64..3.0, b in -2.0f64..3.0)| {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let mut rig = WidgetRig::new(20, 1);
        rig.show_gauge(lo);
        let low = rig.gauge_filled_cells();
        rig.show_gauge(hi);
        proptest::prop_assert!(low <= rig.gauge_filled_cells());
    });
}
