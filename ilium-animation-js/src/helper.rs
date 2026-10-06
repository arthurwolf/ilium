//! Private bounded helper transport. Unverified scripts never execute on the
//! client/UI process. Authority belongs to this parent-created pipe/session;
//! script RPC payloads cannot supply package, principal, generation or epoch.
#![cfg(feature = "v8-runtime")]

use crate::{
    engine::{
        self,
        ArraySpec,
        CompletionState,
        CreateState,
        Engine,
        EngineLimits,
        HostRequest,
        RenderOutput, // Preserve the original engine owner and output types.
        ServiceAuthority,
        ServiceBudget,
        ServicePhase,
        ServiceValue,
        TypedArrayKind, // Add retained binary service and activation contracts.
    },
    error::{AnimationError, Result},
    manifest::AnimationMode,
    package::{Package, PackageLimits},
};
use ilium_execution::{QuotaGroup, QuotaLimits, StorageAdmission, WorkerAdmission}; // Retain original physical admission through failed helper retirement.
use ilium_platform::{
    animation_sandbox::{self, SandboxChild, SandboxLimits},
    owned_worker::{self, OwnedWorker, StopToken, WorkerKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque}, // Bound unissued requests and terminal inventories by retained leases.
    io::{Read, Write},
    path::Path,
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};

#[cfg(test)]
use std::sync::{Condvar, Mutex};

const VERSION: u16 = 2; // Reject old JSON-only service envelopes rather than silently changing their meaning.
const MAX_JSON: usize = 256 * 1024;
const MAX_BINARY: usize = 32 * 1024 * 1024;
const MAX_PLANES: usize = 48; // Up to 16 surface/working planes plus 32 borrowed inputs.
const MAX_REQUESTS: usize = 64;

#[cfg(test)]
#[derive(Debug, Default)]
struct HelperTransportTestState {
    arm_complete_service_ack_failure: bool,
    target_sequence: Option<u64>,
    packet_released_sequence: Option<u64>,
    acknowledgement_failure_sequence: Option<u64>,
    write_succeeded: Option<bool>,
    service_plane_pointer: Option<usize>,
    interrupt_next_physical_retirement: bool,
    retirement_interruptions: usize,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct HelperTransportTestControl {
    state: Mutex<HelperTransportTestState>,
    changed: Condvar,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HelperTransportTestSnapshot {
    pub(crate) target_sequence: Option<u64>,
    pub(crate) packet_released_sequence: Option<u64>,
    pub(crate) acknowledgement_failure_sequence: Option<u64>,
    pub(crate) write_succeeded: Option<bool>,
    pub(crate) service_plane_pointer: Option<usize>,
    pub(crate) retirement_interruptions: usize,
    pub(crate) session_closed: bool,
    pub(crate) physically_retired: bool,
}

#[cfg(test)]
impl HelperTransportTestControl {
    fn state(&self) -> std::sync::MutexGuard<'_, HelperTransportTestState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn arm_complete_service_ack_failure(&self) -> Result<()> {
        let mut state = self.state();
        if state.arm_complete_service_ack_failure || state.target_sequence.is_some() {
            return Err(invalid("test CompleteService interruption already armed"));
        }
        state.arm_complete_service_ack_failure = true;
        state.target_sequence = None;
        state.packet_released_sequence = None;
        state.acknowledgement_failure_sequence = None;
        state.write_succeeded = None;
        state.service_plane_pointer = None;
        state.interrupt_next_physical_retirement = true;
        state.retirement_interruptions = 0;
        Ok(())
    }

    fn register_complete_service(&self, sequence: u64) {
        let mut state = self.state();
        if !state.arm_complete_service_ack_failure {
            return;
        }
        state.arm_complete_service_ack_failure = false;
        state.target_sequence = Some(sequence);
        self.changed.notify_all();
    }

    fn packet_released(
        &self,
        sequence: u64,
        write_succeeded: bool,
        service_plane_pointer: Option<usize>,
    ) {
        let mut state = self.state();
        if state.target_sequence != Some(sequence) {
            return;
        }
        state.packet_released_sequence = Some(sequence);
        state.write_succeeded = Some(write_succeeded);
        state.service_plane_pointer = service_plane_pointer;
        self.changed.notify_all();
    }

    fn intercept_response(&self, sequence: u64) -> bool {
        let mut state = self.state();
        if state.target_sequence != Some(sequence) {
            return false;
        }
        // Missing writer evidence must fail the injection without stranding
        // the original pipe worker during test cleanup.
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.packet_released_sequence != Some(sequence) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, _) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poison| poison.into_inner());
            state = next;
        }
        if state.write_succeeded != Some(true) {
            return false;
        }
        if state.acknowledgement_failure_sequence.is_some() {
            return false;
        }
        state.acknowledgement_failure_sequence = Some(sequence);
        self.changed.notify_all();
        true
    }

    fn interrupt_physical_retirement_once(&self) -> bool {
        let mut state = self.state();
        if !state.interrupt_next_physical_retirement {
            return false;
        }
        state.interrupt_next_physical_retirement = false;
        state.retirement_interruptions += 1;
        self.changed.notify_all();
        true
    }

    fn snapshot(
        &self,
        session_closed: bool,
        physically_retired: bool,
    ) -> HelperTransportTestSnapshot {
        let state = self.state();
        HelperTransportTestSnapshot {
            target_sequence: state.target_sequence,
            packet_released_sequence: state.packet_released_sequence,
            acknowledgement_failure_sequence: state.acknowledgement_failure_sequence,
            write_succeeded: state.write_succeeded,
            service_plane_pointer: state.service_plane_pointer,
            retirement_interruptions: state.retirement_interruptions,
            session_closed,
            physically_retired,
        }
    }
}

