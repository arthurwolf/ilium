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
    let (key, default_ms) = match class {
        CallClass::Event => (LimitKey::EventDeadlineMs, 20),
        CallClass::Command => (LimitKey::CommandDeadlineMs, 50),
        CallClass::Render => (LimitKey::RenderDeadlineMs, 10),
        CallClass::Annotate => (LimitKey::AnnotateDeadlineMs, 10),
        CallClass::Frame => (LimitKey::FrameDeadlineMs, 50),
        CallClass::Detector => (LimitKey::DetectorDeadlineMs, 20),
        CallClass::Activate => (LimitKey::ActivateDeadlineMs, 2_000),
        CallClass::Deactivate => (LimitKey::DeactivateDeadlineMs, 500),
        CallClass::Migrate => (LimitKey::MigrateDeadlineMs, 30_000),
        CallClass::Job => (LimitKey::JobDeadlineMs, 2_000),
        CallClass::MenuItems | CallClass::QueryCatalog | CallClass::ListVirtualChildren => {
            return Duration::from_millis(20);
        }
        CallClass::Tokens => return Duration::from_millis(10),
        CallClass::SettingsValidate => return Duration::from_millis(100),
    };
    Duration::from_millis(limits.0.get(&key).copied().unwrap_or(default_ms))
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
    class: CallClass,
    elapsed: Duration,
    limits: &ResolvedLimits,
) -> BreachVerdict {
    breach_at(class, elapsed, deadline_for(class, limits))
}

fn breach_at(class: CallClass, elapsed: Duration, deadline: Duration) -> BreachVerdict {
    if elapsed <= deadline {
        return BreachVerdict::Timely;
    }
    if streak_limit(class).is_none() {
        return BreachVerdict::HardTrap;
    }
    if elapsed >= deadline.saturating_add(Duration::from_millis(SOFT_GRACE_MS)) {
        BreachVerdict::SoftTrap
    } else {
        BreachVerdict::SoftLate
    }
}

/// The call class of an event batch: `Command` when a command invocation is present (HST-192).
pub fn batch_call_class(batch: &[ilium_plugin_api::generated::types::events::Event]) -> CallClass {
    if batch.iter().any(|event| {
        matches!(
            event,
            ilium_plugin_api::generated::types::events::Event::CommandInvoked(_)
        )
    }) {
        CallClass::Command
    } else {
        CallClass::Event
    }
}

/// How many late calls in a row trap the instance, if the class has a streak limit (HST-190).
pub fn streak_limit(class: CallClass) -> Option<u32> {
    match class {
        CallClass::Frame => Some(30),
        CallClass::Annotate
        | CallClass::Detector
        | CallClass::MenuItems
        | CallClass::QueryCatalog
        | CallClass::ListVirtualChildren
        | CallClass::Tokens
        | CallClass::SettingsValidate => Some(3),
        _ => None,
    }
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
        Self {
            class,
            late_in_a_row: 0,
        }
    }

    /// Records one call; returns true when the streak limit is reached (trap the instance).
    pub fn record(&mut self, timely: bool) -> bool {
        self.late_in_a_row = if timely {
            0
        } else {
            self.late_in_a_row.saturating_add(1)
        };
        streak_limit(self.class).is_some_and(|limit| self.late_in_a_row >= limit)
    }
}

/// Measures one guest call: guest and synchronous import time count; time after return does not
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
        Self {
            class,
            deadline: deadline_for(class, limits),
            used: Duration::ZERO,
            returned: false,
        }
    }

    /// Guest code ran for `ms`.
    pub fn guest_ms(&mut self, ms: u64) {
        if !self.returned {
            self.used = self.used.saturating_add(Duration::from_millis(ms));
        }
    }

    /// A synchronous host import ran for `ms` (charged while the guest waits).
    pub fn import_ms(&mut self, ms: u64) {
        self.guest_ms(ms);
    }

    /// The guest returned.
    pub fn returned(&mut self) {
        self.returned = true;
    }

    /// Time passed after the return (not charged).
    pub fn post_return_ms(&mut self, _ms: u64) {
        // Work completed after return is outside the call deadline.
    }

    /// The charged time.
    pub fn used(&self) -> Duration {
        self.used
    }

    /// The verdict for the charged time.
    pub fn verdict(&self) -> BreachVerdict {
        breach_at(self.class, self.used, self.deadline)
    }

    /// A gap between scheduler ticks of `ms` (a suspended machine is not charged, HST-196).
    pub fn tick_gap(&mut self, ms: u64) {
        if !crate::platform_rules::tick_gap_ignored(Duration::from_millis(ms)) {
            self.guest_ms(ms);
        }
    }
}

/// The number of engine epoch ticks a deadline of `ms` milliseconds spans (reference-host-runtime.md).
pub fn deadline_ticks(ms: u64) -> u64 {
    ms.div_ceil(10).saturating_add(1)
}
