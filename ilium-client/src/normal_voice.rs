//! Bounded FIFO for one normal voice actor. Channel refusal returns the exact
//! original command; replacement actors never inherit an earlier conversation.
use ilium_execution::{QuotaGroup, StorageAdmission};
use ilium_voice::{VoiceCommand, VoiceService};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_COMMANDS: usize = 64;
const METADATA_BYTES: usize = 16 * 1024;

/// Before-copy bound for borrowed client settings and trusted embedded schemas.
/// Each source byte can account for a JSON node (256-byte BTree bookkeeping);
/// all embedded voice descriptions are included, including repeated references.
/// Runtime capture auditing subsequently checks actual capacities/layouts.
pub(crate) fn capture_bytes(
    settings: &crate::config::VoiceSettings,
    template_bytes: usize,
    schema_source_bytes: usize,
) -> Option<usize> {
    let prompt = settings.custom_prompt.len();
    let devices = settings
        .input_device_name
        .as_ref()
        .map_or(0, String::len)
        .checked_add(settings.output_device_name.as_ref().map_or(0, String::len))?;
    if prompt > 64 * 1024 || settings.api_key.len() > 64 * 1024 || devices > 128 * 1024 {
        return None;
    }
    let descriptions = ilium_prompts::catalog()
        .filter(|(name, _)| name.starts_with("voice/"))
        .try_fold(0usize, |total, (_, source)| total.checked_add(source.len()))?;
    schema_source_bytes
        .checked_mul(256)?
        .checked_add(descriptions.checked_mul(32)?)?
        .checked_add(
            template_bytes
                .checked_add(prompt)?
                .checked_add(4096)?
                .checked_mul(64)?,
        )?
        // Environment fallback original is capped at64KiB; key/header copies
        // and device derivatives coexist with captured configuration.
        .checked_add(
            settings
                .api_key
                .len()
                .max(64 * 1024)
                .checked_add(devices)?
                .checked_mul(8)?,
        )
}

