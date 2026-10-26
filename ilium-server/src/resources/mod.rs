//! Native shared-user resource service. The catalog is the sole authority.
mod identity;
mod memory;
mod projections;
pub mod store;
use identity::project_identity;
use memory::{committed_catalog, io, Catalog, Eligibility, ResourceMemory};
#[cfg(test)]
mod service_tests;
use crate::{ipc::DirectEventSender, pane::PaneResource, state::ServerState};
use ilium_core::{resource_queue::*, AllocationSize, NodeId};
use ilium_ipc::{resource::*, ServerEvent};
use ilium_platform::resource_fs::{
    capture_resource_process, resource_clock, resource_process_is_live, ResourceProcessIdentity,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[derive(Default)]
pub(super) struct WaitRegistry {
    pub revision: u64,
    pub entries: BTreeMap<ReservationId, (NodeId, usize)>,
}
const MAX_WAIT_ATTACHMENTS_PER_RESERVATION: usize = 64;
impl WaitRegistry {
    fn attach(&mut self, reservation: ReservationId, pane: NodeId) -> Result<bool, ResourceError> {
        if let Some((registered_pane, count)) = self.entries.get_mut(&reservation) {
            if *registered_pane != pane {
                return Err(err("actor", "Wait reservation belongs to another pane"));
            }
            if *count >= MAX_WAIT_ATTACHMENTS_PER_RESERVATION {
                return Err(err("capacity", "Too many wait attachments"));
            }
            *count += 1;
            Ok(false)
        } else {
            let revision = self
                .revision
                .checked_add(1)
                .ok_or_else(|| err("capacity", "Wait sequence exhausted"))?;
            self.entries.insert(reservation, (pane, 1));
            self.revision = revision;
            Ok(true)
        }
    }
}
pub(crate) struct ResourceService {
    pub path: PathBuf,
    pub waits: std::sync::Mutex<WaitRegistry>,
    pub incarnation: String,
    pub changed: tokio::sync::watch::Sender<u64>,
    pub confirmations: tokio::sync::Mutex<BTreeMap<String, (ResourceId, u64, u64)>>,
}
fn err(code: &str, message: impl ToString) -> ResourceError {
    ResourceError {
        code: code.into(),
        message: message.to_string(),
    }
}
fn clock() -> Result<ResourceClock, ResourceError> {
    let c = resource_clock().map_err(|e| err("clock", e))?;
    Ok(ResourceClock {
        unix_millis: c.unix_millis,
        monotonic_millis: c.monotonic_millis,
        boot_id: c.boot_id,
    })
}
fn key(c: &ResourceClock) -> RequestId {
    RequestId(format!("{}:{}", c.unix_millis, uuid::Uuid::new_v4()))
}
async fn resolve_project_identity(
    state: &ServerState,
    path: &std::path::Path,
) -> Result<ResourceProjectId, ResourceError> {
    let path = path.to_path_buf();
    let identity = io(state, move || project_identity(&path)).await?;
    Ok(identity.value)
}

impl ResourceService {
    pub fn new(path: PathBuf) -> Self {
        Self {
            waits: Default::default(),
            path,
            incarnation: uuid::Uuid::new_v4().to_string(),
            changed: tokio::sync::watch::channel(0).0,
            confirmations: Default::default(),
        }
    }
}
fn service(state: &ServerState) -> Result<&ResourceService, ResourceError> {
    state
        .resources
        .get()
        .ok_or_else(|| err("unavailable", "Resource authority is unavailable"))
}
async fn decode_rpc(
    state: &ServerState,
    frame: ilium_ipc::EncodedFrame,
) -> Result<ResourceMemory<ServerEvent>, ResourceError> {
    io(state, move || {
        ilium_ipc::decode_bounded_frame::<ServerEvent>(&frame).map_err(|e| err("decode", e))
    })
    .await
}
async fn read(state: &ServerState) -> Result<Catalog, ResourceError> {
    let path = service(state)?.path.clone();
    io(state, move || {
        store::ResourceStore::new(path)
            .and_then(|s| s.snapshot())
            .map_err(store_error)
    })
    .await
    .map(Arc::new)
}
fn store_error(e: store::ResourceStoreError) -> ResourceError {
    match e {
        store::ResourceStoreError::Domain(e) => err(&e.code, e.message),
        store::ResourceStoreError::Busy => err("busy", "Catalog busy; retry the same request key"),
        store::ResourceStoreError::OutcomeUnknown(e) => err("outcome_unknown", e),
        e => err("catalog", e),
    }
}
async fn commit(
    state: &ServerState,
    command: ResourceCommand,
    mut context: MutationContext,
    eligibility: Option<Eligibility>,
) -> Result<ResourceMemory<store::CommittedResourceChange>, ResourceError> {
    let path = service(state)?.path.clone();
    // Try both locks without waiting while holding another lock. Keeping these
    // read guards until the durable transaction finishes fences tree moves and
    // provider replacement throughout an agent-authorized mutation.
    let fence = if let ResourceActor::Agent(agent) = &context.actor {
        let (_, project_path) = project(state, None, Some(agent.pane_id)).await?;
        if resolve_project_identity(state, &project_path).await? != agent.project_id {
            return Err(err("stale_invocation", "Agent project changed"));
        }
        let proof = process_from_invocation(&agent.invocation_id)
            .ok_or_else(|| err("stale_invocation", "Invalid invocation process proof"))?;
        let generation = agent
            .invocation_id
            .0
            .rsplit(':')
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| err("stale_invocation", "Invalid invocation generation"))?;
        let tree = state.tree.try_read().map_err(|_| {
            err(
                "busy",
                "Project tree is changing; retry the same request key",
            )
        })?;
        if tree
            .project_ancestor(agent.pane_id)
            .and_then(|id| tree.project_path_for(id))
            != Some(project_path.as_path())
        {
            return Err(err("stale_invocation", "Agent project changed"));
        }
        let panes = state
            .panes
            .try_read()
            .map_err(|_| err("busy", "Agent pane is changing; retry the same request key"))?;
        if !matches!(panes.get(&agent.pane_id),Some(PaneResource::Terminal(runtime)) if runtime.detected_agent_process_id==Some(proof.process_id)&&runtime.agent_generation==generation)
        {
            return Err(err(
                "stale_invocation",
                "Agent invocation changed before mutation",
            ));
        }
        Some((tree, panes, proof))
    } else {
        None
    };
    let process = fence.as_ref().map(|(_, _, proof)| proof.clone());
    let result = io(state, move || {
        if let Some(proof) = process {
            if !resource_process_is_live(&proof).map_err(|e| err("actor", e))? {
                return Err(err(
                    "stale_invocation",
                    "Agent process exited before mutation",
                ));
            }
        }
        if let Some(eligibility) = eligibility {
            context.eligibility = eligibility.value.clone();
        }
        store::ResourceStore::new(path)
            .and_then(|s| s.transact(command, context))
            .map_err(store_error)
    })
    .await;
    drop(fence);
    result
}

