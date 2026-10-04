//! Allocation custody for voice values installed by the UI owner.
use std::sync::Arc;

use ilium_execution::{QuotaGroup, RejectReason, StorageAdmission};
use ilium_voice::{VoiceConnectionState, VoiceEvent};

#[derive(Default)]
pub(crate) struct VoicePresentationStorage {
    pub user: Option<Arc<StorageAdmission>>,
    pub assistant: Option<Arc<StorageAdmission>>,
    pub state: Option<Arc<StorageAdmission>>,
    pub status: Option<Arc<StorageAdmission>>,
}

/// Reserve the replacement allocation before appending or formatting. The
/// original event and old installed allocation remain charged simultaneously.
/// This bounds declared payload storage, not allocator overhead or process RSS.
pub(crate) fn prepare_derivation(
    quota: &QuotaGroup,
    event: &VoiceEvent,
    assistant: Option<&String>,
) -> Result<Option<Arc<StorageAdmission>>, RejectReason> {
    let bytes = match event {
        VoiceEvent::AssistantTranscript(delta) => assistant
            .map_or(0, |text| text.capacity().max(text.len()))
            .checked_add(delta.capacity().max(delta.len())),
        VoiceEvent::StateChanged(VoiceConnectionState::Failed(error))
        | VoiceEvent::ProviderError(error) => Some(error.capacity().max(error.len())),
        _ => return Ok(None),
    }
    .and_then(|bytes| bytes.checked_mul(2))
    .and_then(|bytes| bytes.checked_add(4096))
    .ok_or(RejectReason::InvalidCost)?;
    quota
        .reserve_external_storage(bytes)
        .map(|hold| Some(Arc::new(hold)))
}