fn loaded_helper_digest() -> Result<[u8; 32]> {
    let mut image = animation_sandbox::open_running_helper_image()?;
    let mut digest = Sha256::new();
    let mut scratch = [0u8; 64 * 1024];
    loop {
        let count = image.read(&mut scratch)?;
        if count == 0 {
            break;
        }
        digest.update(&scratch[..count]);
    }
    Ok(digest.finalize().into())
}
fn parse_helper_digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("helper build digest missing or malformed"));
    }
    let mut result = [0u8; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| invalid("helper build digest malformed"))?;
    }
    if result == [0; 32] {
        return Err(invalid("helper build digest absent"));
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HelperAuthority {
    pub package_digest: String,
    pub instance_id: u64,
    pub plan_generation: u64,
    pub authorization_epoch: u64,
}
impl HelperAuthority {
    fn validate(&self) -> Result<()> {
        if self.package_digest.len() != 64
            || !self.package_digest.bytes().all(|b| b.is_ascii_hexdigit())
            || self.instance_id == 0
        {
            return Err(AnimationError::PermissionDenied(
                "invalid helper authority".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct HelperLimits {
    pub sandbox: SandboxLimits,
    pub engine: EngineLimits,
    pub operation_timeout: Duration,
}
impl Default for HelperLimits {
    fn default() -> Self {
        Self {
            sandbox: SandboxLimits::default(),
            engine: EngineLimits::default(),
            operation_timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaneInfo {
    name: String,
    bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u16,
    sequence: u64,
    authority: HelperAuthority,
    kind: String,
    payload: Value,
    planes: Vec<PlaneInfo>,
}
#[derive(Debug)]
struct Packet {
    envelope: Envelope,
    planes: BTreeMap<String, Vec<u8>>,
    // Keep queued outbound bytes admitted even when a timeout retires the
    // session before its pipe writer releases this packet.
    _outbound_storage: Option<Arc<StorageAdmission>>,
    _scratch: Option<Arc<StorageAdmission>>, // Keep bounded metadata staging alive through escaped pipe writers.
    service: Option<ServiceValue>, // Borrow immutable admitted planes without a second bulk allocation.
}
impl Packet {
    fn new(
        sequence: u64,
        authority: HelperAuthority,
        kind: &str,
        payload: Value,
        planes: BTreeMap<String, Vec<u8>>,
    ) -> Self {
        let inventory = planes
            .iter()
            .map(|(name, bytes)| PlaneInfo {
                name: name.clone(),
                bytes: bytes.len(),
            })
            .collect();
        Self {
            envelope: Envelope {
                version: VERSION,
                sequence,
                authority,
                kind: kind.into(),
                payload,
                planes: inventory,
            },
            planes,
            _outbound_storage: None,
            _scratch: None, // Session commands attach their existing pipe scratch before publication.
            service: None,  // Ordinary frame/seed packets continue using their owned plane map.
        } // Finish the packet with independent optional custody fields.
    } // End ordinary packet construction.
    fn attach_service(&mut self, value: ServiceValue) -> Result<()> {
        // Attach existing immutable custody instead of cloning plane bytes.
        if self.service.is_some() || !self.planes.is_empty() {
            return Err(invalid("mixed service and frame plane custody"));
        } // One packet has exactly one binary ownership source.
        self.envelope.planes = value
            .arrays()
            .iter()
            .map(|array| PlaneInfo {
                name: array.name.clone(),
                bytes: value.planes()[&array.name].len(),
            })
            .collect(); // Preserve the validated per-request canonical inventory.
        self.service = Some(value); // The last queued packet owner keeps the original allocation admitted.
        Ok(()) // Header staging remains separately covered by pipe scratch.
    } // End borrowed plane attachment.
    fn plane(&self, name: &str) -> Option<&[u8]> {
        // Resolve bytes without transferring their guard.
        self.service
            .as_ref()
            .and_then(|value| value.planes().get(name))
            .or_else(|| self.planes.get(name))
            .map(Vec::as_slice) // Both branches borrow immutable owned storage.
    } // End bounded binary borrowing.
}
fn invalid(message: &str) -> AnimationError {
    AnimationError::Runtime(format!("animation helper: {message}"))
}

/// Four-byte network-order metadata length, bounded JSON, then exact declared
/// binary planes. No base64 copies, newline search, read_to_end, or ZIP extraction.
fn packet_metadata(packet: &Packet) -> Result<Vec<u8>> {
    // Share exact whole-envelope preflight between admission and the actual writer.
    validate_envelope(&packet.envelope)?;
    struct BoundedMetadata(Vec<u8>);
    impl Write for BoundedMetadata {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_JSON.saturating_sub(self.0.len()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "helper metadata exceeds bound",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bounded = BoundedMetadata(Vec::with_capacity(MAX_JSON));
    serde_json::to_writer(&mut bounded, &packet.envelope)?;
    let metadata = bounded.0;
    validate_json_structure(&metadata)?;
    if packet.envelope.planes.len()
        != packet
            .service
            .as_ref()
            .map_or(packet.planes.len(), |value| value.arrays().len())
    {
        return Err(invalid("packet plane inventory mismatch"));
    } // Reject extra physical planes before writing a prefix.
    for plane in &packet.envelope.planes {
        if packet.plane(&plane.name).map(<[u8]>::len) != Some(plane.bytes) {
            return Err(invalid("packet binary shape mismatch"));
        }
    } // Verify the full borrowed or owned inventory first.
    Ok(metadata) // Callers may refuse one request without damaging the session or consuming sequence.
} // End exact bounded metadata encoding.
fn write_packet(writer: &mut impl Write, packet: &Packet) -> Result<()> {
    // Write one fully validated immutable packet.
    let metadata = packet_metadata(packet)?; // No prefix or partial payload is emitted before complete validation.
    writer.write_all(&(metadata.len() as u32).to_be_bytes())?; // Preserve network-order framing.
    writer.write_all(&metadata)?; // Metadata contains no JSON-flattened binary bytes.
    for plane in &packet.envelope.planes {
        writer.write_all(
            packet
                .plane(&plane.name)
                .ok_or_else(|| invalid("missing binary plane"))?,
        )?;
    } // Borrow the admitted logical plane until synchronous write completion.
    writer.flush()?; // Publish exactly one response for the matching command.
    Ok(()) // All packet guards remain owned by the caller through this return.
} // End binary packet writing.
fn validate_envelope(envelope: &Envelope) -> Result<()> {
    envelope.authority.validate()?;
    if envelope.version != VERSION || envelope.kind.len() > 32 || envelope.planes.len() > MAX_PLANES
    {
        return Err(invalid("protocol version/type/plane count"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    for plane in &envelope.planes {
        if plane.name.is_empty()
            || plane.name.len() > 64
            || !plane
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !names.insert(&plane.name)
        {
            return Err(invalid("invalid or duplicate plane name"));
        }
        total = total
            .checked_add(plane.bytes)
            .ok_or_else(|| invalid("binary length overflow"))?;
        if total > MAX_BINARY {
            return Err(AnimationError::Budget("helper binary bytes".into()));
        }
    }
    Ok(())
}
fn validate_json_structure(bytes: &[u8]) -> Result<()> {
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut nodes = 0usize;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                nodes += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                nodes += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
            }
            b',' | b':' => {
                nodes += 1;
            }
            _ => {}
        }
        if depth > 32 || nodes > 4096 {
            return Err(AnimationError::Budget(
                "helper JSON structural bound".into(),
            ));
        }
    }
    Ok(())
}
fn read_packet(reader: &mut impl Read) -> Result<Packet> {
    let mut prefix = [0u8; 4];
    reader.read_exact(&mut prefix)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if size == 0 || size > MAX_JSON {
        return Err(AnimationError::Budget("helper metadata prefix".into()));
    }
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(size)
        .map_err(|_| AnimationError::Budget("helper metadata allocation".into()))?;
    metadata.resize(size, 0);
    reader.read_exact(&mut metadata)?;
    validate_json_structure(&metadata)?;
    let envelope: Envelope = serde_json::from_slice(&metadata)?;
    validate_envelope(&envelope)?;
    let mut planes = BTreeMap::new();
    for plane in &envelope.planes {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(plane.bytes)
            .map_err(|_| AnimationError::Budget("helper binary allocation".into()))?;
        bytes.resize(plane.bytes, 0);
        reader.read_exact(&mut bytes)?;
        planes.insert(plane.name.clone(), bytes);
    }
    Ok(Packet {
        envelope,
        planes,
        _outbound_storage: None,
        _scratch: None, // Reader buffers remain covered by the existing session/preload scratch.
        service: None,  // Incoming owned planes are admitted separately before escaping the reader.
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineConfig {
    heap_bytes: usize,
    backing_bytes: usize,
    frame_bytes: usize,
    json_bytes: usize,
    pending_requests: usize,
    pending_bytes: usize,
    evaluation_ms: u64,
    preparation_ms: u64,
    render_ms: u64,
    dispose_ms: u64,
}
impl From<EngineLimits> for EngineConfig {
    fn from(v: EngineLimits) -> Self {
        Self {
            heap_bytes: v.heap_bytes,
            backing_bytes: v.backing_bytes,
            frame_bytes: v.frame_bytes,
            json_bytes: v.json_bytes,
            pending_requests: v.pending_requests,
            pending_bytes: v.pending_bytes,
            evaluation_ms: v.evaluation_ms,
            preparation_ms: v.preparation_ms,
            render_ms: v.render_ms,
            dispose_ms: v.dispose_ms,
        }
    }
}
impl From<EngineConfig> for EngineLimits {
    fn from(v: EngineConfig) -> Self {
        Self {
            heap_bytes: v.heap_bytes,
            backing_bytes: v.backing_bytes,
            frame_bytes: v.frame_bytes,
            json_bytes: v.json_bytes,
            pending_requests: v.pending_requests,
            pending_bytes: v.pending_bytes,
            evaluation_ms: v.evaluation_ms,
            preparation_ms: v.preparation_ms,
            render_ms: v.render_ms,
            dispose_ms: v.dispose_ms,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceConfig {
    memory_bytes: u64,
    maximum_tasks: u32,
    cpu_seconds: u64,
}
impl From<SandboxLimits> for ResourceConfig {
    fn from(v: SandboxLimits) -> Self {
        Self {
            memory_bytes: v.memory_bytes,
            maximum_tasks: v.maximum_tasks,
            cpu_seconds: v.cpu_seconds,
        }
    }
}
impl From<ResourceConfig> for SandboxLimits {
    fn from(v: ResourceConfig) -> Self {
        Self {
            memory_bytes: v.memory_bytes,
            maximum_tasks: v.maximum_tasks,
            cpu_seconds: v.cpu_seconds,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArrayConfig {
    name: String,
    kind: String,
    elements: usize,
}
impl From<&ArraySpec> for ArrayConfig {
    fn from(v: &ArraySpec) -> Self {
        Self {
            name: v.name.clone(),
            kind: match v.kind {
                TypedArrayKind::U8 => "u8",
                TypedArrayKind::F32 => "f32",
                TypedArrayKind::U16 => "u16",
                TypedArrayKind::U32 => "u32",
            }
            .into(),
            elements: v.elements,
        }
    }
}
impl TryFrom<ArrayConfig> for ArraySpec {
    type Error = AnimationError;
    fn try_from(v: ArrayConfig) -> Result<Self> {
        Ok(Self {
            name: v.name,
            kind: match v.kind.as_str() {
                "u8" => TypedArrayKind::U8,
                "f32" => TypedArrayKind::F32,
                "u16" => TypedArrayKind::U16,
                "u32" => TypedArrayKind::U32,
                _ => return Err(invalid("unknown array element type")),
            },
            elements: v.elements,
        })
    }
}
/// Pure preflight for admitted native binary seed transport, repeated by the
/// child and engine. Does not confer rendering, source or acquisition authority.
pub fn validate_seed_planes(
    arrays: &[ArraySpec],
    planes: &BTreeMap<String, Vec<u8>>,
    maximum_bytes: usize,
) -> Result<usize> {
    if arrays.len() > MAX_PLANES || arrays.len() != planes.len() {
        return Err(AnimationError::Budget("helper seed plane count".into()));
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    for array in arrays {
        if array.name.is_empty()
            || array.name.len() > 64
            || !array
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !names.insert(&array.name)
        {
            return Err(invalid("invalid or duplicate seed array name"));
        }
        let width = match array.kind {
            TypedArrayKind::U8 => 1,
            TypedArrayKind::U16 => 2,
            TypedArrayKind::F32 | TypedArrayKind::U32 => 4,
        };
        let bytes = array
            .elements
            .checked_mul(width)
            .ok_or_else(|| invalid("seed shape overflow"))?;
        if planes.get(&array.name).map(Vec::len) != Some(bytes) {
            return Err(AnimationError::Integrity(
                "helper seed binary shape mismatch".into(),
            ));
        }
        total = total
            .checked_add(bytes)
            .ok_or_else(|| invalid("seed aggregate overflow"))?;
        if total > maximum_bytes.min(MAX_BINARY) {
            return Err(AnimationError::Budget("helper seed aggregate bytes".into()));
        }
    }
    Ok(total)
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Initialize {
        bootstrap: String,
        sandbox: ResourceConfig,
        engine: EngineConfig,
        mode: AnimationMode,
        ambient_seed: u32,
        service_wire_version: u16, // Negotiate the exact bounded binary service schema.
        service_byte_order: String, // Reject incompatible same-host typed-array byte order.
    },
    Plan {
        settings: Value,
        mode: AnimationMode,
        environment: Value,
    },
    StartCreate {
        settings: Value,
        accepted_plan: Value,
    },
    Pump,
    BindAuthority {
        authority: ActiveAuthority,
    }, // Native activation is distinct from immutable envelope identity.
    DrainRequests, // Nonacquiring one-event page of already admitted native work.
    CompleteService {
        id: u64,
        authority: ActiveAuthority,
        result: Value,
        arrays: Vec<ArrayConfig>,
    }, // Binary result planes travel outside JSON.
    CancelService {
        id: u64,
        authority: ActiveAuthority,
    }, // Signal and settle only one request without running JavaScript.
    CompleteRequest {
        id: u64,
        result: Value,
    },
    PrepareFrameSeed {
        metadata: Value,
        arrays: Vec<ArrayConfig>,
    }, // Inert native typed copy/ACK.
    ActivateFrameSeed {
        seed_id: u64,
    }, // JS hook outside native authority guard.
    DiscardFrameSeed {
        seed_id: u64,
    }, // Inert discard/detach.
    Render {
        context: Value,
        arrays: Vec<ArrayConfig>,
    },
    AcceptFrame {
        accepted: bool,
    },
    Probe,
    Dispose,
}
impl Command {
    /// Derived exclusively from the trusted outbound command, never a response
    /// phase string or helper assertion.
    #[cfg(test)]
    fn permits_acquisition(&self) -> bool {
        matches!(
            self,
            Self::StartCreate { .. } | Self::Pump // Completion and drain commands must never execute acquiring continuations.
        )
    }
}
fn validated_requests(
    payload: &Value,
    permits_acquisition: bool,
    last_id: u64,
    pending_count: usize,
    queued_count: usize,
) -> Result<Vec<RequestRecord>> {
    let Some(raw) = payload.get("requests") else {
        return Ok(Vec::new());
    };
    let records = raw
        .as_array()
        .ok_or_else(|| invalid("helper requests must be an array"))?;
    if !permits_acquisition && !records.is_empty() {
        return Err(AnimationError::PermissionDenied(
            "helper acquisition outside caller phase".into(),
        ));
    }
    if pending_count.saturating_add(records.len()) > MAX_REQUESTS
        || queued_count.saturating_add(records.len()) > MAX_REQUESTS
    {
        return Err(AnimationError::Budget("helper pending requests".into()));
    }
    let records: Vec<RequestRecord> = serde_json::from_value(raw.clone())?;
    let mut previous = last_id;
    for request in &records {
        if request.id <= previous
            || request.method.is_empty()
            || request.method.len() > 80 // Preserve the engine's exact spelling bound.
            || !request.method.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._".contains(&byte)) // Reject arbitrary native symbol spellings.
            || request.timeout_ms == 0
            || request.timeout_ms > 60_000
        {
            return Err(invalid("invalid/replayed helper host request"));
        }
        previous = request.id;
    }
    Ok(records)
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)] // Transport coordinates separately from the immutable session.
#[serde(deny_unknown_fields)] // Reject additional purported authority fields.
struct ActiveAuthority {
    instance_id: u64,
    plan_generation: u64,
    authorization_epoch: u64,
} // These coordinates never replace a broker channel.
impl From<ServiceAuthority> for ActiveAuthority {
    // Serialize only native activation state.
    fn from(value: ServiceAuthority) -> Self {
        Self {
            instance_id: value.instance_id,
            plan_generation: value.plan_generation,
            authorization_epoch: value.authorization_epoch,
        }
    } // Preserve all three coordinates.
} // End native stamp encoding.
impl From<ActiveAuthority> for ServiceAuthority {
    // Decode before comparing with native state.
    fn from(value: ActiveAuthority) -> Self {
        Self {
            instance_id: value.instance_id,
            plan_generation: value.plan_generation,
            authorization_epoch: value.authorization_epoch,
        }
    } // Decoding supplies no permission.
} // End stamp decoding.
#[derive(Debug, Serialize, Deserialize)] // Preserve exact request metadata independently of binary planes.
#[serde(deny_unknown_fields)] // Reject undeclared wire fields.
struct RequestRecord {
    // One record occupies one bounded response page.
    id: u64,                    // Correlate within the immutable helper session.
    method: String,             // The engine validated the closed spelling grammar.
    payload: Value,             // Canonical binary markers remain structural data.
    timeout_ms: u64,            // Remaining monotonic time, never a refreshed original timeout.
    authority: ActiveAuthority, // Capture the accepted native activation.
    phase: String,              // Compare against the parent's actual acquiring command.
    arrays: Vec<ArrayConfig>,   // Plane names are canonical within this single request.
} // End a service request record.
impl From<&HostRequest> for RequestRecord {
    // Borrow every independently admitted request allocation.
    fn from(value: &HostRequest) -> Self {
        // Metadata staging is covered by retained pipe scratch.
        Self {
            id: value.id,
            method: value.method.clone(),
            payload: value.payload.metadata().clone(),
            timeout_ms: value.remaining_ms(),
            authority: value.authority.into(),
            phase: phase_name(value.phase).into(),
            arrays: value
                .payload
                .arrays()
                .iter()
                .map(ArrayConfig::from)
                .collect(),
        } // Binary bytes remain borrowed from ServiceValue.
    } // End bounded wire metadata construction.
} // No uncharged bulk copy occurs here.
fn phase_name(phase: ServicePhase) -> &'static str {
    // Encode the native acquisition phase explicitly.
    match phase {
        ServicePhase::Create => "create",
        ServicePhase::Async => "async",
    } // No script field selects the parent's phase.
} // End phase encoding.
#[derive(Debug, Serialize, Deserialize)] // Send native terminal correlation without source payload copies.
#[serde(deny_unknown_fields)] // Keep cancellation inventory closed.
struct TerminalRecord {
    id: u64,
    authority: ActiveAuthority,
} // Only previously issued IDs are sent to the parent.
#[derive(Debug, Serialize, Deserialize)] // Distinguish ordinary refusal from terminal engine failure.
#[serde(deny_unknown_fields)] // Reject ambiguous completion acknowledgements.
struct CompletionRecord {
    id: u64,
    authority: ActiveAuthority,
    state: String,
    error: Option<String>,
} // An ACK reports delivery, never native-effect rollback.
fn completion_record(
    id: u64,
    authority: ServiceAuthority,
    result: Result<CompletionState>,
) -> CompletionRecord {
    // Encode bounded native completion outcomes.
    let (state, error) = match result {
        // Preserve ordinary prepublication refusal without retiring peers.
        Ok(CompletionState::Delivered) => ("delivered", None), // The child copied and resolved without a checkpoint.
        Ok(CompletionState::Unknown) => ("unknown", None),     // Duplicate correlation is nonfatal.
        Ok(CompletionState::TimedOut) => ("timeout", None), // Native monotonic deadline won publication.
        Ok(CompletionState::Cancelled) => ("cancelled", None), // Cancellation withholds delivery only.
        Err(error) => (
            "refused",
            Some(error.to_string().chars().take(2048).collect()),
        ), // Keep pending custody when the valid engine refuses a copy.
    }; // A poisoned engine is handled by the outer terminal path.
    CompletionRecord {
        id,
        authority: authority.into(),
        state: state.into(),
        error,
    } // Bind the ACK to the actual native request.
} // End native completion encoding.
fn completed_state(
    record: CompletionRecord,
    id: u64,
    authority: ServiceAuthority,
) -> Result<CompletionState> {
    // Validate an authenticated child ACK before releasing custody.
    if record.id != id || record.authority != ActiveAuthority::from(authority) {
        return Err(invalid("completion acknowledgement identity"));
    } // Refuse cross-request or stale acknowledgements.
    match record.state.as_str() {
        // Keep ordinary terminal states explicit.
        "delivered" if record.error.is_none() => Ok(CompletionState::Delivered), // Successful protected publication is complete.
        "unknown" if record.error.is_none() => Ok(CompletionState::Unknown), // No matching resolver remains.
        "timeout" if record.error.is_none() => Ok(CompletionState::TimedOut), // Deadline cancellation is terminal.
        "cancelled" if record.error.is_none() => Ok(CompletionState::Cancelled), // Explicit cancellation is terminal.
        "refused"
            if record
                .error
                .as_ref()
                .is_some_and(|message| !message.is_empty() && message.len() <= 8192) =>
        {
            Err(AnimationError::Budget(record.error.unwrap_or_default()))
        } // Preserve a bounded delivery refusal without claiming native work failed.
        _ => Err(invalid("invalid completion acknowledgement")), // Reject unknown protocol states.
    } // The caller retires only malformed ACKs, not ordinary refusal.
} // End acknowledgement decoding.
/// Result plus independent storage custody. The parent carries this admission
/// into its immutable presentation owner if the helper/session retires earlier.
pub struct RetainedHelperFrame {
    pub output: RenderOutput,
    _storage: StorageAdmission,
}
impl RetainedHelperFrame {
    pub fn into_parts(self) -> (RenderOutput, StorageAdmission) {
        (self.output, self._storage)
    }
}

/// Separate informational diagnostics, never Surface metadata or host requests.
pub struct RetainedHelperStatus {
    pub value: Value,
    _storage: StorageAdmission,
}
impl RetainedHelperStatus {
    pub fn into_parts(self) -> (Value, StorageAdmission) {
        (self.value, self._storage)
    }
}
fn validate_status(value: &Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| invalid("status object"))?;
    if object.len() != 2
        || !object.contains_key("records")
        || !object.contains_key("dropped")
        || object["dropped"].as_u64().is_none()
    {
        return Err(invalid("status schema"));
    }
    let records = object["records"]
        .as_array()
        .ok_or_else(|| invalid("status records"))?;
    if records.len() > 64 {
        return Err(AnimationError::Budget("helper status records".into()));
    }
    let mut bytes = 0usize;
    for record in records {
        let fields = record.as_object().ok_or_else(|| invalid("status record"))?;
        if fields
            .keys()
            .any(|name| !["kind", "level", "message", "percent"].contains(&name.as_str()))
            || !matches!(
                fields.get("kind").and_then(Value::as_str),
                Some("log" | "progress")
            )
            || !matches!(
                fields.get("level").and_then(Value::as_str),
                Some("debug" | "info" | "warning" | "error")
            )
        {
            return Err(invalid("status record schema"));
        }
        let message = fields
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("status message"))?;
        if message.len() > 4096 {
            return Err(AnimationError::Budget("helper status message".into()));
        }
        if let Some(percent) = fields.get("percent") {
            if !percent
                .as_f64()
                .is_some_and(|p| p.is_finite() && (0.0..=100.0).contains(&p))
            {
                return Err(invalid("status percent"));
            }
        }
        bytes = bytes.saturating_add(serde_json::to_vec(record)?.len() + 1);
        if bytes > 16384 {
            return Err(AnimationError::Budget("helper status bytes".into()));
        }
    }
    Ok(())
}
/// Nonclone native copy receipt. IDs alone cannot construct or transfer it.
pub struct PreparedSeed {
    issuer: Arc<()>,
    seed_id: u64,
    authority: ServiceAuthority,
    abandoned: Arc<std::sync::atomic::AtomicBool>,
}
impl PreparedSeed {
    pub(crate) fn authority(&self) -> ServiceAuthority {
        self.authority
    }
}
impl Drop for PreparedSeed {
    fn drop(&mut self) {
        self.abandoned
            .store(true, std::sync::atomic::Ordering::Release);
    } // Owner notices abandonment before its next command; no destructor IPC/worker.
}
struct PendingSeed {
    issuer: Arc<()>,
    seed_id: u64,
    authority: ServiceAuthority,
    abandoned: Arc<std::sync::atomic::AtomicBool>,
}
pub struct HelperSession {
    authority: HelperAuthority,
    helper_build_digest: [u8; 32],
    child: Option<SandboxChild>, // Keep the original process owner available for conservative failed-join quarantine.
    writer: SyncSender<Option<Packet>>,
    reader: Receiver<Result<Packet>>,
    workers: Vec<OwnedWorker>,
    sequence: u64,
    requests: Vec<HostRequest>,
    status: Option<RetainedHelperStatus>,
    pending: BTreeMap<u64, HostRequest>, // Correlation retains immutable input and its original-root admission.
    cancelled: VecDeque<HostRequest>,    // Terminal inventory survives helper-only retirement.
    active: Option<ServiceAuthority>, // Native activation is independent from the immutable IPC session.
    creation: Option<CreateState>,    // Derive each pump's actual native acquiring phase.
    service_budget: Arc<ServiceBudget>, // Preserve configured pending count/bytes through escaped aliases.
    last_request_id: u64,
    limits: HelperLimits,
    quota: QuotaGroup,
    _storage: Option<Arc<StorageAdmission>>, // Release pipe scratch only after actual worker retirement.
    _physical: Option<Arc<WorkerAdmission>>, // Preserve the original physical sandbox admission independently.
    closed: bool,
    retirement_failed: bool, // A failed prior join cannot turn into later optimistic success.
    physically_retired: bool, // Set only after child shutdown and every original worker join succeed.
    native_publication: bool, // Never join workers inside serialized broker publication.
    deferred_retirement: bool, // Uncertain transport keeps original custody until scope exits.
    pending_seed: Option<PendingSeed>,
    #[cfg(test)]
    test_transport: Arc<HelperTransportTestControl>,
}
/// Initial playback choices, passed unchanged into the helper initialization.
#[derive(Debug, Clone)]
pub struct HelperPlayback {
    pub mode: AnimationMode,
    pub ambient_seed: u32,
}

impl HelperSession {
    pub fn launch(
        executable: &Path,
        archive: &[u8],
        bootstrap: &str,
        authority: HelperAuthority,
        limits: HelperLimits,
        quota: QuotaGroup,
        playback: HelperPlayback,
    ) -> Result<Self> {
        let HelperPlayback { mode, ambient_seed } = playback;
        authority.validate()?;
        if limits.engine.pending_requests == 0
            || limits.engine.pending_requests > MAX_REQUESTS
            || limits.engine.pending_bytes == 0
            || limits.engine.pending_bytes > 8 * 1024 * 1024
            || limits.engine.json_bytes == 0
            || limits.engine.json_bytes > MAX_JSON
        {
            return Err(AnimationError::Budget(
                "helper configured service bounds".into(),
            ));
        } // Parent admission uses actual configured limits, never transport maxima.
        if archive.len() > MAX_BINARY
            || bootstrap.len() > 192 * 1024
            || limits.operation_timeout.is_zero()
            || limits.operation_timeout > Duration::from_secs(60)
        {
            return Err(AnimationError::Budget("helper startup/timeout".into()));
        }
        // Parent-root debit precedes process/thread/buffer creation. The helper's
        // separate ledger does not replace this application's physical custody.
        let worker_count = limits.sandbox.maximum_tasks as usize + 2;
        let physical = usize::try_from(limits.sandbox.memory_bytes)
            .map_err(|_| AnimationError::Budget("helper memory limit".into()))?
            .checked_add(4 * 1024 * 1024)
            .ok_or_else(|| invalid("helper worker size overflow"))?;
        let admission = Arc::new(
            quota
                .reserve_external_worker(worker_count, physical)
                .map_err(|e| AnimationError::Budget(format!("helper worker admission: {e:?}")))?,
        );
        let storage = Arc::new(
            quota
                .reserve_external_storage(
                    2 * MAX_BINARY + 128 * MAX_JSON + limits.engine.frame_bytes,
                )
                .map_err(|e| AnimationError::Budget(format!("helper pipe admission: {e:?}")))?,
        );
        let child = animation_sandbox::spawn_helper(executable, limits.sandbox)?; // Physical admission preceded the original native process creation.
        let cancel = child.cancel_handle(); // Capture the supplied native cancellation handle.
        let (writer, writes) = mpsc::sync_channel::<Option<Packet>>(1); // Preserve one bounded outbound queue.
        let (responses, reader) = mpsc::sync_channel::<Result<Packet>>(1); // Preserve one response for each command.
        let service_budget = ServiceBudget::new(&limits.engine); // This occupancy fence debits no independent quota bank.
        #[cfg(test)]
        let test_transport = Arc::new(HelperTransportTestControl::default());
        let mut session = Self {
            // Own native startup before any further fallible operation.
            authority, // Keep immutable session identity independent from accepted activation.
            helper_build_digest: [0; 32], // Filled only from the actual loaded helper's sealed startup response.
            child: Some(child), // Preserve original ownership for actual terminal verification.
            writer,             // Retain the original bounded command sender.
            reader,             // Retain the original bounded response receiver.
            workers: Vec::new(), // Own every later worker before another fallible startup step.
            sequence: 0,        // Initialization owns sequence zero.
            requests: Vec::new(), // No parent request has been issued yet.
            status: None,       // No retained diagnostic snapshot exists.
            pending: BTreeMap::new(), // No admitted parent service requests exist yet.
            cancelled: VecDeque::new(), // Start with no terminal notifications.
            active: None,       // Native consent must bind before seed or creation.
            creation: None,     // No acquiring phase has started.
            service_budget,     // All escaped requests retain this same bounded fence.
            last_request_id: 0, // No native request identity has been observed.
            limits,             // Preserve the original configured engine and sandbox ceilings.
            quota,              // Retain the original parent quota root.
            _storage: Some(Arc::clone(&storage)), // Hold original pipe scratch through actual shutdown.
            _physical: Some(Arc::clone(&admission)), // Keep the original physical worker debit through failed joins.
            closed: false,                           // The new session can accept initialization.
            retirement_failed: false,                // No physical retirement attempt has failed.
            physically_retired: false,
            native_publication: false,
            pending_seed: None,
            deferred_retirement: false, // Launch is not physical retirement.
            #[cfg(test)]
            test_transport: Arc::clone(&test_transport),
        }; // All later startup failures now use one retirement owner.
        let input = session
            .child
            .as_mut()
            .ok_or_else(|| invalid("helper child missing"))?
            .take_stdin()
            .ok_or_else(|| invalid("missing helper stdin"))?; // Startup failure now runs the same proven-retirement-or-quarantine owner.
        let output = session
            .child
            .as_mut()
            .ok_or_else(|| invalid("helper child missing"))?
            .take_stdout()
            .ok_or_else(|| invalid("missing helper stdout"))?; // Keep child and all original guards owned before any fallible pipe extraction.
        let wake = cancel.clone();
        let hold = Arc::clone(&admission);
        let storage_hold = Arc::clone(&storage);
        let sender = session.writer.clone(); // Wake the original bounded writer queue during native cancellation.
        #[cfg(test)]
        let test_writer = Arc::clone(&test_transport);
        session.workers.push(owned_worker::spawn_owned(
            // Session ownership protects every subsequent startup failure.
            "ilium-plugin-pipe-write",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                let _custody = (&hold, &storage_hold);
                let _ = wake.terminate();
                let _ = sender.try_send(None);
            },
            move |stop| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                let mut input = input;
                while !stop.is_stopped() {
                    match writes.recv() {
                        Ok(Some(packet)) => {
                            #[cfg(not(test))]
                            if write_packet(&mut input, &packet).is_err() {
                                break;
                            }
                            #[cfg(test)]
                            {
                                let sequence = packet.envelope.sequence;
                                let service_plane_pointer = packet
                                    .service
                                    .as_ref()
                                    .and_then(|value| value.planes().get("b0"))
                                    .map(|bytes| bytes.as_ptr() as usize);
                                let write_result = write_packet(&mut input, &packet);
                                drop(packet);
                                test_writer.packet_released(
                                    sequence,
                                    write_result.is_ok(),
                                    service_plane_pointer,
                                );
                                if write_result.is_err() {
                                    break;
                                }
                            }
                        }
                        _ => break,
                    }
                }
            },
        )?);
        let wake = cancel.clone();
        let hold = Arc::clone(&admission);
        let storage_hold = Arc::clone(&storage);
        #[cfg(test)]
        let test_reader = Arc::clone(&test_transport);
        let reader_worker = owned_worker::spawn_owned(
            "ilium-plugin-pipe-read",
            WorkerKind::SynchronousIo,
            StopToken::default(),
            move || {
                let _custody = (&hold, &storage_hold);
                let _ = wake.terminate();
            },
            move |stop| {
                ilium_platform::thread_priority::lower_current_thread(
                    ilium_platform::thread_priority::WorkerPriority::BelowNormal,
                );
                let mut output = output;
                while !stop.is_stopped() {
                    let packet = read_packet(&mut output);
                    #[cfg(test)]
                    if let Ok(response) = &packet {
                        if test_reader.intercept_response(response.envelope.sequence) {
                            let _ = responses.try_send(Err(invalid(
                                "test interrupted CompleteService acknowledgement after packet release",
                            )));
                            break;
                        }
                    }
                    let failed = packet.is_err();
                    if responses.try_send(packet).is_err() || failed {
                        break;
                    }
                }
            },
        );
        match reader_worker {
            Ok(worker) => session.workers.push(worker), // Keep the actual worker inside the proven-retirement owner.
            Err(error) => {
                let _ = cancel.terminate();
                for worker in &session.workers {
                    // Cancel all already owned startup workers before Drop verifies their actual exit.
                    worker.ticket().cancel();
                }
                return Err(error.into());
            }
        }
        let mut planes = BTreeMap::new();
        planes.insert("archive".into(), archive.to_vec());
        let payload = serde_json::to_value(Command::Initialize {
            bootstrap: bootstrap.into(),
            sandbox: session.limits.sandbox.into(),
            engine: session.limits.engine.clone().into(),
            mode,
            ambient_seed,
            service_wire_version: engine::SERVICE_WIRE_VERSION, // Negotiate the concrete raw-data service ABI.
            service_byte_order: engine::SERVICE_BYTE_ORDER.into(), // Preserve exact same-host typed-array interpretation.
        })?;
        let mut packet = Packet::new(0, session.authority.clone(), "command", payload, planes); // Initial package bytes remain inside original pipe scratch.
        packet._scratch = session._storage.clone(); // Retain staging even when startup transport escapes during failure.
        packet_metadata(&packet)?; // Reject oversized initialization before writing any prefix.
        let response = session.exchange(packet, false, session.operation_deadline()?)?; // Startup retains the original one-response exchange.
        if response.envelope.payload.get("ready") != Some(&Value::Bool(true))
            || response.envelope.payload.get("service_wire_version").and_then(Value::as_u64) != Some(u64::from(engine::SERVICE_WIRE_VERSION)) // Reject an incompatible helper binary schema.
            || response.envelope.payload.get("service_byte_order").and_then(Value::as_str) != Some(engine::SERVICE_BYTE_ORDER) // Never silently reinterpret native typed-array bytes.
            || !response.planes.is_empty()
        {
            return Err(invalid("helper did not seal before loading package"));
        }
        session.helper_build_digest = parse_helper_digest(
            response
                .envelope
                .payload
                .get("helper_build_sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("helper build digest missing"))?,
        )?;
        Ok(session)
    }
    pub(crate) fn build_digest(&self) -> [u8; 32] {
        self.helper_build_digest
    }
    pub fn resource_usage(&self) -> std::io::Result<animation_sandbox::SandboxUsage> {
        self.child
            .as_ref()
            .ok_or_else(|| std::io::Error::other("helper is physically retired"))?
            .resource_usage() // Retired helpers have no live process resource sample.
    }
    /// Actual immutable per-instance limits after manifest/native constraints.
    pub(crate) fn engine_limits(&self) -> &crate::engine::EngineLimits {
        &self.limits.engine
    }
    pub fn authority(&self) -> &HelperAuthority {
        &self.authority
    }
    fn exchange(
        &mut self,
        packet: Packet,
        service_page: bool,
        deadline: Instant,
    ) -> Result<Packet> {
        // One committed command receives exactly one bounded response.
        if self.closed {
            return Err(invalid("session retired"));
        } // Logical closure never proves physical retirement.
        let sequence = packet.envelope.sequence; // Capture immutable correlation before transferring custody.
        if self.writer.try_send(Some(packet)).is_err() {
            let _ = self.cancel();
            return Err(invalid("helper command queue unavailable"));
        } // A committed sequence cannot remain live after a failed send.
        let mut response = match self
            .reader
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            // Bound the whole acquiring/drain transaction.
            Ok(Ok(response)) => response, // The reader validated envelope size before allocation.
            Ok(Err(error)) => {
                let _ = self.cancel();
                return Err(error);
            } // Broken transport invalidates uncertain delivery.
            Err(_) => {
                let _ = self.cancel();
                return Err(invalid("helper response deadline/EOF"));
            } // Notification and EOF are not physical-retirement certificates.
        }; // Parent pipe scratch remains held throughout validation.
        response._scratch = self._storage.clone(); // Keep this received allocation charged even if validation triggers successful physical retirement.
        if response.envelope.sequence != sequence
            || response.envelope.authority != self.authority
            || !["response", "error"].contains(&response.envelope.kind.as_str())
        {
            // Compare the immutable session on every response.
            let _ = self.cancel();
            return Err(AnimationError::PermissionDenied(
                "helper response authority/sequence mismatch".into(),
            )); // Never accept script-supplied activation as session identity.
        } // Continue only on this committed command's response.
        if response.envelope.kind == "error" {
            // Terminal child faults retain the existing retirement path.
            let message = response
                .envelope
                .payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("helper failed")
                .chars()
                .take(2048)
                .collect::<String>(); // Bound diagnostic retention.
            let _ = self.cancel();
            return Err(invalid(&message)); // Preserve physical failure separately from this transport error.
        } // Ordinary completion refusal uses a normal response instead.
        if !service_page
            && validated_requests(
                &response.envelope.payload,
                false,
                self.last_request_id,
                self.pending.len(),
                self.requests.len(),
            )
            .is_err()
        {
            // Fresh requests are legal only in caller-owned drain transactions.
            let _ = self.cancel();
            return Err(invalid("unexpected helper service records")); // Render, seed, completion and acknowledgement cannot acquire.
        } // Drain validation additionally checks active stamp, phase, shape and quota.
        Ok(response) // Response ownership remains inside the session's admitted pipe scratch.
    } // End one-response exchange.
    fn operation_deadline(&self) -> Result<Instant> {
        // Share one timeout across all bounded drain pages.
        Instant::now()
            .checked_add(self.limits.operation_timeout)
            .ok_or_else(|| invalid("operation deadline overflow")) // Never refresh a transaction on each page.
    } // End transaction deadline construction.
    fn command(&mut self, command: Command) -> Result<Packet> {
        // Keep existing nonbinary native callers concise.
        self.command_at(
            command,
            BTreeMap::new(),
            None,
            None,
            self.operation_deadline()?,
        ) // Every command retains staging storage.
    } // End the no-plane command adapter.
    fn command_with_planes(
        &mut self,
        command: Command,
        planes: BTreeMap<String, Vec<u8>>,
        admission: Option<Arc<StorageAdmission>>,
    ) -> Result<Packet> {
        // Preserve the existing trusted seed caller shape.
        self.command_at(command, planes, admission, None, self.operation_deadline()?)
        // Seed copies keep their separate admission until write completion.
    } // End seed command adaptation.
    fn command_at(
        &mut self,
        command: Command,
        planes: BTreeMap<String, Vec<u8>>,
        admission: Option<Arc<StorageAdmission>>,
        service: Option<ServiceValue>,
        deadline: Instant,
    ) -> Result<Packet> {
        // Preflight before consuming sequence or queue ownership.
        if self.closed {
            return Err(invalid("session retired"));
        } // Do not create packets after closure.
        if let Some(seed) = &self.pending_seed {
            if seed.abandoned.load(std::sync::atomic::Ordering::Acquire) {
                let _ = self.cancel();
                return Err(invalid(
                    "native seed receipt abandoned; helper retirement required",
                ));
            }
            return Err(invalid(
                "native seed receipt must be activated or discarded first",
            ));
        }
        let service_page = matches!(&command, Command::DrainRequests); // Only native page commands can carry new service records.
        let returned_binary = service_page || matches!(&command, Command::Render { .. }); // Keep service planes separate from drawing planes.
        if !planes.is_empty() && !matches!(&command, Command::PrepareFrameSeed { .. }) {
            return Err(invalid("binary outside seed command"));
        } // Owned maps are seed-only.
        if service.is_some() && !matches!(&command, Command::CompleteService { .. }) {
            return Err(invalid("service planes outside completion"));
        } // Completion borrows immutable admitted planes.
        #[cfg(test)]
        let test_complete_service = matches!(&command, Command::CompleteService { .. });
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("helper sequence exhausted"))?; // Reserve correlation without publishing it yet.
        let mut packet = Packet::new(
            sequence,
            self.authority.clone(),
            "command",
            serde_json::to_value(command)?,
            planes,
        ); // Stage bounded command metadata.
        if let Some(service) = service {
            packet.attach_service(service)?;
        } // Attach custody without cloning binary allocations.
        packet._outbound_storage = admission; // Preserve any independently admitted seed copy.
        packet._scratch = self._storage.clone(); // Keep metadata and serialization scratch through escaped writers.
        packet_metadata(&packet)?; // Reject whole-envelope limits before touching the sequence or pipe.
        if Instant::now() >= deadline {
            return Err(invalid("helper transaction deadline"));
        } // Refuse an unissued command without a sequence gap.
        #[cfg(test)]
        if test_complete_service {
            self.test_transport.register_complete_service(sequence);
        }
        self.sequence = sequence; // Commit only a completely validated packet.
        let response = self.exchange(packet, service_page, deadline)?; // All uncertain send/read failures retire the transport.
        if !returned_binary && !response.planes.is_empty() {
            let _ = self.cancel();
            return Err(invalid("unexpected response binary"));
        } // Prevent data smuggling outside the declared command.
        Ok(response) // The caller validates the command-specific payload.
    } // End exact native command framing.
    pub fn bind_service_authority(&mut self, authority: ServiceAuthority) -> Result<()> {
        // Bind from the parent's actual private activation channel.
        authority.validate()?; // Reject uninitialized activation coordinates.
        if authority.instance_id != self.authority.instance_id || self.creation.is_some() {
            return Err(invalid("service activation instance/lifecycle"));
        } // Do not replace a running instance's principal.
        if self.active.is_some_and(|active| active != authority) {
            return Err(invalid(
                "service activation replacement requires a new helper",
            ));
        } // Epoch changes require explicit retirement/recreation.
        self.command(Command::BindAuthority {
            authority: authority.into(),
        })?; // The immutable envelope continues carrying the original launch stamp.
        self.active = Some(authority); // Store only an acknowledged native binding.
        Ok(()) // This is not a PermissionBroker grant.
    } // End active stamp binding.
    fn require_active(&self, authority: ServiceAuthority) -> Result<()> {
        // Revalidate native helper activation before request/result operations.
        if self.closed || self.active != Some(authority) {
            return Err(AnimationError::PermissionDenied(
                "stale helper service activation".into(),
            ));
        } // Accepted-plan JSON cannot restore a retired binding.
        Ok(()) // The runtime separately checks its current private broker channel.
    } // End helper-local activation check.
    fn forget_pending(&mut self, id: u64, cancelled: bool) {
        // Release only this pending-map owner after a terminal ACK.
        self.requests.retain(|request| request.id != id); // Suppress any unissued parent queue entry.
        if let Some(request) = self.pending.remove(&id) {
            // Escaped immutable clones keep their own shared custody.
            if cancelled {
                request.stop_token().stop();
                self.cancelled.push_back(request);
            } // Retain cancellation inventory until the actual native owner drains it.
        } // Successful native delivery needs no cancellation notification.
    } // Map removal never proves native body exit.
    fn cancel_remote(
        &mut self,
        id: u64,
        authority: ServiceAuthority,
        deadline: Instant,
    ) -> Result<CompletionState> {
        // Cancel an issued or not-yet-admitted child request.
        self.require_active(authority)?; // Never send a cancellation under stale coordinates.
        let packet = self.command_at(
            Command::CancelService {
                id,
                authority: authority.into(),
            },
            BTreeMap::new(),
            None,
            None,
            deadline,
        )?; // This command performs no JavaScript or checkpoint.
        let result = self.read_completion(packet, id, authority)?; // Verify native terminal correlation before mutating custody.
        self.forget_pending(id, true); // Cancellation signals native owners even if the child already discarded its resolver.
        Ok(result) // Native work retains its own actual outcome and quota.
    } // End authenticated cancellation delivery.
    pub fn cancel_request(
        &mut self,
        id: u64,
        authority: ServiceAuthority,
    ) -> Result<CompletionState> {
        // Expose individual cancellation to the native operation owner.
        self.require_active(authority)?; // A stale caller cannot cancel current work.
        if !self.pending.contains_key(&id) {
            return Ok(CompletionState::Unknown);
        } // Harmless duplicate cancellation does not retire peers.
        self.cancel_remote(id, authority, self.operation_deadline()?) // Retain pending custody through the actual child ACK.
    } // End native cancellation API.
    fn read_completion(
        &mut self,
        packet: Packet,
        id: u64,
        authority: ServiceAuthority,
    ) -> Result<CompletionState> {
        // Separate expected copy refusal from protocol failure.
        let result = packet
            .envelope
            .payload
            .get("completion")
            .cloned()
            .ok_or_else(|| invalid("missing completion acknowledgement"))
            .and_then(|value| {
                serde_json::from_value::<CompletionRecord>(value).map_err(AnimationError::from)
            })
            .and_then(|record| completed_state(record, id, authority)); // Validate the entire closed ACK schema.
        match result {
            // Only ordinary bounded refusal keeps the transport and pending entry live.
            Ok(state) => Ok(state), // Return an explicit terminal state.
            Err(error @ AnimationError::Budget(_)) => Err(error), // The caller still owns the actual native outcome.
            Err(error) => {
                let _ = self.cancel();
                Err(error)
            } // Malformed or uncorrelated publication retires the session.
        } // No checkpoint or script diagnostics occur in this path.
    } // End completion response validation.
    fn synchronize_cancellations(&mut self, deadline: Instant) -> Result<()> {
        // Forward parent deadlines and actual stop tokens before another acquisition pump.
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Use the accepted native coordinates.
        let stopped: Vec<_> = self
            .pending
            .values()
            .filter(|request| request.is_cancelled())
            .map(|request| request.id)
            .collect(); // The retained request cap bounds this inventory.
        for id in stopped {
            self.cancel_remote(id, authority, deadline)?;
        } // Do not treat local token notification as child settlement.
        Ok(()) // A later separately authorized pump runs terminal continuations.
    } // End bounded parent cancellation forwarding.
    fn drain_services(&mut self, phase: ServicePhase, deadline: Instant) -> Result<()> {
        // Page one request or one terminal event under the original acquiring transaction.
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Preserve native activation throughout this bounded drain.
        let maximum_pages = self
            .limits
            .engine
            .pending_requests
            .checked_mul(2)
            .and_then(|count| count.checked_add(2))
            .ok_or_else(|| invalid("service drain bound overflow"))?; // Each retained request can produce at most an issue and a terminal page.
        for _ in 0..maximum_pages {
            // Never turn helper paging into an unbounded polling loop.
            let started = Instant::now(); // Deduct the entire exchange and parent copy from transmitted remaining time.
            let packet = self.command_at(
                Command::DrainRequests,
                BTreeMap::new(),
                None,
                None,
                deadline,
            )?; // Draining cannot execute JavaScript or acquire new work.
            let more = packet
                .envelope
                .payload
                .get("more")
                .and_then(Value::as_bool)
                .ok_or_else(|| invalid("service page continuation"))?; // Require an explicit bounded continuation flag.
            let records =
                validated_requests(&packet.envelope.payload, true, self.last_request_id, 0, 0)?; // Validate correlation and spelling before any admission.
            let terminal = packet
                .envelope
                .payload
                .get("terminal")
                .filter(|value| !value.is_null()); // Terminal pages carry no binary payload.
            if records.len() > 1 || (!records.is_empty() && terminal.is_some()) {
                return Err(invalid("service page must contain one event"));
            } // Prevent aggregate planes and ambiguous event order.
            if let Some(value) = terminal {
                // Only already-issued native requests need parent terminal notification.
                let record: TerminalRecord = serde_json::from_value(value.clone())?; // The closed schema excludes fabricated source data.
                if !packet.planes.is_empty()
                    || record.id == 0
                    || record.id > self.last_request_id
                    || record.authority != ActiveAuthority::from(authority)
                {
                    return Err(invalid("service terminal identity"));
                } // Unissued child cancellations never cross this protocol.
                self.forget_pending(record.id, true); // Duplicate known terminal IDs remain harmless.
            } else if let Some(record) = records.into_iter().next() {
                // Admit exactly one complete request from this native phase.
                if record.authority != ActiveAuthority::from(authority)
                    || record.phase != phase_name(phase)
                    || record.timeout_ms > self.limits.engine.preparation_ms
                {
                    return Err(invalid("service request activation/phase/deadline"));
                } // Never trust a child phase string independently.
                let id = record.id; // Advance the native high-water mark even if parent admission refuses this request.
                let arrays = record
                    .arrays
                    .into_iter()
                    .map(ArraySpec::try_from)
                    .collect::<Result<Vec<_>>>()?; // Revalidate the exact four-type inventory.
                let copied = ServiceValue::copy_request_from_host(
                    &record.payload,
                    &arrays,
                    &packet.planes,
                    &self.limits.engine,
                    self.quota.clone(),
                    &self.service_budget,
                ); // Original-root admission precedes every retained binary copy.
                self.last_request_id = id; // This issued correlation can never be admitted again.
                let elapsed = started
                    .elapsed()
                    .as_millis()
                    .saturating_add(1)
                    .min(u64::MAX as u128) as u64; // Round conservatively, including all parent validation/copy time.
                let remaining = record.timeout_ms.saturating_sub(elapsed); // A received deadline may shrink but never restart.
                let payload = match copied {
                    // Ordinary parent admission refusal cancels only this unissued native request.
                    Ok(payload) if remaining > 0 => payload, // Transfer already admitted immutable custody.
                    Ok(_) | Err(AnimationError::Budget(_)) => {
                        self.cancel_remote(id, authority, deadline)?;
                        continue;
                    } // Read a fresh page after cancellation, including any newly generated terminal events.
                    Err(error) => return Err(error), // Malformed service structure is a protocol fault.
                }; // Temporary rejected copies release their original-root admission here.
                let request = HostRequest::from_transport(
                    id,
                    record.method,
                    remaining,
                    self.authority.package_digest.clone(),
                    authority,
                    phase,
                    payload,
                )?; // The session supplies immutable package identity.
                self.pending.insert(id, request.clone()); // Preserve the request until an actual terminal acknowledgement.
                self.requests.push(request); // Publish one complete immutable request to the native owner.
            } else if more || !packet.planes.is_empty() {
                return Err(invalid("service page made no progress"));
            } // Empty pages cannot request another spin.
            if !more {
                return Ok(());
            } // The acquiring transaction is completely drained.
        } // An unbounded or replaying child cannot hold the parent in a drain loop.
        Err(invalid("service drain exceeded retained request bound")) // The caller retires this malformed transaction.
    } // No page executes JavaScript or extends native deadlines.
    /// Inert source copy/ACK, suitable only inside native publication scope.
    pub fn prepare_frame_seed(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<PreparedSeed> {
        if self.pending_seed.is_some() {
            return Err(invalid("previous native seed receipt still pending"));
        }
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Only a bound native activation can seed the trusted bootstrap.
        self.require_active(authority)?; // This local check supplements the runtime's current private channel check.
        let bytes = validate_seed_planes(arrays, planes, self.limits.engine.frame_bytes)?;
        let admission = Arc::new(self.quota.reserve_external_storage(bytes.max(1)).map_err(
            |e| AnimationError::Budget(format!("helper outbound seed admission: {e:?}")),
        )?);
        let mut owned = BTreeMap::new();
        for (name, data) in planes {
            let mut copy = Vec::new();
            copy.try_reserve_exact(data.len())
                .map_err(|_| invalid("seed allocation"))?;
            copy.extend_from_slice(data);
            owned.insert(name.clone(), copy);
        }
        let response = self.command_with_planes(
            Command::PrepareFrameSeed {
                metadata: metadata.clone(),
                arrays: arrays.iter().map(ArrayConfig::from).collect(),
            },
            owned,
            Some(admission.clone()),
        )?;
        if !response.planes.is_empty() {
            let _ = self.cancel();
            return Err(invalid("seed response contains binary"));
        }
        let seed_id = match response
            .envelope
            .payload
            .get("seed_id")
            .and_then(Value::as_u64)
            .filter(|id| *id != 0)
        {
            Some(id) => id,
            None => {
                let _ = self.cancel();
                return Err(invalid("native seed copy ACK missing id"));
            }
        };
        if response.envelope.payload.get("copied") != Some(&Value::Bool(true)) {
            let _ = self.cancel();
            return Err(invalid("native seed copy ACK incomplete"));
        }
        let issuer = Arc::new(());
        let abandoned = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.pending_seed = Some(PendingSeed {
            issuer: Arc::clone(&issuer),
            seed_id,
            authority,
            abandoned: Arc::clone(&abandoned),
        });
        drop(admission);
        Ok(PreparedSeed {
            issuer,
            seed_id,
            authority,
            abandoned,
        })
    }
    fn check_seed(&self, seed: &PreparedSeed) -> Result<()> {
        let pending = self
            .pending_seed
            .as_ref()
            .ok_or_else(|| invalid("native seed receipt missing"))?;
        if !Arc::ptr_eq(&pending.issuer, &seed.issuer)
            || pending.seed_id != seed.seed_id
            || pending.authority != seed.authority
        {
            return Err(invalid("foreign or stale native seed receipt"));
        }
        self.require_active(seed.authority)
    }
    /// Execute trusted JS hook ONLY outside native publication scope/guard.
    pub fn activate_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        if self.native_publication {
            return Err(invalid("JS seed hook forbidden in native publication"));
        }
        self.check_seed(&seed)?;
        self.pending_seed.take();
        self.command(Command::ActivateFrameSeed {
            seed_id: seed.seed_id,
        })?;
        Ok(())
    }
    pub fn discard_frame_seed(&mut self, seed: PreparedSeed) -> Result<()> {
        self.check_seed(&seed)?;
        self.pending_seed.take();
        self.command(Command::DiscardFrameSeed {
            seed_id: seed.seed_id,
        })?;
        Ok(())
    }
    pub fn seed_frame(
        &mut self,
        metadata: &Value,
        arrays: &[ArraySpec],
        planes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        if self.native_publication {
            return Err(invalid("combined JS seed forbidden in native publication"));
        }
        let seed = self.prepare_frame_seed(metadata, arrays, planes)?;
        self.activate_frame_seed(seed)
    }
    pub fn plan(
        &mut self,
        settings: &Value,
        mode: AnimationMode,
        environment: &Value,
    ) -> Result<Value> {
        let packet = self.command(Command::Plan {
            settings: settings.clone(),
            mode,
            environment: environment.clone(),
        })?;
        packet
            .envelope
            .payload
            .get("value")
            .cloned()
            .ok_or_else(|| invalid("missing plan"))
    }
    pub fn start_create(&mut self, settings: &Value, accepted_plan: &Value) -> Result<CreateState> {
        // Start only a separately authenticated native activation.
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Script plan fields cannot activate the helper.
        self.require_active(authority)?; // Reject logical retirement before creating a command.
        let deadline = self.operation_deadline()?; // Include every drain page in the same transaction timeout.
        let result = (|| {
            // Retire on malformed acquisition responses without losing physical failure state.
            let response = self.command_at(
                Command::StartCreate {
                    settings: settings.clone(),
                    accepted_plan: accepted_plan.clone(),
                },
                BTreeMap::new(),
                None,
                None,
                deadline,
            )?; // Creation alone selects the native Create phase.
            let state = creation_state(&response.envelope.payload)?; // Validate readiness before exposing requests.
            self.creation = Some(state); // Record the native command outcome, not script permission JSON.
            self.drain_services(ServicePhase::Create, deadline)?; // Drain all admitted requests without a second checkpoint.
            Ok(state) // Native services remain the parent's responsibility.
        })(); // End the complete acquiring transaction.
        if result.is_err() {
            let _ = self.cancel();
        } // A partial protocol transaction cannot remain usable.
        result // Keep the original failure available to runtime cleanup.
    } // End native creation.
    pub fn pump(&mut self) -> Result<CreateState> {
        // Only runtime's current-channel-checked owner may call this acquiring entrypoint.
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Require explicit accepted activation.
        self.require_active(authority)?; // Retirement is terminal for package execution.
        let phase = match self.creation {
            Some(CreateState::Pending) => ServicePhase::Create,
            Some(CreateState::Ready) => ServicePhase::Async,
            None => return Err(invalid("creation has not started")),
        }; // Match Engine::pump's actual native phase.
        let deadline = self.operation_deadline()?; // Do not reset operation time across cancellation and drain pages.
        let result = (|| {
            // Keep the whole authorized pump transaction bounded.
            self.synchronize_cancellations(deadline)?; // Signal the child before running pending package reactions.
            let response = self.command_at(Command::Pump, BTreeMap::new(), None, None, deadline)?; // This is the only continuation checkpoint in the transaction.
            let state = creation_state(&response.envelope.payload)?; // Validate native creation state.
            self.creation = Some(state); // Subsequent pumps derive their phase from this state.
            self.drain_services(phase, deadline)?; // Native paging never acquires or executes JavaScript.
            Ok(state) // Deliver readiness only after request custody is fully reconciled.
        })(); // All request payloads retain their independent original-root guards.
        if result.is_err() {
            let _ = self.cancel();
        } // Reject partial acquiring transactions as terminal transport faults.
        result // The caller still owns actual native service outcomes.
    } // End authorized continuation pumping.
    pub fn take_requests(&mut self) -> Vec<HostRequest> {
        // Drain only live immutable parent requests without another RPC.
        std::mem::take(&mut self.requests)
            .into_iter()
            .filter(|request| !request.is_cancelled() && self.pending.contains_key(&request.id))
            .collect() // Expired pending records remain available for explicit child cancellation.
    } // Native dispatch repeats the deadline and current-channel checks.
    pub fn take_cancelled_requests(&mut self) -> Vec<HostRequest> {
        // Preserve terminal cleanup after activation revocation or helper retirement.
        self.cancelled.drain(..).collect() // Returned aliases retain storage and request occupancy until their actual last owner.
    } // Notification never proves a native body exited.
    pub fn complete_service_request(
        &mut self,
        id: u64,
        authority: ServiceAuthority,
        value: ServiceValue,
    ) -> Result<CompletionState> {
        // Copy/resolve under the parent's protected native publication boundary.
        self.require_active(authority)?; // Current helper activation must match the actual native operation.
        let Some(request) = self.pending.get(&id).cloned() else {
            return Ok(CompletionState::Unknown);
        }; // Duplicate completion does not disturb other work.
        if request.authority != authority {
            return Err(invalid("pending service authority mismatch"));
        } // Preserve exact native request identity.
        if !value.shares_root(&self.quota) {
            return Err(AnimationError::Budget(
                "foreign helper completion quota".into(),
            ));
        } // A child-local ledger cannot legitimize a foreign parent root.
        if request.is_cancelled() {
            // Withhold expired or cancelled protected bytes before IPC copying.
            let timed_out = request.remaining_ms() == 0; // Preserve the parent's conservative terminal deadline classification.
            self.cancel_remote(id, authority, self.operation_deadline()?)?; // Require actual child settlement before reporting the known terminal outcome.
            return Ok(if timed_out {
                CompletionState::TimedOut
            } else {
                CompletionState::Cancelled
            }); // The child may have classified its slightly later native deadline as explicit cancellation.
        } // Native effect outcome and actual body exit remain separately owned.
        value.validate_limits(&self.limits.engine)?; // Apply this instance's configured bounds independently of producer limits.
        let command = Command::CompleteService {
            id,
            authority: authority.into(),
            result: value.metadata().clone(),
            arrays: value.arrays().iter().map(ArrayConfig::from).collect(),
        }; // Stage only metadata under retained pipe scratch.
        let response = self.command_at(
            command,
            BTreeMap::new(),
            None,
            Some(value),
            self.operation_deadline()?,
        )?; // Borrow binary planes until the writer actually releases their packet.
        let state = self.read_completion(response, id, authority)?; // Ordinary refusal leaves the pending request available for native policy.
        self.forget_pending(id, state != CompletionState::Delivered); // Release correlation only after actual terminal child acknowledgement.
        Ok(state) // No diagnostics, seed hook, acknowledgement callback, or microtask checkpoint runs here.
    } // End binary protected publication.
    pub fn complete_request(&mut self, id: u64, result: &Value) -> Result<()> {
        // Preserve the existing JSON-only caller API as a checked adapter.
        let authority = self
            .active
            .ok_or_else(|| invalid("native service authority is not bound"))?; // Never infer activation from returned metadata.
        if !self.pending.contains_key(&id) {
            return Err(invalid("unknown helper request completion"));
        } // Refuse unknown IDs before allocating an adapter copy.
        let value = ServiceValue::copy_from_host(
            result,
            &[],
            &BTreeMap::new(),
            &self.limits.engine,
            self.quota.clone(),
        )?; // Admit the legacy immutable source through the original root.
        match self.complete_service_request(id, authority, value)? {
            CompletionState::Delivered => Ok(()),
            _ => Err(invalid("terminal helper request completion")),
        } // Preserve legacy terminal-error behavior without a checkpoint.
    } // End checked legacy completion.
    pub fn render(&mut self, context: &Value, arrays: &[ArraySpec]) -> Result<RenderOutput> {
        if arrays.len() > MAX_PLANES {
            return Err(AnimationError::Budget("helper frame plane count".into()));
        }
        let frame_bytes = arrays.iter().try_fold(0usize, |total, array| {
            let width = match array.kind {
                TypedArrayKind::U8 => 1,
                TypedArrayKind::U16 => 2,
                _ => 4,
            };
            total
                .checked_add(
                    array
                        .elements
                        .checked_mul(width)
                        .ok_or_else(|| invalid("frame shape overflow"))?,
                )
                .ok_or_else(|| invalid("frame aggregate overflow"))
        })?;
        if frame_bytes > self.limits.engine.frame_bytes {
            return Err(AnimationError::Budget(
                "helper frame aggregate bytes".into(),
            ));
        }
        let admission = self
            .quota
            .reserve_external_storage(frame_bytes.max(1))
            .map_err(|e| {
                AnimationError::Budget(format!("helper returned frame admission: {e:?}"))
            })?;
        let response = self.command(Command::Render {
            context: context.clone(),
            arrays: arrays.iter().map(ArrayConfig::from).collect(),
        })?;
        // Validate shape again in the authority process, irrespective of child claims.
        let expected: BTreeMap<_, _> = arrays
            .iter()
            .map(|array| {
                let width = match array.kind {
                    TypedArrayKind::U8 => 1,
                    TypedArrayKind::U16 => 2,
                    TypedArrayKind::F32 | TypedArrayKind::U32 => 4,
                };
                array
                    .elements
                    .checked_mul(width)
                    .map(|bytes| (array.name.clone(), bytes))
                    .ok_or_else(|| invalid("frame shape overflow"))
            })
            .collect::<Result<_>>()?;
        if expected.len() != arrays.len()
            || expected.len() != response.planes.len()
            || response
                .planes
                .iter()
                .any(|(name, data)| expected.get(name) != Some(&data.len()))
        {
            let _ = self.cancel();
            return Err(AnimationError::Integrity(
                "helper frame shape mismatch".into(),
            ));
        }
        if let Some(status) = response.envelope.payload.get("status") {
            if let Err(error) = validate_status(status) {
                let _ = self.cancel();
                return Err(error);
            }
            let storage = self
                .quota
                .reserve_external_storage(256 * 1024)
                .map_err(|e| AnimationError::Budget(format!("helper retained status: {e:?}")))?;
            self.status = Some(RetainedHelperStatus {
                value: status.clone(),
                _storage: storage,
            });
        }
        Ok(RenderOutput::from_retained_parts(
            response
                .envelope
                .payload
                .get("value")
                .cloned()
                .ok_or_else(|| invalid("missing frame metadata"))?,
            response.planes,
            admission,
        ))
    }
    /// Drain the most recent native-copied diagnostics without another RPC.
    pub fn take_status(&mut self) -> Option<RetainedHelperStatus> {
        self.status.take()
    }
    pub fn render_retained(
        &mut self,
        context: &Value,
        arrays: &[ArraySpec],
    ) -> Result<RetainedHelperFrame> {
        let bytes = arrays.iter().try_fold(0usize, |total, array| {
            let width = match array.kind {
                TypedArrayKind::U8 => 1,
                TypedArrayKind::U16 => 2,
                _ => 4,
            };
            total
                .checked_add(
                    array
                        .elements
                        .checked_mul(width)
                        .ok_or_else(|| invalid("retained shape overflow"))?,
                )
                .ok_or_else(|| invalid("retained bytes overflow"))
        })?;
        let storage = self
            .quota
            .reserve_external_storage(bytes.max(1))
            .map_err(|e| AnimationError::Budget(format!("helper frame retention: {e:?}")))?;
        Ok(RetainedHelperFrame {
            output: self.render(context, arrays)?,
            _storage: storage,
        })
    }
    pub fn accept_frame(&mut self, accepted: bool) -> Result<()> {
        self.command(Command::AcceptFrame { accepted })?;
        Ok(())
    }
    pub fn probe(&mut self) -> Result<Value> {
        self.command(Command::Probe)?
            .envelope
            .payload
            .get("value")
            .cloned()
            .ok_or_else(|| invalid("missing syscall probe"))
    }
    pub fn is_physically_retired(&self) -> bool {
        self.physically_retired
    } // Only successful original child shutdown and every worker join establish this fact.

    #[cfg(test)]
    pub(crate) fn arm_complete_service_ack_failure_for_test(&self) -> Result<()> {
        self.test_transport.arm_complete_service_ack_failure()
    }

    #[cfg(test)]
    pub(crate) fn transport_test_snapshot(&self) -> HelperTransportTestSnapshot {
        self.test_transport
            .snapshot(self.closed, self.physically_retired)
    }

    #[cfg(test)]
    pub(crate) fn sequence_for_test(&self) -> u64 {
        self.sequence
    }

    #[cfg(test)]
    pub(crate) fn has_pending_service_request_for_test(&self, id: u64) -> bool {
        self.pending.contains_key(&id)
    }

    pub fn dispose(&mut self) -> Result<()> {
        // Optional bounded guest disposal never substitutes for native retirement.
        if self.closed {
            return self.cancel();
        } // Preserve a prior failed-retirement result instead of returning optimistic success.
        let disposal = self.command(Command::Dispose); // Guest disposal may fail or be forbidden by pending continuations.
        self.cancel()?; // Require actual native retirement even when guest disposal failed.
        disposal.map(|_| ()) // Preserve guest failure after successful physical cleanup.
    } // End guest-assisted retirement.
    /// Inert native bind/result copy ACK ONLY. The callback may hold a broker
    /// guard; it must not execute guest/trusted hooks or pump/checkpoint/diagnose.
    /// Every callback-local guard is destroyed before deferred physical teardown.
    pub(crate) fn with_native_publication<T>(
        &mut self,
        publish: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if self.native_publication || self.closed {
            return Err(invalid("native publication scope unavailable"));
        }
        self.native_publication = true;
        let result = publish(self);
        self.native_publication = false;
        let retirement = if std::mem::take(&mut self.deferred_retirement) {
            self.cancel()
        } else {
            Ok(())
        };
        match (result, retirement) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(retirement)) => Err(invalid(&format!(
                "native publication failed: {error}; deferred retirement failed: {retirement}"
            ))),
        }
    }
    pub fn cancel(&mut self) -> Result<()> {
        if self.native_publication {
            self.closed = true;
            self.deferred_retirement = true;
            return Err(invalid(
                "physical retirement deferred until native publication guard exits",
            ));
        }
        #[cfg(test)]
        if self.test_transport.interrupt_physical_retirement_once() {
            self.closed = true;
            return Err(invalid(
                "test interrupted physical retirement before child shutdown",
            ));
        }
        // Retire this helper independently of runtime's native Activation and PermissionBroker.
        if self.physically_retired {
            return Ok(());
        } // Return success only for an already observed complete retirement.
        if self.retirement_failed {
            return Err(invalid(
                "prior helper retirement failed; physical exit remains unproven",
            ));
        } // Unsupplied retry/reaper APIs cannot manufacture later terminal proof.
        self.closed = true; // Stop accepting commands before signalling child or workers.
        self.pending_seed.take();
        self.active = None; // Retire only the helper's local activation binding.
        self.creation = None; // Package execution cannot restart in this helper.
        self.service_budget.close(); // Escaped payload leases remain charged after logical closure.
        self.requests.clear(); // Pending records still own the same immutable request allocations.
        for (_, request) in std::mem::take(&mut self.pending) {
            request.stop_token().stop();
            self.cancelled.push_back(request);
        } // Preserve native cancellation inventory and custody.
        self.status = None; // Release locally retained diagnostics under their own guard.
        let child_result = self
            .child
            .as_mut()
            .ok_or_else(|| invalid("helper child missing"))
            .and_then(|child| child.shutdown().map_err(AnimationError::from)); // Call the supplied actual shutdown API once.
        let _ = self.writer.try_send(None); // Wake the writer without claiming pipe closure proves exit.
        for worker in &self.workers {
            worker.ticket().cancel();
        } // Request cooperative worker retirement.
        let deadline = Instant::now() + Duration::from_secs(2); // Preserve the frozen bounded join interval.
        let mut joined = true; // Every original pipe worker must supply successful terminal evidence.
        for worker in &self.workers {
            if worker.ticket().join_until(deadline).is_err() {
                joined = false;
            }
        } // Attempt every join even when an earlier worker failed.
        if child_result.is_err() || !joined {
            // Do not release physical admission on uncertain retirement.
            self.retirement_failed = true; // Latch failure across cancel, dispose and Drop.
            return Err(invalid(
                "helper shutdown/join failed; original physical custody remains retained",
            )); // Playback stays ineligible until a real native terminal mechanism exists.
        } // The supplied native APIs have now reported complete retirement.
        self.physically_retired = true; // Publish physical success only after both child and worker evidence.
        self.workers.clear(); // Release joined worker records and their captured admission clones.
        self.child.take(); // Drop the already retired process owner.
        while self.reader.try_recv().is_ok() {} // Release unread bounded pipe packets before their scratch admission.
        self._physical.take(); // Return the original physical worker admission after terminal observation.
        self._storage.take(); // Return pipe scratch after actual readers and writers have exited.
        Ok(()) // Native activation remains owned by PackageInstance until explicit revocation.
    } // End independent helper retirement.
} // End helper session methods.
impl Drop for HelperSession {
    // An unproven body must never become an uncharged body during owner destruction.
    fn drop(&mut self) {
        // Attempt only the supplied bounded retirement path.
        if self.cancel().is_ok() {
            return;
        } // Proven retirement releases ordinary fields normally.
        if let Some(child) = self.child.take() {
            std::mem::forget(child);
        } // Quarantine the original uncertain process owner rather than fabricate reaping.
        for worker in std::mem::take(&mut self.workers) {
            std::mem::forget(worker);
        } // Retain original worker ownership after failed terminal proof.
        if let Some(admission) = self._physical.take() {
            std::mem::forget(admission);
        } // Original-root physical debit remains until process exit.
        if let Some(storage) = self._storage.take() {
            std::mem::forget(storage);
        } // Keep possible pipe allocations charged through uncertain writers/readers.
    } // Failed-join recovery requires the primary's actual native terminal APIs; this bounded fail-closed quarantine is not playback success.
} // No replacement quota bank, background reaper, or optimistic retry is introduced.
fn creation_state(payload: &Value) -> Result<CreateState> {
    match payload.get("state").and_then(Value::as_str) {
        Some("pending") => Ok(CreateState::Pending),
        Some("ready") => Ok(CreateState::Ready),
        _ => Err(invalid("invalid creation state")),
    }
}
fn state_value(state: CreateState) -> Value {
    json!({"state": match state { CreateState::Pending => "pending", CreateState::Ready => "ready" }})
}

#[derive(Default)] // Native request ownership stays bounded by the engine's retained leases.
struct ChildServices {
    queued: VecDeque<HostRequest>,
    issued: BTreeSet<u64>,
    cancelled: VecDeque<HostRequest>,
} // Keep unissued, issued and terminal lifetimes distinct.
impl ChildServices {
    // Every method runs on the existing V8 helper owner thread.
    fn collect(&mut self, engine: &mut Engine) -> Result<()> {
        // Transfer only native-created request records.
        for request in engine.take_cancelled_requests() {
            // Drain explicit terminal ownership before collecting new requests.
            self.queued.retain(|queued| queued.id != request.id); // Suppress never-issued requests without sending fictitious parent events.
            if self.issued.remove(&request.id) {
                self.cancelled.push_back(request);
            } // Only IDs previously published to the parent need terminal reconciliation.
        } // Never-issued terminal IDs cannot reappear because the engine never reuses IDs.
        self.queued.extend(engine.take_requests()?); // Immutable clones keep original child-root leases throughout paging.
        Ok(()) // No JavaScript hook or checkpoint executes here.
    } // End native queue transfer.
    fn terminal(&mut self, id: u64) {
        // A direct terminal ACK supersedes a later cancellation page for the same ID.
        self.issued.remove(&id); // Remove only native publication bookkeeping.
        self.queued.retain(|request| request.id != id); // Prevent a completed request from being issued afterward.
        self.cancelled.retain(|request| request.id != id); // The direct ACK carries this terminal event instead.
    } // Escaped native body custody remains independently owned by the parent.
    fn page(
        &mut self,
        engine: &mut Engine,
        authority: &HelperAuthority,
        active: ServiceAuthority,
        sequence: u64,
    ) -> Result<(Packet, Option<HostRequest>)> {
        // Produce exactly one response for one drain command.
        engine.expire_service_requests()?; // Settle native timeouts without running continuations.
        self.collect(engine)?; // Keep terminal and queued ownership explicit.
        if let Some(request) = self.cancelled.pop_front() {
            // Terminal pages contain correlation only.
            let record = TerminalRecord {
                id: request.id,
                authority: request.authority.into(),
            }; // Preserve the original native activation.
            let packet = Packet::new(
                sequence,
                authority.clone(),
                "response",
                json!({"requests":[],"terminal":record,"more":!self.queued.is_empty() || !self.cancelled.is_empty()}),
                BTreeMap::new(),
            ); // Do not mix cancellation inventory with binary planes.
            packet_metadata(&packet)?; // Preflight the whole envelope before any output.
            return Ok((packet, Some(request))); // Retain its lease until the actual page write finishes.
        } // Issue pages consume only the already admitted native queue.
        while let Some(request) = self.queued.pop_front() {
            // The engine's request count bounds this loop.
            if request.authority != active {
                return Err(invalid("queued request activation mismatch"));
            } // Never transplant old requests into a new native activation.
            if request.is_cancelled() || request.remaining_ms() == 0 {
                // A submillisecond remaining interval must not be rounded upward across IPC.
                engine.cancel_request(request.id, active)?; // Withhold this never-issued request; cancellation does not assert body rollback.
                self.collect(engine)?; // Release only unissued terminal custody while retaining previously issued notifications.
                continue; // Continue toward another already admitted request without a checkpoint.
            } // Snapshot remaining time immediately before bounded framing.
            let record = RequestRecord::from(&request); // Metadata copies use existing child preload scratch.
            if record.timeout_ms == 0 {
                engine.cancel_request(request.id, active)?;
                self.collect(engine)?;
                continue;
            } // A deadline crossing during metadata construction is an individual terminal event, not a malformed zero-timeout page.
            let mut packet = Packet::new(
                sequence,
                authority.clone(),
                "response",
                json!({"requests":[record],"terminal":null,"more":!self.queued.is_empty() || !self.cancelled.is_empty()}),
                BTreeMap::new(),
            ); // One request can use all 48 canonical plane names.
            packet.attach_service(request.payload.clone())?; // Share immutable storage; never clone a bulk plane map.
            if packet_metadata(&packet).is_err() {
                // A valid engine value may exceed whole-envelope depth or metadata limits.
                engine.cancel_request(request.id, active)?; // Reject only this request at the transport boundary.
                self.collect(engine)?; // The next authorized pump can observe the structured terminal result.
                continue; // Preserve unrelated requests and all original configured limits.
            } // Every outgoing byte now belongs to a validated, admitted packet.
            self.issued.insert(request.id); // A write failure terminates this session, preventing later ambiguous reuse.
            return Ok((packet, Some(request))); // Keep input/header custody through actual synchronous write completion.
        } // No queued request remains publishable.
        let packet = Packet::new(
            sequence,
            authority.clone(),
            "response",
            json!({"requests":[],"terminal":null,"more":false}),
            BTreeMap::new(),
        ); // An empty terminal page ends the bounded drain transaction.
        Ok((packet, None)) // A new acquiring command is required for new work.
    } // End one-event paging.
} // No native service implementation is fabricated by transport bookkeeping.
/// Trusted helper entrypoint. The first binary packet is package bytes; loading
/// executable modules is deliberately after kernel isolation verification/seal.
pub fn run_helper_ipc() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut initial = read_packet(&mut input)?;
    if initial.envelope.kind != "command" || initial.envelope.sequence != 0 {
        return Err(invalid("first packet must initialize"));
    }
    let authority = initial.envelope.authority.clone();
    let Command::Initialize {
        bootstrap,
        sandbox,
        engine,
        mode,
        ambient_seed,
        service_wire_version, // The parent selected the concrete service protocol.
        service_byte_order,   // The parent supplied its native typed-array byte order.
    } = serde_json::from_value(initial.envelope.payload.clone())?
    else {
        return Err(invalid("missing initialization"));
    };
    if service_wire_version != engine::SERVICE_WIRE_VERSION
        || service_byte_order != engine::SERVICE_BYTE_ORDER
    {
        return Err(invalid("service binary ABI mismatch"));
    } // Refuse incompatible payload interpretation before loading code.
    if bootstrap.len() > 192 * 1024 || initial.planes.len() != 1 {
        return Err(AnimationError::Budget("helper startup payload".into()));
    }
    let limits: SandboxLimits = sandbox.into();
    animation_sandbox::verify_helper_environment(limits)?;
    let helper_build_digest = loaded_helper_digest()?;
    let memory_bytes =
        usize::try_from(limits.memory_bytes).map_err(|_| invalid("memory bound overflow"))?;
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: limits.maximum_tasks as usize,
        worker_bytes: memory_bytes,
    });
    let _preload = quota
        .reserve_external_storage(MAX_BINARY + 64 * 1024 * 1024 + 4 * MAX_JSON)
        .map_err(|e| AnimationError::Budget(format!("helper preload: {e:?}")))?;
    let archive = initial
        .planes
        .remove("archive")
        .ok_or_else(|| invalid("missing archive bytes"))?;
    let package = Arc::new(Package::from_bytes(&archive, PackageLimits::default())?);
    if package.digest() != authority.package_digest {
        return Err(AnimationError::Integrity(
            "helper immutable package identity mismatch".into(),
        ));
    }
    drop(archive);
    drop(initial);
    engine::initialize_engine(quota.clone(), 1)?;
    let frame_byte_limit = engine.frame_bytes;
    let engine_limits: EngineLimits = engine.into(); // Retain actual configured service bounds for native completion copies.
    let mut engine = Engine::new(package, engine_limits.clone(), quota.clone())?; // Keep the existing precharged child root for every later copy.
    engine.install_bootstrap(&bootstrap)?;
    // Both modes need the trusted lifecycle declaration. Live configuration
    // returns before changing Date or Math.random; replay alone seals them.
    engine.configure_ambient(mode, ambient_seed)?;
    let proof = animation_sandbox::seal_current_helper()?;
    engine.load()?;
    write_packet(
        &mut output,
        &Packet::new(
            0,
            authority.clone(),
            "response",
            json!({"ready":true,"requests":[],"service_wire_version":engine::SERVICE_WIRE_VERSION,"service_byte_order":engine::SERVICE_BYTE_ORDER,"helper_build_sha256":helper_build_digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),"isolation":{"all_threads_filtered":proof.all_threads_filtered,"descriptors_scrubbed":proof.descriptors_scrubbed,"physical_memory_max":limits.memory_bytes,"maximum_tasks":limits.maximum_tasks}}), // Acknowledge the exact native binary ABI after sealing and loading.
            BTreeMap::new(),
        ),
    )?;
    let mut sequence = 0u64; // Preserve immutable monotonic command correlation.
    let mut active: Option<ServiceAuthority> = None; // Bind separately from the immutable launch envelope.
    let mut services = ChildServices::default(); // Retain unissued request leases across drain commands.
    loop {
        // Existing synchronous IPC remains the sole helper command owner.
        let packet = read_packet(&mut input)?; // Preload scratch bounds the incoming metadata and planes.
        sequence = sequence
            .checked_add(1)
            .ok_or_else(|| invalid("sequence exhausted"))?; // Never reuse a consumed command identity.
        if packet.envelope.kind != "command"
            || packet.envelope.authority != authority
            || packet.envelope.sequence != sequence
        {
            return Err(AnimationError::PermissionDenied(
                "helper command binding/sequence mismatch".into(),
            ));
        } // Reject script-supplied session identity.
        let command: Command = serde_json::from_value(packet.envelope.payload)?; // Deserialize the complete closed command schema.
        if !matches!(
            &command,
            Command::PrepareFrameSeed { .. } | Command::CompleteService { .. }
        ) && !packet.planes.is_empty()
        {
            return Err(invalid("binary outside declared native command"));
        } // Preserve command-specific plane ownership.
        let incoming_planes = packet.planes; // Keep source storage in child preload through all destination copies.
        let disposing = matches!(&command, Command::Dispose); // Exit only after writing the native disposal response.
        let draining = matches!(&command, Command::DrainRequests); // A page command must never execute package code.
        if !draining
            && !matches!(&command, Command::CancelService { .. })
            && !services.queued.is_empty()
        {
            return Err(invalid("undrained native service transaction"));
        } // Individual nonacquiring cancellation may occur between pages; executing another guest phase may not.
        type AdmittedCommandOutput = (Packet, Option<StorageAdmission>, Option<HostRequest>);
        let result: Result<AdmittedCommandOutput> = (|| {
            // Retain every frame/request guard through the actual write below.
            let mut planes = BTreeMap::new(); // Ordinary responses contain no service binary copies.
            let mut frame_admission = None; // Rendering transfers its existing exact admission.
            let payload = match command {
                // Each branch is selected only by a native parent command.
                Command::Initialize { .. } => return Err(invalid("duplicate initialization")), // Never replace the immutable session or limits.
                Command::BindAuthority { authority: stamp } => {
                    // Accept active coordinates through the authenticated native pipe only.
                    let stamp = ServiceAuthority::from(stamp); // Decoding is shape, not broker authorization.
                    stamp.validate()?; // Reject zero native coordinates.
                    if stamp.instance_id != authority.instance_id
                        || active.is_some_and(|current| current != stamp)
                    {
                        return Err(invalid("native activation replacement"));
                    } // A different activation needs a fresh helper.
                    engine.bind_service_authority(&authority.package_digest, stamp)?; // Bind against the actual loaded immutable package digest.
                    active = Some(stamp); // Publish only a successfully bound native activation.
                    json!({}) // The response makes no claim of a permission grant.
                } // End activation binding.
                Command::Plan {
                    settings,
                    mode,
                    environment,
                } => json!({"value":engine.plan(&settings, mode, &environment)?}), // Planning remains nonacquiring.
                Command::StartCreate {
                    settings,
                    accepted_plan,
                } => {
                    // Preparation requires a separately bound native activation.
                    active.ok_or_else(|| invalid("native service authority is not bound"))?; // Accepted-plan JSON cannot supply it.
                    state_value(engine.start_create(&settings, &accepted_plan)?)
                    // The engine derives actual acquisition phase.
                } // End create command.
                Command::Pump => {
                    // Only a separately current-channel-checked parent may request continuation execution.
                    active.ok_or_else(|| invalid("native service authority is not bound"))?; // Do not pump unaccepted packages.
                    state_value(engine.pump()?) // This branch owns the explicit continuation checkpoint.
                } // End acquiring pump.
                Command::DrainRequests => {
                    // Page already admitted work without executing JavaScript.
                    let stamp =
                        active.ok_or_else(|| invalid("native service authority is not bound"))?; // Keep the active stamp distinct from the envelope.
                    let (packet, retained) =
                        services.page(&mut engine, &authority, stamp, sequence)?; // Preflight each one-event packet before writing.
                    return Ok((packet, None, retained)); // Keep the request lease through write completion.
                } // End nonacquiring paging.
                Command::CompleteService {
                    id,
                    authority: stamp,
                    result,
                    arrays,
                } => {
                    // Protected publication cannot invoke package diagnostics or checkpoints.
                    let stamp = ServiceAuthority::from(stamp); // Decode only native-command coordinates.
                    if active != Some(stamp) {
                        return Err(invalid("stale native completion authority"));
                    } // Reject stale command identity before a destination copy.
                    let result = (|| {
                        // Ordinary bounded copy refusal preserves unrelated work.
                        let arrays = arrays
                            .into_iter()
                            .map(ArraySpec::try_from)
                            .collect::<Result<Vec<_>>>()?; // Revalidate kinds and checked shapes.
                        let value = ServiceValue::copy_from_host(
                            &result,
                            &arrays,
                            &incoming_planes,
                            &engine_limits,
                            quota.clone(),
                        )?; // Admit the child copy within the original precharged physical envelope.
                        engine.complete_service_request(id, stamp, value) // Fresh V8 backing is independently allocator-charged.
                    })(); // Incoming packet bytes remain held throughout overlap.
                    if engine.is_invalid() {
                        return Err(invalid("engine retired during service completion"));
                    } // Genuine engine failure retains terminal session handling.
                    if matches!(
                        &result,
                        Ok(CompletionState::Delivered
                            | CompletionState::Unknown
                            | CompletionState::TimedOut
                            | CompletionState::Cancelled)
                    ) {
                        services.terminal(id);
                    } // Direct ACK replaces later duplicate cancellation pages.
                    json!({"completion":completion_record(id, stamp, result)}) // Return only native settlement state, never a JS hook result.
                } // End binary completion.
                Command::CancelService {
                    id,
                    authority: stamp,
                } => {
                    // Cancellation is native bookkeeping and Promise settlement only.
                    let stamp = ServiceAuthority::from(stamp); // Preserve the exact active coordinates.
                    if active != Some(stamp) {
                        return Err(invalid("stale native cancellation authority"));
                    } // Do not cancel another activation's request.
                    engine.expire_service_requests()?; // Preserve actual timeout classification when the native deadline already elapsed.
                    let result = engine.cancel_request(id, stamp); // Explicit cancellation remains distinct from native body exit.
                    if engine.is_invalid() {
                        return Err(invalid("engine retired during cancellation"));
                    } // Do not disguise V8 failure as a normal terminal ACK.
                    if result.is_ok() {
                        services.terminal(id);
                    } // The direct response carries this terminal event.
                    json!({"completion":completion_record(id, stamp, result)}) // No diagnostics or microtask checkpoint runs here.
                } // End native cancellation command.
                Command::CompleteRequest { id, result } => {
                    // Retain the old JSON command as a nonacquiring native compatibility adapter.
                    let stamp =
                        active.ok_or_else(|| invalid("native service authority is not bound"))?; // Never activate from compatibility payload JSON.
                    let result = engine
                        .complete_request(id, &result)
                        .map(|_| CompletionState::Delivered); // The engine adapter independently admits JSON storage.
                    if engine.is_invalid() {
                        return Err(invalid("engine retired during JSON completion"));
                    } // Preserve terminal failure semantics.
                    if result.is_ok() {
                        services.terminal(id);
                    } // Legacy success has the same terminal correlation rule.
                    json!({"completion":completion_record(id, stamp, result)}) // Compatibility cannot acquire services or execute diagnostics.
                } // End JSON compatibility command.
                Command::PrepareFrameSeed { metadata, arrays } => {
                    // Native seed installation retains its separate nonacquiring phase.
                    active.ok_or_else(|| invalid("native service authority is not bound"))?; // A script permission snapshot is insufficient.
                    let arrays = arrays
                        .into_iter()
                        .map(ArraySpec::try_from)
                        .collect::<Result<Vec<_>>>()?; // Preserve the existing four-type seed inventory.
                    validate_seed_planes(&arrays, &incoming_planes, frame_byte_limit)?; // Validate exact shape before engine copying.
                    let seed_id =
                        engine.prepare_frame_seed(&metadata, &arrays, &incoming_planes)?;
                    json!({"seed_id": seed_id, "copied": true}) // Native provenance remains external to seed metadata.
                } // End seed command.
                Command::ActivateFrameSeed { seed_id } => {
                    active.ok_or_else(|| invalid("native service authority is not bound"))?;
                    engine.activate_frame_seed(seed_id)?;
                    json!({})
                }
                Command::DiscardFrameSeed { seed_id } => {
                    active.ok_or_else(|| invalid("native service authority is not bound"))?;
                    engine.discard_frame_seed(seed_id)?;
                    json!({})
                }
                Command::Render { context, arrays } => {
                    // Preserve the synchronous render contract and existing detachment path.
                    let arrays = arrays
                        .into_iter()
                        .map(ArraySpec::try_from)
                        .collect::<Result<Vec<_>>>()?; // Retain original frame inventory validation.
                    let frame = engine.render(&context, &arrays)?; // Render cannot acquire services.
                    let (metadata, returned_planes, admission) = frame.into_parts(); // Transfer frame output and its actual storage admission.
                    planes = returned_planes; // Service result buffers never enter this frame ownership path.
                    frame_admission = Some(admission); // Hold native frame bytes through pipe write completion.
                    json!({"value":metadata,"status":engine.take_status()?}) // Diagnostics execute only in this separate non-publication command.
                } // End synchronous render.
                Command::AcceptFrame { accepted } => {
                    engine.accept_frame(accepted)?;
                    json!({})
                } // Preserve bounded frame acknowledgement outside service delivery.
                Command::Probe => {
                    // Preserve the exact native isolation qualification hook.
                    let values = animation_sandbox::hostile_syscall_probe()?; // Probe only inside the already sealed helper.
                    if values.iter().any(|(_, denied)| !*denied) {
                        return Err(AnimationError::PermissionDenied(
                            "hostile syscall escaped helper seal".into(),
                        ));
                    } // Fail closed on an actual denied-syscall regression.
                    json!({"value":values.into_iter().map(|(name, denied)| (name.to_owned(), Value::Bool(denied))).collect::<serde_json::Map<_,_>>()})
                    // Return informational probe results only.
                } // End native probe.
                Command::Dispose => {
                    engine.dispose()?;
                    json!({})
                } // Physical process/worker retirement remains the parent's independent responsibility.
            }; // Every ordinary response carries no fresh service records.
            if !disposing {
                services.collect(&mut engine)?;
            } // Collect native ownership without running a hook or checkpoint.
            let mut payload = payload; // Preserve command-specific metadata.
            payload["requests"] = json!([]); // New requests are delivered only through explicit bounded pages.
            let packet = Packet::new(sequence, authority.clone(), "response", payload, planes); // Frame and service planes never share one packet.
            packet_metadata(&packet)?; // Reject whole-envelope overflow before writing a prefix.
            Ok((packet, frame_admission, None)) // Transfer all native guards to the synchronous write scope.
        })(); // No V8 handle or writable backing crosses a thread.
        match result {
            // Preserve one response packet per committed command.
            Ok((packet, _frame_admission, _request_guard)) => write_packet(&mut output, &packet)?, // Retain admitted source storage until all bytes are written.
            Err(error) => {
                // Malformed commands and actual engine failures retire this helper.
                engine.cancel(); // Signal native cancellation before process teardown.
                write_packet(
                    &mut output,
                    &Packet::new(
                        sequence,
                        authority.clone(),
                        "error",
                        json!({"error":error.to_string().chars().take(2048).collect::<String>()}),
                        BTreeMap::new(),
                    ),
                )?; // Emit only bounded failure metadata.
                return Err(error); // No later command can revive this V8 instance.
            } // End terminal protocol failure.
        } // Ordinary completion refusal used the nonterminal response branch.
        if disposing {
            return Ok(());
        } // Native parent shutdown/join still establishes physical retirement.
    } // End the original owner-thread helper loop.
} // End the trusted helper entrypoint.
#[cfg(test)]
mod protocol_tests {
    use super::*;
    fn authority() -> HelperAuthority {
        HelperAuthority {
            package_digest: "a".repeat(64),
            instance_id: 1,
            plan_generation: 2,
            authorization_epoch: 3,
        }
    }
    #[test]
    fn forged_helper_acquisition_is_rejected_by_every_forbidden_caller_phase() {
        let forbidden = [
            Command::Plan {
                settings: json!({}),
                mode: AnimationMode::Live,
                environment: json!({}),
            },
            Command::Render {
                context: json!({}),
                arrays: vec![],
            },
            Command::AcceptFrame { accepted: true },
            Command::PrepareFrameSeed {
                metadata: json!({}),
                arrays: vec![],
            },
            Command::ActivateFrameSeed { seed_id: 1 },
            Command::DiscardFrameSeed { seed_id: 1 },
            Command::Probe,
            Command::Dispose,
            Command::BindAuthority {
                authority: active_authority().into(),
            }, // Native activation binding cannot run an acquiring continuation.
            Command::DrainRequests, // Paging already admitted requests is not package execution.
            Command::CompleteService {
                id: 9,
                authority: active_authority().into(),
                result: json!({}),
                arrays: vec![],
            }, // Binary copy and native ACK must never acquire.
            Command::CancelService {
                id: 9,
                authority: active_authority().into(),
            }, // Cancellation settlement performs no checkpoint.
            Command::CompleteRequest {
                id: 9,
                result: json!({}),
            }, // The legacy JSON completion adapter is equally nonacquiring.
        ];
        let payload = json!({"phase":"create", "requests":[request_record(10)]}); // A forged response phase cannot authorize even a fully well-formed request.
        let mut bytes = Vec::new();
        write_packet(
            &mut bytes,
            &Packet::new(3, authority(), "response", payload, BTreeMap::new()),
        )
        .unwrap();
        let packet = read_packet(&mut bytes.as_slice()).unwrap();
        for command in forbidden {
            assert!(matches!(
                validated_requests(
                    &packet.envelope.payload,
                    command.permits_acquisition(),
                    9,
                    1,
                    1
                ),
                Err(AnimationError::PermissionDenied(_))
            ));
        }
        // Initialize/load exchange passes this literal false before any script.
        assert!(validated_requests(&packet.envelope.payload, false, 9, 1, 1).is_err());
        for command in [
            Command::StartCreate {
                settings: json!({}),
                accepted_plan: json!({}),
            },
            Command::Pump, // Only native start-create and explicit pump commands can acquire.
        ] {
            assert_eq!(
                validated_requests(
                    &packet.envelope.payload,
                    command.permits_acquisition(),
                    9,
                    1,
                    1
                )
                .unwrap()
                .len(),
                1
            );
        }
    }
    #[test]
    fn invalid_request_batch_never_partially_mutates_pending_custody() {
        let pending = BTreeSet::from([9]);
        let payload = json!({"requests":[request_record(10), request_record(10)]}); // Include the complete valid schema so replay rejection is the actual tested failure.
        assert!(validated_requests(&payload, true, 9, pending.len(), 1).is_err());
        assert_eq!(pending, BTreeSet::from([9]));
        assert!(validated_requests(&json!({"requests":null}), true, 9, 1, 1).is_err());
        assert!(validated_requests(&json!({"requests":[]}), false, 9, 1, 1)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn queued_seed_packet_retains_original_storage_after_local_guard_drops() {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 1024,
        });
        let storage = Arc::new(quota.reserve_external_storage(16).unwrap());
        let mut packet = Packet::new(
            1,
            authority(),
            "command",
            json!({"op":"seed_frame"}),
            BTreeMap::from([("work_data".into(), vec![0; 16])]),
        );
        packet._outbound_storage = Some(storage.clone());
        drop(storage);
        assert_eq!(quota.snapshot().worker_bytes, 16);
        let mut bytes = Vec::new();
        write_packet(&mut bytes, &packet).unwrap();
        let copied = read_packet(&mut bytes.as_slice()).unwrap();
        assert_eq!(copied.planes["work_data"].len(), 16);
        drop(packet);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    fn informational_status_has_closed_schema_and_bounded_utf8() {
        assert!(validate_status(
            &json!({"records":[{"kind":"log","level":"info","message":"chess"}],"dropped":0})
        )
        .is_ok());
        for invalid in [
            json!({"records":[],"dropped":0,"owner":1}),
            json!({"records":[{"kind":"log","level":"info","message":"x".repeat(4097)}],"dropped":0}),
            json!({"records":[{"kind":"progress","level":"info","message":"x","percent":101}],"dropped":0}),
        ] {
            assert!(validate_status(&invalid).is_err());
        }
    }
    #[test]
    fn binary_frame_roundtrips_without_base64_or_identity_mutation() {
        let packet = Packet::new(
            4,
            authority(),
            "response",
            json!({"value":{"present":true}}),
            BTreeMap::from([("gray".into(), vec![0, 1, 2, 255])]),
        );
        let mut bytes = Vec::new();
        write_packet(&mut bytes, &packet).unwrap();
        let recovered = read_packet(&mut bytes.as_slice()).unwrap();
        assert_eq!(recovered.envelope.authority, authority());
        assert_eq!(recovered.planes["gray"], [0, 1, 2, 255]);
    }
    #[test]
    fn dense_or_deep_json_is_rejected_before_dom_allocation() {
        assert!(validate_json_structure(
            format!("{}0{}", "[".repeat(33), "]".repeat(33)).as_bytes()
        )
        .is_err());
        assert!(validate_json_structure(format!("[{}0]", "0,".repeat(4097)).as_bytes()).is_err());
        assert!(validate_json_structure(br#"{"value":"braces { [ \" ignored"}"#).is_ok());
    }
    #[test]
    fn oversized_outgoing_metadata_never_writes_a_partial_packet() {
        let packet = Packet::new(
            1,
            authority(),
            "command",
            json!({"value":"x".repeat(MAX_JSON)}),
            BTreeMap::new(),
        );
        let mut output = Vec::new();
        assert!(write_packet(&mut output, &packet).is_err());
        assert!(output.is_empty());
    }
    #[test]
    fn oversized_metadata_is_rejected_before_body_allocation() {
        assert!(read_packet(&mut ((MAX_JSON + 1) as u32).to_be_bytes().as_slice()).is_err());
    }
    #[test]
    fn duplicate_planes_overflow_and_truncation_fail_closed() {
        let mut packet = Packet::new(0, authority(), "response", json!({}), BTreeMap::new());
        packet.envelope.planes = vec![
            PlaneInfo {
                name: "gray".into(),
                bytes: 1,
            },
            PlaneInfo {
                name: "gray".into(),
                bytes: 1,
            },
        ];
        assert!(validate_envelope(&packet.envelope).is_err());
        packet.envelope.planes = vec![PlaneInfo {
            name: "gray".into(),
            bytes: MAX_BINARY + 1,
        }];
        assert!(validate_envelope(&packet.envelope).is_err());
        assert!(read_packet(&mut &[0, 0, 0, 20, 1, 2][..]).is_err());
    }
    fn active_authority() -> ServiceAuthority {
        ServiceAuthority {
            instance_id: 1,
            plan_generation: 7,
            authorization_epoch: 11,
        }
    } // Separate active and session stamps.
    fn request_record(id: u64) -> Value {
        json!({"id":id,"method":"http.request","payload":{},"timeout_ms":1000,"authority":ActiveAuthority::from(active_authority()),"phase":"create","arrays":[]})
    } // Use the complete request schema.
    fn service_quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 0,
            worker_bytes: 16 * 1024 * 1024,
        })
    } // Use one finite original test root.
    fn typed_parts(count: usize) -> (Value, Vec<ArraySpec>, BTreeMap<String, Vec<u8>>) {
        // Transport fixtures confer no authority.
        let mut metadata = Vec::new(); // Reference each binary leaf once.
        let mut arrays = Vec::new(); // Keep typed shape out of JSON.
        let mut planes = BTreeMap::new(); // Retain fixture sources during copying.
        for index in 0..count {
            // Exercise the existing plane ceiling.
            let name = format!("b{index}"); // Restart canonical names per request.
            let (kind, bytes) = match index % 4 {
                // Preserve four kinds and native bits.
                0 => (TypedArrayKind::U8, vec![0, 255]), // Keep bytes outside JSON.
                1 => (
                    TypedArrayKind::F32,
                    [(-0.0_f32).to_ne_bytes(), 0.75_f32.to_ne_bytes()].concat(),
                ), // Preserve negative zero and fractions.
                2 => (
                    TypedArrayKind::U16,
                    [0x1234_u16.to_ne_bytes(), 0xfedc_u16.to_ne_bytes()].concat(),
                ), // Preserve native u16 byte order.
                _ => (
                    TypedArrayKind::U32,
                    [16_777_217_u32.to_ne_bytes(), u32::MAX.to_ne_bytes()].concat(),
                ), // Detect lossy integer coercion.
            }; // Recheck lengths at the boundary.
            metadata.push(json!({"$ilium_binary":name})); // Markers grant no authority.
            arrays.push(ArraySpec {
                name: name.clone(),
                kind,
                elements: 2,
            }); // Declare two elements per plane.
            planes.insert(name, bytes); // Retain physical fixture sources.
        } // Use distinct canonical planes.
        (Value::Array(metadata), arrays, planes) // Return sources for real admission.
    } // No dispatcher is simulated here.
    #[test] // Exercise real framing and custody.
    fn binary_service_pages_reuse_48_names_and_queue_custody_survives_aliases() {
        // Reuse all 48 names per page.
        let limits = EngineLimits {
            pending_requests: 1,
            ..EngineLimits::default()
        }; // Enforce the configured single slot.
        let quota = service_quota(); // Keep the original root.
        let budget = ServiceBudget::new(&limits); // Add no replacement bank.
        let (metadata, arrays, planes) = typed_parts(48); // Fill the existing plane inventory.
        for sequence in 1..=2 {
            // Reuse names across separate pages.
            let value = ServiceValue::copy_request_from_host(
                &metadata,
                &arrays,
                &planes,
                &limits,
                quota.clone(),
                &budget,
            )
            .unwrap(); // Admit before making the copy.
            let retained_bytes = quota.snapshot().worker_bytes; // Measure actual admitted bytes.
            assert!(retained_bytes > 0); // Require live payload custody.
            let alias = value.clone(); // Retain storage and slot occupancy.
            let mut record = request_record(sequence); // Correlate each page separately.
            record["payload"] = metadata.clone(); // Retain structural markers only.
            record["arrays"] =
                serde_json::to_value(arrays.iter().map(ArrayConfig::from).collect::<Vec<_>>())
                    .unwrap(); // Include all typed descriptors.
            let mut packet = Packet::new(
                sequence,
                authority(),
                "response",
                json!({"requests":[record],"terminal":null,"more":false}),
                BTreeMap::new(),
            ); // Emit one event per page.
            packet.attach_service(value.clone()).unwrap(); // Borrow immutable admitted planes.
            assert!(std::ptr::eq(
                packet.plane("b0").unwrap().as_ptr(),
                value.planes()["b0"].as_ptr()
            )); // Forbid a hidden attachment copy.
            let (sender, receiver) = mpsc::sync_channel(1); // Keep the original queue capacity.
            sender.send(packet).unwrap(); // Transfer custody into the queue.
            drop(value); // Drop only the producer alias.
            assert_eq!(quota.snapshot().worker_bytes, retained_bytes); // Preserve the original debit.
            assert!(ServiceValue::copy_request_from_host(
                &metadata,
                &arrays,
                &planes,
                &limits,
                quota.clone(),
                &budget
            )
            .is_err()); // Do not recycle an occupied slot.
            let packet = receiver.recv().unwrap(); // Recover the same packet owner.
            let mut wire = Vec::new(); // Use raw test wire, outside quota.
            write_packet(&mut wire, &packet).unwrap(); // Run real complete preflight.
            let prefix = u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize; // Decode network-order length.
            assert_eq!(wire.len(), 4 + prefix + alias.binary_bytes()); // Forbid JSON expansion or suffixes.
            let recovered = read_packet(&mut wire.as_slice()).unwrap(); // Run real receive validation.
            assert_eq!(recovered.envelope.authority, authority()); // Preserve immutable session identity.
            assert_eq!(
                recovered.envelope.payload["requests"][0]["authority"],
                serde_json::to_value(ActiveAuthority::from(active_authority())).unwrap()
            ); // Keep active stamp independently visible.
            assert_eq!(recovered.planes, planes); // Preserve every plane's exact bytes.
            drop(packet); // Drop only the writer's alias.
            assert_eq!(quota.snapshot().worker_bytes, retained_bytes); // Retain the escaped payload debit.
            drop(alias); // Destroy the final admitted owner.
            assert_eq!(quota.snapshot().worker_bytes, 0); // Release admitted ServiceValue custody.
        } // Plane names remain request-local.
    } // No OS helper/service is claimed here.
    #[test] // Preflight before any prefix write.
    fn valid_service_tree_can_fail_whole_envelope_without_writing_or_releasing_its_owner() {
        // Include complete envelope depth.
        let limits = EngineLimits::default(); // Preserve original limits.
        let quota = service_quota(); // Keep the original admission root.
        let mut nested = json!(0); // Start a depth-32 service tree.
        for _ in 0..32 {
            nested = json!([nested]);
        } // Envelope wrapping adds depth.
        let value =
            ServiceValue::copy_from_host(&nested, &[], &BTreeMap::new(), &limits, quota.clone())
                .unwrap(); // Admit the valid source first.
        let mut packet = Packet::new(
            9,
            authority(),
            "command",
            json!({"op":"complete_service","id":1,"authority":ActiveAuthority::from(active_authority()),"result":nested,"arrays":[]}),
            BTreeMap::new(),
        ); // Wrap the complete command.
        packet.attach_service(value.clone()).unwrap(); // Retain custody across refusal.
        let retained = quota.snapshot().worker_bytes; // Measure the actual debit.
        let mut output = Vec::new(); // Observe all attempted wire bytes.
        assert!(write_packet(&mut output, &packet).is_err()); // Reject oversized envelope depth.
        assert!(output.is_empty()); // Leave the pipe untouched.
        assert_eq!(quota.snapshot().worker_bytes, retained); // Refusal preserves source custody.
        drop(packet); // Drop only the refused packet.
        assert_eq!(quota.snapshot().worker_bytes, retained); // Keep the producer's debit.
        drop(value); // Destroy the final payload owner.
        assert_eq!(quota.snapshot().worker_bytes, 0); // Release admitted source custody.
        let mut mismatch = Packet::new(
            10,
            authority(),
            "response",
            json!({}),
            BTreeMap::from([("b0".into(), vec![1, 2])]),
        ); // Own the physical test plane.
        mismatch.envelope.planes[0].bytes = 1; // Corrupt only the declared length.
        assert!(write_packet(&mut output, &mismatch).is_err()); // Reject before writing a prefix.
        assert!(output.is_empty()); // Publish no partial packet.
        mismatch.envelope.planes.clear(); // Hide a physical plane.
        assert!(write_packet(&mut output, &mismatch).is_err()); // Reject undeclared physical bytes.
        assert!(output.is_empty()); // Keep the pipe untouched.
    } // Sequence commits after shared preflight.
    #[test] // Check real request lifetime rules.
    fn transport_request_deadline_and_cancel_aliases_retain_the_original_root() {
        // Separate cancellation from release.
        let limits = EngineLimits {
            pending_requests: 1,
            ..EngineLimits::default()
        }; // Use one finite request slot.
        let quota = service_quota(); // Keep the original quota root.
        let budget = ServiceBudget::new(&limits); // Retain request occupancy.
        let (metadata, arrays, planes) = typed_parts(4); // Preserve all four typed kinds.
        let payload = ServiceValue::copy_request_from_host(
            &metadata,
            &arrays,
            &planes,
            &limits,
            quota.clone(),
            &budget,
        )
        .unwrap(); // Admit the real ingress copy.
        let request = HostRequest::from_transport(
            1,
            "math.transform".into(),
            20,
            authority().package_digest,
            active_authority(),
            ServicePhase::Create,
            payload,
        )
        .unwrap(); // Assign a bounded native deadline.
        let alias = request.clone(); // Retain the request independently.
        let value_alias = request.payload.clone(); // Keep payload custody after request drop.
        let before = RequestRecord::from(&request).timeout_ms; // Export only remaining time.
        assert!(before <= 20); // Never extend the deadline.
        request.stop_token().stop(); // Signal without releasing storage.
        assert!(alias.is_cancelled()); // Share the original stop token.
        std::thread::sleep(Duration::from_millis(25)); // Cross the monotonic deadline.
        assert_eq!(alias.remaining_ms(), 0); // Preserve the original deadline.
        assert_eq!(RequestRecord::from(&alias).timeout_ms, 0); // Do not renew terminal requests.
        drop(request); // Drop one request owner.
        drop(alias); // Keep the escaped payload alive.
        assert!(quota.snapshot().worker_bytes > 0); // Preserve its admitted bytes.
        assert!(ServiceValue::copy_request_from_host(
            &metadata,
            &arrays,
            &planes,
            &limits,
            quota.clone(),
            &budget
        )
        .is_err()); // Do not recycle its occupied slot.
        assert_eq!(value_alias.planes(), &planes); // Preserve bytes after cancellation.
        budget.close(); // Close admission without release.
        assert!(quota.snapshot().worker_bytes > 0); // Closure is not physical release.
        drop(value_alias); // Drop the final payload owner.
        assert_eq!(quota.snapshot().worker_bytes, 0); // Release admitted ServiceValue custody.
    } // No native body exit is claimed.
    #[test] // Use real native preflight.
    fn binary_service_preflight_refuses_shape_and_reference_smuggling_before_admission() {
        // Reject malformed retained sources.
        let limits = EngineLimits::default(); // Preserve original hard limits.
        let quota = service_quota(); // Admit nothing on invalid input.
        let (metadata, arrays, planes) = typed_parts(4); // Begin with a valid source.
        for invalid in [
            json!({"$ilium_binary":"b0","extra":0}),
            json!([{"$ilium_binary":"b0"},{"$ilium_binary":"b0"}]),
            json!({"$ilium_binary":"missing"}),
            json!({"constructor":0}),
        ] {
            // Reject malformed reference graphs.
            assert!(ServiceValue::copy_from_host(
                &invalid,
                &arrays,
                &planes,
                &limits,
                quota.clone()
            )
            .is_err()); // Run actual metadata validation.
            assert_eq!(quota.snapshot().worker_bytes, 0); // Reject before admission.
        } // Markers cannot mint native handles.
        let mut overflow = arrays.clone(); // Corrupt shape without altering bytes.
        overflow[1].elements = usize::MAX; // Exercise checked multiplication.
        assert!(ServiceValue::copy_from_host(
            &metadata,
            &overflow,
            &planes,
            &limits,
            quota.clone()
        )
        .is_err()); // Reject shape overflow.
        let mut hidden = planes.clone(); // Add an undeclared physical plane.
        hidden.insert("extra".into(), vec![1]); // Expose hidden-byte smuggling.
        assert!(
            ServiceValue::copy_from_host(&metadata, &arrays, &hidden, &limits, quota.clone())
                .is_err()
        ); // Require complete inventory.
        assert!(ArraySpec::try_from(ArrayConfig {
            name: "b0".into(),
            kind: "f64".into(),
            elements: 1
        })
        .is_err()); // Reject unsupported element kinds.
        assert_eq!(quota.snapshot().worker_bytes, 0); // Retain zero admitted custody.
        let other = service_quota(); // Create a deliberately foreign root.
        let value =
            ServiceValue::copy_from_host(&metadata, &arrays, &planes, &limits, other.clone())
                .unwrap(); // Admit the real foreign value.
        assert!(!value.shares_root(&quota)); // Reject foreign completion custody.
        assert!(value.shares_root(&other)); // Preserve its actual root identity.
        drop(value); // Drop its final admitted owner.
        assert_eq!(other.snapshot().worker_bytes, 0); // Release its actual admission.
    } // Authorization and registries stay separate.
    #[test] // Check closed native ACKs.
    fn completion_ack_is_correlated_closed_and_refusal_is_not_delivery() {
        // ACKs do not prove body exit.
        let stamp = active_authority(); // Bind the expected active stamp.
        for state in [
            CompletionState::Delivered,
            CompletionState::Unknown,
            CompletionState::TimedOut,
            CompletionState::Cancelled,
        ] {
            // Preserve every terminal outcome.
            assert_eq!(
                completed_state(completion_record(7, stamp, Ok(state)), 7, stamp).unwrap(),
                state
            ); // Require exact ACK classification.
        } // Keep terminal states distinct.
        assert!(matches!(
            completed_state(
                completion_record(7, stamp, Err(AnimationError::Budget("copy refused".into()))),
                7,
                stamp
            ),
            Err(AnimationError::Budget(_))
        )); // Refusal must remain an error.
        assert!(completed_state(
            completion_record(7, stamp, Ok(CompletionState::Delivered)),
            8,
            stamp
        )
        .is_err()); // Reject another request's ACK.
        let stale = ServiceAuthority {
            authorization_epoch: stamp.authorization_epoch + 1,
            ..stamp
        }; // Change only mutable epoch.
        assert!(completed_state(
            completion_record(7, stamp, Ok(CompletionState::Delivered)),
            7,
            stale
        )
        .is_err()); // Reject stale active authority.
        for (state, error) in [
            ("delivered", Some("unexpected".to_owned())),
            ("refused", None),
            ("unknown_state", None),
        ] {
            // Reject contradictory ACK states.
            assert!(completed_state(
                CompletionRecord {
                    id: 7,
                    authority: stamp.into(),
                    state: state.into(),
                    error
                },
                7,
                stamp
            )
            .is_err()); // Never publish through malformed ACKs.
        } // Real IPC tests check no checkpoint.
    } // Broker publication remains separate.
}

#[cfg(test)]
pub(crate) mod isolation_qualification {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    pub(crate) fn archive(source: &str) -> Vec<u8> {
        let manifest = json!({"api_version":1,"id":"helper-qualification","name":"Helper qualification","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]});
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("entry.mjs", options).unwrap();
        archive.write_all(source.as_bytes()).unwrap();
        archive.start_file("manifest.json", options).unwrap();
        archive
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        archive.finish().unwrap().into_inner()
    }
    pub(crate) fn quota() -> QuotaGroup {
        QuotaGroup::new(QuotaLimits {
            clients: 0,
            jobs: 0,
            service_jobs: 0,
            input_bytes: 0,
            result_bytes: 0,
            worker_threads: 32,
            worker_bytes: 1024 * 1024 * 1024,
        })
    }
    pub(crate) const BOOTSTRAP: &str = r#"
        globalThis.__ilium_host=Object.freeze({http:{request:payload=>__ilium_dispatch('http.request',payload)}});
        globalThis.__ilium_make_frame=()=>({gray:new Float32Array(4),present(){this.submitted=true;}});
        globalThis.__ilium_finish_frame=frame=>({metadata:{submitted:!!frame.submitted},planes:{gray:frame.gray}});
        globalThis.__ilium_accept_frame=()=>{};
    "#;
    fn launch(source: &str, quota: QuotaGroup) -> HelperSession {
        let executable = std::env::var("ILIUM_ANIMATION_HELPER")
            .expect("qualification requires actual built helper absolute path");
        let bytes = archive(source);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let mut helper = HelperSession::launch(
            // Retain the actual sealed native helper before binding its independent accepted coordinates.
            Path::new(&executable),
            &bytes,
            BOOTSTRAP,
            HelperAuthority {
                package_digest: digest,
                instance_id: 73,
                plan_generation: 3,
                authorization_epoch: 9,
            },
            HelperLimits::default(),
            quota,
            HelperPlayback {
                mode: AnimationMode::Live,
                ambient_seed: 0,
            },
        )
        .unwrap(); // Successful launch alone does not activate package service acquisition.
        helper
            .bind_service_authority(ServiceAuthority {
                instance_id: 73,
                plan_generation: 3,
                authorization_epoch: 9,
            })
            .unwrap(); // Bind through the real authenticated helper command before seed or create.
        helper // The original immutable IPC authority remains unchanged.
    }
    #[test]
    fn helper_build_digest_requires_exact_nonzero_lowercase_sha256() {
        assert_eq!(parse_helper_digest(&"ab".repeat(32)).unwrap(), [0xab; 32]);
        for malformed in [
            "0".repeat(64),
            "A".repeat(64),
            "g".repeat(64),
            "a".repeat(63),
        ] {
            assert!(parse_helper_digest(&malformed).is_err());
        }
    }
    /// Explicit integration gate: build the real helper, set its absolute path,
    /// then run this ignored test. Ordinary unit checks cannot qualify isolation.
    #[test]
    #[ignore = "requires built helper and delegated Linux cgroup/bwrap isolation"]
    fn actual_sealed_helper_denies_syscalls_and_preserves_broker_frame_custody() {
        let quota = quota();
        let mut helper = launch(
            "export function plan(){return {fps:20};} export async function create(host){const result=await host.http.request({url:'https://example.org'});return {render(context,frame){frame.gray.fill(result.value);frame.present();},dispose(){}};}",
            quota.clone(),
        );
        let mut installed =
            std::fs::File::open(std::env::var("ILIUM_ANIMATION_HELPER").unwrap()).unwrap();
        let mut digest = Sha256::new();
        let mut scratch = [0u8; 64 * 1024];
        loop {
            let count = installed.read(&mut scratch).unwrap();
            if count == 0 {
                break;
            }
            digest.update(&scratch[..count]);
        }
        let expected: [u8; 32] = digest.finalize().into();
        assert_eq!(
            helper.build_digest(),
            expected,
            "the ready response must name the actual loaded helper image"
        );
        let resources = helper.resource_usage().unwrap();
        eprintln!("actual sealed V8 helper resources: {resources:?}");
        let probe = helper.probe().unwrap();
        assert_eq!(probe.as_object().unwrap().len(), 9);
        assert!(probe
            .as_object()
            .unwrap()
            .values()
            .all(|value| value == &Value::Bool(true)));
        assert_eq!(
            helper
                .plan(&json!({}), AnimationMode::Live, &json!({}))
                .unwrap(),
            json!({"fps":20})
        );
        assert_eq!(
            helper.start_create(&json!({}), &json!({})).unwrap(),
            CreateState::Pending
        );
        let requests = helper.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "http.request");
        assert!(helper
            .complete_request(requests[0].id + 1, &json!({"value":0.5}))
            .is_err());
        helper
            .complete_request(requests[0].id, &json!({"value":0.5}))
            .unwrap();
        assert_eq!(helper.pump().unwrap(), CreateState::Ready);
        drop(requests); // The actual escaped request alias otherwise correctly retains its original debit after completion.
        let frame = helper
            .render(
                &json!({}),
                &[ArraySpec {
                    name: "gray".into(),
                    kind: TypedArrayKind::F32,
                    elements: 4,
                }],
            )
            .unwrap();
        assert_eq!(frame.metadata, json!({"submitted":true}));
        assert_eq!(frame.planes["gray"], 0.5_f32.to_ne_bytes().repeat(4));
        helper.accept_frame(true).unwrap();
        helper.dispose().unwrap();
        drop(helper);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert!(
            quota.snapshot().worker_bytes > 0,
            "retained frame must remain admitted after helper disposal"
        );
        drop(frame);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
    #[test]
    #[ignore = "requires built helper and delegated Linux cgroup/bwrap isolation"]
    fn actual_helper_cancellation_and_render_oversize_fail_closed() {
        let quota = quota();
        let mut helper = launch(
            "export function plan(){return {};} export async function create(){return {render(context,frame){frame.present();},dispose(){}};}",
            quota.clone(),
        );
        assert_eq!(
            helper.start_create(&json!({}), &json!({})).unwrap(),
            CreateState::Ready
        );
        assert!(helper
            .render(
                &json!({}),
                &[ArraySpec {
                    name: "gray".into(),
                    kind: TypedArrayKind::F32,
                    elements: usize::MAX
                }]
            )
            .is_err());
        helper.cancel().unwrap();
        assert!(helper.pump().is_err());
        drop(helper);
        assert_eq!(quota.snapshot().worker_threads, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }
}

#[cfg(test)]
#[path = "helper_boundary_tests.rs"]
mod boundary_tests;