pub(crate) struct CommandRefusal {
    pub command: VoiceCommand,
    pub reason: String,
}
#[derive(Clone)]
pub(crate) struct ContextSlot(Arc<()>);
enum QueuedCommand {
    Ready(VoiceCommand),
    Context(ContextSlot),
}
pub(crate) struct CancelledVoiceCommands {
    commands: VecDeque<QueuedCommand>,
    // Last allocation owner: cancellation receiver must inspect originals.
    _metadata: Option<Arc<StorageAdmission>>,
}
impl CancelledVoiceCommands {
    pub fn len(&self) -> usize {
        self.commands.len()
    }
}
#[derive(Default)]
pub(crate) struct NormalVoiceCommands {
    owner: Option<Arc<()>>,
    queue: VecDeque<QueuedCommand>,
    retry_at: Option<Instant>,
    // The queue allocation remains charged even when it is empty.
    metadata: Option<Arc<StorageAdmission>>,
}
pub(crate) struct ReservedVoiceCommands<'a> {
    queue: &'a mut NormalVoiceCommands,
    remaining: usize,
}
impl ReservedVoiceCommands<'_> {
    pub fn send(&mut self, command: VoiceCommand) {
        // Only the exact preflighted sentence iterator constructs commands;
        // exclusive queue custody prevents concurrent slot consumption.
        assert!(self.remaining != 0, "reserved voice batch count invariant");
        self.remaining -= 1;
        self.queue.queue.push_back(QueuedCommand::Ready(command));
    }
}
impl NormalVoiceCommands {
    pub fn available(&self) -> usize {
        MAX_COMMANDS.saturating_sub(self.queue.len())
    }
    pub fn is_pending(&self) -> bool {
        !self.queue.is_empty()
    }
    pub fn reserve_batch(
        &mut self,
        owner: Arc<()>,
        count: usize,
        quota: &QuotaGroup,
    ) -> Result<ReservedVoiceCommands<'_>, String> {
        if count > self.available() {
            return Err("Normal voice command FIFO full; original offer retained".into());
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !Arc::ptr_eq(old, &owner))
        {
            return Err("Original voice actor has not retired; offer retained".into());
        }
        self.prepare_metadata(quota)?;
        self.owner = Some(owner);
        Ok(ReservedVoiceCommands {
            queue: self,
            remaining: count,
        })
    }
    fn prepare_metadata(&mut self, quota: &QuotaGroup) -> Result<(), String> {
        if self.metadata.is_some() {
            return Ok(());
        }
        if MAX_COMMANDS * std::mem::size_of::<QueuedCommand>() + 4096 > METADATA_BYTES {
            return Err("Voice command metadata declaration exceeded".into());
        }
        let allocation = Arc::new(
            quota
                .reserve_external_storage(METADATA_BYTES)
                .map_err(|reason| format!("Voice command metadata admission: {reason:?}"))?,
        );
        self.queue
            .try_reserve_exact(MAX_COMMANDS)
            .map_err(|error| format!("Voice FIFO allocation failed: {error}"))?;
        self.metadata = Some(allocation);
        Ok(())
    }
    pub fn enqueue(
        &mut self,
        owner: Arc<()>,
        command: VoiceCommand,
        quota: &QuotaGroup,
    ) -> Result<(), Box<CommandRefusal>> {
        let reject = |reason: &str, command| {
            Box::new(CommandRefusal {
                command,
                reason: reason.into(),
            })
        };
        if self.available() == 0 {
            return Err(reject(
                "Normal voice command FIFO full; original retained",
                command,
            ));
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !Arc::ptr_eq(old, &owner))
        {
            return Err(reject(
                "Original voice actor has not retired; command cannot be replayed",
                command,
            ));
        }
        if let Err(reason) = self.prepare_metadata(quota) {
            return Err(Box::new(CommandRefusal { command, reason }));
        }
        self.owner = Some(owner);
        self.queue.push_back(QueuedCommand::Ready(command));
        Ok(())
    }
    pub fn reserve_context(
        &mut self,
        owner: Arc<()>,
        quota: &QuotaGroup,
    ) -> Result<ContextSlot, String> {
        if self.available() == 0 {
            return Err("Voice context FIFO slot unavailable".into());
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !Arc::ptr_eq(old, &owner))
        {
            return Err("Original voice actor owns FIFO".into());
        }
        self.prepare_metadata(quota)?;
        self.owner = Some(owner);
        let slot = ContextSlot(Arc::new(()));
        self.queue.push_back(QueuedCommand::Context(slot.clone()));
        Ok(slot)
    }
    pub fn fill_context(
        &mut self,
        slot: &ContextSlot,
        command: VoiceCommand,
    ) -> Result<(), VoiceCommand> {
        let Some(entry) = self.queue.iter_mut().find(
            |entry| matches!(entry,QueuedCommand::Context(found) if Arc::ptr_eq(&found.0,&slot.0)),
        ) else {
            return Err(command);
        };
        *entry = QueuedCommand::Ready(command);
        Ok(())
    }
    pub fn cancel_context(&mut self, slot: &ContextSlot) {
        self.queue.retain(
            |entry| !matches!(entry,QueuedCommand::Context(found) if Arc::ptr_eq(&found.0,&slot.0)),
        );
    }
    /// At most64 sends per turn, no capacity await and no full-queue spin.
    pub fn publish(&mut self, service: &VoiceService, now: Instant) -> Result<(), String> {
        self.publish_to(&service.instance_identity(), &service.command_sender(), now)
    }
    fn publish_to(
        &mut self,
        owner: &Arc<()>,
        sender: &tokio::sync::mpsc::Sender<VoiceCommand>,
        now: Instant,
    ) -> Result<(), String> {
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !Arc::ptr_eq(old, owner))
        {
            return Err(
                "Voice FIFO belongs to another actor; originals retained for cancellation".into(),
            );
        }
        if self.retry_at.is_some_and(|deadline| now < deadline) {
            return Ok(());
        }
        while let Some(entry) = self.queue.pop_front() {
            let command = match entry {
                QueuedCommand::Ready(command) => command,
                barrier @ QueuedCommand::Context(_) => {
                    self.queue.push_front(barrier);
                    return Ok(());
                }
            };
            match sender.try_send(command) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(command)) => {
                    self.queue.push_front(QueuedCommand::Ready(command));
                    self.retry_at = Some(now + Duration::from_millis(20));
                    return Ok(());
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(command)) => {
                    self.queue.push_front(QueuedCommand::Ready(command));
                    return Err(
                        "Voice actor command receiver closed; original FIFO retained".into(),
                    );
                }
            }
        }
        self.retry_at = None;
        Ok(())
    }
    /// Actor exit is the explicit cancellation boundary, not replacement start.
    /// Caller must inspect/report every original before releasing it.
    pub fn cancel_after_actor_exit(&mut self) -> CancelledVoiceCommands {
        self.owner = None;
        self.retry_at = None;
        CancelledVoiceCommands {
            commands: std::mem::take(&mut self.queue),
            _metadata: self.metadata.take(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_actor_fifo_preserves_order_originals_and_dated_retry() {
        let bank = crate::execution::test_client();
        let quota = bank.quota_group();
        let owner = Arc::new(());
        let mut queue = NormalVoiceCommands::default();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender.try_send(VoiceCommand::StopPushToTalk).unwrap();
        queue
            .enqueue(owner.clone(), VoiceCommand::StartPushToTalk, &quota)
            .ok()
            .unwrap();
        queue
            .enqueue(owner.clone(), VoiceCommand::StopPushToTalk, &quota)
            .ok()
            .unwrap();
        let now = Instant::now();
        queue.publish_to(&owner, &sender, now).unwrap();
        assert_eq!(queue.queue.len(), 2);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::StopPushToTalk
        ));
        queue.publish_to(&owner, &sender, now).unwrap();
        assert!(
            receiver.try_recv().is_err(),
            "retry deadline prevents full-channel spin"
        );
        queue
            .publish_to(&owner, &sender, now + Duration::from_millis(20))
            .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::StartPushToTalk
        ));
        queue
            .publish_to(&owner, &sender, now + Duration::from_millis(40))
            .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::StopPushToTalk
        ));
        assert!(!queue.is_pending());
    }

    #[test]
    fn replacement_cannot_inherit_originals_and_metadata_outlives_cancellation_receipt() {
        let bank = crate::execution::test_client();
        let quota = bank.quota_group();
        let owner = Arc::new(());
        let replacement = Arc::new(());
        let mut queue = NormalVoiceCommands::default();
        for _ in 0..MAX_COMMANDS {
            queue
                .enqueue(owner.clone(), VoiceCommand::StartPushToTalk, &quota)
                .ok()
                .unwrap();
        }
        let refused = queue
            .enqueue(owner.clone(), VoiceCommand::StopPushToTalk, &quota)
            .err()
            .unwrap();
        assert!(matches!(refused.command, VoiceCommand::StopPushToTalk));
        assert_eq!(queue.queue.len(), MAX_COMMANDS);
        let hold = Arc::downgrade(queue.metadata.as_ref().unwrap());
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        assert!(queue
            .publish_to(&replacement, &sender, Instant::now())
            .is_err());
        assert!(receiver.try_recv().is_err());
        let originals = queue.cancel_after_actor_exit();
        drop(queue);
        assert_eq!(originals.len(), MAX_COMMANDS);
        assert!(hold.upgrade().is_some());
        drop(originals);
        assert!(
            hold.upgrade().is_none(),
            "actual last cancellation owner releases the metadata"
        );
    }

    #[test]
    fn whole_batch_refusal_leaves_existing_fifo_unchanged() {
        let bank = crate::execution::test_client();
        let quota = bank.quota_group();
        let owner = Arc::new(());
        let mut queue = NormalVoiceCommands::default();
        queue
            .enqueue(owner.clone(), VoiceCommand::StartPushToTalk, &quota)
            .ok()
            .unwrap();
        assert!(queue
            .reserve_batch(owner.clone(), MAX_COMMANDS, &quota)
            .is_err());
        assert_eq!(queue.queue.len(), 1);
        let mut batch = queue.reserve_batch(owner, 2, &quota).unwrap();
        batch.send(VoiceCommand::StopPushToTalk);
        batch.send(VoiceCommand::StartPushToTalk);
        assert_eq!(queue.queue.len(), 3);
    }
    #[test]
    fn borrowed_startup_bound_covers_actual_context_layout_and_maximum_prompt() {
        let bank = crate::execution::test_client();
        let settings = crate::config::VoiceSettings {
            custom_prompt: "x".repeat(64 * 1024),
            ..Default::default()
        };
        let bound = capture_bytes(
            &settings,
            ilium_prompts::voice::VOICE_MOD_SYSTEM_INSTRUCTIONS.len(),
            include_str!("control/tools.rs").len(),
        )
        .unwrap();
        let held = bank.quota_group().reserve_external_storage(bound).unwrap();
        let instructions = crate::control::system_instructions(
            &settings.custom_prompt,
            crate::control::VoiceTargetContext::NoDetectedAgent,
        );
        let tools = crate::control::ControlPlane::default().tool_definitions();
        let actual = ilium_voice::context_capture_bytes(&instructions, &tools).unwrap()
            + instructions.capacity()
            + tools.capacity() * std::mem::size_of::<ilium_voice::VoiceToolDefinition>();
        assert!(
            actual <= bound,
            "actual {actual} exceeds borrowed preallocation declaration {bound}"
        );
        drop(tools);
        drop(instructions);
        drop(held);
    }
    #[test]
    fn actual_prepared_context_fills_original_slot_before_younger_semantic_input() {
        let client = crate::execution::test_client();
        let quota = client.quota_group();
        let actor = Arc::new(());
        let mut queue = NormalVoiceCommands::default();
        queue
            .enqueue(actor.clone(), VoiceCommand::StartPushToTalk, &quota)
            .ok()
            .unwrap();
        let slot = queue.reserve_context(actor.clone(), &quota).unwrap();
        queue
            .enqueue(actor.clone(), VoiceCommand::StopPushToTalk, &quota)
            .ok()
            .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        queue.publish_to(&actor, &sender, Instant::now()).unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::StartPushToTalk
        ));
        assert!(
            receiver.try_recv().is_err(),
            "younger input cannot overtake original context slot"
        );
        let mut preparation = crate::voice_preparation::VoicePreparation::new(client);
        let settings = crate::config::VoiceSettings {
            api_key: "isolated-no-network".into(),
            custom_prompt: "ordered policy".into(),
            ..Default::default()
        };
        preparation
            .request(
                &settings,
                crate::voice_preparation::PreparationKind::Context {
                    target: crate::control::VoiceTargetContext::NoDetectedAgent,
                    actor: actor.clone(),
                },
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let prepared = loop {
            if let Some(prepared) = preparation.collect() {
                break prepared;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        let crate::voice_preparation::PreparedValue::Context(context) = prepared.value.unwrap()
        else {
            panic!("context")
        };
        assert!(context.instructions().contains("ordered policy"));
        assert!(queue
            .fill_context(&slot, VoiceCommand::UpdateContext(context))
            .is_ok());
        queue.publish_to(&actor, &sender, Instant::now()).unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::UpdateContext(_)
        ));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            VoiceCommand::StopPushToTalk
        ));
        assert!(!queue.is_pending());
    }
}
