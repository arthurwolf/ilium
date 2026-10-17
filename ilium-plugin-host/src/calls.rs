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
pub fn deadline_for(class: CallClass, limits: &ResolvedLimits) -> Duration {
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
pub fn evaluate_breach(class: CallClass, elapsed: Duration, limits: &ResolvedLimits) -> BreachVerdict {
    unimplemented!("host::calls::evaluate_breach")
}

/// The call class of an event batch: `Command` when a command invocation is present (HST-192).
pub fn batch_call_class(batch: &[ilium_plugin_api::generated::types::events::Event]) -> CallClass {
    unimplemented!("host::calls::batch_call_class")
}

/// How many late calls in a row trap the instance, if the class has a streak limit (HST-190).
pub fn streak_limit(class: CallClass) -> Option<u32> {
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
    pub fn new(class: CallClass) -> StreakCounter {
        unimplemented!("host::calls::StreakCounter::new")
    }

    /// Records one call; returns true when the streak limit is reached (trap the instance).
    pub fn record(&mut self, timely: bool) -> bool {
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
    pub fn new(class: CallClass, limits: &ResolvedLimits) -> CallBudget {
        unimplemented!("host::calls::CallBudget::new")
    }

    /// Guest code ran for `ms`.
    pub fn guest_ms(&mut self, ms: u64) {
        unimplemented!("host::calls::CallBudget::guest_ms")
    }

    /// A host import ran for `ms` (not charged).
    pub fn import_ms(&mut self, ms: u64) {
        unimplemented!("host::calls::CallBudget::import_ms")
    }

    /// The guest returned.
    pub fn returned(&mut self) {
        unimplemented!("host::calls::CallBudget::returned")
    }

    /// Time passed after the return (not charged).
    pub fn post_return_ms(&mut self, ms: u64) {
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
    pub fn tick_gap(&mut self, ms: u64) {
        unimplemented!("host::calls::CallBudget::tick_gap")
    }
}

/// The number of engine epoch ticks a deadline of `ms` milliseconds spans (reference-host-runtime.md).
pub fn deadline_ticks(ms: u64) -> u64 {
    unimplemented!("host::calls::deadline_ticks")
}
