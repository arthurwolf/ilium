//! Public startup admission custody, without actor/runtime/backend creation.
use ilium_execution::{QuotaGroup, QuotaLimits, RejectReason, StorageAdmission};
use ilium_voice::{
    runtime_capture_bytes, OwnedVoiceStartup, ReasoningEffort, VadEagerness, VoiceInputMode,
    VoiceModel, VoiceName, VoiceRuntimeConfig, VoiceService, VoiceTextAllocation,
};
use secrecy::SecretString;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const METADATA_BYTES: usize = 128 * 1024;

#[derive(Debug)]
struct StartupAllocation {
    released: Arc<AtomicBool>,
    _storage: StorageAdmission,
}
impl VoiceTextAllocation for StartupAllocation {}
impl Drop for StartupAllocation {
    fn drop(&mut self) {
        self.released.store(true, Ordering::Release);
    }
}
fn config() -> VoiceRuntimeConfig {
    let mut instructions = String::with_capacity(4096);
    instructions.push_str("original startup instructions λ");
    VoiceRuntimeConfig {
        api_key: SecretString::from("fixture-key-never-used"),
        model: VoiceModel::GptRealtimeMini,
        voice: VoiceName::Marin,
        reasoning_effort: ReasoningEffort::Low,
        input_mode: VoiceInputMode::SemanticVad,
        vad_eagerness: VadEagerness::Auto,
        input_device_name: None,
        output_device_name: None,
        output_volume_percent: 0,
        instructions,
    }
}
fn quota(worker_bytes: usize) -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        // Admission alone must not create/reserve any physical audio owner.
        worker_threads: 0,
        worker_bytes,
    })
}
fn startup(
    quota: &QuotaGroup,
    config: VoiceRuntimeConfig,
    bytes: usize,
    released: Arc<AtomicBool>,
) -> OwnedVoiceStartup {
    let storage = quota.reserve_external_storage(bytes).unwrap();
    OwnedVoiceStartup::charged(
        config,
        Vec::new(),
        bytes,
        Arc::new(StartupAllocation {
            released,
            _storage: storage,
        }),
    )
    .unwrap()
}

#[test]
fn metadata_refusal_keeps_original_valid_startup_until_explicit_caller_cancel() {
    let config = config();
    let pointer = config.instructions.as_ptr();
    let capacity = config.instructions.capacity();
    let bytes = runtime_capture_bytes(&config, &Vec::new()).unwrap();
    let quota = quota(bytes + METADATA_BYTES - 1);
    let released = Arc::new(AtomicBool::new(false));
    let original = startup(&quota, config, bytes, released.clone());
    assert!(matches!(
        VoiceService::admit_startup(quota.clone()),
        Err(RejectReason::WorkerBytes)
    ));
    assert_eq!(original.config().instructions.as_ptr(), pointer);
    assert_eq!(original.config().instructions.capacity(), capacity);
    assert_eq!(
        original.config().instructions,
        "original startup instructions λ"
    );
    assert!(!released.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().worker_bytes, bytes);
    assert_eq!(quota.snapshot().worker_threads, 0);
    // Refusal did not take ownership; only this explicit caller cancellation
    // destroys the exact original source and its independent allocation.
    drop(original);
    assert!(released.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn unused_startup_token_holds_exact_metadata_and_releases_without_actor_or_backend() {
    let config = config();
    let pointer = config.instructions.as_ptr();
    let bytes = runtime_capture_bytes(&config, &Vec::new()).unwrap();
    let quota = quota(bytes + METADATA_BYTES);
    let released = Arc::new(AtomicBool::new(false));
    let original = startup(&quota, config, bytes, released.clone());
    let token = VoiceService::admit_startup(quota.clone()).unwrap_or_else(|reason| {
        panic!("exact source-plus-metadata allowance refused: {reason:?}")
    });
    assert_eq!(quota.snapshot().worker_bytes, bytes + METADATA_BYTES);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(original.config().instructions.as_ptr(), pointer);
    assert!(!released.load(Ordering::Acquire));
    drop(token);
    assert_eq!(quota.snapshot().worker_bytes, bytes);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert_eq!(original.config().instructions.as_ptr(), pointer);
    assert!(!released.load(Ordering::Acquire));
    drop(original);
    assert!(released.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
