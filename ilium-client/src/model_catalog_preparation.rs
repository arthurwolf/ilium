//! Finite CPU phase after provider discovery. Refusal retains the original
//! catalog and source lease; it never fetches the provider a second time.
use ilium_execution::{
    Client, Job, JobContext, JobCost, JobOutcome, JobPoll, Lane, Receipt, StorageAdmission,
};
use ilium_inference::InferenceProviderKind;
use std::sync::Arc;
use std::time::Duration;
const MIB: usize = 1024 * 1024;
const MAX_MODELS: usize = 65_536;
const MAX_CATALOG_BYTES: usize = 16 * MIB;
const SOURCE_BYTES: usize = 40 * MIB;

pub(crate) struct CatalogRequest {
    pub provider: InferenceProviderKind,
    pub endpoint: String,
    pub elapsed: Duration,
    pub models: Vec<String>,
    pub keyed_revision: Option<u64>,
    pub selected_model: String,
    generation: u64,
    // This independent IO source lease survives provider/host-permit release.
    pub source: Arc<StorageAdmission>,
}
impl CatalogRequest {
    pub fn new(
        provider: InferenceProviderKind,
        endpoint: String,
        elapsed: Duration,
        models: Vec<String>,
        keyed_revision: Option<u64>,
        selected_model: &str,
        source: Option<Arc<StorageAdmission>>,
    ) -> Result<Self, String> {
        checked_catalog(&models, models.capacity(), endpoint.capacity())?;
        if selected_model.len() > 64 * 1024 {
            return Err("Selected model exceeds catalog capture bound".into());
        }
        let source = match source {
            Some(source) => source,
            None => Arc::new(
                crate::execution::process_quota()
                    .reserve_external_storage(SOURCE_BYTES)
                    .map_err(|reason| format!("Model catalog source admission: {reason:?}"))?,
            ),
        };
        Ok(Self {
            provider,
            endpoint,
            elapsed,
            models,
            keyed_revision,
            selected_model: selected_model.to_owned(),
            generation: 0,
            source,
        })
    }
}
pub(crate) struct PreparedCatalog {
    pub request: CatalogRequest,
    pub result: Result<(), String>,
    pub select_first: bool,
}
struct Prepare(CatalogRequest);
impl Job for Prepare {
    type Output = PreparedCatalog;
    type Error = std::convert::Infallible;
    fn run(mut self, context: JobContext) -> Result<PreparedCatalog, Self::Error> {
        let result = normalize(&mut self.0, &context);
        let select_first = result.is_ok()
            && self.0.provider == InferenceProviderKind::Ollama
            && (self.0.selected_model.is_empty()
                || !self.0.models.contains(&self.0.selected_model));
        Ok(PreparedCatalog {
            request: self.0,
            result,
            select_first,
        })
    }
}
fn checked_catalog(
    models: &[String],
    capacity: usize,
    endpoint_capacity: usize,
) -> Result<(), String> {
    let bytes = capacity
        .saturating_mul(std::mem::size_of::<String>())
        .saturating_add(models.iter().map(String::capacity).sum::<usize>());
    if models.len() > MAX_MODELS
        || bytes > MAX_CATALOG_BYTES
        || models.iter().any(|model| model.capacity() > 64 * 1024)
        || endpoint_capacity > 64 * 1024
    {
        return Err("Complete model catalog exceeds preparation count/physical-byte bound; previous catalog retained".into());
    }
    Ok(())
}
fn normalize(request: &mut CatalogRequest, context: &JobContext) -> Result<(), String> {
    if context.stop_requested() {
        return Err("Model catalog preparation cancelled".into());
    }
    if request.provider != InferenceProviderKind::KiloGateway {
        return Ok(());
    }
    let mut models = ilium_inference::kilo_gateway_fallback_models();
    models.append(&mut request.models);
    checked_catalog(&models, models.capacity(), request.endpoint.capacity())?;
    // Sort indices, not keys. Earlier equal entries win, then the keep-mask
    // removes duplicates in original order (fallbacks first, provider next).
    let mut indices: Vec<usize> = (0..models.len()).collect();
    indices
        .sort_unstable_by(|left, right| models[*left].cmp(&models[*right]).then(left.cmp(right)));
    let mut keep = vec![true; models.len()];
    for pair in indices.windows(2) {
        if models[pair[0]] == models[pair[1]] {
            keep[pair[1]] = false;
        }
    }
    if context.stop_requested() {
        return Err("Model catalog preparation cancelled".into());
    }
    let mut position = 0;
    models.retain(|_| {
        let retain = keep[position];
        position += 1;
        retain
    });
    checked_catalog(&models, models.capacity(), request.endpoint.capacity())?;
    request.models = models;
    Ok(())
}
/// At most one active/retiring job and one replaceable desired catalog.
pub(crate) struct ModelCatalogPreparation {
    client: Option<Client>,
    pending: Option<CatalogRequest>,
    active: Option<(u64, Receipt<Prepare>, CatalogRequest)>,
    generation: u64,
    closed: bool,
    diagnostic: Option<String>,
}
impl Default for ModelCatalogPreparation {
    fn default() -> Self {
        Self {
            client: {
                #[cfg(test)]
                {
                    Some(crate::execution::test_client())
                }
                #[cfg(not(test))]
                {
                    None
                }
            },
            pending: None,
            active: None,
            generation: 0,
            closed: false,
            diagnostic: None,
        }
    }
}
impl ModelCatalogPreparation {
    pub fn configure(&mut self, client: Client) {
        self.client = Some(client);
    }
    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
    pub fn request(&mut self, mut request: CatalogRequest) -> Result<(), String> {
        if self.closed {
            return Err("Model catalog preparation closed; previous catalog retained".into());
        }
        let Some(generation) = self.generation.checked_add(1) else {
            return Err("Model catalog generation exhausted".into());
        };
        self.generation = generation;
        request.generation = generation;
        self.pending = Some(request);
        if let Some((_, receipt, _)) = &self.active {
            receipt.cancel();
        }
        self.pump();
        Ok(())
    }
    fn pump(&mut self) {
        if self.closed || self.active.is_some() {
            return;
        }
        let Some(client) = &self.client else {
            self.diagnostic =
                Some("Model catalog CPU owner unavailable; original catalog retained".into());
            return;
        };
        let Some(request) = self.pending.take() else {
            return;
        };
        let generation = request.generation;
        // This fixed metadata copy is covered by the same independent source;
        // it can report a lost/panicked job without cloning the catalog itself.
        let fallback = CatalogRequest {
            provider: request.provider,
            endpoint: request.endpoint.clone(),
            elapsed: request.elapsed,
            models: Vec::new(),
            keyed_revision: request.keyed_revision,
            selected_model: request.selected_model.clone(),
            generation,
            source: Arc::clone(&request.source),
        };
        match client.try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: 64 * MIB,
                result_bytes: 4096,
            },
            Prepare(request),
        ) {
            Ok(receipt) => {
                self.active = Some((generation, receipt, fallback));
                self.diagnostic = None;
            }
            Err(rejected) => {
                self.pending = Some(rejected.value.0);
                self.diagnostic = Some(format!(
                    "Model catalog CPU admission: {:?}; original result retained for retry",
                    rejected.reason
                ));
            }
        }
    }
    pub fn collect(&mut self) -> Option<PreparedCatalog> {
        let mut prepared = None;
        if let Some((generation, receipt, _)) = &mut self.active {
            let generation = *generation;
            let outcome = match receipt.try_take() {
                JobPoll::Pending => None,
                JobPoll::Ready(outcome) => Some(Some(outcome.into_parts().0)),
                _ => Some(None),
            };
            if let Some(outcome) = outcome {
                let fallback = self.active.take().map(|(_, _, fallback)| fallback);
                match outcome {
                    Some(JobOutcome::Finished(Ok(result)))
                        if generation == self.generation && !self.closed =>
                    {
                        prepared = Some(result)
                    }
                    Some(JobOutcome::NotStarted { job, .. })
                        if generation == self.generation && !self.closed =>
                    {
                        self.pending = Some(job.0)
                    }
                    Some(JobOutcome::Panicked) | None
                        if generation == self.generation && !self.closed =>
                    {
                        let error = "Model catalog CPU worker failed; previous catalog retained"
                            .to_string();
                        self.diagnostic = Some(error.clone());
                        if let Some(request) = fallback {
                            prepared = Some(PreparedCatalog {
                                request,
                                result: Err(error),
                                select_first: false,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        self.pump();
        prepared
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some() || self.active.is_some()
    }
    pub fn invalidate(&mut self) {
        self.pending = None;
        match self.generation.checked_add(1) {
            Some(generation) => self.generation = generation,
            None => self.closed = true,
        }
        if let Some((_, receipt, _)) = &self.active {
            receipt.cancel();
        }
    }
    pub fn close(&mut self) {
        self.closed = true;
        self.pending = None;
        if let Some((_, receipt, _)) = &self.active {
            receipt.cancel();
        }
    }
}
impl Drop for ModelCatalogPreparation {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode,
    };
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    use std::time::Instant;

    struct Block {
        entered: SyncSender<std::thread::ThreadId>,
        release: Receiver<()>,
    }
    impl Job for Block {
        type Output = ();
        type Error = ();
        fn run(self, _: JobContext) -> Result<(), ()> {
            self.entered.send(std::thread::current().id()).unwrap();
            self.release.recv().unwrap();
            Ok(())
        }
    }
    fn bank(client_jobs: usize) -> (Execution, Client, QuotaGroup) {
        let quota = QuotaGroup::new(QuotaLimits {
            clients: 2,
            jobs: 8,
            service_jobs: 0,
            input_bytes: 128 * MIB,
            result_bytes: MIB,
            worker_threads: 1,
            // Includes the actual native worker, bounded bank metadata and
            // independent original/result storage; no production cap change.
            worker_bytes: 256 * MIB,
        });
        let lane = |threads| LaneConfig {
            threads,
            queue_slots: if threads == 0 { 0 } else { 4 },
            priority: None,
            resident_bytes_per_thread: 64 * MIB,
        };
        let owner = Execution::start(
            quota.clone(),
            ExecutionConfig {
                cpu: lane(1),
                io: lane(0),
                service: lane(0),
            },
        )
        .unwrap();
        let client = owner
            .client(ClientLimits {
                jobs: client_jobs,
                service_jobs: 0,
                input_bytes: 128 * MIB,
                result_bytes: MIB,
            })
            .unwrap();
        (owner, client, quota)
    }
    fn request(quota: &QuotaGroup, models: Vec<String>) -> CatalogRequest {
        CatalogRequest::new(
            InferenceProviderKind::KiloGateway,
            "https://catalog.invalid/models".into(),
            Duration::from_millis(19),
            models,
            None,
            "saved-model",
            Some(Arc::new(
                quota.reserve_external_storage(SOURCE_BYTES).unwrap(),
            )),
        )
        .unwrap()
    }
    fn settle(actor: &mut ModelCatalogPreparation) -> PreparedCatalog {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(prepared) = actor.collect() {
                return prepared;
            }
            assert!(Instant::now() < deadline, "{:?}", actor.diagnostic());
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn finish(mut owner: Execution, client: Client, quota: &QuotaGroup) {
        drop(client);
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(owner);
        assert_eq!(quota.snapshot().jobs, 0);
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn busy_cpu_retains_original_catalog_without_provider_replay_and_releases_finite_envelope() {
        let (mut owner, client, quota) = bank(1);
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let block = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: std::mem::size_of::<Block>(),
                    result_bytes: 1,
                },
                Block {
                    entered: entered_tx,
                    release: release_rx,
                },
            )
            .unwrap();
        assert_ne!(
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            std::thread::current().id()
        );
        let original = request(
            &quota,
            vec![
                "extra/free".into(),
                "extra/free".into(),
                "another/free".into(),
            ],
        );
        let vector = original.models.as_ptr();
        let name = original.models[0].as_ptr();
        let source = Arc::downgrade(&original.source);
        let mut actor = ModelCatalogPreparation::default();
        actor.configure(client.clone());
        let started = Instant::now();
        actor.request(original).unwrap();
        assert!(started.elapsed() < Duration::from_millis(100));
        for _ in 0..16 {
            assert!(actor.collect().is_none());
            let pending = actor.pending.as_ref().unwrap();
            assert_eq!(pending.models.as_ptr(), vector);
            assert_eq!(pending.models[0].as_ptr(), name);
            assert!(source.upgrade().is_some());
            assert_eq!(
                quota.snapshot().jobs,
                1,
                "refusal never creates another job"
            );
        }
        release_tx.send(()).unwrap();
        drop(block);
        let prepared = settle(&mut actor);
        assert!(prepared.result.is_ok());
        let mut expected = ilium_inference::kilo_gateway_fallback_models();
        expected.extend(["extra/free".into(), "another/free".into()]);
        assert_eq!(
            prepared.request.models, expected,
            "complete fallback-first stable catalog"
        );
        assert_eq!(
            prepared
                .request
                .models
                .iter()
                .find(|name| name.as_str() == "extra/free")
                .unwrap()
                .as_ptr(),
            name
        );
        assert_eq!(prepared.request.elapsed, Duration::from_millis(19));
        assert_eq!(
            quota.snapshot().jobs,
            0,
            "installed catalog never pins finite credit"
        );
        drop(actor);
        drop(client);
        owner.request_shutdown(ShutdownMode::Cancel);
        assert_eq!(
            owner
                .join_until_background(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .remaining_workers,
            0
        );
        drop(owner);
        assert!(
            source.upgrade().is_some(),
            "source remains charged after owner join"
        );
        drop(prepared);
        assert!(source.upgrade().is_none());
        assert_eq!(quota.snapshot().worker_bytes, 0);
    }

    #[test]
    fn superseded_queued_catalog_retires_before_new_generation_and_close_is_nonblocking() {
        let (owner, client, quota) = bank(8);
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let block = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: std::mem::size_of::<Block>(),
                    result_bytes: 1,
                },
                Block {
                    entered: entered_tx,
                    release: release_rx,
                },
            )
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut actor = ModelCatalogPreparation::default();
        actor.configure(client.clone());
        let first = request(&quota, vec!["obsolete/free".into()]);
        let obsolete = Arc::downgrade(&first.source);
        actor.request(first).unwrap();
        actor
            .request(request(&quota, vec!["current/free".into()]))
            .unwrap();
        for _ in 0..16 {
            assert!(actor.collect().is_none());
            assert_eq!(
                quota.snapshot().jobs,
                2,
                "one retiring CPU job plus controlled blocker"
            );
        }
        release_tx.send(()).unwrap();
        drop(block);
        let prepared = settle(&mut actor);
        assert!(prepared.result.is_ok());
        assert!(prepared
            .request
            .models
            .iter()
            .any(|name| name == "current/free"));
        assert!(!prepared
            .request
            .models
            .iter()
            .any(|name| name == "obsolete/free"));
        assert!(obsolete.upgrade().is_none(), "old request really retired");
        drop(prepared);
        let started = Instant::now();
        actor.close();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(actor
            .request(request(&quota, vec!["after-close".into()]))
            .is_err());
        drop(actor);
        finish(owner, client, &quota);
    }

    #[test]
    fn cancelled_before_start_retries_original_catalog_and_spare_capacity_is_rejected() {
        let (owner, client, quota) = bank(8);
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        let block = client
            .try_submit(
                Lane::Cpu,
                JobCost {
                    input_bytes: std::mem::size_of::<Block>(),
                    result_bytes: 1,
                },
                Block {
                    entered: entered_tx,
                    release: release_rx,
                },
            )
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut actor = ModelCatalogPreparation::default();
        actor.configure(client.clone());
        let original = request(&quota, vec!["retried/free".into()]);
        let name = original.models[0].as_ptr();
        actor.request(original).unwrap();
        actor.active.as_ref().unwrap().1.cancel();
        release_tx.send(()).unwrap();
        drop(block);
        let prepared = settle(&mut actor);
        assert!(prepared.result.is_ok());
        assert_eq!(
            prepared
                .request
                .models
                .iter()
                .find(|name| name.as_str() == "retried/free")
                .unwrap()
                .as_ptr(),
            name
        );
        let mut excessive = String::with_capacity(128 * 1024);
        excessive.push_str("short");
        assert!(CatalogRequest::new(
            InferenceProviderKind::Ollama,
            "endpoint".into(),
            Duration::ZERO,
            vec![excessive],
            None,
            "",
            Some(Arc::clone(&prepared.request.source))
        )
        .is_err());
        drop(prepared);
        drop(actor);
        finish(owner, client, &quota);
    }
}
