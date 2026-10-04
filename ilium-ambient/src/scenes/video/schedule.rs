//! What to play next, and the timing arithmetic for frames and clocks.
//! Pure logic: no I/O, no clock, deterministic for a given seed.

use super::discover::MediaInput;
use super::settings::{PlaybackMode, VideoSettings};
use std::time::Duration;

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

/// Seed for a scene: the user's fixed seed, or a fresh random one for 0.
pub fn resolve_seed(setting: u32) -> u64 {
    if setting != 0 {
        return u64::from(setting);
    }
    use std::hash::{BuildHasher, Hasher};
    // RandomState is seeded from the operating system.
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

/// One clip to play.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayItem {
    pub input: MediaInput,
    pub start_seconds: f64,
    pub limit_seconds: Option<f64>,
    /// Total length of the source when known, for the status line.
    pub duration_seconds: Option<f64>,
}

type PrepareClip<'a> = dyn FnMut(&MediaInput, bool) -> Result<Option<f64>, String> + 'a;

pub struct Scheduler {
    mode: PlaybackMode,
    scene_seconds: f64,
    shuffle: bool,
    repeat_one: bool,
    rng: Rng,
    entries: Vec<MediaInput>,
    order: Vec<usize>,
    cursor: usize,
    last: Option<MediaInput>,
    last_repeatable: bool,
}

impl Scheduler {
    pub fn new(settings: &VideoSettings, seed: u64) -> Self {
        Self {
            mode: settings.mode,
            scene_seconds: f64::from(settings.scene_seconds),
            shuffle: settings.shuffle,
            repeat_one: settings.repeat_one,
            rng: Rng::new(seed),
            entries: Vec::new(),
            order: Vec::new(),
            cursor: 0,
            last: None,
            last_repeatable: false,
        }
    }

