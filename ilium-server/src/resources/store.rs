//! Synchronous shared-user catalog repository. Call only on admitted I/O workers.
use ilium_core::{resource_queue::*, AllocationSize};
use ilium_platform::resource_fs::ResourceLedgerFile;
use std::io::Write;
use std::path::{Path, PathBuf};
struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) as u64 > MAX_CATALOG_BYTES {
            return Err(std::io::Error::other("catalog byte capacity exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;
/// A byte gate supplements, and never reduces, the count gates. One MiB of the existing 128 MiB general result budget is
/// reserved for command results/events and the eventual transport envelope.
pub const MAX_CATALOG_RETAINED_BYTES: usize = 127 * 1024 * 1024;
pub(super) const MAX_COMMITTED_RETAINED_BYTES: usize = 128 * 1024 * 1024 - 4096;
fn check_catalog_capacity(catalog: &ResourceCatalog) -> Result<(), ResourceStoreError> {
    if catalog.retained_bytes() > MAX_CATALOG_RETAINED_BYTES {
        return Err(ResourceStoreError::Capacity);
    }
    Ok(())
}
#[derive(Debug, thiserror::Error)]
pub enum ResourceStoreError {
    #[error("resource catalog is busy; retry with the same request ID")]
    Busy,
    #[error("resource filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("resource catalog cannot be decoded: {0}")]
    Decode(#[from] serde_json::Error),
    #[error(transparent)]
    Domain(#[from] ResourceDomainError),
    #[error("resource commit outcome unknown; retry the same request ID: {0}")]
    OutcomeUnknown(std::io::Error),
    #[error(
        "resource catalog exceeds its 32 MiB serialized or 127 MiB retained allocation envelope"
    )]
    Capacity,
}
#[derive(Debug, Clone)]
pub struct ResourceStore {
    catalog_path: PathBuf,
    read_cache: std::sync::Arc<ilium_platform::resource_fs::ResourceLedgerReadCache>,
}
#[derive(Debug, Clone)]
pub struct CommittedResourceChange {
    pub catalog: ResourceCatalog,
    pub revision: u64,
    pub result: ResourceCommandResult,
    pub events: Vec<ResourceEvent>,
}
pub trait ResourceRepository {
    fn snapshot(&self) -> Result<ResourceCatalog, ResourceStoreError>;
    fn transact(
        &self,
        command: ResourceCommand,
        context: MutationContext,
    ) -> Result<CommittedResourceChange, ResourceStoreError>;
}
impl ResourceStore {
    pub fn new(catalog_path: impl Into<PathBuf>) -> Result<Self, ResourceStoreError> {
        let catalog_path = catalog_path.into();
        if !catalog_path.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "catalog path must be absolute",
            )
            .into());
        }
        Ok(Self {
            catalog_path,
            read_cache: Default::default(),
        })
    }
    pub(super) fn with_read_cache(
        mut self,
        cache: std::sync::Arc<ilium_platform::resource_fs::ResourceLedgerReadCache>,
    ) -> Self {
        self.read_cache = cache;
        self
    }
    pub fn catalog_path(&self) -> &Path {
        &self.catalog_path
    }
    fn locked(&self) -> Result<ResourceLedgerFile, ResourceStoreError> {
        ResourceLedgerFile::try_lock(&self.catalog_path)?.ok_or(ResourceStoreError::Busy)
    }
    fn read(&self, file: &ResourceLedgerFile) -> Result<ResourceCatalog, ResourceStoreError> {
        let catalog = match file.read_reconciled_cached(MAX_CATALOG_BYTES, &self.read_cache)? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => ResourceCatalog::default(),
        };
        validate_resource_catalog(&catalog)?;
        check_catalog_capacity(&catalog)?;
        Ok(catalog)
    }
    pub fn snapshot(&self) -> Result<ResourceCatalog, ResourceStoreError> {
        let file = self.locked()?;
        self.read(&file)
    }
    pub fn transact(
        &self,
        command: ResourceCommand,
        context: MutationContext,
    ) -> Result<CommittedResourceChange, ResourceStoreError> {
        let file = self.locked()?;
        let catalog = self.read(&file)?;
        let transition = apply_resource_command(&catalog, command, &context)?;
        check_catalog_capacity(&transition.catalog)?;
        if std::mem::size_of::<CommittedResourceChange>()
            .saturating_add(transition.catalog.heap_bytes())
            .saturating_add(transition.result.heap_bytes())
            .saturating_add(transition.events.heap_bytes())
            > MAX_COMMITTED_RETAINED_BYTES
        {
            return Err(ResourceStoreError::Capacity);
        }
        if transition.catalog.revision != catalog.revision {
            let mut bytes = BoundedBytes(Vec::new());
            serde_json::to_writer(&mut bytes, &transition.catalog).map_err(|e| {
                if e.is_io() {
                    ResourceStoreError::Capacity
                } else {
                    ResourceStoreError::Decode(e)
                }
            })?;
            file.replace(&bytes.0)
                .map_err(ResourceStoreError::OutcomeUnknown)?;
        }
        Ok(CommittedResourceChange {
            revision: transition.catalog.revision,
            catalog: transition.catalog,
            result: transition.result,
            events: transition.events,
        })
    }
}
impl ResourceRepository for ResourceStore {
    fn snapshot(&self) -> Result<ResourceCatalog, ResourceStoreError> {
        ResourceStore::snapshot(self)
    }
    fn transact(
        &self,
        command: ResourceCommand,
        context: MutationContext,
    ) -> Result<CommittedResourceChange, ResourceStoreError> {
        ResourceStore::transact(self, command, context)
    }
}
#[cfg(test)]
pub(super) fn durable_test_directory() -> tempfile::TempDir {
    let root = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/data/ilium-resource-queue-tests"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(".scratch-reaper"), b"").unwrap();
    tempfile::tempdir_in(root).unwrap()
}
#[cfg(test)]
pub(super) fn capacity_test_catalog() -> ResourceCatalog {
    let mut catalog = ResourceCatalog {
        revision: 8192,
        boot_id: "capacity-boot".into(),
        ..Default::default()
    };
    for resource_index in 0..8 {
        let resource_id = ResourceId(format!("resource-{resource_index}"));
        let definition = ResourceDefinition {
            policy: ResourcePolicy::ExclusiveFifo,
            created_at_unix_millis: 0,
            updated_at_unix_millis: 0,
            id: resource_id.clone(),
            title: "Synthetic capacity fixture".into(),
            kind: "gpu".into(),
            physical_identity: None,
            description: String::new(),
            rules: String::new(),
            definition_revision: 1,
            archived: false,
        };
        catalog.links.push(ProjectResourceLink {
            created_at_unix_millis: 0,
            project_id: "capacity-project".into(),
            resource_id: resource_id.clone(),
            slug: format!("resource-{resource_index}"),
            project_path: "/synthetic-capacity-project".into(),
            desired_instruction_revision: 1,
            sync_debt: None,
        });
        let mut reservations = Vec::new();
        for index in 0..1024 {
            let number = resource_index * 1024 + index;
            let request_id = RequestId(format!("1:reserve-{number}"));
            let reservation_id = ReservationId(format!("claim-{number}"));
            let agent = AgentIdentity {
                project_id: "capacity-project".into(),
                session_id: "capacity-session".into(),
                pane_id: ilium_core::NodeId(number + 1),
                runtime_generation: "capacity-runtime".into(),
                invocation_id: InvocationId(format!("invocation-{number}")),
                label: format!("Agent {number}"),
            };
            let clock = ResourceClock {
                unix_millis: 1,
                monotonic_millis: 1,
                boot_id: "capacity-boot".into(),
            };
            reservations.push(Reservation {
                id: reservation_id.clone(),
                request_id: request_id.clone(),
                resource_id: resource_id.clone(),
                agent: agent.clone(),
                ticket: index + 1,
                estimate_seconds: 60,
                requested_at: clock.clone(),
                acquired_at: (index == 0).then_some(clock),
                terminal_at: None,
                state: if index == 0 {
                    ReservationState::Owned
                } else {
                    ReservationState::Queued
                },
                ownership_generation: if index == 0 { 1 } else { 0 },
                acquisition_pending: index == 0,
            });
            catalog.receipts.insert(
                request_id,
                MutationReceipt {
                    command: ResourceCommand::Reserve {
                        resource_id: resource_id.clone(),
                        reservation_id: reservation_id.clone(),
                        estimate_seconds: 60,
                    },
                    actor: ResourceActor::Agent(agent),
                    result: ResourceCommandResult {
                        resource_id: Some(resource_id.clone()),
                        reservation_id: Some(reservation_id),
                        revision: number + 1,
                        reused: false,
                    },
                    created_at: 1,
                },
            );
        }
        catalog.resources.insert(
            resource_id,
            ResourceRecord {
                definition,
                gate: ResourceGate::Open,
                clearance_required: false,
                next_ticket: 1025,
                next_generation: 2,
                reservations,
            },
        );
    }
    catalog
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximum_history_and_active_queue_fit_the_same_durable_envelope() {
        let mut catalog = capacity_test_catalog();
        let template = catalog.resources[&ResourceId::from("resource-0")].clone();
        for index in 8..16 {
            let mut record = template.clone();
            record.definition.id = ResourceId(format!("resource-{index}"));
            record.reservations.clear();
            catalog
                .resources
                .insert(record.definition.id.clone(), record);
        }
        catalog.history = (1..=MAX_RESOURCE_HISTORY_EVENTS)
            .map(|index| ResourceLifecycleEvent {
                resource_id: ResourceId(format!("resource-{}", index % 16)),
                reservation_id: None,
                revision: index as u64,
                ordinal: 0,
                unix_millis: index as i64,
                actor: ResourceActor::User,
                kind: ResourceLifecycleKind::GateChanged,
                message: "x".repeat(1024),
            })
            .collect();
        validate_resource_catalog(&catalog).unwrap();
        check_catalog_capacity(&catalog).unwrap();
        let encoded = serde_json::to_vec(&catalog).unwrap();
        assert!(encoded.len() as u64 <= MAX_CATALOG_BYTES);
        assert_eq!(catalog.history.len(), 4096);
        assert_eq!(
            catalog
                .resources
                .values()
                .map(|record| record.reservations.len())
                .sum::<usize>(),
            8192
        );
        eprintln!("resource history capacity: per_resource={} global={} active_claims=8192 retained_bytes={} serialized_bytes={} retained_limit={} serialized_limit={}",MAX_HISTORY_EVENTS_PER_RESOURCE,MAX_RESOURCE_HISTORY_EVENTS,catalog.retained_bytes(),encoded.len(),MAX_CATALOG_RETAINED_BYTES,MAX_CATALOG_BYTES);
    }
    #[test]
    fn full_planned_active_queue_count_fits_result_envelope() {
        let catalog = super::capacity_test_catalog();
        validate_resource_catalog(&catalog).unwrap();
        check_catalog_capacity(&catalog).unwrap();
        assert_eq!(
            catalog
                .resources
                .values()
                .map(|r| r.reservations.len())
                .sum::<usize>(),
            8192
        );
        assert!(catalog.retained_bytes() < MAX_CATALOG_RETAINED_BYTES);
    }
    #[test]
    fn corruption_is_not_empty_catalog() {
        let dir = durable_test_directory();
        let path = dir.path().join("catalog.json");
        std::fs::write(&path, b"{").unwrap();
        assert!(matches!(
            ResourceStore::new(path).unwrap().snapshot(),
            Err(ResourceStoreError::Decode(_))
        ));
    }
    #[test]
    fn bounded_read_refuses_oversized_file() {
        let dir = durable_test_directory();
        let path = dir.path().join("catalog.json");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_CATALOG_BYTES + 1).unwrap();
        assert!(ResourceStore::new(path).unwrap().snapshot().is_err());
    }
    #[test]
    fn replacement_lock_is_stable() {
        let dir = durable_test_directory();
        let path = dir.path().join("catalog.json");
        let first = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
        first.replace(b"{}").unwrap();
        assert!(ResourceLedgerFile::try_lock(&path).unwrap().is_none());
        drop(first);
        assert!(ResourceLedgerFile::try_lock(&path).unwrap().is_some());
    }
}
#[cfg(test)]
mod process_tests {
    use super::*;
    use std::{
        io::{BufRead, Read},
        process::{Command, Stdio},
        time::{SystemTime, UNIX_EPOCH},
    };
    fn context(label: &str, user: bool) -> MutationContext {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        MutationContext {
            request_id: RequestId(format!("{now}:{label}")),
            clock: ResourceClock {
                unix_millis: now,
                monotonic_millis: now as u64,
                boot_id: "test-boot".into(),
            },
            actor: if user {
                ResourceActor::User
            } else {
                ResourceActor::Agent(agent(label))
            },
            expected_revision: None,
            eligibility: [(InvocationId(label.into()), InvocationEligibility::Live)].into(),
        }
    }
    fn agent(label: &str) -> AgentIdentity {
        AgentIdentity {
            project_id: "test-project".into(),
            session_id: "test-session".into(),
            pane_id: ilium_core::NodeId(1),
            runtime_generation: "test-runtime".into(),
            invocation_id: InvocationId(label.into()),
            label: label.into(),
        }
    }
    fn create(store: &ResourceStore) {
        store
            .transact(
                ResourceCommand::Create {
                    definition: ResourceDefinition {
                        policy: ResourcePolicy::ExclusiveFifo,
                        created_at_unix_millis: 0,
                        updated_at_unix_millis: 0,
                        id: "gpu".into(),
                        title: "GPU".into(),
                        kind: "gpu".into(),
                        physical_identity: None,
                        description: String::new(),
                        rules: String::new(),
                        definition_revision: 0,
                        archived: false,
                    },
                    link: ProjectResourceLink {
                        created_at_unix_millis: 0,
                        project_id: "test-project".into(),
                        resource_id: "gpu".into(),
                        slug: "gpu".into(),
                        project_path: "/test-project".into(),
                        desired_instruction_revision: 0,
                        sync_debt: None,
                    },
                },
                context("create", true),
            )
            .unwrap();
    }
    #[test]
    fn child_reserve() {
        let Ok(path) = std::env::var("ILIUM_RESOURCE_STORE_CHILD_PATH") else {
            return;
        };
        let label = std::env::var("ILIUM_RESOURCE_STORE_CHILD_LABEL").unwrap();
        let store = ResourceStore::new(path).unwrap();
        println!("ready");
        std::io::stdout().flush().unwrap();
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        let mut ctx = context(&label, false);
        if let Ok(boot) = std::env::var("ILIUM_RESOURCE_STORE_CHILD_BOOT") {
            ctx.clock.boot_id = boot;
            let mut fence = ctx.clone();
            fence.actor = ResourceActor::System;
            fence.request_id = RequestId(format!("{}:boot-{label}", ctx.clock.unix_millis));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
            loop {
                match store.transact(ResourceCommand::ReconcileBoot, fence.clone()) {
                    Ok(_) => break,
                    Err(ResourceStoreError::Busy) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        }
        let command = ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: ReservationId(label),
            estimate_seconds: 600,
        };
        // Retry only nonblocking lock admission, bounded to keep this test finite.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        while std::time::Instant::now() < deadline {
            match store.transact(command.clone(), ctx.clone()) {
                Ok(_) => {
                    if std::env::var_os("ILIUM_RESOURCE_STORE_DROP_ACK").is_some() {
                        std::process::exit(71)
                    }
                    return;
                }
                Err(ResourceStoreError::Busy) => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        }
        panic!("cross-process lock admission exhausted");
    }
    #[test]
    fn independent_processes_serialize_ownership() {
        let dir = durable_test_directory();
        let path = dir.path().join("catalog.json");
        let store = ResourceStore::new(&path).unwrap();
        create(&store);
        let mut children = Vec::new();
        for index in 0..6 {
            children.push(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "resources::store::process_tests::child_reserve",
                        "--nocapture",
                    ])
                    .env("ILIUM_RESOURCE_STORE_CHILD_PATH", &path)
                    .env("ILIUM_RESOURCE_STORE_CHILD_LABEL", format!("agent-{index}"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for child in &mut children {
            let mut ready = String::new();
            let mut output = std::io::BufReader::new(child.stdout.as_mut().unwrap());
            loop {
                ready.clear();
                assert!(output.read_line(&mut ready).unwrap() > 0);
                if ready.trim() == "ready" {
                    break;
                }
            }
        }
        for child in &mut children {
            child.stdin.take().unwrap().write_all(b"g").unwrap();
        }
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let c = store.snapshot().unwrap();
        let r = &c.resources[&"gpu".into()];
        assert_eq!(r.reservations.len(), 6);
        assert_eq!(
            r.reservations
                .iter()
                .filter(|q| q.state == ReservationState::Owned)
                .count(),
            1
        );
        assert_eq!(
            r.reservations
                .iter()
                .filter(|q| q.state == ReservationState::Queued)
                .count(),
            5
        );
    }
    #[test]
    fn acknowledged_commit_roundtrips_and_retries() {
        let dir = durable_test_directory();
        let store = ResourceStore::new(dir.path().join("catalog.json")).unwrap();
        create(&store);
        let ctx = context("a", false);
        let command = ResourceCommand::Reserve {
            resource_id: "gpu".into(),
            reservation_id: "a".into(),
            estimate_seconds: 1,
        };
        let first = store.transact(command.clone(), ctx.clone()).unwrap();
        drop(store);
        let store = ResourceStore::new(dir.path().join("catalog.json")).unwrap();
        let again = store.transact(command, ctx).unwrap();
        assert_eq!(again.catalog, first.catalog);
        assert_eq!(again.revision, first.revision);
        assert!(again.result.reused);
    }
    #[test]
    fn process_exit_before_response_preserves_durable_grant() {
        let dir = durable_test_directory();
        let path = dir.path().join("catalog.json");
        let store = ResourceStore::new(&path).unwrap();
        create(&store);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "resources::store::process_tests::child_reserve",
                "--nocapture",
            ])
            .env("ILIUM_RESOURCE_STORE_CHILD_PATH", &path)
            .env("ILIUM_RESOURCE_STORE_CHILD_LABEL", "lost-ack")
            .env("ILIUM_RESOURCE_STORE_DROP_ACK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        loop {
            ready.clear();
            assert!(output.read_line(&mut ready).unwrap() > 0);
            if ready.trim() == "ready" {
                break;
            }
        }
        child.stdin.take().unwrap().write_all(b"g").unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(71));
        let catalog = store.snapshot().unwrap();
        let reservation = &catalog.resources[&"gpu".into()].reservations[0];
        assert_eq!(reservation.state, ReservationState::Owned);
        let receipt = &catalog.receipts[&reservation.request_id];
        let context = MutationContext {
            request_id: reservation.request_id.clone(),
            clock: reservation.requested_at.clone(),
            actor: receipt.actor.clone(),
            expected_revision: None,
            eligibility: [(
                reservation.agent.invocation_id.clone(),
                InvocationEligibility::Live,
            )]
            .into(),
        };
        let retried = store.transact(receipt.command.clone(), context).unwrap();
        assert!(retried.result.reused);
        assert_eq!(retried.catalog, catalog);
    }

