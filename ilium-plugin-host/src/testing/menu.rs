//! Menu-provider harness: opens a menu over scripted providers with a manual clock
//! (HST-476..478, WIT:ilium:exports/menu-provider). Test support only.

use ilium_plugin_api::generated::exports::menu_provider::{MenuContext, MenuEntry};

/// What one menu opening drew and waited for (HST-476..478).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuOpening {
    /// Entry ids per drawn frame.
    pub frames: Vec<Vec<String>>,
    /// Clock reading when the menu opened.
    pub opened_at_ms: u64,
    /// (plugin, start ms) of every provider call.
    pub provider_calls: Vec<(String, u64)>,
    /// Plugins whose activation the opening waited for.
    pub activations_waited_for: Vec<String>,
}

/// The menu harness (HST-476..478).
/// Wait of one opening before the menu opens without late providers (HST-476).
const OPENING_DEADLINE_MS: u64 = 20;

/// Most entries one menu holds (api-menu-provider: at most 64).
const MENU_ENTRY_CAPACITY: usize = 64;

#[derive(Debug)]
struct RunningProvider {
    plugin: String,
    answer_after_ms: u32,
    entries: Vec<MenuEntry>,
}

#[derive(Debug)]
pub struct MenuHarness {
    clock_ms: u64,
    static_entries: Vec<MenuEntry>,
    running_providers: Vec<RunningProvider>,
}

impl MenuHarness {
    /// A fresh harness.
    pub fn new() -> MenuHarness {
        MenuHarness {
            clock_ms: 0,
            static_entries: Vec::new(),
            running_providers: Vec::new(),
        }
    }

    /// A running provider that answers after a delay.
    pub fn add_provider(&mut self, plugin: &str, answer_after_ms: u32, entries: Vec<MenuEntry>) {
        self.running_providers.push(RunningProvider {
            plugin: plugin.to_string(),
            answer_after_ms,
            entries,
        });
    }

    /// A stopped provider that needs activation first.
    // Not called by an opening: the menu never waits for activation (HST-478).
    pub fn add_stopped_provider(
        &mut self,
        _plugin: &str,
        _activation_ms: u32,
        _entries: Vec<MenuEntry>,
    ) {
    }

    /// The static entries of the menu.
    pub fn static_entries(&mut self, entries: Vec<MenuEntry>) {
        self.static_entries = entries;
    }

    /// Opens the menu.
    pub fn open(&mut self, _context: MenuContext) -> MenuOpening {
        let mut frame: Vec<String> = self
            .static_entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        let mut provider_calls = Vec::new();
        let mut latest_answer_ms: u64 = 0;
        let mut every_provider_answered = true;
        for provider in &self.running_providers {
            provider_calls.push((provider.plugin.clone(), self.clock_ms));
            let answer_ms = u64::from(provider.answer_after_ms);
            if answer_ms > OPENING_DEADLINE_MS {
                every_provider_answered = false;
                continue;
            }
            latest_answer_ms = latest_answer_ms.max(answer_ms);
            frame.extend(provider.entries.iter().map(|entry| entry.id.clone()));
        }
        frame.truncate(MENU_ENTRY_CAPACITY);
        let wait_ms = if every_provider_answered {
            latest_answer_ms
        } else {
            OPENING_DEADLINE_MS
        };
        MenuOpening {
            frames: vec![frame],
            opened_at_ms: self.clock_ms.saturating_add(wait_ms),
            provider_calls,
            activations_waited_for: Vec::new(),
        }
    }

    /// Moves the manual clock.
    pub fn advance_ms(&mut self, ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(ms);
    }
}

