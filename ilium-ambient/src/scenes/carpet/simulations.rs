//! Bounded, deterministic hidden-body simulations in normalized ground coordinates.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvd_reflection_handles_multiple_border_crossings() {
        assert!((reflected(3.25) - 0.75).abs() < 1e-6);
        assert!((reflected(-0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn hamiltonian_cycle_is_closed_adjacent_and_unique() {
        for side in [4, 6, 12, 32] {
            let cycle = hamiltonian_cycle(side);
            assert_eq!(cycle.len(), side * side);
            let mut sorted = cycle.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), side * side);
            for index in 0..cycle.len() {
                let a = cycle[index];
                let b = cycle[(index + 1) % cycle.len()];
                assert_eq!(a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]), 1);
            }
        }
    }

    #[test]
    fn life_blinker_uses_conway_rules() {
        let mut state = LifeState::new(5, 0.0, false, &mut Rng::new(1));
        state.cells.fill(false);
        for x in 1..=3 { state.cells[2 * 5 + x] = true; }
        state.step(false);
        let alive: Vec<_> = state.cells.iter().enumerate().filter_map(|(i, alive)| alive.then_some(i)).collect();
        assert_eq!(alive, vec![7, 12, 17]);
    }

    #[test]
    fn clock_modes_depend_only_on_civil_time() {
        for mode in [Mode::DigitalClock, Mode::AnalogClock] {
            let options = SimulationOptions::default();
            let mut a = Simulations::new(4);
            let mut b = Simulations::new(4);
            let mut first = Vec::new();
            let mut second = Vec::new();
            a.update(mode, &options, 0.0, 45296.25, None, &mut first);
            b.update(mode, &options, 999999.0, 45296.25, None, &mut second);
            assert_bodies_equal(&first, &second);
        }
    }

    fn assert_bodies_equal(a: &[Body], b: &[Body]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!(a.from, b.from);
            assert_eq!(a.to, b.to);
            assert_eq!(a.height, b.height);
            assert_eq!(a.radius, b.radius);
        }
    }
}
