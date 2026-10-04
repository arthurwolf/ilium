use super::*;

fn galaxy(seed: u64, star_count: usize, empire_count: usize) -> Galaxy {
    Galaxy::new(
        seed,
        GenerationConfig {
            star_count,
            empire_count,
            ..GenerationConfig::default()
        },
    )
}

fn assert_connected(galaxy: &Galaxy) {
    let mut seen = vec![false; galaxy.stars.len()];
    let mut stack = vec![0];
    while let Some(index) = stack.pop() {
        if seen[index] {
            continue;
        }
        seen[index] = true;
        stack.extend(galaxy.neighbors[index].iter().copied());
    }
    assert!(seen.into_iter().all(|value| value));
}

#[test]
fn a_truce_before_arrival_prevents_combat_and_returns_the_fleet() {
    let mut galaxy = galaxy(42, 120, 3);
    let (from, to) = galaxy.lanes[0];
    galaxy.stars[from].owner = Some(0);
    galaxy.stars[to].owner = Some(1);
    galaxy.relations[0][1].war = true;
    galaxy.launch(0, from, to, 160.0, false);
    galaxy.fleets[0].progress = 1.0;
    galaxy.relations[0][1].war = false;
    galaxy.move_fleets();
    assert_eq!(galaxy.stars[to].owner, Some(1));
    assert_eq!(galaxy.stats.battles, 0);
    assert_eq!(galaxy.stats.captures, 0);
    assert_eq!(galaxy.fleets.len(), 1);
    assert_eq!((galaxy.fleets[0].from, galaxy.fleets[0].to), (to, from));
    let ships = galaxy.empires[0].ships;
    galaxy.fleets[0].progress = 1.0;
    galaxy.move_fleets();
    assert!(galaxy.fleets.is_empty());
    assert!(galaxy.empires[0].ships > ships);
}

#[test]
fn galaxies_are_connected_reproducible_and_have_diverse_resources() {
    for seed in 1..=12 {
        let first = galaxy(seed, 240, 8);
        let second = galaxy(seed, 240, 8);
        assert_eq!(first.stars, second.stars);
        assert_eq!(first.lanes, second.lanes);
        assert_connected(&first);
        assert!(first.stars.iter().any(|star| star.minerals > 3.0));
        assert!(first.stars.iter().any(|star| star.energy < 2.0));
        assert_eq!(first.living_empires(), 8);
    }
    assert_ne!(galaxy(1, 240, 8).stars, galaxy(2, 240, 8).stars);
}

#[test]
fn simulation_changes_diplomacy_grows_and_never_teleports_ownership() {
    let mut battles = 0;
    let mut wars = 0;
    let mut treaties = 0;
    for seed in 1..=8 {
        let mut galaxy = galaxy(seed, 120, 6);
        for _ in 0..1000 {
            let before: Vec<_> = galaxy.stars.iter().map(|star| star.owner).collect();
            let captures_before = galaxy.stats.captures;
            galaxy.step();
            let changed = before
                .iter()
                .zip(&galaxy.stars)
                .filter(|(owner, star)| **owner != star.owner)
                .count();
            assert!(changed as u64 <= galaxy.stats.captures - captures_before);
            let mut replayed = before.clone();
            for &(from, to, owner) in &galaxy.last_captures {
                assert!(galaxy.neighbors[from].contains(&to));
                replayed[to] = Some(owner);
            }
            assert_eq!(
                replayed,
                galaxy
                    .stars
                    .iter()
                    .map(|star| star.owner)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                galaxy.last_captures.len() as u64,
                galaxy.stats.captures - captures_before
            );
            assert!(galaxy.empires.iter().all(|empire| empire.ships.is_finite()
                && empire.ships >= 0.0
                && empire.credits.is_finite()));
            assert!(galaxy
                .stars
                .iter()
                .all(|star| star.garrison.is_finite() && star.garrison >= 0.0));
            assert!(galaxy.fleets.len() <= galaxy.empires.len() * 3 + 1);
        }
        battles += galaxy.stats.battles;
        wars += galaxy.stats.wars;
        treaties += galaxy.stats.treaties;
        assert!(galaxy.stats.captures > 20);
    }
    assert!(battles > 0 && wars > 0 && treaties > 0);
}

