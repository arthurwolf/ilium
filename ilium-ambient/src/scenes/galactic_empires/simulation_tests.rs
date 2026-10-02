use super::*;

#[test]
fn a_truce_before_arrival_prevents_combat_and_returns_the_fleet() {
    let mut galaxy = Galaxy::new(42, 120, 3);
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
        let first = Galaxy::new(seed, 240, 8);
        let second = Galaxy::new(seed, 240, 8);
        assert_eq!(first.stars, second.stars);
        assert_eq!(first.lanes, second.lanes);
        let mut seen = vec![false; first.stars.len()];
        let mut stack = vec![0];
        while let Some(index) = stack.pop() {
            if seen[index] {
                continue;
            }
            seen[index] = true;
            stack.extend(first.neighbors[index].iter().copied());
        }
        assert!(seen.into_iter().all(|value| value));
        assert!(first.stars.iter().any(|star| star.minerals > 3.0));
        assert!(first.stars.iter().any(|star| star.energy < 2.0));
        assert_eq!(first.living_empires(), 8);
    }
    assert_ne!(Galaxy::new(1, 240, 8).stars, Galaxy::new(2, 240, 8).stars);
}

#[test]
fn simulation_changes_diplomacy_grows_and_never_teleports_ownership() {
    let mut battles = 0;
    let mut wars = 0;
    let mut treaties = 0;
    for seed in 1..=8 {
        let mut galaxy = Galaxy::new(seed, 120, 6);
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
        let mut galaxy = Galaxy::new(seed, count, empire_count);
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
    let mut first = Galaxy::new(734, 180, 7);
    let mut second = Galaxy::new(734, 180, 7);
    for _ in 0..1600 {
        first.step();
        second.step();
    }
    assert_eq!(first.stars, second.stars);
    assert_eq!(first.fleets, second.fleets);
    assert_eq!(first.stats, second.stats);
    assert_eq!(first.winner, second.winner);
}
