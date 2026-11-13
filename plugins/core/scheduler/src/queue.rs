//! The per-pane prompt queue and the send-when-idle rule. Pure: a pane is a `u64` id.
//! Ported from ilium-core's queue methods, ilium-client/src/prompt_queue.rs (dialog
//! validation) and ilium-server/src/prompt_queue.rs (the delivery gate).

/// Most prompts one pane may hold; stops a misbehaving client from growing a snapshot.
pub const MAXIMUM_PROMPT_QUEUE_LEN: usize = 100;

/// How often one queued prompt is delivered after successive agent-finished transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Once,
    Times { remaining_runs: u32 },
    Forever,
}

impl Delivery {
    /// A `Times` entry with zero runs left is invalid.
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Times { remaining_runs: 0 })
    }

    fn rank(&self) -> usize {
        match self {
            Self::Once => 0,
            Self::Times { .. } => 1,
            Self::Forever => 2,
        }
    }
}

/// One durable FIFO prompt waiting for an agent-finished transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub text: String,
    pub delivery: Delivery,
    /// Persisted before the first byte is admitted; an attempted head is kept for inspection
    /// but never replayed after an uncertain exit.
    pub attempted_delivery: bool,
}

impl QueuedPrompt {
    pub fn is_valid(&self) -> bool {
        !self.text.trim().is_empty() && self.delivery.is_valid()
    }
}

/// Why a queue mutation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    EmptyPrompt,
    Full(usize),
}

/// The FIFO of one terminal pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptQueue {
    prompts: Vec<QueuedPrompt>,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.prompts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }

    pub fn prompts(&self) -> &[QueuedPrompt] {
        &self.prompts
    }

    pub fn enqueue(&mut self, prompt: QueuedPrompt) -> Result<(), QueueError> {
        if !prompt.is_valid() {
            return Err(QueueError::EmptyPrompt);
        }
        if self.prompts.len() >= MAXIMUM_PROMPT_QUEUE_LEN {
            return Err(QueueError::Full(MAXIMUM_PROMPT_QUEUE_LEN));
        }
        self.prompts.push(prompt);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.prompts.clear();
    }

    pub fn next(&self) -> Option<&QueuedPrompt> {
        self.prompts.first()
    }

    /// Compare-and-mark the head before any irreversible write; a stale completion cannot
    /// mark a replacement prompt after Clear.
    pub fn mark_attempted(&mut self, expected: &QueuedPrompt) -> bool {
        match self.prompts.first_mut() {
            Some(head) if &*head == expected && !head.attempted_delivery => {
                head.attempted_delivery = true;
                true
            }
            _ => false,
        }
    }

    /// Acknowledges delivery only while the head is still the entry that was read. A one-shot
    /// entry (or `Times` on its last run) is removed; a recurring entry rotates to the tail so
    /// it can never block the entries behind it.
    pub fn acknowledge(&mut self, expected: &QueuedPrompt) -> bool {
        if self.prompts.first() != Some(expected) {
            return false;
        }
        let mut delivered = self.prompts.remove(0);
        delivered.attempted_delivery = false;
        match &mut delivered.delivery {
            Delivery::Once => {}
            Delivery::Times { remaining_runs } if *remaining_runs > 1 => {
                *remaining_runs -= 1;
                self.prompts.push(delivered);
            }
            Delivery::Times { .. } => {}
            Delivery::Forever => self.prompts.push(delivered),
        }
        true
    }
}

/// Which delivery the dialog offers, in cycling order.
pub const DELIVERY_CHOICES: [Delivery; 3] = [
    Delivery::Once,
    Delivery::Times { remaining_runs: 2 },
    Delivery::Forever,
];

/// Cycles the delivery choice by `direction` (wrapping) without overflow for any `i8`.
pub fn cycle_delivery(current: &Delivery, direction: i8) -> Delivery {
    let position = DELIVERY_CHOICES
        .iter()
        .position(|choice| choice.rank() == current.rank())
        .unwrap_or(0);
    let next = (position as i32 + i32::from(direction)).rem_euclid(DELIVERY_CHOICES.len() as i32);
    DELIVERY_CHOICES[next as usize].clone()
}

/// Validates the dialog: non-blank text and, for "Run X times", a positive whole count.
pub fn validated_request(
    text: &str,
    choice: &Delivery,
    times_field: &str,
) -> Result<(String, Delivery), String> {
    if text.trim().is_empty() {
        return Err("Write a prompt before enqueueing it".to_string());
    }
    let delivery = match choice {
        Delivery::Once => Delivery::Once,
        Delivery::Forever => Delivery::Forever,
        Delivery::Times { .. } => {
            let remaining_runs = times_field
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| "Run count must be a positive whole number".to_string())?;
            Delivery::Times { remaining_runs }
        }
    };
    Ok((text.to_string(), delivery))
}

/// Steps the repeat count by `direction`, clamped to `1..=u32::MAX`. Fails on a draft that is
/// not a positive whole number.
pub fn step_times(field: &str, direction: i32) -> Result<String, String> {
    let value = field
        .trim()
        .parse::<i128>()
        .map_err(|_| "Run count must be a whole number".to_string())?;
    if !(1..=i128::from(u32::MAX)).contains(&value) {
        return Err("Run count must be a whole number".to_string());
    }
    let next = (value + i128::from(direction)).clamp(1, i128::from(u32::MAX));
    Ok(next.to_string())
}

/// Normalises pasted text: CRLF and lone CR become single newlines.
pub fn normalize_paste(pasted: &str) -> String {
    pasted.replace("\r\n", "\n").replace('\r', "\n")
}

/// What the agent was doing, as far as delivery cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Working,
    Idle,
}

/// The turn the detector reports for a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activity {
    pub turn: Turn,
}

/// Decides whether the queue head may be sent now. The detection loop alone says an agent has
/// finished; this rule additionally requires that the pane still reads idle, that the head was
/// not already attempted (no replay after an uncertain exit) and that the completion epoch is
/// still unclaimed.
pub fn next_send<'a>(
    queue: &'a PromptQueue,
    activity: Option<Activity>,
    epoch: &CompletionEpoch,
    expected_epoch: u64,
) -> Option<&'a QueuedPrompt> {
    if !matches!(activity, Some(Activity { turn: Turn::Idle })) {
        return None;
    }
    if !epoch.is_available(expected_epoch) {
        return None;
    }
    let head = queue.next()?;
    if head.attempted_delivery {
        return None;
    }
    Some(head)
}

/// A completion authorizes at most one queued submission. Changing turn or invocation
/// invalidates pending and claimed authorizations alike.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CompletionEpoch {
    current: u64,
    claimed: Option<u64>,
}

impl CompletionEpoch {
    pub fn current(&self) -> u64 {
        self.current
    }

    pub fn invalidate(&mut self) {
        self.current = self.current.wrapping_add(1);
        self.claimed = None;
    }

    /// Admitting Enter must not authorize another head while the detector still shows Idle;
    /// only a new observed turn reopens delivery.
    pub fn retire_submission(&mut self) {
        self.current = self.current.wrapping_add(1);
        self.claimed = Some(self.current);
    }

    pub fn is_available(&self, expected: u64) -> bool {
        self.current == expected && self.claimed != Some(expected)
    }

    pub fn claim(&mut self, expected: u64) -> bool {
        if !self.is_available(expected) {
            return false;
        }
        self.claimed = Some(expected);
        true
    }

    pub fn is_claimed_current(&self, expected: u64) -> bool {
        self.current == expected && self.claimed == Some(expected)
    }
}
