//! Server-owned advisory polling projection and operation boundary.
//! Runtime coordination is deliberately separate from the client connection.
use ilium_core::polling_detection::{DetectorStatus, PollingSettings};
use ilium_ipc::polling::{
    PollingAccounting, PollingOperation, PollingRejection, PollingReply, PollingSnapshot,
};
use tokio::sync::{Mutex, RwLock};

/// Application paths supplied by the composition root; tests never infer HOME.
#[derive(Debug, Clone)]
pub struct PollingPaths {
    pub config_directory: std::path::PathBuf,
    pub data_directory: std::path::PathBuf,
    pub registry_directory: std::path::PathBuf,
}

pub(crate) struct PollingService {
    incarnation: uuid::Uuid,
    paths: std::sync::OnceLock<PollingPaths>,
    snapshot: std::sync::Arc<RwLock<PollingSnapshot>>,
    reload_transaction: Mutex<()>,
    // The completed I/O result owns both its storage charge and lease lifetime.
    // No SQLite connection crosses out of the bounded I/O worker.
    runtime: RwLock<Option<std::sync::Arc<ilium_execution::Retained<Bootstrap>>>>,
    /// Detector state only the elected leader's runtime can observe.
    leader_status: std::sync::Mutex<Option<DetectorStatus>>,
    provider: std::sync::OnceLock<crate::polling_leader::Provider>,
}

/// Day, month and all-time accounting as last read from the ledger.
pub(crate) type AccountingProjection = Option<(
    Option<PollingAccounting>,
    Option<PollingAccounting>,
    Option<PollingAccounting>,
)>;

/// One leader-runtime publication into the shared projection.
#[derive(Default)]
pub(crate) struct LeaderUpdate {
    pub finding: Option<ilium_core::polling_detection::Finding>,
    pub closed: Option<ilium_core::polling_detection::AgentBinding>,
    /// `Some(None)` clears the leader-observed status.
    pub status: Option<Option<DetectorStatus>>,
    pub accounting: Option<AccountingProjection>,
    pub counters: Option<(u64, u64)>,
}

const MAXIMUM_FINDINGS: usize = 256;

/// Owned runtime inputs; admitted storage and the lease follow physical work.
/// Custody is not dispatch permission: current settings, generation and native
/// ledger ownership still require validation immediately before paid admission.
pub(crate) struct LeaderGrant {
    pub owner: std::sync::Arc<crate::polling_ledger::LedgerOwner>,
    pub configuration: std::sync::Arc<crate::polling_config::PollingConfig>,
    _retention: std::sync::Arc<ilium_execution::Retained<Bootstrap>>,
}

struct Bootstrap {
    settings: PollingSettings,
    status: DetectorStatus,
    key_configured: bool,
    day: Option<PollingAccounting>,
    month: Option<PollingAccounting>,
    all_time: Option<PollingAccounting>,
    _owner: Option<std::sync::Arc<crate::polling_ledger::LedgerOwner>>,
    _configuration: Option<std::sync::Arc<crate::polling_config::PollingConfig>>,
}