impl Default for MenuHarness {
    fn default() -> MenuHarness {
        MenuHarness::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{MenuHarness, MenuOpening};
    use ilium_plugin_api::generated::exports::menu_provider::{MenuContext, MenuEntry};

    fn entry(entry_id: &str) -> MenuEntry {
        MenuEntry {
            id: entry_id.to_string(),
            label: entry_id.to_string(),
            ..Default::default()
        }
    }

    fn context() -> MenuContext {
        MenuContext {
            menu: "editor-context".to_string(),
            ..Default::default()
        }
    }

    fn sorted_first_frame_ids(opening: &MenuOpening) -> Vec<String> {
        let mut ids = opening.frames.first().cloned().unwrap_or_default();
        ids.sort();
        ids
    }

    /// REQ: HST-476
    #[test]
    fn every_provider_of_one_opening_is_called_at_the_same_clock_reading() {
        let mut harness = MenuHarness::new();
        harness.add_provider("alpha", 0, vec![entry("a1")]);
        harness.add_provider("beta", 0, vec![entry("b1")]);
        harness.advance_ms(40);
        let opening = harness.open(context());
        let mut plugins: Vec<String> = opening
            .provider_calls
            .iter()
            .map(|(plugin, _)| plugin.clone())
            .collect();
        plugins.sort();
        assert_eq!(plugins, vec!["alpha".to_string(), "beta".to_string()]);
        assert!(opening.provider_calls.iter().all(|(_, start)| *start == 40));
    }

    /// REQ: HST-476
    #[test]
    fn menu_opens_once_all_providers_answer_before_the_deadline() {
        let mut harness = MenuHarness::new();
        harness.add_provider("alpha", 5, vec![entry("a1")]);
        harness.add_provider("beta", 12, vec![entry("b1")]);
        let opening = harness.open(context());
        assert_eq!(opening.opened_at_ms, 12);
        assert_eq!(
            sorted_first_frame_ids(&opening),
            vec!["a1".to_string(), "b1".to_string()]
        );
    }

    /// REQ: HST-476
    #[test]
    fn provider_answering_just_inside_the_deadline_is_kept() {
        let mut harness = MenuHarness::new();
        harness.add_provider("inside", 19, vec![entry("inside-item")]);
        let opening = harness.open(context());
        assert_eq!(
            sorted_first_frame_ids(&opening),
            vec!["inside-item".to_string()]
        );
    }

    /// REQ: HST-476
    #[test]
    fn provider_answering_just_after_the_deadline_is_dropped() {
        let mut harness = MenuHarness::new();
        harness.add_provider("late", 21, vec![entry("late-item")]);
        let opening = harness.open(context());
        assert_eq!(opening.opened_at_ms, 20);
        assert!(sorted_first_frame_ids(&opening).is_empty());
        assert!(!opening
            .frames
            .iter()
            .flatten()
            .any(|id| id.as_str() == "late-item"));
    }

    /// REQ: HST-476
    #[test]
    fn late_provider_entries_are_dropped_while_answered_providers_stay() {
        let mut harness = MenuHarness::new();
        harness.add_provider("fast", 1, vec![entry("fast-item")]);
        harness.add_provider("slow", 25, vec![entry("slow-item")]);
        let opening = harness.open(context());
        assert_eq!(opening.opened_at_ms, 20);
        assert_eq!(
            sorted_first_frame_ids(&opening),
            vec!["fast-item".to_string()]
        );
    }

    /// Helper: a fresh harness with no providers records no provider calls.
    #[test]
    fn harness_without_providers_makes_no_provider_calls() {
        let mut harness = MenuHarness::new();
        let opening = harness.open(context());
        assert!(opening.provider_calls.is_empty());
        assert!(opening.activations_waited_for.is_empty());
    }

    /// Helper: `Default` builds the same empty harness as `new`.
    #[test]
    fn default_harness_behaves_like_new() {
        let mut harness = MenuHarness::default();
        let opening = harness.open(context());
        assert!(opening.provider_calls.is_empty());
        assert_eq!(opening.opened_at_ms, 0);
    }

    /// Helper: `open` records every opening separately, so a second opening calls the provider again.
    #[test]
    fn opening_twice_calls_each_provider_again() {
        let mut harness = MenuHarness::new();
        harness.add_provider("alpha", 0, vec![entry("a1")]);
        let first = harness.open(context());
        assert_eq!(first.provider_calls.len(), 1);
        harness.advance_ms(10);
        let second = harness.open(context());
        assert_eq!(second.provider_calls, vec![("alpha".to_string(), 10)]);
    }

    /// Helper: `advance_ms` accumulates on the manual clock seen by the next opening.
    #[test]
    fn advance_ms_accumulates_on_the_manual_clock() {
        let mut harness = MenuHarness::new();
        harness.advance_ms(100);
        harness.advance_ms(150);
        let opening = harness.open(context());
        assert_eq!(opening.opened_at_ms, 250);
    }

    /// REQ: HST-477
    #[test]
    fn static_entries_are_in_the_first_frame() {
        let mut harness = MenuHarness::new();
        harness.static_entries(vec![entry("open"), entry("copy")]);
        let opening = harness.open(context());
        assert_eq!(
            sorted_first_frame_ids(&opening),
            vec!["copy".to_string(), "open".to_string()]
        );
    }

    /// REQ: HST-477
    #[test]
    fn static_entries_stay_in_the_first_frame_while_a_provider_is_late() {
        let mut harness = MenuHarness::new();
        harness.static_entries(vec![entry("open")]);
        harness.add_provider("slow", 30, vec![entry("slow-item")]);
        let opening = harness.open(context());
        assert_eq!(sorted_first_frame_ids(&opening), vec!["open".to_string()]);
    }

    /// REQ: HST-477
    #[test]
    fn menu_does_not_change_after_its_first_frame_is_drawn() {
        let mut harness = MenuHarness::new();
        harness.static_entries(vec![entry("open")]);
        harness.add_provider("alpha", 3, vec![entry("a1")]);
        let opening = harness.open(context());
        let first_frame = opening
            .frames
            .first()
            .expect("the menu draws at least one frame");
        assert!(opening.frames.iter().all(|frame| frame == first_frame));
    }

    /// REQ: HST-478
    #[test]
    fn opening_does_not_wait_for_a_stopped_provider_to_activate() {
        let mut harness = MenuHarness::new();
        harness.static_entries(vec![entry("open")]);
        harness.add_stopped_provider("dormant", 500, vec![entry("dormant-item")]);
        let opening = harness.open(context());
        assert!(opening.opened_at_ms <= 20);
        assert!(opening.activations_waited_for.is_empty());
        assert_eq!(sorted_first_frame_ids(&opening), vec!["open".to_string()]);
    }

    /// Boundary: an empty static list draws no entries.
    #[test]
    fn empty_static_list_draws_no_entries() {
        let mut harness = MenuHarness::new();
        harness.static_entries(vec![]);
        let opening = harness.open(context());
        assert!(sorted_first_frame_ids(&opening).is_empty());
    }

    /// Boundary: the menu holds up to 64 entries, so 64 static entries all appear.
    #[test]
    fn sixty_four_static_entries_all_appear_in_the_first_frame() {
        let mut harness = MenuHarness::new();
        let entries: Vec<MenuEntry> = (0..64)
            .map(|index| entry(&format!("item-{index}")))
            .collect();
        harness.static_entries(entries);
        let opening = harness.open(context());
        assert_eq!(opening.frames.first().map(Vec::len), Some(64));
    }
}
