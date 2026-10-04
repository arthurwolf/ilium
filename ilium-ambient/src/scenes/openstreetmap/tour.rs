//! Bounded, allocation-free selection over the actual list length.
//! Geometry acquisition and failure/admission policy remain with the scene.
use super::{
    places,
    settings::{OpenStreetMapSettings, PLACE_NAMES},
};

/// Largest compiled list, not the size of the legacy ten-place catalogue.
const MAX_PLACES: usize = places::DESTINATIONS.len();

/// `seconds` is elapsed scene time for the legacy offline tour and unscaled
/// elapsed wall time for a configured list. The caller chooses that clock.
/// Invalid saved list state yields index 0 only as a transport value: the
/// request resolver independently rejects invalid list/member identities.
pub fn place_index(settings: &OpenStreetMapSettings, seconds: f64, seed: u64) -> usize {
    let (count, selected, minimum) = if settings.is_list_tour() {
        match (settings.active_list(), settings.selected_list_index()) {
            (Ok(list), Ok(selected)) => (list.members.len(), selected, 60),
            _ => return 0,
        }
    } else {
        (
            PLACE_NAMES.len(),
            settings.place.min(PLACE_NAMES.len() - 1),
            30,
        )
    };
    index_for(
        count,
        selected,
        settings.tour,
        seconds,
        settings.dwell_seconds.clamp(minimum, 1800),
        seed,
    )
    .unwrap_or(0)
}

fn index_for(
    count: usize,
    selected: usize,
    mode: usize,
    seconds: f64,
    dwell: i32,
    seed: u64,
) -> Option<usize> {
    if count == 0 || count > MAX_PLACES {
        return None;
    }
    let selected = selected.min(count - 1);
    if mode == 0 || count == 1 {
        return Some(selected);
    }
    let seconds = if seconds.is_finite() {
        seconds.max(0.)
    } else {
        0.
    };
    let dwell = f64::from(dwell.max(1));
    // Reduce in floating point BEFORE integer conversion. Saturating a very
    // large elapsed time to u64 would freeze it at an unrelated list position.
    let slot = ((seconds.rem_euclid(dwell * count as f64) / dwell).floor() as usize).min(count - 1);
    if mode == 1 {
        return Some((selected + slot) % count);
    }
    let mut order = [0_usize; MAX_PLACES];
    for (index, value) in order.iter_mut().take(count).enumerate() {
        *value = index;
    }
    order.swap(0, selected);
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
    Some(order[slot])
}

#[cfg(test)]
mod tests {
    use super::super::settings::SelectionMode;
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn selected_place_is_held_and_ordered_tour_visits_every_place() {
        let mut settings = OpenStreetMapSettings {
            place: 4,
            ..Default::default()
        };
        assert_eq!(place_index(&settings, 1_000_000., 19), 4);
        settings.tour = 1;
        for slot in 0..PLACE_NAMES.len() {
            assert_eq!(
                place_index(&settings, slot as f64 * 120., 19),
                (slot + 4) % PLACE_NAMES.len()
            );
        }
    }
    #[test]
    fn every_actual_list_is_a_permutation_with_selected_first_and_wraps() {
        for list in places::LISTS {
            for selected in 0..list.members.len() {
                for mode in [1, 2] {
                    let settings = OpenStreetMapSettings {
                        source: 2,
                        selection: SelectionMode::List,
                        place_list: list.id.into(),
                        destination_id: list.members[selected].into(),
                        tour: mode,
                        dwell_seconds: 120,
                        ..Default::default()
                    };
                    let order: Vec<_> = (0..list.members.len())
                        .map(|slot| place_index(&settings, slot as f64 * 120., 19))
                        .collect();
                    assert_eq!(order[0], selected);
                    assert_eq!(
                        order.iter().copied().collect::<BTreeSet<_>>(),
                        (0..list.members.len()).collect()
                    );
                    assert_eq!(
                        place_index(&settings, list.members.len() as f64 * 120., 19),
                        selected
                    );
                    if mode == 1 {
                        assert_eq!(
                            order,
                            (0..list.members.len())
                                .map(|i| (i + selected) % list.members.len())
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn empty_singleton_and_two_member_lists_do_not_underflow() {
        assert_eq!(index_for(0, 0, 2, 1., 1, 0), None);
        assert_eq!(index_for(MAX_PLACES + 1, 0, 2, 1., 1, 0), None);
        for seed in [0, 1, u64::MAX] {
            for seconds in [0., 60., 1e200] {
                assert_eq!(index_for(1, usize::MAX, 2, seconds, 60, seed), Some(0));
            }
            assert_eq!(index_for(2, 1, 2, 0., 60, seed), Some(1));
            assert_eq!(index_for(2, 1, 2, 60., 60, seed), Some(0));
        }
    }

    #[test]
    fn offline_theme_tour_uses_its_member_count_and_selected_first() {
        let list = places::list("offline-europe").unwrap();
        let settings = OpenStreetMapSettings {
            source: 0,
            selection: SelectionMode::List,
            place_list: list.id.into(),
            destination_id: "venice".into(),
            tour: 2,
            dwell_seconds: 60,
            ..Default::default()
        };
        let order: Vec<_> = (0..list.members.len())
            .map(|slot| place_index(&settings, slot as f64 * 60., 42))
            .collect();
        assert_eq!(order[0], list.index_of("venice").unwrap());
        assert_eq!(
            order.iter().copied().collect::<BTreeSet<_>>(),
            (0..list.members.len()).collect()
        );
        assert_eq!(
            place_index(&settings, list.members.len() as f64 * 60., 42),
            order[0]
        );
    }
    #[test]
    fn shuffle_seed_is_reproducible_and_changes_the_suffix() {
        let settings = OpenStreetMapSettings {
            tour: 2,
            ..Default::default()
        };
        let order = |seed| {
            (0..PLACE_NAMES.len())
                .map(|i| place_index(&settings, i as f64 * 120., seed))
                .collect::<Vec<_>>()
        };
        assert_eq!(order(7), order(7));
        assert_ne!(order(7), order(123));
        assert_eq!(
            order(7).into_iter().collect::<BTreeSet<_>>(),
            (0..PLACE_NAMES.len()).collect()
        );
    }
    #[test]
    fn bad_and_very_large_clocks_stay_bounded_and_remote_lists_use_a_sixty_second_floor() {
        for seconds in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.] {
            assert_eq!(index_for(3, 2, 1, seconds, 60, 0), Some(2));
        }
        assert!(index_for(3, 2, 1, f64::MAX, 60, 0).unwrap() < 3);
        let settings = OpenStreetMapSettings {
            source: 2,
            selection: SelectionMode::List,
            place_list: "historic-towns".into(),
            tour: 1,
            dwell_seconds: 30,
            ..Default::default()
        };
        assert_eq!(place_index(&settings, 59.999, 0), 0);
        assert_eq!(place_index(&settings, 60., 0), 1);
        assert_eq!(place_index(&settings, 240., 0), 0);
    }
}
