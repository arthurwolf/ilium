//! What to play next, and the clock arithmetic for frames and the status line.
//! Pure logic: no I/O, no clock, deterministic for a given seed.

use super::settings::{PlaybackMode, VideoSettings};

/// SplitMix64: tiny, fast, and reproducible across platforms.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    /// Uniform in `0..bound` (`bound` of 0 gives 0).
    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The seed of a scene: the user's fixed seed, or one taken from the clock for 0.
pub fn resolve_seed(setting: u16, clock_seed: u64) -> u64 {
    if setting != 0 {
        u64::from(setting)
    } else {
        clock_seed
    }
}

/// One clip to play: an index into the entry list and, in random-scene mode, the excerpt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayItem {
    pub entry: usize,
    /// Where playback starts, in milliseconds of the source.
    pub start_millis: u64,
    /// How long it plays, in milliseconds of the source; `None` plays to the end.
    pub limit_millis: Option<u64>,
}

/// Picks the next clip of a list of `len` entries.
pub struct Scheduler {
    mode: PlaybackMode,
    scene_millis: u64,
    shuffle: bool,
    repeat_one: bool,
    rng: Rng,
    len: usize,
    order: Vec<usize>,
    cursor: usize,
    last: Option<usize>,
    last_repeatable: bool,
}

impl Scheduler {
    pub fn new(settings: &VideoSettings, seed: u64) -> Self {
        Self {
            mode: settings.mode,
            scene_millis: u64::from(settings.scene_seconds) * 1_000,
            shuffle: settings.shuffle,
            repeat_one: settings.repeat_one,
            rng: Rng::new(seed),
            len: 0,
            order: Vec::new(),
            cursor: 0,
            last: None,
            last_repeatable: false,
        }
    }