    #[test]
    fn two_new_boot_processes_fence_old_claims_before_reserving() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let store = ResourceStore::new(&path).unwrap();
        create(&store);
        for label in ["old-owner", "old-queued"] {
            store
                .transact(
                    ResourceCommand::Reserve {
                        resource_id: "gpu".into(),
                        reservation_id: label.into(),
                        estimate_seconds: 600,
                    },
                    context(label, false),
                )
                .unwrap();
        }
        let mut children = Vec::new();
        for label in ["new-alpha", "new-beta"] {
            children.push(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "resources::store::process_tests::child_reserve",
                        "--nocapture",
                    ])
                    .env("ILIUM_RESOURCE_STORE_CHILD_PATH", &path)
                    .env("ILIUM_RESOURCE_STORE_CHILD_LABEL", label)
                    .env("ILIUM_RESOURCE_STORE_CHILD_BOOT", "next-test-boot")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for child in &mut children {
            let mut output = std::io::BufReader::new(child.stdout.as_mut().unwrap());
            loop {
                let mut line = String::new();
                assert!(output.read_line(&mut line).unwrap() > 0);
                if line.trim() == "ready" {
                    break;
                }
            }
        }
        for child in &mut children {
            child.stdin.take().unwrap().write_all(b"g").unwrap();
        }
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let persisted = store.snapshot().unwrap();
        assert_eq!(persisted.boot_id, "next-test-boot");
        let claims = &persisted.resources[&"gpu".into()].reservations;
        assert!(matches!(
            claims[0].state,
            ReservationState::NeedsAttention { .. }
        ));
        assert_eq!(claims[1].state, ReservationState::Cancelled);
        assert_eq!(
            claims
                .iter()
                .filter(|claim| claim.state == ReservationState::Queued)
                .count(),
            2
        );
        assert!(!claims
            .iter()
            .any(|claim| claim.state == ReservationState::Owned));
    }

    fn retry_transaction(
        store: &ResourceStore,
        command: ResourceCommand,
        context: MutationContext,
    ) -> Result<CommittedResourceChange, ResourceStoreError> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            match store.transact(command.clone(), context.clone()) {
                Err(ResourceStoreError::Busy) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                outcome => return outcome,
            }
        }
    }
    #[test]
    fn concurrent_cancel_and_grant_never_drop_an_acquired_owner() {
        let directory = durable_test_directory();
        let store = ResourceStore::new(directory.path().join("catalog.json")).unwrap();
        create(&store);
        for label in ["owner", "waiter"] {
            store
                .transact(
                    ResourceCommand::Reserve {
                        resource_id: "gpu".into(),
                        reservation_id: label.into(),
                        estimate_seconds: 600,
                    },
                    context(label, false),
                )
                .unwrap();
        }
        let before = store.snapshot().unwrap();
        let generation = before.resources[&"gpu".into()].reservations[0].ownership_generation;
        let eligibility: std::collections::BTreeMap<_, _> = [
            ("owner".into(), InvocationEligibility::Live),
            ("waiter".into(), InvocationEligibility::Live),
        ]
        .into();
        let mut release = context("release-race", false);
        release.actor = ResourceActor::Agent(agent("owner"));
        release.eligibility = eligibility.clone();
        let mut cancel = context("cancel-race", false);
        cancel.actor = ResourceActor::Agent(agent("waiter"));
        cancel.eligibility = eligibility;
        let barrier = std::sync::Barrier::new(2);
        let (released, cancelled) = std::thread::scope(|scope| {
            let release_job = scope.spawn(|| {
                barrier.wait();
                retry_transaction(
                    &store,
                    ResourceCommand::Release {
                        reservation_id: "owner".into(),
                        ownership_generation: generation,
                    },
                    release,
                )
            });
            let cancel_job = scope.spawn(|| {
                barrier.wait();
                retry_transaction(
                    &store,
                    ResourceCommand::Cancel {
                        reservation_id: "waiter".into(),
                    },
                    cancel,
                )
            });
            (release_job.join().unwrap(), cancel_job.join().unwrap())
        });
        assert!(released.is_ok());
        let persisted = store.snapshot().unwrap();
        validate_resource_catalog(&persisted).unwrap();
        let claims = &persisted.resources[&"gpu".into()].reservations;
        assert_eq!(claims[0].state, ReservationState::Released);
        match cancelled {
            Ok(_) => {
                assert_eq!(claims[1].state, ReservationState::Cancelled);
                assert!(!claims.iter().any(|claim| claim.state.occupied()));
            }
            Err(ResourceStoreError::Domain(error)) => {
                assert_eq!(error.code, "already_owned");
                assert_eq!(claims[1].state, ReservationState::Owned);
            }
            Err(error) => panic!("unexpected cancel race failure: {error}"),
        }
    }

    #[test]
    fn wrong_project_and_compound_invocation_cannot_mutate_authority() {
        let directory = durable_test_directory();
        let store = ResourceStore::new(directory.path().join("catalog.json")).unwrap();
        create(&store);
        let owned = store
            .transact(
                ResourceCommand::Reserve {
                    resource_id: "gpu".into(),
                    reservation_id: "owner".into(),
                    estimate_seconds: 600,
                },
                context("owner", false),
            )
            .unwrap();
        let mut unrelated = context("foreign-reserve", false);
        let mut foreign = agent("foreign");
        foreign.project_id = "unlinked-project".into();
        unrelated.actor = ResourceActor::Agent(foreign);
        unrelated.eligibility = [("foreign".into(), InvocationEligibility::Live)].into();
        let denied = store
            .transact(
                ResourceCommand::Reserve {
                    resource_id: "gpu".into(),
                    reservation_id: "foreign".into(),
                    estimate_seconds: 60,
                },
                unrelated,
            )
            .unwrap_err();
        assert!(matches!(denied, ResourceStoreError::Domain(error) if error.code == "not_linked"));
        let mut stale = context("stale-release", false);
        let mut identity = agent("owner");
        identity.runtime_generation = "different-server-incarnation".into();
        stale.actor = ResourceActor::Agent(identity);
        let generation =
            owned.catalog.resources[&"gpu".into()].reservations[0].ownership_generation;
        let denied = store
            .transact(
                ResourceCommand::Release {
                    reservation_id: "owner".into(),
                    ownership_generation: generation,
                },
                stale,
            )
            .unwrap_err();
        assert!(
            matches!(denied, ResourceStoreError::Domain(error) if error.code == "stale_invocation")
        );
        assert_eq!(store.snapshot().unwrap(), owned.catalog);
    }

    #[test]
    fn concurrent_guarded_revoke_and_release_cannot_clear_a_new_owner() {
        let directory = durable_test_directory();
        let store = ResourceStore::new(directory.path().join("catalog.json")).unwrap();
        create(&store);
        for label in ["owner", "waiter"] {
            store
                .transact(
                    ResourceCommand::Reserve {
                        resource_id: "gpu".into(),
                        reservation_id: label.into(),
                        estimate_seconds: 600,
                    },
                    context(label, false),
                )
                .unwrap();
        }
        let before = store.snapshot().unwrap();
        let generation = before.resources[&"gpu".into()].reservations[0].ownership_generation;
        let eligibility: std::collections::BTreeMap<_, _> = [
            ("owner".into(), InvocationEligibility::Live),
            ("waiter".into(), InvocationEligibility::Live),
        ]
        .into();
        let mut release = context("release-revoke-race", false);
        release.actor = ResourceActor::Agent(agent("owner"));
        release.eligibility = eligibility.clone();
        let mut revoke = context("guarded-revoke-race", true);
        revoke.expected_revision = Some(before.revision);
        revoke.eligibility = eligibility;
        let barrier = std::sync::Barrier::new(2);
        let (released, revoked) = std::thread::scope(|scope| {
            let release_job = scope.spawn(|| {
                barrier.wait();
                retry_transaction(
                    &store,
                    ResourceCommand::Release {
                        reservation_id: "owner".into(),
                        ownership_generation: generation,
                    },
                    release,
                )
            });
            let revoke_job = scope.spawn(|| {
                barrier.wait();
                retry_transaction(
                    &store,
                    ResourceCommand::Revoke {
                        reservation_id: "owner".into(),
                        reason: "Synthetic guarded race".into(),
                    },
                    revoke,
                )
            });
            (release_job.join().unwrap(), revoke_job.join().unwrap())
        });
        let persisted = store.snapshot().unwrap();
        validate_resource_catalog(&persisted).unwrap();
        let resource = &persisted.resources[&"gpu".into()];
        match revoked {
            Ok(_) => {
                assert!(
                    matches!(released, Err(ResourceStoreError::Domain(error)) if error.code == "stale_generation")
                );
                assert!(resource.clearance_required);
                assert!(matches!(resource.gate, ResourceGate::Paused { .. }));
                assert_eq!(resource.reservations[1].state, ReservationState::Queued);
                assert!(!resource
                    .reservations
                    .iter()
                    .any(|claim| claim.state == ReservationState::Owned));
            }
            Err(ResourceStoreError::Domain(error)) => {
                assert_eq!(error.code, "revision_conflict");
                assert!(released.is_ok());
                assert!(!resource.clearance_required);
                assert_eq!(resource.reservations[1].state, ReservationState::Owned);
            }
            Err(error) => panic!("unexpected guarded revoke race failure: {error}"),
        }
    }
    #[test]
    fn corrupt_truncated_and_unsupported_catalogs_refuse_every_mutation() {
        let source = durable_test_directory();
        let valid_store = ResourceStore::new(source.path().join("valid.json")).unwrap();
        create(&valid_store);
        for label in ["owner", "waiter"] {
            valid_store
                .transact(
                    ResourceCommand::Reserve {
                        resource_id: "gpu".into(),
                        reservation_id: label.into(),
                        estimate_seconds: 60,
                    },
                    context(label, false),
                )
                .unwrap();
        }
        let valid = valid_store.snapshot().unwrap();
        let bytes = serde_json::to_vec(&valid).unwrap();
        let mut unknown = valid.clone();
        unknown.schema_version = 999;
        let mut duplicate = valid.clone();
        let claims = &mut duplicate
            .resources
            .get_mut(&ResourceId::from("gpu"))
            .unwrap()
            .reservations;
        claims[1].state = ReservationState::Owned;
        claims[1].acquired_at = Some(claims[1].requested_at.clone());
        claims[1].ownership_generation = 1;
        let mut missing_history = serde_json::to_value(&valid).unwrap();
        missing_history.as_object_mut().unwrap().remove("history");
        let mut missing_policy = serde_json::to_value(&valid).unwrap();
        missing_policy["resources"]["gpu"]["definition"]
            .as_object_mut()
            .unwrap()
            .remove("policy");
        let mut invalid_policy = serde_json::to_value(&valid).unwrap();
        invalid_policy["resources"]["gpu"]["definition"]["policy"] =
            serde_json::Value::String("SharedFifo".into());
        let mut invalid = vec![
            bytes[..bytes.len() / 2].to_vec(),
            serde_json::to_vec(&unknown).unwrap(),
            serde_json::to_vec(&duplicate).unwrap(),
            br#"{"resources":42}"#.to_vec(),
            serde_json::to_vec(&missing_history).unwrap(),
            serde_json::to_vec(&missing_policy).unwrap(),
            serde_json::to_vec(&invalid_policy).unwrap(),
        ];
        for field in ["created_at_unix_millis", "updated_at_unix_millis"] {
            let mut absent = serde_json::to_value(&valid).unwrap();
            absent["resources"]["gpu"]["definition"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            invalid.push(serde_json::to_vec(&absent).unwrap());
        }
        let mut missing_link_creation = serde_json::to_value(&valid).unwrap();
        missing_link_creation["links"][0]
            .as_object_mut()
            .unwrap()
            .remove("created_at_unix_millis");
        invalid.push(serde_json::to_vec(&missing_link_creation).unwrap());
        for (index, bytes) in invalid.into_iter().enumerate() {
            let directory = durable_test_directory();
            let path = directory.path().join("catalog.json");
            let ledger = ResourceLedgerFile::try_lock(&path).unwrap().unwrap();
            ledger.replace(&bytes).unwrap();
            drop(ledger);
            let repository = ResourceStore::new(path.clone()).unwrap();
            assert!(repository.snapshot().is_err(), "invalid case {index}");
            assert!(
                repository
                    .transact(
                        ResourceCommand::Reserve {
                            resource_id: "gpu".into(),
                            reservation_id: "must-not-grant".into(),
                            estimate_seconds: 60
                        },
                        context("untrusted", false)
                    )
                    .is_err(),
                "invalid case {index}"
            );
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "invalid authority must never be rewritten or replaced by empty state"
            );
        }
    }
    #[test]
    fn invalid_create_plus_link_is_atomically_absent() {
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let repository = ResourceStore::new(path.clone()).unwrap();
        let mut candidate = capacity_test_catalog().resources[&ResourceId::from("resource-0")]
            .definition
            .clone();
        candidate.id = "atomic-resource".into();
        candidate.definition_revision = 0;
        let result = repository.transact(
            ResourceCommand::Create {
                definition: candidate,
                link: ProjectResourceLink {
                    created_at_unix_millis: 0,
                    project_id: "test-project".into(),
                    resource_id: "atomic-resource".into(),
                    slug: "invalid--slug".into(),
                    project_path: "/test-project".into(),
                    desired_instruction_revision: 0,
                    sync_debt: None,
                },
            },
            context("invalid-create", true),
        );
        assert!(result.is_err());
        assert_eq!(repository.snapshot().unwrap(), ResourceCatalog::default());
        assert!(
            !path.exists(),
            "invalid create cannot leave a half-created authoritative catalog"
        );
    }
    #[test]
    fn fixed_seed_thousand_repository_interleavings_read_back_every_result() {
        const SEED: u64 = 0x5245_534f_5552_4345;
        let directory = durable_test_directory();
        let path = directory.path().join("catalog.json");
        let first = ResourceStore::new(path.clone()).unwrap();
        let second = ResourceStore::new(path).unwrap();
        create(&first);
        let mut random = SEED;
        let mut recent = std::collections::VecDeque::new();
        let mut replay: Option<(ResourceCommand, MutationContext)> = None;
        for operation in 0..1000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let repository = if random & 1 == 0 { &first } else { &second };
            let before = repository.snapshot().unwrap();
            validate_resource_catalog(&before).unwrap();
            let record = &before.resources[&ResourceId::from("gpu")];
            let mut mutation = context(&format!("generated-{operation}"), true);
            mutation.eligibility = (0..8)
                .map(|index| {
                    (
                        InvocationId(format!("generated-agent-{index}")),
                        InvocationEligibility::Live,
                    )
                })
                .collect();
            let label = format!("generated-agent-{}", (random >> 8) % 8);
            let selected = record.reservations.iter().find(|claim| {
                claim.state.active() && claim.agent.invocation_id == InvocationId(label.clone())
            });
            let default_reserve = || ResourceCommand::Reserve {
                resource_id: "gpu".into(),
                reservation_id: ReservationId(format!("generated-claim-{operation}")),
                estimate_seconds: 1 + random % 600,
            };
            let command = if let Some((command, context)) =
                replay.as_ref().filter(|_| operation % 17 == 0)
            {
                mutation = context.clone();
                command.clone()
            } else {
                match (random >> 16) % 8 {
                    0 | 1 => {
                        mutation.actor = ResourceActor::Agent(agent(&label));
                        default_reserve()
                    }
                    2 => {
                        if let Some(owner) = record
                            .reservations
                            .iter()
                            .find(|claim| claim.state.occupied())
                        {
                            if owner.state == ReservationState::Owned {
                                mutation.actor = ResourceActor::Agent(owner.agent.clone());
                                ResourceCommand::Release {
                                    reservation_id: owner.id.clone(),
                                    ownership_generation: owner.ownership_generation,
                                }
                            } else {
                                mutation.expected_revision = Some(before.revision);
                                ResourceCommand::Revoke {
                                    reservation_id: owner.id.clone(),
                                    reason: "Generated stale-owner recovery".into(),
                                }
                            }
                        } else {
                            mutation.actor = ResourceActor::Agent(agent(&label));
                            default_reserve()
                        }
                    }
                    3 => {
                        if let Some(waiter) = record
                            .reservations
                            .iter()
                            .find(|claim| claim.state == ReservationState::Queued)
                        {
                            mutation.actor = ResourceActor::Agent(waiter.agent.clone());
                            ResourceCommand::Cancel {
                                reservation_id: waiter.id.clone(),
                            }
                        } else {
                            ResourceCommand::SetGate {
                                resource_id: "gpu".into(),
                                gate: ResourceGate::Open,
                            }
                        }
                    }
                    4 => {
                        if let Some(claim) = selected {
                            mutation.actor = ResourceActor::Agent(claim.agent.clone());
                            ResourceCommand::Estimate {
                                reservation_id: claim.id.clone(),
                                estimate_seconds: 1 + random % 600,
                            }
                        } else {
                            mutation.actor = ResourceActor::Agent(agent(&label));
                            default_reserve()
                        }
                    }
                    5 => ResourceCommand::SetGate {
                        resource_id: "gpu".into(),
                        gate: if random & 2 == 0 {
                            ResourceGate::Open
                        } else {
                            ResourceGate::Paused {
                                reason: "Generated maintenance gate".into(),
                            }
                        },
                    },
                    6 => {
                        mutation.actor = ResourceActor::System;
                        ResourceCommand::ReconcileInvocation {
                            agent: agent(&label),
                            evidence: InvocationEligibility::Dead,
                        }
                    }
                    _ => {
                        mutation.expected_revision = Some(before.revision);
                        ResourceCommand::ConfirmClear{resource_id:"gpu".into(),reason:"Generated explicit physical-clearance assertion; synthetic fixture only".into()}
                    }
                }
            };
            recent.push_back(format!("{operation}: {command:?}"));
            if recent.len() > 32 {
                recent.pop_front();
            }
            let replayed_receipt = before.receipts.contains_key(&mutation.request_id);
            let result = repository.transact(command.clone(), mutation.clone());
            let persisted = ResourceStore::new(repository.catalog_path())
                .unwrap()
                .snapshot()
                .unwrap();
            match result {
                Ok(committed) => {
                    assert_eq!(
                        persisted, committed.catalog,
                        "seed {SEED:x}; recent {recent:?}"
                    );
                    if replayed_receipt {
                        assert!(committed.result.reused);
                        assert_eq!(
                            persisted, before,
                            "replay altered authority; seed {SEED:x}; recent {recent:?}"
                        );
                    }
                    replay = Some((command, mutation));
                }
                Err(ResourceStoreError::Domain(_)) => assert_eq!(
                    persisted, before,
                    "refused transition altered authority; seed {SEED:x}; recent {recent:?}"
                ),
                Err(error) => {
                    panic!("repository failure {error}; seed {SEED:x}; recent {recent:?}")
                }
            }
            validate_resource_catalog(&persisted).unwrap_or_else(|error| {
                panic!("invalid durable state {error}; seed {SEED:x}; recent {recent:?}")
            });
            let record = &persisted.resources[&ResourceId::from("gpu")];
            assert!(
                record
                    .reservations
                    .iter()
                    .filter(|claim| claim.state.occupied())
                    .count()
                    <= 1,
                "seed {SEED:x}; recent {recent:?}"
            );
            let mut invocations = std::collections::BTreeSet::new();
            let mut tickets = std::collections::BTreeSet::new();
            for claim in &record.reservations {
                assert!(
                    tickets.insert(claim.ticket),
                    "duplicate ticket; seed {SEED:x}; recent {recent:?}"
                );
                if claim.state.active() {
                    assert!(
                        invocations.insert(claim.agent.invocation_id.clone()),
                        "duplicate active invocation; seed {SEED:x}; recent {recent:?}"
                    );
                }
            }
            if let Some(owner) = record
                .reservations
                .iter()
                .find(|claim| claim.state == ReservationState::Owned)
            {
                assert!(
                    record
                        .reservations
                        .iter()
                        .filter(|claim| claim.state == ReservationState::Queued)
                        .all(|claim| claim.ticket > owner.ticket),
                    "FIFO violation; seed {SEED:x}; recent {recent:?}"
                );
            }
        }
    }

    #[test]
    fn child_initializes_first_use_catalog_and_reserves() {
        let Some(path) = std::env::var_os("ILIUM_RESOURCE_FIRST_USE_PATH") else {
            return;
        };
        let label = std::env::var("ILIUM_RESOURCE_FIRST_USE_LABEL").unwrap();
        let repository = ResourceStore::new(PathBuf::from(path)).unwrap();
        println!("ready");
        std::io::stdout().flush().unwrap();
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        let definition = ResourceDefinition {
            policy: ResourcePolicy::ExclusiveFifo,
            created_at_unix_millis: 0,
            updated_at_unix_millis: 0,
            id: "gpu".into(),
            title: "First-use synthetic GPU".into(),
            kind: "gpu".into(),
            physical_identity: None,
            description: String::new(),
            rules: String::new(),
            definition_revision: 0,
            archived: false,
        };
        let create = ResourceCommand::Create {
            definition,
            link: ProjectResourceLink {
                created_at_unix_millis: 0,
                project_id: "test-project".into(),
                resource_id: "gpu".into(),
                slug: "gpu".into(),
                project_path: "/test-project".into(),
                desired_instruction_revision: 0,
                sync_debt: None,
            },
        };
        match retry_transaction(
            &repository,
            create,
            context(&format!("first-create-{label}"), true),
        ) {
            Ok(_) => {}
            Err(ResourceStoreError::Domain(error)) if error.code == "conflict" => {}
            Err(error) => panic!("first use failed: {error}"),
        }
        retry_transaction(
            &repository,
            ResourceCommand::Reserve {
                resource_id: "gpu".into(),
                reservation_id: ReservationId(label.clone()),
                estimate_seconds: 60,
            },
            context(&label, false),
        )
        .unwrap();
    }
    #[test]
    fn simultaneous_first_use_processes_retain_both_claims_and_one_owner() {
        let directory = durable_test_directory();
        let path = directory.path().join("new-authority/catalog.json");
        assert!(!path.exists());
        let mut children = Vec::new();
        for label in ["first-use-alpha", "first-use-beta"] {
            let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","resources::store::process_tests::child_initializes_first_use_catalog_and_reserves","--nocapture"]).env("ILIUM_RESOURCE_FIRST_USE_PATH",&path).env("ILIUM_RESOURCE_FIRST_USE_LABEL",label).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                assert!(output.read_line(&mut line).unwrap() > 0);
                if line.trim() == "ready" {
                    break;
                }
            }
            children.push((child, output));
        }
        for (child, _) in &mut children {
            child.stdin.as_mut().unwrap().write_all(b"g").unwrap();
        }
        for (child, _) in &mut children {
            assert!(child.wait().unwrap().success());
        }
        let persisted = ResourceStore::new(path).unwrap().snapshot().unwrap();
        validate_resource_catalog(&persisted).unwrap();
        assert_eq!(persisted.resources.len(), 1);
        let claims = &persisted.resources[&ResourceId::from("gpu")].reservations;
        assert_eq!(claims.len(), 2);
        assert_eq!(
            claims
                .iter()
                .filter(|claim| claim.state == ReservationState::Owned)
                .count(),
            1
        );
        assert_eq!(
            claims
                .iter()
                .filter(|claim| claim.state == ReservationState::Queued)
                .count(),
            1
        );
        assert!(claims
            .iter()
            .any(|claim| claim.id == ReservationId::from("first-use-alpha")));
        assert!(claims
            .iter()
            .any(|claim| claim.id == ReservationId::from("first-use-beta")));
        assert!(claims[0].ticket < claims[1].ticket);
    }

    fn fingerprint_definition(id: &str, fingerprint: &str) -> ResourceDefinition {
        ResourceDefinition {
            policy: ResourcePolicy::ExclusiveFifo,
            created_at_unix_millis: 0,
            updated_at_unix_millis: 0,
            id: ResourceId(id.into()),
            title: format!("Synthetic board {id}"),
            kind: "usb-board".into(),
            physical_identity: Some(fingerprint.into()),
            description: "Test descriptor; no physical device access".into(),
            rules: String::new(),
            definition_revision: 0,
            archived: false,
        }
    }
    fn fingerprint_link(id: &str) -> ProjectResourceLink {
        ProjectResourceLink {
            created_at_unix_millis: 0,
            project_id: "test-project".into(),
            resource_id: ResourceId(id.into()),
            slug: id.into(),
            project_path: "/test-project".into(),
            desired_instruction_revision: 0,
            sync_debt: None,
        }
    }
    #[test]
    fn child_competes_for_normalized_physical_fingerprint() {
        let Some(path) = std::env::var_os("ILIUM_RESOURCE_FINGERPRINT_PATH") else {
            return;
        };
        let id = std::env::var("ILIUM_RESOURCE_FINGERPRINT_ID").unwrap();
        let mode = std::env::var("ILIUM_RESOURCE_FINGERPRINT_MODE").unwrap();
        let repository = ResourceStore::new(PathBuf::from(path)).unwrap();
        let fingerprint = if id == "board-alpha" {
            "USB:shared-serial"
        } else {
            "usb:SHARED-SERIAL"
        };
        let command = if mode == "edit" {
            let mut definition = repository.snapshot().unwrap().resources[&ResourceId(id.clone())]
                .definition
                .clone();
            definition.physical_identity = Some(fingerprint.into());
            ResourceCommand::Edit { definition }
        } else {
            ResourceCommand::Create {
                definition: fingerprint_definition(&id, fingerprint),
                link: fingerprint_link(&id),
            }
        };
        println!("ready");
        std::io::stdout().flush().unwrap();
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        match retry_transaction(
            &repository,
            command,
            context(&format!("fingerprint-{mode}-{id}"), true),
        ) {
            Ok(_) => {}
            Err(ResourceStoreError::Domain(error))
                if error.code == "duplicate_physical_identity" =>
            {
                std::process::exit(88)
            }
            Err(error) => panic!("unexpected fingerprint race failure: {error}"),
        }
    }
    #[test]
    fn concurrent_process_create_and_edit_preserve_physical_identity_uniqueness() {
        for mode in ["create", "edit"] {
            let directory = durable_test_directory();
            let path = directory.path().join("catalog.json");
            let repository = ResourceStore::new(path.clone()).unwrap();
            if mode == "edit" {
                for id in ["board-alpha", "board-beta"] {
                    repository
                        .transact(
                            ResourceCommand::Create {
                                definition: fingerprint_definition(
                                    id,
                                    &format!("usb:original-{id}"),
                                ),
                                link: fingerprint_link(id),
                            },
                            context(&format!("initial-{id}"), true),
                        )
                        .unwrap();
                }
            }
            let mut children = Vec::new();
            for id in ["board-alpha", "board-beta"] {
                let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","resources::store::process_tests::child_competes_for_normalized_physical_fingerprint","--nocapture"]).env("ILIUM_RESOURCE_FINGERPRINT_PATH",&path).env("ILIUM_RESOURCE_FINGERPRINT_MODE",mode).env("ILIUM_RESOURCE_FINGERPRINT_ID",id).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
                let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    assert!(output.read_line(&mut line).unwrap() > 0);
                    if line.trim() == "ready" {
                        break;
                    }
                }
                children.push((child, output));
            }
            for (child, _) in &mut children {
                child.stdin.as_mut().unwrap().write_all(b"g").unwrap();
            }
            let mut statuses = children
                .iter_mut()
                .map(|(child, _)| child.wait().unwrap().code().unwrap())
                .collect::<Vec<_>>();
            statuses.sort();
            assert_eq!(
                statuses,
                vec![0, 88],
                "exactly one {mode} wins the normalized physical fingerprint"
            );
            let persisted = repository.snapshot().unwrap();
            validate_resource_catalog(&persisted).unwrap();
            assert_eq!(
                persisted
                    .resources
                    .values()
                    .filter(
                        |resource| resource.definition.physical_identity.as_ref().is_some_and(
                            |value| value.trim().eq_ignore_ascii_case("usb:shared-serial")
                        )
                    )
                    .count(),
                1
            );
            assert_eq!(
                persisted.resources.len(),
                if mode == "edit" { 2 } else { 1 }
            );
            assert_eq!(
                persisted.links.len(),
                persisted.resources.len(),
                "losing creation cannot leave an authoritative binding"
            );
        }
    }
}
