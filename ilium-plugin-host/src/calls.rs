//! Guest call classes, deadlines and traps (reference-runtime-model.md, reference-errors-and-limits.md).

use std::collections::BTreeMap;
use std::time::Duration;

use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::Code;

/// The class of a call into a guest; each has its own deadline (reference-runtime-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallClass {
    /// `on-events`.
    Event,
    /// A command handler.
    Command,
    /// `render`.
    Render,
    /// An annotator.
    Annotate,
    /// An animation frame.
    Frame,
    /// A detector `identify` or `classify`.
    Detector,
    /// `activate`.
    Activate,
    /// `deactivate`.
    Deactivate,
    /// A data migration.
    Migrate,
    /// A job run.
    Job,
    /// `menu-items` (reference-host-scheduling.md HST-188).
    MenuItems,
    /// A catalog query (reference-host-scheduling.md HST-188).
    QueryCatalog,
    /// `list-virtual-children` (reference-host-scheduling.md HST-188).
    ListVirtualChildren,
    /// A tokenizer call (reference-host-scheduling.md HST-188).
    Tokens,
    /// A settings validation (reference-host-scheduling.md HST-188).
    SettingsValidate,
}

/// The limits of one plugin after clamping to its tier (reference-manifest.md PKG-031).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLimits(pub BTreeMap<LimitKey, u64>);

/// A guest call that ended abnormally (reference-errors-and-limits.md `trap.*`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("trap {}: {interface}.{function}", .code.0)]
pub struct Trap {
    /// The `trap.*` code.
    pub code: Code,
    /// The interface being called.
    pub interface: String,
    /// The function being called.
    pub function: String,
}

/// The deadline of a call class under the resolved limits (reference-runtime-model.md).
pub fn deadline_for(_class: CallClass, _limits: &ResolvedLimits) -> Duration {
    unimplemented!("host::calls::deadline_for")
}

/// Grace after a soft deadline before a late call counts as a trap (reference-host-scheduling.md HST-189).
pub const SOFT_GRACE_MS: u64 = 100;

/// The verdict on one call's elapsed time against its class deadline (reference-host-scheduling.md HST-189).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreachVerdict {
    /// Within the deadline.
    Timely,
    /// Past a hard deadline: the instance traps.
    HardTrap,
    /// Past a soft deadline but within the grace.
    SoftLate,
    /// Past the soft deadline and its grace.
    SoftTrap,
}

/// Classifies an elapsed time against the class deadline (reference-host-scheduling.md HST-189).
pub fn evaluate_breach(
    _class: CallClass,
    _elapsed: Duration,
    _limits: &ResolvedLimits,
) -> BreachVerdict {
    unimplemented!("host::calls::evaluate_breach")
}

/// The call class of an event batch: `Command` when a command invocation is present (HST-192).
pub fn batch_call_class(_batch: &[ilium_plugin_api::generated::types::events::Event]) -> CallClass {
    unimplemented!("host::calls::batch_call_class")
}

/// How many late calls in a row trap the instance, if the class has a streak limit (HST-190).
pub fn streak_limit(_class: CallClass) -> Option<u32> {
    unimplemented!("host::calls::streak_limit")
}

/// Counts consecutive late calls of one class (reference-host-scheduling.md HST-190).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreakCounter {
    class: CallClass,
    late_in_a_row: u32,
}

impl StreakCounter {
    /// A counter for one class.
    pub fn new(_class: CallClass) -> StreakCounter {
        unimplemented!("host::calls::StreakCounter::new")
    }

    /// Records one call; returns true when the streak limit is reached (trap the instance).
    pub fn record(&mut self, _timely: bool) -> bool {
        unimplemented!("host::calls::StreakCounter::record")
    }
}

/// Measures one guest call: guest time counts, import time and time after return do not
/// (reference-host-scheduling.md HST-193..196).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallBudget {
    class: CallClass,
    deadline: Duration,
    used: Duration,
    returned: bool,
}

impl CallBudget {
    /// A budget for one call of `class` under `limits`.
    pub fn new(_class: CallClass, _limits: &ResolvedLimits) -> CallBudget {
        unimplemented!("host::calls::CallBudget::new")
    }

    /// Guest code ran for `ms`.
    pub fn guest_ms(&mut self, _ms: u64) {
        unimplemented!("host::calls::CallBudget::guest_ms")
    }

    /// A host import ran for `ms` (not charged).
    pub fn import_ms(&mut self, _ms: u64) {
        unimplemented!("host::calls::CallBudget::import_ms")
    }