impl Bootstrap {
    fn load(
        paths: PollingPaths,
        retained_owner: Option<std::sync::Arc<crate::polling_ledger::LedgerOwner>>,
    ) -> Result<Self, String> {
        let settings = crate::polling_config::load_settings(&paths.config_directory)
            .map_err(|error| error.to_string())?;
        let ledger = match retained_owner {
            Some(owner) => crate::polling_ledger::Ledger::connect(owner),
            None => crate::polling_ledger::Ledger::open(&paths.data_directory),
        };
        let (owner, day, month, all_time) = match ledger {
            Ok(ledger) => {
                // Startup owns the freshly recovered ledger exclusively; no
                // provider jobs are admitted before this projection is ready.
                let now = chrono::Local::now();
                let day = ledger
                    .summary_day(&now.format("%Y-%m-%d").to_string())
                    .map_err(|error| error.to_string())?
                    .map(accounting)
                    .transpose()?;
                let month = accounting(
                    ledger
                        .summary(&now.format("%Y-%m").to_string())
                        .map_err(|error| error.to_string())?,
                )?;
                let all_time = accounting(
                    ledger
                        .summary_all_time()
                        .map_err(|error| error.to_string())?,
                )?;
                (Some(ledger.into_owner()), day, Some(month), Some(all_time))
            }
            Err(crate::polling_ledger::LedgerError::AlreadyOwned) => (None, None, None, None),
            Err(error) => return Err(error.to_string()),
        };
        // Followers deliberately never deserialize or retain the credential.
        let configuration = if owner.is_some() {
            Some(std::sync::Arc::new(
                crate::polling_config::load(&paths.config_directory)
                    .map_err(|error| error.to_string())?,
            ))
        } else {
            None
        };
        let key_configured = configuration
            .as_ref()
            .is_some_and(|value| value.has_openai_key());
        let status = if !settings.enabled {
            DetectorStatus::Off
        } else if owner.is_none() {
            // The authoritative leader projection is not connected yet.
            DetectorStatus::Offline
        } else if !key_configured {
            DetectorStatus::NoApiKey
        } else {
            DetectorStatus::WaitingForActivity
        };
        Ok(Self {
            settings,
            status,
            key_configured,
            day,
            month,
            all_time,
            _owner: owner,
            _configuration: configuration,
        })
    }
}

pub(crate) fn accounting(
    summary: crate::polling_ledger::LedgerSummary,
) -> Result<PollingAccounting, String> {
    let nonnegative = |value: i64| {
        u64::try_from(value)
            .map_err(|_| "polling ledger contains invalid negative totals".to_string())
    };
    Ok(PollingAccounting {
        attempts: nonnegative(summary.attempts)?,
        known_cost_nanodollars: nonnegative(summary.known_cost_nanodollars)?,
        reserved_nanodollars: nonnegative(summary.reserved_nanodollars)?,
        unknown_attempts: nonnegative(summary.unknown_attempts)?,
        input_tokens: nonnegative(summary.known_input_tokens)?,
    })
}

impl Default for PollingService {
    fn default() -> Self {
        Self {
            incarnation: uuid::Uuid::new_v4(),
            paths: std::sync::OnceLock::new(),
            runtime: RwLock::new(None),
            leader_status: std::sync::Mutex::new(None),
            provider: std::sync::OnceLock::new(),
            reload_transaction: Mutex::new(()),
            snapshot: std::sync::Arc::new(RwLock::new(PollingSnapshot {
                revision: 0,
                status: DetectorStatus::Off,
                settings: PollingSettings::default(),
                key_configured: false,
                findings: Vec::new(),
                day: None,
                month: None,
                all_time: None,
                coalesced: 0,
                dropped_capacity: 0,
            })),
        }
    }
}

impl PollingService {
    pub(crate) async fn leader_grant(&self) -> Option<LeaderGrant> {
        // Follow refresh's runtime -> snapshot lock order. Neither guard is
        // retained by the returned work, so physical I/O never holds a lock.
        let runtime = self.runtime.read().await;
        let retained = runtime.as_ref()?;
        let snapshot = self.snapshot.read().await;
        if !matches!(
            snapshot.status,
            DetectorStatus::Watching | DetectorStatus::WaitingForActivity
        ) {
            return None;
        }
        let bootstrap = retained.view();
        let configuration = bootstrap._configuration.as_ref()?;
        if !configuration.settings.enabled
            || !configuration.has_openai_key()
            || configuration.settings.validate().is_err()
        {
            return None;
        }
        Some(LeaderGrant {
            owner: bootstrap._owner.as_ref()?.clone(),
            configuration: configuration.clone(),
            _retention: retained.clone(),
        })
    }

    /// Composition seam for the Decisions transport; set once before the
    /// leader runtime starts. Absent means OpenAI's fixed endpoint.
    #[allow(dead_code)]
    pub(crate) fn inject_provider(&self, provider: crate::polling_leader::Provider) -> bool {
        self.provider.set(provider).is_ok()
    }

