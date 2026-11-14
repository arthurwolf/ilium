use super::*;

fn territory(stars: &[Star]) -> Territory {
    Territory::new(stars, 100, 100)
}

fn star(x: f32, y: f32, owner: Option<usize>) -> Star {
    Star {
        position: (x, y),
        owner,
        minerals: 1.0,
        energy: 1.0,
        garrison: 1.0,
    }
}

fn coverage(field: &Territory, point: (f32, f32)) -> f32 {
    field.sample(point).map_or(0.0, |sample| sample.coverage)
}

#[test]
fn off_grid_colony_has_a_round_monotone_soft_edge() {
    let center = (0.123, -0.087);
    let field = territory(&[star(center.0, center.1, Some(2))]);
    let mut smallest = f32::INFINITY;
    let mut largest = 0.0_f32;
    for angle in 0..64 {
        let theta = angle as f32 * std::f32::consts::TAU / 64.0;
        let point = |r: f32| (center.0 + r * theta.cos(), center.1 + r * theta.sin());
        let mut previous = 1.0;
        let mut soft = false;
        for step in 0..=80 {
            let value = coverage(&field, point(step as f32 * 0.0025));
            assert!(value.is_finite() && (0.0..=1.0).contains(&value));
            assert!(value <= previous + 0.002, "non-monotone ray {angle}");
            soft |= value > 0.05 && value < 0.95;
            previous = value;
        }
        assert!(soft && previous == 0.0);
        let (mut low, mut high) = (0.0, 0.2);
        for _ in 0..24 {
            let mid = (low + high) * 0.5;
            if coverage(&field, point(mid)) >= 0.5 {
                low = mid;
            } else {
                high = mid;
            }
        }
        smallest = smallest.min(low);
        largest = largest.max(high);
    }
    assert!(smallest > 0.04 && largest < 0.14);
    assert!(
        largest - smallest < STEP,
        "anisotropic contour {smallest}..{largest}"
    );
}

#[test]
fn nearby_colonies_merge_but_distant_colonies_do_not_claim_the_gap() {
    let nearby = territory(&[star(-0.05, 0.0, Some(0)), star(0.05, 0.0, Some(0))]);
    assert!(coverage(&nearby, (0.0, 0.0)) > 0.5);
    assert!(nearby.sample((0.0, 0.25)).is_none());
    let detached = territory(&[star(-0.4, 0.0, Some(0)), star(0.4, 0.0, Some(0))]);
    assert_eq!(detached.sample((-0.4, 0.0)).unwrap().owner, 0);
    assert_eq!(detached.sample((0.4, 0.0)).unwrap().owner, 0);
    assert!(detached.sample((0.0, 0.0)).is_none());
}

#[test]
fn surrounded_colony_neutral_hole_and_capture_retain_local_identity() {
    let center = (0.123, -0.087);
    let mut stars = vec![star(center.0, center.1, Some(0))];
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::TAU / 8.0;
        stars.push(star(
            center.0 + 0.04 * angle.cos(),
            center.1 + 0.04 * angle.sin(),
            Some(1),
        ));
    }
    let mut field = territory(&stars);
    assert_eq!(field.sample(center).unwrap().owner, 0);
    stars[0].owner = None;
    assert!(field.sync(&stars));
    assert!(field.sample(center).is_none());
    assert!(field.fields.iter().all(|node| node[0] == 0.0));
    stars[0].owner = Some(2);
    assert!(field.sync(&stars));
    assert_eq!(field.sample(center).unwrap().owner, 2);
    assert!(field.fields.iter().all(|node| node[NEUTRAL] == 0.0));
}

#[test]
fn dirty_updates_match_fresh_final_ownership_without_rebuilding_geometry() {
    let mut stars = vec![
        star(-0.25, 0.0, Some(0)),
        star(0.0, 0.0, None),
        star(0.25, 0.0, Some(1)),
    ];
    let mut field = territory(&stars);
    let geometry = field.stencils.as_ptr();
    let storage = field.fields.as_ptr();
    for tick in 0..12 {
        // Several owner changes between renders; no event log is consulted.
        stars[0].owner = Some((tick + 1) % 3);
        stars[1].owner = if tick % 2 == 0 { Some(1) } else { None };
        stars[2].owner = Some((tick + 2) % 3);
        assert!(field.sync(&stars));
        let fresh = territory(&stars);
        for (a, b) in field
            .fields
            .iter()
            .flatten()
            .zip(fresh.fields.iter().flatten())
        {
            assert_eq!(a.to_bits(), b.to_bits());
        }
        assert_eq!(field.stencils.as_ptr(), geometry);
        assert_eq!(field.fields.as_ptr(), storage);
        assert_eq!(
            field.sample(stars[0].position).unwrap().owner,
            stars[0].owner.unwrap()
        );
        let before = stars.clone();
        assert!(!field.sync(&stars));
        assert_eq!(stars, before);
        stars[0].minerals += 1.0;
        assert!(!field.sync(&stars));
    }
    stars[0].position = (-0.3, 0.1);
    assert!(field.sync(&stars));
    assert_eq!(field.fields, territory(&stars).fields);
}

#[test]
fn interpolation_crosses_grid_lines_continuously_and_rejects_invalid_points() {
    let field = territory(&[star(0.123, -0.087, Some(0))]);
    let x = -1.0 + 113.0 * STEP;
    let left = coverage(&field, (x - 0.00001, -0.087));
    let right = coverage(&field, (x + 0.00001, -0.087));
    assert!(left > 0.05 && left < 0.95);
    assert!((left - right).abs() < 0.002);
    for point in [
        (f32::NAN, 0.0),
        (0.0, f32::INFINITY),
        (1.0, 0.0),
        (-1.01, 0.0),
    ] {
        assert!(field.sample(point).is_none());
    }
    assert!(territory(&[]).sample((0.0, 0.0)).is_none());
}

#[test]
fn hostile_contact_is_two_sided_and_neutral_contact_fades_to_black() {
    let mut stars = [star(-0.04, 0.0, Some(0)), star(0.04, 0.0, Some(1))];
    let mut field = territory(&stars);
    let left = field.sample((-0.00025, 0.0)).unwrap();
    let right = field.sample((0.00025, 0.0)).unwrap();
    assert_eq!((left.owner, right.owner), (0, 1));
    assert_eq!(left.contact.unwrap().0, 1);
    assert_eq!(right.contact.unwrap().0, 0);
    assert!(left.contact.unwrap().1 > 0.9 && right.contact.unwrap().1 > 0.9);
    assert!((left.coverage - right.coverage).abs() < 0.002);
    stars[1].owner = None;
    field.sync(&stars);
    assert!(coverage(&field, (0.0, 0.0)) < 0.001);
    assert!(coverage(&field, (-0.02, 0.0)) > 0.5);
    assert!(field.sample((0.02, 0.0)).is_none());
}

#[test]
fn territory_size_and_edge_softness_are_effective_without_owner_changes() {
    let stars = [star(0.0, 0.0, Some(0))];
    let small = Territory::new(&stars, 50, 100);
    let large = Territory::new(&stars, 150, 100);
    assert!(small.sample((0.08, 0.0)).is_none());
    assert!(large.sample((0.08, 0.0)).is_some());

    let mut field = territory(&stars);
    let firm = field.sample((0.05, 0.0)).unwrap();
    field.set_softness(200);
    let soft = field.sample((0.05, 0.0)).unwrap();
    assert_eq!(firm.owner, soft.owner);
    assert!(firm.coverage > soft.coverage);
    assert_eq!(field.radius_percent, 100);
}