    /// Sets the number of entries. An unchanged length keeps the position.
    pub fn set_len(&mut self, len: usize) {
        if len == self.len {
            return;
        }
        self.len = len;
        self.order.clear();
        self.cursor = 0;
        self.last = None;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn rebuild_order(&mut self) {
        self.order = (0..self.len).collect();
        if self.shuffle {
            for index in (1..self.order.len()).rev() {
                let other = self.rng.below(index + 1);
                self.order.swap(index, other);
            }
            // Never play the same clip twice in a row across a reshuffle.
            if self.order.len() > 1 && self.order.first().copied() == self.last {
                let last_index = self.order.len() - 1;
                self.order.swap(0, last_index);
            }
        }
        self.cursor = 0;
    }

    /// The entry to open next. Random-scene mode needs the clip's length too: call
    /// [`Scheduler::excerpt`] once it is known.
    pub fn next_entry(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let entry = match self.mode {
            PlaybackMode::RandomScenes => self.pick_random(),
            PlaybackMode::Live | PlaybackMode::Slowed => self.pick_sequential(),
        };
        self.last = Some(entry);
        Some(entry)
    }

    /// The item to play for `entry` whose source lasts `duration_millis` (when known).
    pub fn excerpt(&mut self, entry: usize, duration_millis: Option<u64>) -> PlayItem {
        self.last_repeatable = true;
        match self.mode {
            PlaybackMode::RandomScenes => {
                let room =
                    duration_millis.map_or(0, |total| total.saturating_sub(self.scene_millis));
                // Tenth-of-a-second granularity.
                let start = ((self.rng.unit() * room as f64 / 100.0).floor() as u64) * 100;
                PlayItem {
                    entry,
                    start_millis: start,
                    limit_millis: Some(self.scene_millis),
                }
            }
            PlaybackMode::Live | PlaybackMode::Slowed => PlayItem {
                entry,
                start_millis: 0,
                limit_millis: None,
            },
        }
    }

    /// The clip could not be played: release repeat-one so the next pick moves on.
    pub fn mark_failed(&mut self) {
        self.last_repeatable = false;
    }

    fn pick_sequential(&mut self) -> usize {
        if self.repeat_one && self.last_repeatable {
            if let Some(last) = self.last.filter(|last| *last < self.len) {
                return last;
            }
        }
        if self.cursor >= self.order.len() {
            self.rebuild_order();
        }
        let index = self.order[self.cursor];
        self.cursor += 1;
        index
    }

    fn pick_random(&mut self) -> usize {
        let mut index = self.rng.below(self.len);
        if self.len > 1 && Some(index) == self.last {
            index = (index + 1 + self.rng.below(self.len - 1)) % self.len;
        }
        index
    }
}

/// "01:30" or "1:02:03".
pub fn format_clock(seconds: f64) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: PlaybackMode) -> VideoSettings {
        VideoSettings {
            mode,
            ..VideoSettings::default()
        }
    }

    fn take(scheduler: &mut Scheduler, count: usize) -> Vec<usize> {
        (0..count)
            .map(|_| {
                let entry = scheduler.next_entry().unwrap();
                scheduler.excerpt(entry, Some(100_000));
                entry
            })
            .collect()
    }

    #[test]
    fn an_empty_list_has_nothing_to_play() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::Live), 1);
        assert!(scheduler.is_empty());
        assert_eq!(scheduler.next_entry(), None);
    }

    #[test]
    fn sequential_play_loops_the_list_in_order() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::Live), 1);
        scheduler.set_len(3);
        assert_eq!(take(&mut scheduler, 7), vec![0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn shuffle_is_reproducible_and_never_repeats_across_a_reshuffle() {
        let shuffled = VideoSettings {
            shuffle: true,
            ..VideoSettings::default()
        };
        let mut first = Scheduler::new(&shuffled, 7);
        let mut second = Scheduler::new(&shuffled, 7);
        first.set_len(5);
        second.set_len(5);
        let plays = take(&mut first, 40);
        assert_eq!(plays, take(&mut second, 40));
        assert!(plays.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn repeat_one_keeps_the_first_clip_until_it_fails() {
        let repeat = VideoSettings {
            repeat_one: true,
            ..VideoSettings::default()
        };
        let mut scheduler = Scheduler::new(&repeat, 1);
        scheduler.set_len(3);
        assert_eq!(take(&mut scheduler, 4), vec![0, 0, 0, 0]);
        scheduler.mark_failed();
        assert_eq!(scheduler.next_entry(), Some(1));
    }

    #[test]
    fn random_scenes_pick_a_start_inside_the_clip_and_never_the_same_clip_twice() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::RandomScenes), 3);
        scheduler.set_len(4);
        let mut last = None;
        for _ in 0..30 {
            let entry = scheduler.next_entry().unwrap();
            assert_ne!(Some(entry), last);
            last = Some(entry);
            let item = scheduler.excerpt(entry, Some(300_000));
            assert_eq!(item.limit_millis, Some(20_000));
            assert!(item.start_millis <= 280_000);
            assert_eq!(item.start_millis % 100, 0);
        }
        // A clip shorter than a scene, or of unknown length, starts at the beginning.
        assert_eq!(scheduler.excerpt(0, Some(5_000)).start_millis, 0);
        assert_eq!(scheduler.excerpt(0, None).start_millis, 0);
    }

    #[test]
    fn a_changed_length_restarts_the_order() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::Live), 1);
        scheduler.set_len(3);
        assert_eq!(take(&mut scheduler, 2), vec![0, 1]);
        scheduler.set_len(3);
        assert_eq!(take(&mut scheduler, 1), vec![2]);
        scheduler.set_len(2);
        assert_eq!(take(&mut scheduler, 2), vec![0, 1]);
    }

    #[test]
    fn seed_zero_uses_the_clock_seed_and_a_fixed_seed_wins() {
        assert_eq!(resolve_seed(0, 99), 99);
        assert_eq!(resolve_seed(5, 99), 5);
    }

    #[test]
    fn clock_text_reads_minutes_and_hours() {
        assert_eq!(format_clock(0.0), "00:00");
        assert_eq!(format_clock(90.4), "01:30");
        assert_eq!(format_clock(3723.0), "1:02:03");
        assert_eq!(format_clock(f64::NAN), "00:00");
        assert_eq!(format_clock(-4.0), "00:00");
    }
}
