//! Constant-cost selection from a finite offline catalogue; no provider polling.
use super::settings::{OpenStreetMapSettings, PLACE_NAMES};
pub fn place_index(settings: &OpenStreetMapSettings, seconds: f64, seed: u64) -> usize {
    let count = PLACE_NAMES.len();
    let selected = settings.place.min(count - 1);
    if settings.tour == 0 {
        return selected;
    }
    let seconds = if seconds.is_finite() {
        seconds.max(0.)
    } else {
        0.
    };
    let slot = ((seconds / f64::from(settings.dwell_seconds.clamp(30, 1800))) as u64 % count as u64)
        as usize;
    if settings.tour == 1 {
        return (selected + slot) % count;
    }
    let mut order = std::array::from_fn::<_, 10, _>(|i| i);
    order.swap(0, selected);
    // Keep the explicitly selected place first, shuffle each remaining place once.
    let mut state = seed;
    for end in (2..count).rev() {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^= value >> 31;
        let other = 1 + (value % end as u64) as usize;
        order.swap(end, other);
    }
    order[slot]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_place_is_held_and_ordered_tour_visits_every_place() {
        let mut s = OpenStreetMapSettings {
            place: 4,
            ..Default::default()
        };
        assert_eq!(place_index(&s, 1_000_000., 19), 4);
        s.tour = 1;
        let mut seen = std::collections::HashSet::new();
        for slot in 0..PLACE_NAMES.len() {
            let i = place_index(&s, slot as f64 * f64::from(s.dwell_seconds), 19);
            assert_eq!(i, (slot + 4) % PLACE_NAMES.len());
            seen.insert(i);
        }
        assert_eq!(seen.len(), 10);
    }
    #[test]
    fn shuffled_tour_never_skips_a_place_and_seed_changes_order() {
        let s = OpenStreetMapSettings {
            tour: 2,
            ..Default::default()
        };
        let order = |seed| {
            (0..10)
                .map(|slot| place_index(&s, slot as f64 * f64::from(s.dwell_seconds), seed))
                .collect::<Vec<_>>()
        };
        let a = order(7);
        let b = order(123);
        assert_eq!(
            a.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            10
        );
        assert_eq!(
            b.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            10
        );
        assert_ne!(a, b);
        assert_eq!(order(7), a);
    }
}