fn projection(mut catalog: ResourceCatalog) -> ResourceCatalog {
    catalog.receipts.clear();
    for r in catalog.resources.values_mut() {
        let history: Vec<_> = r
            .reservations
            .iter()
            .rev()
            .filter(|claim| !claim.state.active())
            .take(16)
            .map(|claim| claim.id.clone())
            .collect();
        r.reservations
            .retain(|q| q.state.active() || history.contains(&q.id));
    }
    catalog
}
fn locate<'a>(
    catalog: &'a ResourceCatalog,
    id: &ReservationId,
) -> Result<&'a Reservation, ResourceError> {
    catalog
        .resources
        .values()
        .flat_map(|r| &r.reservations)
        .find(|q| q.id == *id)
        .ok_or_else(|| err("not_found", "Reservation not found"))
}
async fn project(
    state: &ServerState,
    requested: Option<NodeId>,
    pane: Option<NodeId>,
) -> Result<(NodeId, PathBuf), ResourceError> {
    let tree = state.tree.read().await;
    let id = requested
        .or_else(|| pane.and_then(|p| tree.project_ancestor(p)))
        .or_else(|| {
            tree.project_ids()
                .into_iter()
                .find(|p| tree.project_path_for(*p) == Some(state.session_cwd.as_path()))
        })
        .ok_or_else(|| err("project", "Select a project"))?;
    let path = tree
        .get(id)
        .and_then(|n| n.project_path())
        .ok_or_else(|| err("project", "Selected node is not a project"))?
        .to_path_buf();
    if pane.is_some_and(|p| tree.project_ancestor(p) != Some(id)) {
        return Err(err("actor", "Agent does not belong to selected project"));
    }
    Ok((id, path))
}
async fn actor(
    state: &ServerState,
    pane_id: Option<NodeId>,
    path: &std::path::Path,
) -> Result<ResourceActor, ResourceError> {
    let Some(pane_id) = pane_id else {
        return Ok(ResourceActor::User);
    };
    let (process_id, generation, label) = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(err("actor", "Agent pane is not a live terminal"));
        };
        let process = runtime.detected_agent_process_id.ok_or_else(|| {
            err(
                "actor",
                "Agent invocation has not been confirmed by detection",
            )
        })?;
        (
            process,
            runtime.agent_generation,
            format!("Agent {}", pane_id.0),
        )
    };
    let proof = io(state, move || {
        capture_resource_process(process_id).map_err(|e| err("actor", e))
    })
    .await?;
    {
        let panes = state.panes.read().await;
        if !matches!(panes.get(&pane_id),Some(PaneResource::Terminal(runtime)) if runtime.detected_agent_process_id==Some(process_id)&&runtime.agent_generation==generation)
        {
            return Err(err(
                "stale_invocation",
                "Agent invocation changed while capturing its process proof",
            ));
        }
    }
    Ok(ResourceActor::Agent(AgentIdentity {
        project_id: resolve_project_identity(state, path).await?,
        session_id: state.session_name.clone(),
        pane_id,
        runtime_generation: service(state)?.incarnation.clone(),
        invocation_id: InvocationId(format!(
            "{}:{}:{}:{}",
            proof.boot_id, proof.process_id, proof.started_at, generation
        )),
        label,
    }))
}
fn process_from_invocation(id: &InvocationId) -> Option<ResourceProcessIdentity> {
    let mut parts = id.0.split(':');
    Some(ResourceProcessIdentity {
        boot_id: parts.next()?.into(),
        process_id: parts.next()?.parse().ok()?,
        started_at: parts.next()?.parse().ok()?,
    })
}
async fn evidence(state: &ServerState, catalog: &Catalog) -> Result<Eligibility, ResourceError> {
    let source = Arc::clone(catalog);
    let claims = Arc::new(
        io(state, move || {
            Ok(source
                .resources
                .values()
                .flat_map(|r| &r.reservations)
                .filter(|q| q.state.active())
                .map(|q| q.agent.clone())
                .collect::<Vec<_>>())
        })
        .await?,
    );
    let bytes = claims
        .len()
        .saturating_mul(std::mem::size_of::<(NodeId, Option<(u32, u64)>)>())
        .saturating_add(4096);
    let storage =
        memory::reserve_after_contention(state, bytes, "Invocation observation metadata").await?;
    let local = {
        let panes = state.panes.read().await;
        let mut local = Vec::with_capacity(claims.len());
        for agent in &claims.value {
            if agent.runtime_generation == service(state)?.incarnation {
                let current = match panes.get(&agent.pane_id) {
                    Some(PaneResource::Terminal(runtime)) => runtime
                        .detected_agent_process_id
                        .map(|p| (p, runtime.agent_generation)),
                    _ => None,
                };
                local.push((agent.pane_id, current));
            }
        }
        ResourceMemory::from_parts(local, storage)
    };
    let process_claims = Arc::clone(&claims);
    let mut eligibility = io(state, move || {
        let mut result = BTreeMap::new();
        for claim in &process_claims.value {
            let status = if let Some(proof) = process_from_invocation(&claim.invocation_id) {
                match resource_process_is_live(&proof) {
                    Ok(false) => InvocationEligibility::Dead,
                    Err(_) => InvocationEligibility::Unknown,
                    Ok(true) => {
                        if let Some((_, current)) =
                            local.iter().find(|(pane, _)| *pane == claim.pane_id)
                        {
                            match current {
                                Some((pid, generation))
                                    if *pid == proof.process_id
                                        && claim
                                            .invocation_id
                                            .0
                                            .ends_with(&format!(":{generation}")) =>
                                {
                                    InvocationEligibility::Live
                                }
                                Some(_) => InvocationEligibility::Dead,
                                None => InvocationEligibility::Unknown,
                            }
                        } else {
                            InvocationEligibility::Unknown
                        }
                    }
                }
            } else {
                InvocationEligibility::Unknown
            };
            result.insert(claim.invocation_id.clone(), status);
        }
        Ok(result)
    })
    .await?;
    for agent in claims.value.iter().filter(|agent| {
        agent.runtime_generation != service(state).map(|s| s.incarnation.as_str()).unwrap_or("")
    }) {
        if eligibility.get(&agent.invocation_id) == Some(&InvocationEligibility::Dead) {
            continue;
        }
        let path = service(state)?
            .path
            .parent()
            .ok_or_else(|| err("paths", "Catalog directory absent"))?
            .join(format!("server-{}.json", agent.runtime_generation));
        let socket = io(state, move || {
            use std::io::Read;
            let file = std::fs::File::open(path).map_err(|e| err("observation", e))?;
            let mut bytes = Vec::with_capacity(4097);
            file.take(4097)
                .read_to_end(&mut bytes)
                .map_err(|e| err("observation", e))?;
            if bytes.len() > 4096 {
                return Err(err("observation", "Endpoint record exceeds limit"));
            }
            serde_json::from_slice::<PathBuf>(&bytes).map_err(|e| err("observation", e))
        })
        .await;
        let proof = async {
            let socket = socket?;
            let endpoint = ilium_transport::SessionEndpoint::from_path(&socket.value);
            let mut stream = endpoint
                .connect()
                .await
                .map_err(|e| err("observation", e))?;
            let request = ilium_ipc::ClientRequest::Resource {
                request_id: 1,
                request: ResourceRequest::new(
                    None,
                    ResourceOperation::ValidateInvocation {
                        agent: agent.clone(),
                    },
                ),
            };
            ilium_ipc::write_frame(&mut stream, &request)
                .await
                .map_err(|e| err("observation", e))?;
            let mut reader = ilium_ipc::FrameReader::new(&mut stream);
            loop {
                let length = reader
                    .read_encoded_length()
                    .await
                    .map_err(|e| err("observation", e))?;
                if length > 64 * 1024 {
                    return Err(err(
                        "observation",
                        "Invocation proof frame exceeds bounded capacity",
                    ));
                }
                let frame = reader
                    .read_encoded_payload(length)
                    .await
                    .map_err(|e| err("observation", e))?;
                let event = decode_rpc(state, frame).await?;
                if let ServerEvent::ResourceResult {
                    request_id: 1,
                    result,
                } = event.value
                {
                    return result.map(|_| ());
                }
            }
        };
        let status = match tokio::time::timeout(std::time::Duration::from_secs(2), proof).await {
            Ok(Ok(())) => InvocationEligibility::Live,
            Ok(Err(e)) if e.code == "stale_invocation" => InvocationEligibility::Dead,
            _ => InvocationEligibility::Unknown,
        };
        eligibility.insert(agent.invocation_id.clone(), status);
    }
    Ok(Arc::new(eligibility))
}
async fn reconciled(state: &ServerState) -> Result<(Catalog, ResourceClock), ResourceError> {
    let mut catalog = read(state).await?;
    let now = clock()?;
    let eligibility = evidence(state, &catalog).await?;
    if catalog.boot_id != now.boot_id {
        catalog = committed_catalog(
            commit(
                state,
                ResourceCommand::ReconcileBoot,
                MutationContext {
                    request_id: key(&now),
                    clock: now.clone(),
                    actor: ResourceActor::System,
                    expected_revision: Some(catalog.revision),
                    eligibility: BTreeMap::new(),
                },
                Some(Arc::clone(&eligibility)),
            )
            .await?,
        )
        .0;
    }
    let source = Arc::clone(&catalog);
    let observed = Arc::clone(&eligibility);
    let dead = io(state, move || {
        Ok(source
            .resources
            .values()
            .flat_map(|r| &r.reservations)
            .filter(|q| {
                matches!(q.state, ReservationState::Queued | ReservationState::Owned)
                    && observed.get(&q.agent.invocation_id) == Some(&InvocationEligibility::Dead)
            })
            .map(|q| q.agent.clone())
            .collect::<Vec<_>>())
    })
    .await?;
    for agent in &dead.value {
        catalog = committed_catalog(
            commit(
                state,
                ResourceCommand::ReconcileInvocation {
                    agent: agent.clone(),
                    evidence: InvocationEligibility::Dead,
                },
                MutationContext {
                    request_id: key(&now),
                    clock: now.clone(),
                    actor: ResourceActor::System,
                    expected_revision: Some(catalog.revision),
                    eligibility: BTreeMap::new(),
                },
                Some(Arc::clone(&eligibility)),
            )
            .await?,
        )
        .0;
    }
    let promotable = catalog.resources.values().any(|r| {
        r.gate == ResourceGate::Open
            && !r.clearance_required
            && !r.reservations.iter().any(|q| q.state.occupied())
            && r.reservations
                .iter()
                .filter(|q| q.state == ReservationState::Queued)
                .min_by_key(|q| q.ticket)
                .is_some_and(|q| {
                    eligibility.get(&q.agent.invocation_id) == Some(&InvocationEligibility::Live)
                })
    });
    if promotable {
        catalog = committed_catalog(
            commit(
                state,
                ResourceCommand::ReconcileEligibility,
                MutationContext {
                    request_id: key(&now),
                    clock: now.clone(),
                    actor: ResourceActor::System,
                    expected_revision: Some(catalog.revision),
                    eligibility: BTreeMap::new(),
                },
                Some(Arc::clone(&eligibility)),
            )
            .await?,
        )
        .0;
    }
    Ok((catalog, now))
}
async fn local_invocations(
    state: &ServerState,
    catalog: &ResourceCatalog,
) -> Result<ResourceMemory<Vec<AgentIdentity>>, ResourceError> {
    let mut count = 0usize;
    let bytes = catalog
        .resources
        .values()
        .flat_map(|r| &r.reservations)
        .filter(|q| q.state.active())
        .fold(4096usize, |bytes, claim| {
            count += 1;
            bytes.saturating_add(claim.agent.retained_bytes().saturating_mul(2))
        });
    let storage =
        memory::reserve_after_contention(state, bytes, "Local invocation projection").await?;
    let mut live = Vec::with_capacity(count);
    for claim in catalog
        .resources
        .values()
        .flat_map(|resource| &resource.reservations)
        .filter(|claim| claim.state.active())
    {
        if service(state)?.incarnation != claim.agent.runtime_generation {
            continue;
        }
        let Ok((_, path)) = project(state, None, Some(claim.agent.pane_id)).await else {
            continue;
        };
        if let Ok(ResourceActor::Agent(current)) =
            actor(state, Some(claim.agent.pane_id), &path).await
        {
            if current.same_invocation(&claim.agent) {
                live.push(current);
            }
        }
    }
    Ok(ResourceMemory::from_parts(live, storage))
}
fn active_waits(
    state: &ServerState,
) -> Result<ResourceMemory<(u64, Vec<(NodeId, ReservationId)>)>, ResourceError> {
    let registry = service(state)?
        .waits
        .lock()
        .map_err(|_| err("wait", "Wait registry unavailable"))?;
    let bytes = registry.entries.iter().fold(
        4096usize.saturating_add(
            registry
                .entries
                .len()
                .saturating_mul(std::mem::size_of::<(NodeId, ReservationId)>()),
        ),
        |bytes, (id, _)| bytes.saturating_add(id.heap_bytes()),
    );
    let storage = memory::reserve_storage(state, bytes)?;
    let entries = registry
        .entries
        .iter()
        .map(|(id, (pane, _))| (*pane, id.clone()))
        .collect();
    Ok(ResourceMemory::from_parts(
        (registry.revision, entries),
        storage,
    ))
}
async fn snapshot_from(
    state: &ServerState,
    catalog: Catalog,
    clock: ResourceClock,
) -> Result<ResourceMemory<ServerEvent>, ResourceError> {
    let local_invocations = local_invocations(state, &catalog).await?;
    let waits = active_waits(state)?;
    io(state, move || {
        // Move the full envelopes into the worker, rather than letting precise
        // closure capture retire their leases before execution admission.
        let ((wait_revision, active_waits), wait_hold) = waits.into_parts();
        let (local_invocations, local_hold) = local_invocations.into_parts();
        let event = ServerEvent::ResourceSnapshot {
            active_waits,
            wait_revision,
            local_invocations,
            catalog: projection(catalog.value.clone()),
            clock,
        };
        drop((wait_hold, local_hold));
        Ok(event)
    })
    .await
}
pub(crate) async fn snapshot(
    state: &ServerState,
) -> Result<ResourceMemory<ServerEvent>, ResourceError> {
    let (catalog, clock) = reconciled(state).await?;
    snapshot_from(state, catalog, clock).await
}
// Keep resource reconciliation out of the inline future layout of every
// connection's Attach branch. Heap placement changes no admission or payload
// ownership: ResourceMemory still carries the catalog/event allocation lease.
pub(crate) fn send_snapshot<'a>(
    state: &'a ServerState,
    replies: &'a DirectEventSender,
) -> std::pin::Pin<Box<impl std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(send_snapshot_inner(state, replies))
}
async fn send_snapshot_inner(state: &ServerState, replies: &DirectEventSender) {
    match snapshot(state).await {
        Ok(event) => {
            if let Err(error) = memory::send(state, replies, event).await {
                let _ = replies
                    .send(ServerEvent::ResourceObservationFailed {
                        message: error.to_string(),
                    })
                    .await;
            }
        }
        Err(error) => {
            let _ = replies
                .send(ServerEvent::ResourceObservationFailed {
                    message: error.to_string(),
                })
                .await;
        }
    }
}
// A blocking wait embeds execute/reconciliation more than once. Without a
// heap boundary even unrelated IPC requests inherit that entire state machine
// through the request enum's largest branch.
pub(crate) fn handle<'a>(
    state: &'a Arc<ServerState>,
    request_id: u64,
    request: ResourceRequest,
    replies: &'a DirectEventSender,
) -> std::pin::Pin<Box<impl std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(handle_inner(state, request_id, request, replies))
}
async fn handle_inner(
    state: &Arc<ServerState>,
    request_id: u64,
    request: ResourceRequest,
    replies: &DirectEventSender,
) {
    let waiting = matches!(request.operation, ResourceOperation::Wait { .. });
    let result = if waiting {
        wait(state, &request, replies).await
    } else {
        execute(state, request).await
    };
    match result {
        Ok(reply) => {
            let event = reply.map(|reply| ServerEvent::ResourceResult {
                request_id,
                result: Ok(reply),
            });
            if let Err(error) = memory::send(state, replies, event).await {
                let _ = replies
                    .send(ServerEvent::ResourceResult {
                        request_id,
                        result: Err(error),
                    })
                    .await;
            }
        }
        Err(error) => {
            let _ = replies
                .send(ServerEvent::ResourceResult {
                    request_id,
                    result: Err(error),
                })
                .await;
        }
    }
}
async fn execute(
    state: &ServerState,
    mut request: ResourceRequest,
) -> Result<ResourceMemory<ResourceReply>, ResourceError> {
    if let ResourceOperation::ValidateInvocation { agent } = &request.operation {
        if service(state)?.incarnation != agent.runtime_generation
            || state.session_name != agent.session_id
        {
            return Err(err(
                "stale_invocation",
                "Invocation server generation changed",
            ));
        }
        let (_, path) = project(state, None, Some(agent.pane_id)).await?;
        let current = actor(state, Some(agent.pane_id), &path).await?;
        if !matches!(current,ResourceActor::Agent(ref actual) if actual.same_invocation(agent)) {
            return Err(err("stale_invocation", "Agent invocation changed"));
        }
        let now = clock()?;
        return io(state, move || {
            Ok(ResourceReply {
                active_waits: Vec::new(),
                wait_revision: 0,
                local_invocations: Vec::new(),
                catalog: ResourceCatalog::default(),
                clock: now,
                result: None,
                reservation: None,
                recovery_token: None,
                message: Some("live".into()),
            })
        })
        .await;
    }
    let (_, path) = project(state, request.project_id, request.pane_id).await?;
    let (mut catalog, now) = reconciled(state).await?;
    let mut result = None;
    let mut reservation = None;
    let mut token = None;
    let mut message = None;
    match request.operation {
        ResourceOperation::ValidateInvocation { .. } => {
            return Err(err("operation", "Invocation validation dispatch failed"))
        }
        ResourceOperation::Snapshot => {}
        ResourceOperation::RepairInstructions { resource_id } => {
            if request.pane_id.is_some() {
                return Err(err(
                    "actor",
                    "Instruction repair is a project management action",
                ));
            }
            let project_id = resolve_project_identity(state, &path).await?;
            if !catalog
                .links
                .iter()
                .any(|link| link.project_id == project_id && link.resource_id == resource_id)
            {
                return Err(err("not_found", "Resource is not linked to this project"));
            }
            message = projections::synchronize_all(state, &mut catalog, Some(&path)).await?;
            service(state)?.changed.send_replace(catalog.revision);
        }
        ResourceOperation::Status { reservation_id }
        | ResourceOperation::Wait { reservation_id } => {
            let q = locate(&catalog, &reservation_id)?.clone();
            let who = actor(state, request.pane_id, &path).await?;
            if let ResourceActor::Agent(agent) = &who {
                if !agent.same_invocation(&q.agent) {
                    return Err(err("actor", "Reservation belongs to another invocation"));
                }
            }
            if q.state == ReservationState::Owned
                && q.acquisition_pending
                && matches!(who, ResourceActor::Agent(_))
            {
                catalog = committed_catalog(
                    commit(
                        state,
                        ResourceCommand::Acknowledge {
                            reservation_id: q.id.clone(),
                            ownership_generation: q.ownership_generation,
                        },
                        MutationContext {
                            request_id: key(&now),
                            clock: now.clone(),
                            actor: who,
                            expected_revision: None,
                            eligibility: BTreeMap::new(),
                        },
                        Some(evidence(state, &catalog).await?),
                    )
                    .await?,
                )
                .0;
            }
            reservation = Some(locate(&catalog, &reservation_id)?.clone());
        }
        ResourceOperation::PrepareRecovery { resource_id } => {
            if request.pane_id.is_some() {
                return Err(err(
                    "confirmation",
                    "Recovery requires the interactive resource view or terminal confirmation",
                ));
            }
            if !catalog.resources.contains_key(&resource_id) {
                return Err(err("not_found", "Resource not found"));
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            let mut tokens = service(state)?.confirmations.lock().await;
            tokens.retain(|_, (_, _, expires)| *expires > now.monotonic_millis);
            if tokens.len() >= 128 {
                return Err(err(
                    "capacity",
                    "Too many outstanding recovery confirmations",
                ));
            }
            tokens.insert(
                nonce.clone(),
                (resource_id, catalog.revision, now.monotonic_millis + 120000),
            );
            token = Some(nonce);
        }
        ResourceOperation::Mutate(ref mut command) => {
            if matches!(
                command,
                ResourceCommand::ReconcileBoot
                    | ResourceCommand::ReconcileEligibility
                    | ResourceCommand::ReconcileInvocation { .. }
                    | ResourceCommand::SetSyncDebt { .. }
            ) {
                return Err(err(
                    "system_required",
                    "Lifecycle operations are server internal",
                ));
            }
            match command {
                ResourceCommand::Create { link, .. } | ResourceCommand::Link { link } => {
                    link.project_id = resolve_project_identity(state, &path).await?;
                    link.project_path = path.to_string_lossy().into_owned();
                }
                ResourceCommand::Unlink { project_id, .. } => {
                    *project_id = resolve_project_identity(state, &path).await?
                }
                _ => {}
            }
            let current_project = resolve_project_identity(state, &path).await?;
            if let ResourceCommand::RebindProject {
                project_id,
                project_path,
            } = command
            {
                if *project_id != current_project || std::path::Path::new(project_path) != path {
                    return Err(err("project","Rebind must name this project's stable identity and canonical current path"));
                }
                let previous_paths: Vec<_> = catalog
                    .links
                    .iter()
                    .filter(|link| {
                        link.project_id == current_project
                            && std::path::Path::new(&link.project_path) != path
                    })
                    .map(|link| PathBuf::from(&link.project_path))
                    .collect();
                let new_path = path.clone();
                io(state,move||{for previous in previous_paths{match previous.canonicalize(){Ok(existing)if existing!=new_path=>return Err(err("project_identity_conflict","The previous bound project still exists at a different path; refusing to rebind a copied project identity")),Ok(_)=>{},Err(error)if error.kind()==std::io::ErrorKind::NotFound=>{},Err(error)=>return Err(err("project",error))}}Ok(())}).await?;
            } else if catalog.links.iter().any(|link| {
                link.project_id == current_project
                    && std::path::Path::new(&link.project_path) != path
            }) {
                return Err(err("project_moved","Project moved; use resource rebind before mutating its resource links or claims"));
            }
            let who = actor(state, request.pane_id, &path).await?;
            let replay = request
                .request_key
                .as_ref()
                .and_then(|key| catalog.receipts.get(&RequestId(key.clone())))
                .is_some_and(|receipt| receipt.command == *command && receipt.actor == who);
            if !replay
                && matches!(
                    command,
                    ResourceCommand::Revoke { .. }
                        | ResourceCommand::ConfirmClear { .. }
                        | ResourceCommand::CancelByUser { .. }
                )
            {
                if !matches!(who, ResourceActor::User) {
                    return Err(err(
                        "confirmation",
                        "Only an interactive user may recover a resource",
                    ));
                }
                let resource = match command {
                    ResourceCommand::ConfirmClear { resource_id, .. } => resource_id.clone(),
                    ResourceCommand::Revoke { reservation_id, .. }
                    | ResourceCommand::CancelByUser { reservation_id, .. } => {
                        locate(&catalog, reservation_id)?.resource_id.clone()
                    }
                    _ => unreachable!(),
                };
                let nonce = request
                    .recovery_token
                    .as_ref()
                    .ok_or_else(|| err("confirmation", "Fresh confirmation required"))?;
                let confirmations = service(state)?.confirmations.lock().await;
                let proof = confirmations.get(nonce).cloned().ok_or_else(|| {
                    err("confirmation", "Confirmation expired or already consumed")
                })?;
                if proof.0 != resource
                    || proof.1 != catalog.revision
                    || proof.2 < now.monotonic_millis
                    || request.expected_revision != Some(catalog.revision)
                {
                    return Err(err(
                        "stale_confirmation",
                        "Resource changed; inspect and confirm again",
                    ));
                }
            }
            if matches!(
                command,
                ResourceCommand::Reserve { .. }
                    | ResourceCommand::Release { .. }
                    | ResourceCommand::Cancel { .. }
                    | ResourceCommand::Estimate { .. }
                    | ResourceCommand::Acknowledge { .. }
            ) && !matches!(who, ResourceActor::Agent(_))
            {
                return Err(err(
                    "actor",
                    "An explicit live agent invocation is required",
                ));
            }
            let mut eligibility = evidence(state, &catalog).await?;
            if let ResourceActor::Agent(a) = &who {
                let agent = a.clone();
                let previous = eligibility;
                eligibility = Arc::new(
                    io(state, move || {
                        let mut map = previous.value.clone();
                        map.insert(agent.invocation_id, InvocationEligibility::Live);
                        Ok(map)
                    })
                    .await?,
                );
            }
            let committed = commit(
                state,
                command.clone(),
                MutationContext {
                    request_id: request.request_key.map(RequestId).ok_or_else(|| {
                        err("request_key", "Mutations require a timestamped retry key")
                    })?,
                    clock: now.clone(),
                    actor: who,
                    expected_revision: request.expected_revision,
                    eligibility: BTreeMap::new(),
                },
                Some(Arc::clone(&eligibility)),
            )
            .await?;
            let (next, command_result) = committed_catalog(committed);
            catalog = next;
            result = Some(command_result);
            if let Some(nonce) = request.recovery_token.as_ref() {
                service(state)?.confirmations.lock().await.remove(nonce);
            }
            if matches!(
                command,
                ResourceCommand::Create { .. }
                    | ResourceCommand::Link { .. }
                    | ResourceCommand::Edit { .. }
                    | ResourceCommand::Unlink { .. }
                    | ResourceCommand::Archive { .. }
                    | ResourceCommand::RebindProject { .. }
            ) {
                match projections::synchronize_all(state, &mut catalog, Some(&path)).await {
                    Ok(warning) => message = warning,
                    Err(error) => {
                        message = Some(format!(
                        "Resource committed; instruction/settings synchronization needs repair: {}",
                        error.message
                    ))
                    }
                }
            }
            service(state)?.changed.send_replace(catalog.revision);
            if let Some(id) = result.as_ref().and_then(|r| r.reservation_id.as_ref()) {
                reservation = Some(locate(&catalog, id)?.clone());
            }
            if let Ok(event) = snapshot_from(state, Arc::clone(&catalog), now.clone()).await {
                let _ = memory::publish(state, event);
            }
        }
    }
    let local_invocations = local_invocations(state, &catalog).await?;
    let waits = active_waits(state)?;
    io(state, move || {
        let ((wait_revision, active_waits), wait_hold) = waits.into_parts();
        let (local_invocations, local_hold) = local_invocations.into_parts();
        let reply = ResourceReply {
            active_waits,
            wait_revision,
            local_invocations,
            catalog: projection(catalog.value.clone()),
            clock: now,
            result,
            reservation,
            recovery_token: token,
            message,
        };
        drop((wait_hold, local_hold));
        Ok(reply)
    })
    .await
}
struct WaitGuard<'a> {
    state: &'a ServerState,
    pane: NodeId,
    reservation: ReservationId,
}
impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        if let Ok(service) = service(self.state) {
            if let Ok(mut waits) = service.waits.lock() {
                if let Some((_, count)) = waits.entries.get_mut(&self.reservation) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        waits.entries.remove(&self.reservation);
                        waits.revision = waits.revision.saturating_add(1);
                        let _ = self.state.events.publish(ServerEvent::ResourceWaitChanged {
                            wait_revision: waits.revision,
                            pane_id: self.pane,
                            reservation_id: None,
                        });
                    }
                }
            }
        }
    }
}
async fn wait(
    state: &ServerState,
    request: &ResourceRequest,
    replies: &DirectEventSender,
) -> Result<ResourceMemory<ResourceReply>, ResourceError> {
    let ResourceOperation::Wait { reservation_id } = &request.operation else {
        return Err(err("operation", "Expected wait"));
    };
    let pane = request
        .pane_id
        .ok_or_else(|| err("actor", "Wait requires an agent pane"))?;
    // Authenticate before publishing pane state; rejected callers cannot spoof waiting badges.
    let initial = execute(state, request.clone()).await?;
    if initial
        .reservation
        .as_ref()
        .is_some_and(|claim| claim.state != ReservationState::Queued)
    {
        return Ok(initial);
    }
    drop(initial);
    {
        let mut waits = service(state)?
            .waits
            .lock()
            .map_err(|_| err("wait", "Wait registry unavailable"))?;
        if waits.attach(reservation_id.clone(), pane)? {
            let _ = state.events.publish(ServerEvent::ResourceWaitChanged {
                wait_revision: waits.revision,
                pane_id: pane,
                reservation_id: Some(reservation_id.clone()),
            });
        }
    }
    let _guard = WaitGuard {
        state,
        pane,
        reservation: reservation_id.clone(),
    };
    let mut updates = service(state)?.changed.subscribe();
    let outcome = loop {
        let current = execute(state, request.clone()).await;
        match current {
            Ok(reply) => match reply.reservation.as_ref().map(|q| &q.state) {
                Some(ReservationState::Owned) => break Ok(reply),
                Some(ReservationState::Queued) => {}
                Some(ReservationState::NeedsAttention { .. }) => {
                    break Err(err(
                        "needs_attention",
                        "Ownership requires guarded recovery",
                    ))
                }
                _ => break Err(err("terminal", "Reservation is no longer queued or owned")),
            },
            Err(error) => break Err(error),
        }
        tokio::select! {_=replies.closed()=>break Err(err("interrupted","Wait connection closed; reservation retained")),changed=updates.changed()=>if changed.is_err(){break Err(err("unavailable","Resource observer stopped"))}}
    };
    outcome
}
async fn repair_local_projections(
    state: &ServerState,
    current: &mut Catalog,
    paths: &[String],
) -> Result<(), ResourceError> {
    for path in paths {
        let catalog = Arc::clone(current);
        let path = path.clone();
        let links = io(state, move || {
            Ok(catalog
                .links
                .iter()
                .filter(|l| l.project_path == path)
                .cloned()
                .collect::<Vec<_>>())
        })
        .await?;
        if links.is_empty() {
            continue;
        }
        let reason =
            projections::synchronize(state, current, std::path::Path::new(&links[0].project_path))
                .await
                .err()
                .map(|e| e.message.chars().take(512).collect::<String>());
        for link in &links.value {
            if link.sync_debt != reason {
                let now = clock()?;
                *current = committed_catalog(
                    commit(
                        state,
                        ResourceCommand::SetSyncDebt {
                            project_id: link.project_id.clone(),
                            resource_id: link.resource_id.clone(),
                            reason: reason.clone(),
                        },
                        MutationContext {
                            request_id: key(&now),
                            clock: now,
                            actor: ResourceActor::System,
                            expected_revision: Some(current.revision),
                            eligibility: BTreeMap::new(),
                        },
                        None,
                    )
                    .await?,
                )
                .0;
            }
        }
    }
    Ok(())
}
pub(crate) fn spawn(state: Arc<ServerState>) -> tokio::task::JoinHandle<()> {
    // The observer is an independently owned task; place its reconciliation
    // state machine on the heap before passing it through Tokio's spawn frame.
    tokio::spawn(Box::pin(async move {
        if service(&state).is_err() {
            return;
        }
        if let Ok(service) = service(&state) {
            let path = service
                .path
                .parent()
                .map(|p| p.join(format!("server-{}.json", service.incarnation)));
            let socket = state.socket_path.clone();
            if let Some(path) = path {
                let _ = io(&state, move || {
                    if let Some(parent) = path.parent() {
                        ilium_platform::secure_fs::create_private_directory(parent)
                            .map_err(|e| err("registry", e))?;
                    }
                    let bytes = serde_json::to_vec(&socket).map_err(|e| err("registry", e))?;
                    use std::io::Write;
                    let mut file = ilium_platform::secure_fs::private_open_options()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|e| err("registry", e))?;
                    file.write_all(&bytes).map_err(|e| err("registry", e))?;
                    Ok(())
                })
                .await;
            }
        }
        let mut observer_charge = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut last = None;
        let mut projected = None;
        let mut delivered = std::collections::BTreeSet::new();
        let mut notified: BTreeMap<(NodeId, u64), Vec<(ResourceId, u64)>> = BTreeMap::new();
        let mut active = true;
        loop {
            let _metadata_owner = &observer_charge;
            interval.tick().await;
            let has_waits = service(&state)
                .ok()
                .and_then(|service| {
                    service
                        .waits
                        .lock()
                        .ok()
                        .map(|waits| !waits.entries.is_empty())
                })
                .unwrap_or(true);
            if !active && !state.events.has_subscribers() && !has_waits {
                continue;
            }
            let outcome:Result<ResourceMemory<ServerEvent>,ResourceError>=async{
 let(mut catalog,mut observed_clock)=reconciled(&state).await?;
 let pane_count=state.panes.read().await.len();let links_bytes=catalog.links.iter().fold(0usize,|bytes,link|bytes.saturating_add(std::mem::size_of::<(ResourceId,u64)>()).saturating_add(link.resource_id.heap_bytes()));let potential_notices=pane_count.saturating_mul(links_bytes.saturating_add(1024));let potential_deliveries=catalog.resources.values().flat_map(|r|&r.reservations).filter(|q|q.state.active()).fold(0usize,|bytes,claim|bytes.saturating_add(claim.id.heap_bytes()).saturating_add(1024));let anticipated=memory::observer_bytes(&projected,&delivered,&notified).saturating_add(catalog.links.retained_bytes()).saturating_add(potential_notices).saturating_add(potential_deliveries);observer_charge=Some(memory::reserve_storage(&state,anticipated)?);
 let paths={let tree=state.tree.read().await;let ids=tree.project_ids();let bytes=ids.iter().filter_map(|id|tree.project_path_for(*id)).fold(4096usize,|bytes,path|bytes.saturating_add(std::mem::size_of::<String>()).saturating_add(path.as_os_str().len().saturating_mul(3)));let storage=memory::reserve_storage(&state,bytes)?;let values=ids.into_iter().filter_map(|id|tree.project_path_for(id).map(|p|p.to_string_lossy().into_owned())).collect::<Vec<_>>();ResourceMemory::from_parts(values,storage)};
 active=catalog.links.iter().any(|l|paths.contains(&l.project_path));
 let signature:Vec<_>=catalog.links.iter().filter(|l|paths.contains(&l.project_path)).map(|l|(l.project_id.clone(),l.resource_id.clone(),l.desired_instruction_revision)).collect();
 if projected.as_ref()!=Some(&signature)||catalog.links.iter().any(|l|paths.contains(&l.project_path)&&l.sync_debt.is_some()) {match repair_local_projections(&state,&mut catalog,&paths).await{Ok(())=>projected=Some(signature),Err(error)=>state.events.publish(ServerEvent::ResourceObservationFailed{message:error.to_string()})}if let Ok(now)=clock(){observed_clock=now;}}
            // Instruction changes are delivered through the existing fenced composer,
            // once per live invocation and binding revision, after readiness.
            let candidates:Vec<_>={let panes=state.panes.read().await;panes.iter().filter_map(|(id,pane)|match pane{PaneResource::Terminal(runtime) if runtime.detected_agent_process_id.is_some()=>Some((*id,runtime.agent_generation)),_=>None}).collect()};
            notified.retain(|key,_|candidates.contains(key));
            for (pane,generation) in candidates {
                let project_path={let tree=state.tree.read().await;tree.project_ancestor(pane).and_then(|id|tree.project_path_for(id).map(|p|p.to_path_buf()))};
                let Some(path)=project_path else{continue};
                let bindings:Vec<_>=catalog.links.iter().filter(|link|std::path::Path::new(&link.project_path)==path).map(|link|(link.resource_id.clone(),link.desired_instruction_revision)).collect();
                if notified.get(&(pane,generation))==Some(&bindings) || (bindings.is_empty()&&!notified.contains_key(&(pane,generation))){continue}
                let Ok(ResourceActor::Agent(agent))=actor(&state,Some(pane),&path).await else{continue};
                let text=if bindings.is_empty(){"The project's shared resource bindings were removed. Existing reservations still require explicit release or cancellation; inspect `ilium resource status` before further use."}else{"The project's shared resource instructions changed. Read the managed resource block in your project agent instructions before using shared hardware. Reserve exclusive access, wait once when queued, and explicitly release after use; estimates never clear ownership."};
                if matches!(crate::agent_delivery::try_resource_notice(&state,&agent,text).await,Ok(true)){notified.insert((pane,generation),bindings);}
            }

            for q in catalog.resources.values().flat_map(|r|&r.reservations).filter(|q|q.state==ReservationState::Owned&&q.acquisition_pending&&q.agent.runtime_generation==service(&state).map(|s|s.incarnation.as_str()).unwrap_or("")) {
                let receipt=(q.id.clone(),q.ownership_generation);
                let attached=service(&state).ok().and_then(|s|s.waits.lock().ok().map(|w|w.entries.contains_key(&q.id))).unwrap_or(true);
                if attached||delivered.contains(&receipt){continue}
                let text=format!("Resource reservation {} acquired generation {}. Confirm current ownership with `ilium resource status {}` before hardware use; release promptly if not ready.",q.id,q.ownership_generation,q.id);
                if matches!(crate::agent_delivery::try_resource_notice(&state,&q.agent,&text).await,Ok(true)){delivered.insert(receipt);}
            }
            delivered.retain(|(id,generation)|catalog.resources.values().flat_map(|r|&r.reservations).any(|q|q.id==*id&&q.ownership_generation==*generation&&q.acquisition_pending));
 snapshot_from(&state,catalog,observed_clock).await
 }.await;
            if let Ok(storage) = memory::reserve_storage(
                &state,
                memory::observer_bytes(&projected, &delivered, &notified),
            ) {
                observer_charge = Some(storage);
            }
            match outcome {
                Ok(event) => {
                    let revision = match &event.value {
                        ServerEvent::ResourceSnapshot { catalog, .. } => catalog.revision,
                        _ => 0,
                    };
                    if last != Some(revision) {
                        match memory::publish(&state, event) {
                            Ok(()) => last = Some(revision),
                            Err(error) => {
                                tracing::warn!(%error,"Resource event publication refused")
                            }
                        }
                    }
                    if active {
                        if let Ok(service) = service(&state) {
                            service.changed.send_replace(revision);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error,"Resource catalog observation failed");
                    state
                        .events
                        .publish(ServerEvent::ResourceObservationFailed {
                            message: error.to_string(),
                        });
                }
            }
        }
    }))
}