    pub(crate) fn provider(&self) -> Option<crate::polling_leader::Provider> {
        self.provider.get().cloned()
    }

    /// Merge a leader publication; every change advances the snapshot revision.
    pub(crate) async fn publish(&self, update: LeaderUpdate) {
        if let Some(status) = update.status {
            *self
                .leader_status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
        }
        let mut snapshot = self.snapshot.write().await;
        let before = snapshot.revision;
        let mut changed = false;
        if let Some(Some(status)) = update.status {
            if matches!(
                snapshot.status,
                DetectorStatus::WaitingForActivity | DetectorStatus::Watching
            ) && snapshot.status != status
            {
                snapshot.status = status;
                changed = true;
            }
        }
        if let Some(binding) = update.closed {
            let count = snapshot.findings.len();
            snapshot.findings.retain(|finding| {
                !(finding.session_incarnation == binding.incarnation
                    && finding.pane_id == binding.pane
                    && finding.generation == binding.generation)
            });
            changed |= snapshot.findings.len() != count;
        }
        if let Some(finding) = update.finding {
            snapshot.findings.retain(|existing| {
                !(existing.session_incarnation == finding.session_incarnation
                    && existing.pane_id == finding.pane_id
                    && existing.generation == finding.generation)
            });
            snapshot.findings.push(finding);
            while snapshot.findings.len() > MAXIMUM_FINDINGS {
                snapshot.findings.remove(0);
            }
            changed = true;
        }
        if let Some(Some((day, month, all_time))) = update.accounting {
            snapshot.day = day;
            snapshot.month = month;
            snapshot.all_time = all_time;
            changed = true;
        }
        if let Some((coalesced, dropped)) = update.counters {
            if snapshot.coalesced != coalesced || snapshot.dropped_capacity != dropped {
                snapshot.coalesced = coalesced;
                snapshot.dropped_capacity = dropped;
                changed = true;
            }
        }
        if changed && snapshot.revision == before {
            snapshot.revision = snapshot.revision.saturating_add(1);
        }
    }

    pub(crate) fn incarnation(&self) -> uuid::Uuid {
        self.incarnation
    }