    /// Replace the playlist. An unchanged list keeps its position.
    pub fn set_entries(&mut self, entries: Vec<MediaInput>) {
        if entries == self.entries {
            return;
        }
        self.entries = entries;
        self.order.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn rebuild_order(&mut self) {
        self.order = (0..self.entries.len()).collect();
        if self.shuffle {
            for index in (1..self.order.len()).rev() {
                let other = self.rng.below(index + 1);
                self.order.swap(index, other);
            }
            // Never play the same file twice in a row across a reshuffle.
            let repeats_last = self
                .order
                .first()
                .zip(self.last.as_ref())
                .is_some_and(|(first, last)| &self.entries[*first] == last);
            if repeats_last && self.order.len() > 1 {
                let last_index = self.order.len() - 1;
                self.order.swap(0, last_index);
            }
        }
        self.cursor = 0;
    }

    /// Pick the next clip. `probe` returns a file's duration in seconds; it is
    /// called for local files always and for URLs only in random-scene mode.
    #[cfg(test)]
    pub fn next(&mut self, probe: &mut dyn FnMut(&MediaInput) -> Option<f64>) -> Option<PlayItem> {
        self.next_prepared(
            &mut |input, should_probe| Ok(should_probe.then(|| probe(input)).flatten()),
            false,
        )
        .ok()
        .flatten()
    }

    /// Preparation runs before probing, including remote random-scene seeks.
    pub fn next_prepared(
        &mut self,
        prepare: &mut PrepareClip<'_>,
        probe_remote: bool,
    ) -> Result<Option<PlayItem>, String> {
        if self.entries.is_empty() {
            return Ok(None);
        }
        let input = match self.mode {
            PlaybackMode::RandomScenes => self.pick_random(),
            PlaybackMode::Live | PlaybackMode::Slowed => self.pick_sequential(),
        };
        self.last = Some(input.clone());
        let should_probe =
            probe_remote || !input.is_url() || self.mode == PlaybackMode::RandomScenes;
        let duration = match prepare(&input, should_probe) {
            Ok(duration) => {
                self.last_repeatable = true;
                duration
            }
            Err(error) => {
                self.mark_failed();
                return Err(error);
            }
        };
        Ok(Some(match self.mode {
            PlaybackMode::RandomScenes => self.random_scene(input, duration),
            PlaybackMode::Live | PlaybackMode::Slowed => PlayItem {
                input,
                start_seconds: 0.0,
                limit_seconds: None,
                duration_seconds: duration,
            },
        }))
    }

    fn pick_sequential(&mut self) -> MediaInput {
        if self.repeat_one && self.last_repeatable {
            if let Some(last) = self
                .last
                .as_ref()
                .filter(|last| self.entries.contains(last))
            {
                return last.clone();
            }
        }
        if self.cursor >= self.order.len() {
            self.rebuild_order();
        }
        let index = self.order[self.cursor];
        self.cursor += 1;
        self.entries[index].clone()
    }

    fn pick_random(&mut self) -> MediaInput {
        let mut index = self.rng.below(self.entries.len());
        if self.entries.len() > 1 && Some(&self.entries[index]) == self.last.as_ref() {
            index = (index + 1 + self.rng.below(self.entries.len() - 1)) % self.entries.len();
        }
        self.entries[index].clone()
    }

    /// Preserve the last identity for shuffle avoidance, but release repeat-one.
    pub fn mark_failed(&mut self) {
        self.last_repeatable = false;
    }

    fn random_scene(&mut self, input: MediaInput, duration: Option<f64>) -> PlayItem {
        let room = duration.map_or(0.0, |total| (total - self.scene_seconds).max(0.0));
        // Tenth-of-a-second granularity keeps command lines readable.
        let start = (self.rng.unit() * room * 10.0).floor() / 10.0;
        PlayItem {
            input,
            start_seconds: start,
            limit_seconds: Some(self.scene_seconds),
            duration_seconds: duration,
        }
    }
}

/// Presentation time of the `index`-th output frame.
pub fn frame_time(index: u64, frames_per_second: u32) -> Duration {
    Duration::from_secs_f64(index as f64 / f64::from(frames_per_second.max(1)))
}

/// Position inside the source after `output` time of playback at `ratio`
/// (source seconds per output second) starting at `start_seconds`.
pub fn source_position(start_seconds: f64, output: Duration, ratio: f64) -> f64 {
    start_seconds + output.as_secs_f64() * ratio
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
    use std::path::PathBuf;

    fn files(names: &[&str]) -> Vec<MediaInput> {
        names
            .iter()
            .map(|name| MediaInput::File(PathBuf::from(name)))
            .collect()
    }

    fn settings(mode: PlaybackMode) -> VideoSettings {
        VideoSettings {
            mode,
            ..VideoSettings::default()
        }
    }

    fn take(scheduler: &mut Scheduler, count: usize) -> Vec<PlayItem> {
        (0..count)
            .filter_map(|_| scheduler.next(&mut |_| Some(100.0)))
            .collect()
    }

    fn name(item: &PlayItem) -> String {
        item.input.display_name()
    }

    #[test]
    fn rng_is_deterministic_and_spread_out() {
        let mut first = Rng::new(7);
        let mut second = Rng::new(7);
        let a: Vec<u64> = (0..5).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..5).map(|_| second.next_u64()).collect();
        assert_eq!(a, b);
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            assert!(rng.below(3) < 3);
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
        }
        assert_eq!(rng.below(0), 0);
        assert_eq!(resolve_seed(42), 42);
    }

    #[test]
    fn live_mode_plays_in_order_and_wraps_around() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::Live), 1);
        scheduler.set_entries(files(&["a", "b", "c"]));
        let played: Vec<String> = take(&mut scheduler, 7).iter().map(name).collect();
        assert_eq!(played, ["a", "b", "c", "a", "b", "c", "a"]);
        let item = scheduler.next(&mut |_| Some(61.0)).unwrap();
        assert_eq!(item.start_seconds, 0.0);
        assert_eq!(item.limit_seconds, None);
        assert_eq!(item.duration_seconds, Some(61.0));
    }

    #[test]
    fn shuffle_visits_everything_once_per_cycle_and_is_seeded() {
        let mut config = settings(PlaybackMode::Live);
        config.shuffle = true;
        let run = |seed| {
            let mut scheduler = Scheduler::new(&config, seed);
            scheduler.set_entries(files(&["a", "b", "c", "d", "e"]));
            take(&mut scheduler, 10)
                .iter()
                .map(name)
                .collect::<Vec<_>>()
        };
        let first = run(5);
        assert_eq!(first, run(5));
        for cycle in first.chunks(5) {
            let mut sorted = cycle.to_vec();
            sorted.sort();
            assert_eq!(sorted, ["a", "b", "c", "d", "e"]);
        }
        assert!(first.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(
            (1..30).any(|seed| run(seed) != first),
            "different seeds differ"
        );
    }

    #[test]
    fn repeat_one_keeps_the_first_file() {
        let mut config = settings(PlaybackMode::Live);
        config.repeat_one = true;
        let mut scheduler = Scheduler::new(&config, 1);
        scheduler.set_entries(files(&["a", "b"]));
        let played: Vec<String> = take(&mut scheduler, 4).iter().map(name).collect();
        assert_eq!(played, ["a", "a", "a", "a"]);
        scheduler.set_entries(files(&["z", "y"]));
        assert_eq!(name(&scheduler.next(&mut |_| None).unwrap()), "z");
    }

    #[test]
    fn failed_preparation_advances_even_when_repeat_one_is_enabled() {
        let mut config = settings(PlaybackMode::Live);
        config.repeat_one = true;
        let mut scheduler = Scheduler::new(&config, 1);
        scheduler.set_entries(files(&["a", "b"]));
        assert!(scheduler
            .next_prepared(&mut |_, _| Err("source refused".into()), true)
            .is_err());
        let item = scheduler
            .next_prepared(&mut |_, _| Ok(Some(20.0)), true)
            .unwrap()
            .unwrap();
        assert_eq!(
            name(&item),
            "b",
            "a failed source cannot trap repeat-one forever"
        );
    }

    #[test]
    fn empty_playlist_yields_nothing() {
        let mut scheduler = Scheduler::new(&settings(PlaybackMode::Live), 1);
        assert!(scheduler.is_empty());
        assert!(scheduler.next(&mut |_| Some(1.0)).is_none());
    }

    #[test]
    fn random_scenes_pick_start_inside_the_file_and_are_seeded() {
        let mut config = settings(PlaybackMode::RandomScenes);
        config.scene_seconds = 20;
        let run = |seed| {
            let mut scheduler = Scheduler::new(&config, seed);
            scheduler.set_entries(files(&["a", "b", "c"]));
            take(&mut scheduler, 30)
        };
        let items = run(9);
        assert_eq!(items, run(9));
        for item in &items {
            assert_eq!(item.limit_seconds, Some(20.0));
            assert!(item.start_seconds >= 0.0 && item.start_seconds <= 80.0);
        }
        assert!(items.windows(2).all(|pair| pair[0].input != pair[1].input));
        let starts: std::collections::BTreeSet<u64> = items
            .iter()
            .map(|item| (item.start_seconds * 10.0) as u64)
            .collect();
        assert!(starts.len() > 10, "start times vary: {starts:?}");
        let names: std::collections::BTreeSet<String> = items.iter().map(name).collect();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn random_scene_in_a_short_or_unknown_file_starts_at_zero() {
        let mut config = settings(PlaybackMode::RandomScenes);
        config.scene_seconds = 30;
        let mut scheduler = Scheduler::new(&config, 3);
        scheduler.set_entries(files(&["short"]));
        let short = scheduler.next(&mut |_| Some(10.0)).unwrap();
        assert_eq!(short.start_seconds, 0.0);
        let unknown = scheduler.next(&mut |_| None).unwrap();
        assert_eq!(unknown.start_seconds, 0.0);
        assert_eq!(unknown.limit_seconds, Some(30.0));
    }

    #[test]
    fn urls_are_only_probed_in_random_mode() {
        let url = vec![MediaInput::Url("https://x.example/v.mp4".to_owned())];
        let mut probed = 0;
        let mut live = Scheduler::new(&settings(PlaybackMode::Live), 1);
        live.set_entries(url.clone());
        live.next(&mut |_| {
            probed += 1;
            Some(1.0)
        });
        assert_eq!(probed, 0);
        let mut random = Scheduler::new(&settings(PlaybackMode::RandomScenes), 1);
        random.set_entries(url);
        random.next(&mut |_| {
            probed += 1;
            Some(1.0)
        });
        assert_eq!(probed, 1);
    }

    #[test]
    fn timing_math() {
        assert_eq!(frame_time(0, 12), Duration::ZERO);
        assert_eq!(frame_time(12, 12), Duration::from_secs(1));
        assert!((frame_time(1, 24).as_secs_f64() - 1.0 / 24.0).abs() < 1e-9);
        // 10 s of output at 25 % speed covers 2.5 s of source.
        let position = source_position(100.0, Duration::from_secs(10), 0.25);
        assert!((position - 102.5).abs() < 1e-9);
        assert_eq!(format_clock(0.0), "00:00");
        assert_eq!(format_clock(72.9), "01:12");
        assert_eq!(format_clock(3723.0), "1:02:03");
        assert_eq!(format_clock(f64::NAN), "00:00");
    }
}