    /// The guest returned.
    pub fn returned(&mut self) {
        unimplemented!("host::calls::CallBudget::returned")
    }

    /// Time passed after the return (not charged).
    pub fn post_return_ms(&mut self, _ms: u64) {
        unimplemented!("host::calls::CallBudget::post_return_ms")
    }

    /// The charged time.
    pub fn used(&self) -> Duration {
        unimplemented!("host::calls::CallBudget::used")
    }

    /// The verdict for the charged time.
    pub fn verdict(&self) -> BreachVerdict {
        unimplemented!("host::calls::CallBudget::verdict")
    }

    /// A gap between scheduler ticks of `ms` (a suspended machine is not charged, HST-196).
    pub fn tick_gap(&mut self, _ms: u64) {
        unimplemented!("host::calls::CallBudget::tick_gap")
    }
}

/// The number of engine epoch ticks a deadline of `ms` milliseconds spans (reference-host-runtime.md).
pub fn deadline_ticks(_ms: u64) -> u64 {
    unimplemented!("host::calls::deadline_ticks")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use ilium_plugin_api::generated::types::events::{
        CommandContext, CommandInvokedEvent, CommandOrigin, Event,
    };
    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::Code;

    use super::{
        batch_call_class, deadline_for, deadline_ticks, evaluate_breach, streak_limit,
        BreachVerdict, CallBudget, CallClass, ResolvedLimits, StreakCounter, Trap, SOFT_GRACE_MS,
    };

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    /// The deadline keys with the defaults of the call-class table (reference-host-scheduling.md).
    fn default_limits() -> ResolvedLimits {
        let mut entries = BTreeMap::new();
        entries.insert(LimitKey::EventDeadlineMs, 20);
        entries.insert(LimitKey::CommandDeadlineMs, 50);
        entries.insert(LimitKey::RenderDeadlineMs, 10);
        entries.insert(LimitKey::AnnotateDeadlineMs, 10);
        entries.insert(LimitKey::FrameDeadlineMs, 50);
        entries.insert(LimitKey::DetectorDeadlineMs, 20);
        entries.insert(LimitKey::ActivateDeadlineMs, 2_000);
        entries.insert(LimitKey::DeactivateDeadlineMs, 500);
        entries.insert(LimitKey::MigrateDeadlineMs, 30_000);
        ResolvedLimits(entries)
    }

    fn limits_with(entries: &[(LimitKey, u64)]) -> ResolvedLimits {
        ResolvedLimits(entries.iter().cloned().collect())
    }

    fn no_limits() -> ResolvedLimits {
        ResolvedLimits(BTreeMap::new())
    }

    fn command_invoked_event() -> Event {
        Event::CommandInvoked(CommandInvokedEvent {
            command: "demo.run".to_owned(),
            arguments_json: "{}".to_owned(),
            origin: CommandOrigin::Palette,
            context: CommandContext::default(),
        })
    }

    // ---- HST-188: call-class defaults -----------------------------------------------------

    /// REQ: HST-188
    #[test]
    fn hst_188_event_batch_defaults_to_20_ms() {
        assert_eq!(deadline_for(CallClass::Event, &default_limits()), ms(20));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_command_batch_defaults_to_50_ms() {
        assert_eq!(deadline_for(CallClass::Command, &default_limits()), ms(50));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_render_and_annotate_default_to_10_ms() {
        assert_eq!(deadline_for(CallClass::Render, &default_limits()), ms(10));
        assert_eq!(deadline_for(CallClass::Annotate, &default_limits()), ms(10));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_activate_and_deactivate_default_to_2000_and_500_ms() {
        assert_eq!(deadline_for(CallClass::Activate, &default_limits()), ms(2_000));
        assert_eq!(deadline_for(CallClass::Deactivate, &default_limits()), ms(500));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_detector_frame_and_migrate_defaults() {
        assert_eq!(deadline_for(CallClass::Detector, &default_limits()), ms(20));
        assert_eq!(deadline_for(CallClass::Frame, &default_limits()), ms(50));
        assert_eq!(deadline_for(CallClass::Migrate, &default_limits()), ms(30_000));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_fixed_provider_classes_have_their_table_defaults() {
        assert_eq!(deadline_for(CallClass::MenuItems, &no_limits()), ms(20));
        assert_eq!(deadline_for(CallClass::QueryCatalog, &no_limits()), ms(20));
        assert_eq!(deadline_for(CallClass::ListVirtualChildren, &no_limits()), ms(20));
        assert_eq!(deadline_for(CallClass::Tokens, &no_limits()), ms(10));
        assert_eq!(deadline_for(CallClass::SettingsValidate, &no_limits()), ms(100));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_missing_limit_key_falls_back_to_the_default() {
        assert_eq!(deadline_for(CallClass::Event, &no_limits()), ms(20));
        assert_eq!(deadline_for(CallClass::Activate, &no_limits()), ms(2_000));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_effective_limit_below_default_is_used() {
        let limits = limits_with(&[(LimitKey::EventDeadlineMs, 5)]);
        assert_eq!(deadline_for(CallClass::Event, &limits), ms(5));
    }

    /// REQ: HST-188
    #[test]
    fn hst_188_each_limit_key_feeds_only_its_own_class() {
        let limits = limits_with(&[(LimitKey::RenderDeadlineMs, 15)]);
        assert_eq!(deadline_for(CallClass::Render, &limits), ms(15));
        assert_eq!(deadline_for(CallClass::Event, &limits), ms(20));
    }

    // ---- HST-192: batch classification ----------------------------------------------------

    /// REQ: HST-192
    #[test]
    fn hst_192_empty_batch_is_an_event_batch() {
        assert_eq!(batch_call_class(&[]), CallClass::Event);
    }

    /// REQ: HST-192
    #[test]
    fn hst_192_batch_without_command_is_an_event_batch() {
        assert_eq!(batch_call_class(&[Event::ThemeChanged]), CallClass::Event);
        assert_eq!(
            batch_call_class(&[Event::KeymapChanged, Event::ThemeChanged]),
            CallClass::Event
        );
    }

    /// REQ: HST-192
    #[test]
    fn hst_192_batch_with_a_command_invoked_event_is_a_command_batch() {
        assert_eq!(batch_call_class(&[command_invoked_event()]), CallClass::Command);
    }

    /// REQ: HST-192
    #[test]
    fn hst_192_command_anywhere_in_a_mixed_batch_selects_command_class() {
        let batch = [
            Event::ThemeChanged,
            command_invoked_event(),
            Event::KeymapChanged,
        ];
        assert_eq!(batch_call_class(&batch), CallClass::Command);
        assert_eq!(
            batch_call_class(&[command_invoked_event(), command_invoked_event()]),
            CallClass::Command
        );
    }

    // ---- HST-189: breach verdicts ---------------------------------------------------------

    /// REQ: HST-189
    #[test]
    fn hst_189_soft_grace_is_100_ms() {
        assert_eq!(SOFT_GRACE_MS, 100);
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_hard_call_within_deadline_is_timely() {
        assert_eq!(
            evaluate_breach(CallClass::Event, ms(19), &default_limits()),
            BreachVerdict::Timely
        );
        assert_eq!(
            evaluate_breach(CallClass::Render, ms(9), &default_limits()),
            BreachVerdict::Timely
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_hard_call_past_deadline_traps() {
        assert_eq!(
            evaluate_breach(CallClass::Event, ms(21), &default_limits()),
            BreachVerdict::HardTrap
        );
        assert_eq!(
            evaluate_breach(CallClass::Render, ms(11), &default_limits()),
            BreachVerdict::HardTrap
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_every_hard_class_traps_past_its_deadline() {
        let limits = default_limits();
        let hard_classes = [
            CallClass::Event,
            CallClass::Command,
            CallClass::Render,
            CallClass::Activate,
            CallClass::Deactivate,
            CallClass::Migrate,
        ];
        for class in hard_classes {
            let deadline = deadline_for(class, &limits);
            let past = deadline + ms(1);
            assert_eq!(evaluate_breach(class, past, &limits), BreachVerdict::HardTrap, "{class:?}");
        }
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_zero_elapsed_is_timely_for_hard_and_soft_classes() {
        assert_eq!(
            evaluate_breach(CallClass::Event, Duration::ZERO, &default_limits()),
            BreachVerdict::Timely
        );
        assert_eq!(
            evaluate_breach(CallClass::Annotate, Duration::ZERO, &default_limits()),
            BreachVerdict::Timely
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_soft_call_within_deadline_is_timely() {
        assert_eq!(
            evaluate_breach(CallClass::Annotate, ms(9), &default_limits()),
            BreachVerdict::Timely
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_soft_call_just_past_deadline_is_late_not_trapped() {
        assert_eq!(
            evaluate_breach(CallClass::Annotate, ms(11), &default_limits()),
            BreachVerdict::SoftLate
        );
        assert_eq!(
            evaluate_breach(CallClass::Annotate, ms(109), &default_limits()),
            BreachVerdict::SoftLate
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_soft_call_past_deadline_plus_grace_is_trapped() {
        assert_eq!(
            evaluate_breach(CallClass::Annotate, ms(111), &default_limits()),
            BreachVerdict::SoftTrap
        );
        assert_eq!(
            evaluate_breach(CallClass::Detector, ms(121), &default_limits()),
            BreachVerdict::SoftTrap
        );
    }

    /// REQ: HST-189
    #[test]
    fn hst_189_settings_validate_uses_its_fixed_100_ms_deadline() {
        assert_eq!(
            evaluate_breach(CallClass::SettingsValidate, ms(99), &no_limits()),
            BreachVerdict::Timely
        );
        assert_eq!(
            evaluate_breach(CallClass::SettingsValidate, ms(150), &no_limits()),
            BreachVerdict::SoftLate
        );
        assert_eq!(
            evaluate_breach(CallClass::SettingsValidate, ms(201), &no_limits()),
            BreachVerdict::SoftTrap
        );
    }

    // ---- HST-190: streak limits -----------------------------------------------------------

    /// REQ: HST-190
    #[test]
    fn hst_190_streak_limit_is_3_for_the_soft_provider_and_detector_classes() {
        let three = [
            CallClass::Annotate,
            CallClass::Detector,
            CallClass::MenuItems,
            CallClass::QueryCatalog,
            CallClass::ListVirtualChildren,
            CallClass::Tokens,
            CallClass::SettingsValidate,
        ];
        for class in three {
            assert_eq!(streak_limit(class), Some(3), "{class:?}");
        }
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_frame_streak_limit_is_30() {
        assert_eq!(streak_limit(CallClass::Frame), Some(30));
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_classes_without_a_streak_have_none() {
        let none = [
            CallClass::Event,
            CallClass::Command,
            CallClass::Render,
            CallClass::Activate,
            CallClass::Deactivate,
            CallClass::Migrate,
            CallClass::Job,
        ];
        for class in none {
            assert_eq!(streak_limit(class), None, "{class:?}");
        }
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_third_late_call_in_a_row_traps_annotate() {
        let mut streak = StreakCounter::new(CallClass::Annotate);
        assert!(!streak.record(false));
        assert!(!streak.record(false));
        assert!(streak.record(false));
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_timely_call_resets_the_streak() {
        let mut streak = StreakCounter::new(CallClass::Tokens);
        assert!(!streak.record(false));
        assert!(!streak.record(false));
        assert!(!streak.record(true));
        assert!(!streak.record(false));
        assert!(!streak.record(false));
        assert!(streak.record(false));
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_frame_streak_traps_on_the_thirtieth_late_frame() {
        let mut streak = StreakCounter::new(CallClass::Frame);
        for _ in 0..29 {
            assert!(!streak.record(false));
        }
        assert!(streak.record(false));
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_timely_frame_resets_a_run_of_29_late_frames() {
        let mut streak = StreakCounter::new(CallClass::Frame);
        for _ in 0..29 {
            assert!(!streak.record(false));
        }
        assert!(!streak.record(true));
        for _ in 0..29 {
            assert!(!streak.record(false));
        }
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_timely_only_history_never_traps() {
        let mut streak = StreakCounter::new(CallClass::Detector);
        for _ in 0..100 {
            assert!(!streak.record(true));
        }
    }

    /// REQ: HST-190
    #[test]
    fn hst_190_class_without_streak_never_reports_a_streak_trap() {
        let mut streak = StreakCounter::new(CallClass::Event);
        for _ in 0..100 {
            assert!(!streak.record(false));
        }
    }

    // ---- HST-194 / HST-195: charged time --------------------------------------------------

    /// REQ: HST-194
    #[test]
    fn hst_194_fresh_budget_has_used_nothing_and_is_timely() {
        let budget = CallBudget::new(CallClass::Event, &default_limits());
        assert_eq!(budget.used(), Duration::ZERO);
        assert_eq!(budget.verdict(), BreachVerdict::Timely);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_guest_time_accumulates() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.guest_ms(8);
        budget.guest_ms(7);
        assert_eq!(budget.used(), ms(15));
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_zero_guest_time_charges_nothing() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.guest_ms(0);
        assert_eq!(budget.used(), Duration::ZERO);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_five_synchronous_imports_of_5_ms_use_25_ms() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        for _ in 0..5 {
            budget.import_ms(5);
        }
        budget.returned();
        assert_eq!(budget.used(), ms(25));
        assert_eq!(budget.verdict(), BreachVerdict::HardTrap);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_import_time_and_guest_time_add_up() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.guest_ms(8);
        budget.import_ms(8);
        budget.returned();
        assert_eq!(budget.used(), ms(16));
        assert_eq!(budget.verdict(), BreachVerdict::Timely);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_time_after_return_is_not_charged() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.guest_ms(5);
        budget.returned();
        budget.post_return_ms(10_000);
        assert_eq!(budget.used(), ms(5));
        assert_eq!(budget.verdict(), BreachVerdict::Timely);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_soft_budget_past_deadline_is_late_and_past_grace_is_trapped() {
        let mut late = CallBudget::new(CallClass::Annotate, &default_limits());
        late.guest_ms(11);
        assert_eq!(late.verdict(), BreachVerdict::SoftLate);

        let mut trapped = CallBudget::new(CallClass::Annotate, &default_limits());
        trapped.guest_ms(111);
        assert_eq!(trapped.verdict(), BreachVerdict::SoftTrap);
    }

    /// REQ: HST-194
    #[test]
    fn hst_194_budget_uses_the_effective_deadline_from_limits() {
        let limits = limits_with(&[(LimitKey::EventDeadlineMs, 50)]);
        let mut budget = CallBudget::new(CallClass::Event, &limits);
        budget.guest_ms(40);
        assert_eq!(budget.verdict(), BreachVerdict::Timely);
        budget.guest_ms(11);
        assert_eq!(budget.verdict(), BreachVerdict::HardTrap);
    }

    /// REQ: HST-195
    #[test]
    fn hst_195_tick_gap_over_one_second_is_ignored() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.guest_ms(5);
        budget.tick_gap(1_500);
        budget.returned();
        assert_eq!(budget.used(), ms(5));
        assert_eq!(budget.verdict(), BreachVerdict::Timely);
    }

    /// REQ: HST-195
    #[test]
    fn hst_195_tick_gap_of_exactly_one_second_is_charged() {
        let mut budget = CallBudget::new(CallClass::Event, &default_limits());
        budget.tick_gap(1_000);
        budget.returned();
        assert_eq!(budget.used(), ms(1_000));
        assert_eq!(budget.verdict(), BreachVerdict::HardTrap);
    }

    // ---- HST-081: epoch tick window -------------------------------------------------------

    /// REQ: HST-081
    #[test]
    fn hst_081_zero_deadline_is_one_tick() {
        assert_eq!(deadline_ticks(0), 1);
    }

    /// REQ: HST-081
    #[test]
    fn hst_081_ticks_round_up_to_the_next_10_ms_plus_one() {
        assert_eq!(deadline_ticks(1), 2);
        assert_eq!(deadline_ticks(10), 2);
        assert_eq!(deadline_ticks(11), 3);
        assert_eq!(deadline_ticks(20), 3);
    }

    /// REQ: HST-081
    #[test]
    fn hst_081_default_activate_deadline_spans_201_ticks() {
        assert_eq!(deadline_ticks(2_000), 201);
    }

    // ---- HST-083: trap value ---------------------------------------------------------------

    /// REQ: HST-083
    #[test]
    fn hst_083_deadline_trap_carries_trap_deadline_code_and_names_the_call() {
        let trap = Trap {
            code: Code::from_static("trap.deadline"),
            interface: "ilium:plugin/lifecycle".to_owned(),
            function: "activate".to_owned(),
        };
        assert_eq!(trap.code.as_str(), "trap.deadline");
        assert_eq!(
            trap.to_string(),
            "trap trap.deadline: ilium:plugin/lifecycle.activate"
        );
    }

    /// REQ: HST-083
    #[test]
    fn hst_083_traps_with_different_functions_are_not_equal() {
        let activate = Trap {
            code: Code::from_static("trap.deadline"),
            interface: "ilium:plugin/lifecycle".to_owned(),
            function: "activate".to_owned(),
        };
        let mut deactivate = activate.clone();
        assert_eq!(activate, activate.clone());
        deactivate.function = "deactivate".to_owned();
        assert_ne!(activate, deactivate);
    }

    /// REQ: HST-083
    #[test]
    fn hst_083_trap_with_empty_names_still_formats() {
        let trap = Trap {
            code: Code::from_static("trap.deadline"),
            interface: String::new(),
            function: String::new(),
        };
        assert_eq!(trap.to_string(), "trap trap.deadline: .");
    }
}