    /// The peer comes from the accepted transport before splitting, never IPC.
    pub(crate) async fn admit_feed(
        &self,
        peer_process_id: Option<u32>,
        subscription: ilium_ipc::polling::PollingEvidenceSubscription,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<ilium_execution::Retained<Option<crate::polling_feed::AuthorizedFeed>>, String>
    {
        let paths = self.paths.get().cloned();
        let serving_incarnation = self.incarnation.to_string();
        let mut admitted = execution
            .run(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 2 * 1024 * 1024,
                },
                move |_| {
                    let Some(paths) = paths else {
                        return Ok::<_, String>(None);
                    };
                    // Followers read settings only; credentials never enter a feed.
                    let settings = crate::polling_config::load_settings(&paths.config_directory)
                        .map_err(|error| error.to_string())?;
                    crate::polling_auth::AuthenticatedDetector::admit(
                        crate::polling_auth::FeedAdmission {
                            lease_path: &paths.data_directory.join("polling-detector.lock"),
                            peer_process_id,
                            subscription: &subscription,
                            serving_incarnation: &serving_incarnation,
                            settings: &settings,
                        },
                    )
                    .map(|proof| {
                        proof.map(|proof| crate::polling_feed::AuthorizedFeed {
                            proof: std::sync::Arc::new(proof),
                            configuration_directory: paths.config_directory,
                        })
                    })
                    .map_err(|error| error.to_string())
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        admitted
            .try_reduce_to_result_storage(2 * 1024 * 1024)
            .map_err(|reason| format!("polling feed admission retention failed: {reason:?}"))?;
        Ok(admitted)
    }

    pub(crate) async fn initialize(
        &self,
        paths: PollingPaths,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<(), String> {
        if self.paths.set(paths.clone()).is_err() {
            return Err("polling service was already initialized".into());
        }
        self.refresh(paths, execution).await
    }

    pub(crate) async fn refresh(
        &self,
        paths: PollingPaths,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<(), String> {
        // Serialize reloads and keep the last valid lease alive on any failure.
        // Reconnecting under this owner must never repeat startup recovery.
        let _transaction = self.reload_transaction.lock().await;
        let retained_owner = self
            .runtime
            .read()
            .await
            .as_ref()
            .and_then(|value| value.view()._owner.clone());
        let input_bytes = paths
            .config_directory
            .as_os_str()
            .len()
            .checked_add(paths.data_directory.as_os_str().len())
            .and_then(|bytes| bytes.checked_add(paths.registry_directory.as_os_str().len()))
            .and_then(|bytes| bytes.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(32 * 1024 * 1024))
            .ok_or_else(|| "polling startup allocation overflow".to_string())?;
        let loaded = execution
            .run(
                ilium_execution::Lane::Io,
                ilium_execution::JobCost {
                    input_bytes,
                    result_bytes: 2 * 1024 * 1024,
                },
                move |_| Bootstrap::load(paths, retained_owner),
            )
            .await;
        let mut loaded = match loaded {
            Ok(value) => value,
            Err(error) => {
                self.snapshot.write().await.status = DetectorStatus::HistoryUnavailable;
                return Err(error.to_string());
            }
        };
        // The long-lived lease needs storage custody, not an occupied I/O
        // execution slot or the temporary bootstrap-input reservation.
        if let Err(reason) = loaded.try_reduce_to_result_storage(2 * 1024 * 1024) {
            self.snapshot.write().await.status = DetectorStatus::HistoryUnavailable;
            return Err(format!(
                "polling startup result retention failed: {reason:?}"
            ));
        }
        // Reads can capture the current owner while slow reload I/O runs.
        let mut runtime = self.runtime.write().await;
        let mut snapshot = self.snapshot.write().await;
        snapshot.revision = snapshot.revision.saturating_add(1);
        snapshot.settings = loaded.view().settings.clone();
        snapshot.status = match (
            loaded.view().status,
            *self
                .leader_status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ) {
            (DetectorStatus::WaitingForActivity, Some(observed)) => observed,
            (loaded, _) => loaded,
        };
        snapshot.key_configured = loaded.view().key_configured;
        snapshot.day = loaded.view().day.clone();
        snapshot.month = loaded.view().month.clone();
        snapshot.all_time = loaded.view().all_time.clone();
        *runtime = Some(std::sync::Arc::new(loaded));
        Ok(())
    }

    pub(crate) async fn handle(
        &self,
        operation: PollingOperation,
        execution: &crate::execution::ExecutionClient,
    ) -> Result<ilium_execution::Retained<PollingReply>, String> {
        use crate::polling_projection::ProjectionJob;
        let job = if operation.validate().is_err() {
            ProjectionJob::Rejected(PollingRejection::InvalidRequest)
        } else {
            match operation {
                PollingOperation::Snapshot => ProjectionJob::Snapshot(self.snapshot.clone()),
                PollingOperation::History { cursor, limit } => {
                    let owner = self
                        .runtime
                        .read()
                        .await
                        .as_ref()
                        .and_then(|value| value.view()._owner.clone());
                    match owner {
                        Some(owner) => ProjectionJob::History {
                            owner,
                            cursor,
                            limit,
                        },
                        None => ProjectionJob::Rejected(PollingRejection::Unavailable),
                    }
                }
                // Provider and mutation workflows need their own owners.
                _ => ProjectionJob::Rejected(PollingRejection::Unavailable),
            }
        };
        let cost = job.cost();
        let mut reply = execution
            .run(ilium_execution::Lane::Io, cost, job)
            .await
            .map_err(|error| error.to_string())?;
        reply
            .try_reduce_to_result_storage(cost.result_bytes)
            .map_err(|reason| format!("polling projection retention failed: {reason:?}"))?;
        Ok(reply)
    }
}

/// The session owns reload even with no attached client or sound watcher.
/// Missed ticks coalesce; shutdown drops the caller while physical I/O keeps
/// its own admission and lease until completion.
pub(crate) fn spawn_config_watcher(
    state: std::sync::Arc<crate::state::ServerState>,
    paths: PollingPaths,
) -> tokio::task::JoinHandle<()> {
    let execution = state
        .execution
        .get()
        .map(|execution| execution.client.clone());
    tokio::spawn(async move {
        let Some(execution) = execution else { return };
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut failure_reported = false;
        loop {
            interval.tick().await;
            match state.polling.refresh(paths.clone(), &execution).await {
                Ok(()) => failure_reported = false,
                Err(error) => {
                    // One retained diagnostic per failure episode avoids
                    // growing the error journal on every unchanged tick.
                    if !failure_reported {
                        state.record_blocking_error(None, "polling_reload", &error);
                        failure_reported = true;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &std::path::Path) -> PollingPaths {
        PollingPaths {
            config_directory: root.join("config"),
            data_directory: root.join("data"),
            registry_directory: root.join("registry"),
        }
    }

    // Physical work must keep the accepted lease after the service is dropped;
    // invalid reload must block new grants without revoking existing custody.
    #[tokio::test]
    async fn polling_leader_grant_keeps_physical_custody_and_invalid_reload_closes_admission() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        std::fs::create_dir(&paths.config_directory).unwrap();
        std::fs::write(paths.config_directory.join("config.toml"),
            "[inference.openai]\napi_key='synthetic-local-key'\n[polling_detection]\nenabled=true\n").unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let service = PollingService::default();
        service
            .initialize(paths.clone(), &execution.client)
            .await
            .unwrap();
        let before = execution.client.quota_group().snapshot();
        let grant = service
            .leader_grant()
            .await
            .expect("enabled elected leader grants owned work");
        assert!(grant.configuration.has_openai_key());
        assert!(crate::polling_ledger::Ledger::connect(grant.owner.clone()).is_ok());
        assert_eq!(execution.client.quota_group().snapshot().jobs, before.jobs);
        std::fs::write(paths.config_directory.join("config.toml"), "[broken").unwrap();
        assert!(service
            .refresh(paths.clone(), &execution.client)
            .await
            .is_err());
        assert!(
            service.leader_grant().await.is_none(),
            "invalid settings close new admission"
        );
        drop(service);
        assert!(matches!(
            crate::polling_ledger::Ledger::open(&paths.data_directory),
            Err(crate::polling_ledger::LedgerError::AlreadyOwned)
        ));
        assert!(
            execution.client.quota_group().snapshot().result_bytes > 0,
            "physical work retains the original admitted storage"
        );
        drop(grant);
        assert_eq!(execution.client.quota_group().snapshot().result_bytes, 0);
        assert!(crate::polling_ledger::Ledger::open(&paths.data_directory).is_ok());
    }

    // A reload must not demote its own still-held lease to a follower.
    #[tokio::test]
    async fn polling_reload_applies_saved_settings_without_releasing_leadership() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        std::fs::create_dir(&paths.config_directory).unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let service = PollingService::default();
        service
            .initialize(paths.clone(), &execution.client)
            .await
            .unwrap();
        std::fs::write(
            paths.config_directory.join("config.toml"),
            "[polling_detection]\nenabled=true\nevent_interval=9\n",
        )
        .unwrap();
        service
            .refresh(paths.clone(), &execution.client)
            .await
            .unwrap();
        let reply = service
            .handle(PollingOperation::Snapshot, &execution.client)
            .await
            .unwrap();
        let PollingReply::Snapshot(snapshot) = reply.view() else {
            panic!("expected polling snapshot")
        };
        assert!(snapshot.settings.enabled);
        assert_eq!(snapshot.settings.event_interval, 9);
        assert_eq!(snapshot.status, DetectorStatus::NoApiKey);
        assert!(matches!(
            crate::polling_ledger::Ledger::open(&paths.data_directory),
            Err(crate::polling_ledger::LedgerError::AlreadyOwned)
        ));
    }

    // Broken configuration must stop detection without giving up its ledger.
    #[tokio::test]
    async fn polling_invalid_reload_fails_closed_and_recovers_under_the_same_lease() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        std::fs::create_dir(&paths.config_directory).unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let service = PollingService::default();
        service
            .initialize(paths.clone(), &execution.client)
            .await
            .unwrap();
        std::fs::write(paths.config_directory.join("config.toml"), "[broken").unwrap();
        assert!(service
            .refresh(paths.clone(), &execution.client)
            .await
            .is_err());
        let reply = service
            .handle(PollingOperation::Snapshot, &execution.client)
            .await
            .unwrap();
        let PollingReply::Snapshot(snapshot) = reply.view() else {
            panic!("expected polling snapshot")
        };
        assert_eq!(snapshot.status, DetectorStatus::HistoryUnavailable);
        assert!(matches!(
            crate::polling_ledger::Ledger::open(&paths.data_directory),
            Err(crate::polling_ledger::LedgerError::AlreadyOwned)
        ));
        std::fs::write(
            paths.config_directory.join("config.toml"),
            "[polling_detection]\nenabled=false\nevent_interval=8\n",
        )
        .unwrap();
        service.refresh(paths, &execution.client).await.unwrap();
        let reply = service
            .handle(PollingOperation::Snapshot, &execution.client)
            .await
            .unwrap();
        let PollingReply::Snapshot(snapshot) = reply.view() else {
            panic!("expected polling snapshot")
        };
        assert_eq!(snapshot.settings.event_interval, 8);
        assert_eq!(snapshot.status, DetectorStatus::Off);
    }

    // Returning a cloned snapshot without custody bypasses the output quota.
    #[tokio::test]
    async fn polling_snapshot_result_keeps_output_storage_until_consumed() {
        let root = tempfile::tempdir().unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let service = PollingService::default();
        service
            .initialize(paths(root.path()), &execution.client)
            .await
            .unwrap();
        let before = execution.client.quota_group().snapshot();
        let reply = service
            .handle(PollingOperation::Snapshot, &execution.client)
            .await
            .unwrap();
        let retained = execution.client.quota_group().snapshot();
        assert!(
            retained.result_bytes > before.result_bytes,
            "an outgoing polling projection must retain its allocation charge"
        );
        assert_eq!(
            retained.jobs, before.jobs,
            "a completed projection must not hold a worker slot"
        );
        drop(reply);
        assert_eq!(
            execution.client.quota_group().snapshot().result_bytes,
            before.result_bytes
        );
    }

    // Retaining the leader must not permanently occupy an I/O job credit.
    #[tokio::test]
    async fn polling_startup_releases_execution_credit_while_retaining_leadership() {
        let root = tempfile::tempdir().unwrap();
        let execution = crate::execution::ServerExecution::start().unwrap();
        let before = execution.client.quota_group().snapshot();
        let service = PollingService::default();
        let data_directory = root.path().join("data");
        service
            .initialize(
                PollingPaths {
                    config_directory: root.path().join("config"),
                    data_directory: data_directory.clone(),
                    registry_directory: root.path().join("registry"),
                },
                &execution.client,
            )
            .await
            .unwrap();
        let after = execution.client.quota_group().snapshot();
        assert_eq!(
            after.jobs, before.jobs,
            "a retained startup result must release its completed worker credit"
        );
        assert_eq!(
            after.input_bytes, before.input_bytes,
            "temporary startup input is no longer live"
        );
        assert!(matches!(
            crate::polling_ledger::Ledger::open(&data_directory),
            Err(crate::polling_ledger::LedgerError::AlreadyOwned)
        ));
        drop(service);
        assert!(crate::polling_ledger::Ledger::open(&data_directory).is_ok());
    }
}