#[test]
fn every_seed_converges_by_gradual_frontier_conquest_including_extreme_sizes() {
    for seed in 1..=16 {
        let count = if seed % 2 == 0 { 420 } else { 120 };
        let empire_count = if seed % 3 == 0 { 12 } else { 3 };
        let mut galaxy = galaxy(seed, count, empire_count);
        let mut previous_dominant_count = 0;
        for _ in 0..20_000 {
            galaxy.step();
            if let Some(dominant) = galaxy.hegemon {
                let owned = galaxy
                    .stars
                    .iter()
                    .filter(|star| star.owner == Some(dominant))
                    .count();
                assert!(owned >= previous_dominant_count);
                previous_dominant_count = owned;
            }
            if galaxy.winner.is_some() {
                break;
            }
        }
        let winner = galaxy
            .winner
            .expect("all valid generated galaxies converge");
        assert_eq!(galaxy.living_empires(), 1);
        assert!(galaxy.stars.iter().all(|star| star.owner == Some(winner)));
        assert!(galaxy.stats.eliminations >= empire_count - 1);
        assert!(galaxy.stats.captures >= (count - empire_count * 2) as u64);
    }
}

#[test]
fn identical_seed_and_tick_count_produce_identical_history() {
    let mut first = galaxy(734, 180, 7);
    let mut second = galaxy(734, 180, 7);
    for _ in 0..1600 {
        first.step();
        second.step();
    }
    assert_eq!(first.stars, second.stars);
    assert_eq!(first.fleets, second.fleets);
    assert_eq!(first.stats, second.stats);
    assert_eq!(first.winner, second.winner);
}

#[test]
fn legacy_240_star_geometry_remains_identical_at_default_shape() {
    let galaxy = galaxy(42, 240, 8);
    assert_eq!(galaxy.lanes.len(), 321);
    assert_eq!(
        &galaxy.lanes[..5],
        &[(0, 168), (0, 188), (68, 168), (68, 204), (68, 180)]
    );
    assert_eq!(
        galaxy.stars[..5]
            .iter()
            .map(|star| star.position)
            .collect::<Vec<_>>(),
        vec![
            (0.43520552, -0.14868936),
            (-0.1388955, 0.49823976),
            (-0.55704874, -0.45112786),
            (-0.14158894, -0.36720318),
            (0.31866172, -0.14413214),
        ]
    );
}

#[test]
fn default_and_maximum_density_preserve_count_connectivity_and_convergence() {
    for (seed, config) in [
        (42, GenerationConfig::default()),
        (
            71,
            GenerationConfig {
                star_count: 720,
                empire_count: 12,
                spiral_arms: 6,
                arm_spread: 25,
                arm_twist: 200,
                lane_links: 0,
            },
        ),
    ] {
        let mut galaxy = Galaxy::new(seed, config);
        assert_eq!(galaxy.stars.len(), config.star_count);
        assert_eq!(galaxy.stars, Galaxy::new(seed, config).stars);
        assert_connected(&galaxy);
        for _ in 0..20_000 {
            galaxy.step();
            if galaxy.winner.is_some() {
                break;
            }
        }
        let winner = galaxy.winner.expect("connected bounded galaxy must unify");
        assert!(galaxy.stars.iter().all(|star| star.owner == Some(winner)));
    }
}

#[test]
fn shape_and_short_link_controls_change_maps_without_breaking_the_backbone() {
    let sparse = GenerationConfig {
        star_count: 240,
        lane_links: 0,
        ..GenerationConfig::default()
    };
    let looped = GenerationConfig {
        lane_links: 4,
        ..sparse
    };
    let shaped = GenerationConfig {
        spiral_arms: 5,
        arm_spread: 150,
        arm_twist: 50,
        ..sparse
    };
    let sparse_galaxy = Galaxy::new(42, sparse);
    let looped_galaxy = Galaxy::new(42, looped);
    let shaped_galaxy = Galaxy::new(42, shaped);
    assert_eq!(sparse_galaxy.lanes.len(), 239);
    assert!(looped_galaxy.lanes.len() > sparse_galaxy.lanes.len());
    assert_ne!(sparse_galaxy.stars, shaped_galaxy.stars);
    for galaxy in [&sparse_galaxy, &looped_galaxy, &shaped_galaxy] {
        assert_connected(galaxy);
    }
}
