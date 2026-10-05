//! Native HTTPS/source authority for one ORIGINAL submitted operation.
//! This module starts no executor, DNS, network or credential-store lookup.
use crate::{
    engine::HostRequest,
    error::{AnimationError, Result},
    http::{self, DnsResolver, HttpAuthority, HttpOptions, HttpPhase},
    network::{classify_address, validate_https_origin, NetworkAddressClass},
    permissions::{
        Channel, HttpMethod, OperationNeed, OperationTicket, PackageIdentity, PermissionBroker,
    },
};
use ilium_execution::{Client, JobContext, QuotaGroup, StorageAdmission};
use ilium_platform::owned_worker::StopToken;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use url::Url;
const AUTHORITY_BYTES: usize = 64 * 1024;
const CREDENTIAL_BYTES: usize = 32 * 1024;

fn denied(message: &'static str) -> AnimationError {
    AnimationError::PermissionDenied(message.into())
}
fn auth(error: crate::permissions::PermissionError) -> AnimationError {
    AnimationError::PermissionDenied(error.to_string())
}

/// Inject an ACTUAL native selected-credential backend. It must bind its own
/// handle to this authenticated principal/exact origin and current selection.
/// Store IO occurs OUTSIDE broker guard; all returned data must be root-admitted.
/// No actual keyring/backend is supplied by this trait or by the authority factory.
pub trait HostCredentialAdapter: Send {
    fn bound_headers(
        &mut self,
        principal: &PackageIdentity,
        handle: &str,
        exact_origin: &str,
        quota: &QuotaGroup,
    ) -> Result<BoundCredentialHeaders>;
}
/// Native, admitted selected-credential output; no Deserialize/public fields.
pub struct BoundCredentialHeaders {
    principal: PackageIdentity,
    handle: String,
    origin: String,
    headers: BTreeMap<String, String>,
    quota: QuotaGroup,
    _storage: StorageAdmission,
}
impl BoundCredentialHeaders {
    /// Real native store calls this with ALREADY admitted selected source data.
    /// Reserve original output charge BEFORE cloning any key/value/identity.
    pub fn from_host_selection(
        principal: &PackageIdentity,
        handle: &str,
        origin: &str,
        headers: &BTreeMap<String, String>,
        quota: &QuotaGroup,
    ) -> Result<Self> {
        if handle.is_empty() || handle.len() > 128 || handle.chars().any(char::is_control) {
            return Err(denied("invalid native credential handle"));
        }
        if origin.len() > 4096 || headers.len() > 32 {
            return Err(denied("native credential origin or header budget"));
        }
        let storage = quota
            .reserve_external_storage(CREDENTIAL_BYTES)
            .map_err(|_| AnimationError::Budget("native credential output admission".into()))?;
        if validate_https_origin(origin)? != origin {
            return Err(denied("native credential origin must be canonical"));
        }
        validate_credential_headers(headers)?;
        Ok(Self {
            principal: principal.clone(),
            handle: handle.into(),
            origin: origin.into(),
            headers: headers.clone(),
            quota: quota.clone(),
            _storage: storage,
        })
    }
}
fn validate_credential_headers(headers: &BTreeMap<String, String>) -> Result<()> {
    if headers.len() > 32 {
        return Err(denied("native credential header count"));
    }
    let mut bytes = 0usize;
    let mut names = BTreeSet::new();
    for (name, value) in headers {
        if name.is_empty()
            || name.len() > 64
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.len() > 8192
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(denied("invalid native credential header"));
        }
        let lower = name.to_ascii_lowercase();
        if !names.insert(lower.clone())
            || matches!(
                lower.as_str(),
                "host"
                    | "proxy-authorization"
                    | "connection"
                    | "upgrade"
                    | "transfer-encoding"
                    | "content-length"
                    | "te"
                    | "trailer"
                    | "user-agent"
            )
        {
            return Err(denied("native credential transport header refused"));
        }
        bytes = bytes.saturating_add(name.len()).saturating_add(value.len());
        if bytes > 8192 {
            return Err(denied("native credential header budget"));
        }
    }
    Ok(()) // Authorization/Cookie are allowed only from actual host-bound credential output.
}

/// Construct before finite submission; it confers NO issued/network authority.
/// The original native IO owner calls enter only in its ACTUAL JobContext.
pub(crate) struct NativeHttpAuthorityFactory {
    broker: Arc<Mutex<PermissionBroker>>,
    channel: Channel,
    ticket: Arc<OperationTicket>,
    request: Option<HostRequest>,
    feed: Option<(StopToken, Instant)>,
    principal: PackageIdentity,
    quota: QuotaGroup,
    credentials: Option<Box<dyn HostCredentialAdapter>>,
    _storage: StorageAdmission,
}
impl NativeHttpAuthorityFactory {
    pub(crate) fn from_native(
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        ticket: Arc<OperationTicket>,
        request: HostRequest,
        quota: QuotaGroup,
        credentials: Option<Box<dyn HostCredentialAdapter>>,
    ) -> Result<Self> {
        if !request.payload.shares_root(&quota) || request.is_cancelled() {
            return Err(denied("native HTTP request root or lifetime mismatch"));
        }
        let storage = quota
            .reserve_external_storage(AUTHORITY_BYTES)
            .map_err(|_| AnimationError::Budget("native HTTP authority admission".into()))?;
        let principal = {
            let owner = broker
                .lock()
                .map_err(|_| denied("native HTTP authority poisoned"))?;
            owner
                .check_operation_lineage(&ticket, &channel)
                .map_err(auth)?;
            let (instance, revision, epoch) = owner.channel_coordinates(&channel).map_err(auth)?;
            if request.authority.instance_id != instance
                || request.authority.plan_generation != revision
                || request.authority.authorization_epoch != epoch
            {
                return Err(denied("native HTTP request channel coordinates mismatch"));
            }
            owner.identity().clone()
        };
        Ok(Self {
            broker,
            channel,
            ticket,
            request: Some(request),
            feed: None,
            principal,
            quota,
            credentials,
            _storage: storage,
        })
    }
    /// A persistent source refresh owns a new native async ticket and its own
    /// bounded transaction deadline. It never borrows the consumed opener.
    pub(crate) fn from_feed(
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        ticket: Arc<OperationTicket>,
        quota: QuotaGroup,
        stop: StopToken,
        deadline: Instant,
        credentials: Option<Box<dyn HostCredentialAdapter>>,
    ) -> Result<Self> {
        if stop.is_stopped() || Instant::now() >= deadline {
            return Err(denied("native source feed refresh stopped or expired"));
        }
        let storage = quota
            .reserve_external_storage(AUTHORITY_BYTES)
            .map_err(|_| AnimationError::Budget("native HTTP feed authority admission".into()))?;
        let principal = {
            let owner = broker
                .lock()
                .map_err(|_| denied("native HTTP feed authority poisoned"))?;
            owner
                .check_operation_lineage(&ticket, &channel)
                .map_err(auth)?;
            owner.channel_coordinates(&channel).map_err(auth)?;
            owner.identity().clone()
        };
        Ok(Self {
            broker,
            channel,
            ticket,
            request: None,
            feed: Some((stop, deadline)),
            principal,
            quota,
            credentials,
            _storage: storage,
        })
    }
    fn stopped_or_expired(&self) -> bool {
        self.request.as_ref().is_some_and(HostRequest::is_cancelled)
            || self
                .feed
                .as_ref()
                .is_some_and(|(stop, deadline)| stop.is_stopped() || Instant::now() >= *deadline)
    }
    pub(crate) fn enter(
        self,
        context: &JobContext,
        original_client: &Client,
    ) -> Result<NativeHttpAuthority> {
        if !original_client.quota_group().shares_root(&self.quota)
            || context.stop_requested()
            || self.stopped_or_expired()
        {
            return Err(denied("native HTTP IO owner root or cancellation mismatch"));
        }
        {
            let owner = self
                .broker
                .lock()
                .map_err(|_| denied("native HTTP authority poisoned"))?;
            owner
                .check_committed_operation(&self.ticket, &self.channel)
                .map_err(auth)?;
        }
        Ok(NativeHttpAuthority {
            factory: self,
            io_stop: context.stop_token(),
            hop: None,
            credential_output: None,
        })
    }
}
struct Hop {
    method: HttpMethod,
    url: String,
    addresses: Vec<SocketAddr>,
    dispatched: bool,
}
/// Owned ONLY by original native submitted job/result owner, never by JS.
pub(crate) struct NativeHttpAuthority {
    factory: NativeHttpAuthorityFactory,
    io_stop: StopToken,
    hop: Option<Hop>,
    credential_output: Option<BoundCredentialHeaders>, // Keep provider's original charge through HTTP copies.
}
impl NativeHttpAuthority {
    fn check_lifetime(&self) -> Result<()> {
        if self.io_stop.is_stopped() || self.factory.stopped_or_expired() {
            return Err(denied("native HTTP operation cancelled or expired"));
        }
        Ok(())
    }
    fn check_needs(&self, needs: &[OperationNeed]) -> Result<()> {
        self.check_lifetime()?;
        let broker = self
            .factory
            .broker
            .lock()
            .map_err(|_| denied("native HTTP authority poisoned"))?;
        broker
            .check_committed_needs(&self.factory.ticket, &self.factory.channel, needs)
            .map_err(auth)
    }
    fn current_hop_needs(&self) -> Result<Vec<OperationNeed>> {
        let hop = self
            .hop
            .as_ref()
            .filter(|hop| hop.dispatched)
            .ok_or_else(|| denied("native HTTP hop not dispatched"))?;
        OperationNeed::http_hop(
            &hop.url,
            hop.method,
            &hop.addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>(),
        )
        .map_err(auth)
    }
    /// Bounded native final body handoff only. No IO/DNS/decode/JS/credential
    /// lookup/wait/reentrant broker access in callback. This DOES NOT settle the
    /// whole original source job; root later delivers/settles its original ticket.
    pub(crate) fn with_body_delivery<T>(
        &mut self,
        publish: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.check_lifetime()?;
        let needs = self.current_hop_needs()?;
        let broker = self
            .factory
            .broker
            .lock()
            .map_err(|_| denied("native HTTP authority poisoned"))?;
        broker
            .check_committed_needs(&self.factory.ticket, &self.factory.channel, &needs)
            .map_err(auth)?;
        self.check_lifetime()?;
        publish()
    }
    /// Real raw reader/source path, same original root, no eager Value expansion.
    /// Caller already owns declared IO input/scratch and keeps actual Receipt.
    pub(crate) fn request_source_response(
        &mut self,
        options: &HttpOptions,
        dns: &impl DnsResolver,
        stop: &AtomicBool,
    ) -> Result<crate::sources::SourceHttpResponse> {
        self.check_lifetime()?;
        if stop.load(Ordering::Acquire) {
            return Err(denied("native HTTP source cancelled"));
        }
        let quota = self.factory.quota.clone();
        let response = http::request_source_response(options, self, dns, stop, &quota)?;
        self.with_body_delivery(|| {
            if stop.load(Ordering::Acquire) {
                return Err(denied("native HTTP source cancelled"));
            }
            Ok(response)
        })
    }
}
impl HttpAuthority for NativeHttpAuthority {
    fn authorize(
        &mut self,
        phase: HttpPhase,
        method: HttpMethod,
        url: &Url,
        addresses: &[SocketAddr],
    ) -> Result<()> {
        self.check_lifetime()?;
        if url.as_str().len() > 4096 {
            return Err(denied("native HTTP target budget"));
        }
        match phase {
            HttpPhase::Preflight => {
                if !addresses.is_empty() {
                    return Err(denied("preflight cannot accept DNS authority"));
                }
                let need = OperationNeed::http_preflight(url.as_str(), method).map_err(auth)?;
                self.check_needs(&[need])?;
                self.hop = Some(Hop {
                    method,
                    url: url.as_str().into(),
                    addresses: Vec::new(),
                    dispatched: false,
                });
                self.credential_output = None;
            }
            HttpPhase::Dispatch => {
                let hop = self
                    .hop
                    .as_ref()
                    .ok_or_else(|| denied("HTTP dispatch before preflight"))?;
                if hop.url != url.as_str() || hop.method != method || hop.dispatched {
                    return Err(denied("HTTP dispatch hop mismatch"));
                }
                let port = url
                    .port_or_known_default()
                    .ok_or_else(|| denied("native HTTP port"))?;
                if addresses.is_empty()
                    || addresses.len() > 16
                    || addresses.iter().any(|address| {
                        address.port() != port
                            || classify_address(address.ip()) == NetworkAddressClass::Forbidden
                    })
                {
                    return Err(denied("native HTTP DNS answers refused"));
                }
                let needs = OperationNeed::http_hop(
                    url.as_str(),
                    method,
                    &addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>(),
                )
                .map_err(auth)?;
                self.check_needs(&needs)?;
                self.hop = Some(Hop {
                    method,
                    url: url.as_str().into(),
                    addresses: addresses.to_vec(),
                    dispatched: true,
                });
            }
            HttpPhase::Delivery => {
                let hop = self
                    .hop
                    .as_ref()
                    .ok_or_else(|| denied("HTTP delivery before dispatch"))?;
                if !hop.dispatched
                    || hop.url != url.as_str()
                    || hop.method != method
                    || hop.addresses != addresses
                {
                    return Err(denied("HTTP delivery hop mismatch"));
                }
                self.check_needs(&self.current_hop_needs()?)?;
            }
        }
        Ok(()) // Each lock released before real DNS/connect/read or next native transport phase.
    }
    fn credential_headers(&mut self, handle: &str, url: &Url) -> Result<BTreeMap<String, String>> {
        if handle.is_empty() || handle.len() > 128 || handle.chars().any(char::is_control) {
            return Err(denied("invalid native credential handle"));
        }
        let hop = self
            .hop
            .as_ref()
            .filter(|hop| hop.dispatched && hop.url == url.as_str())
            .ok_or_else(|| denied("native credential lookup outside dispatched hop"))?;
        let needs = OperationNeed::http_hop(
            &hop.url,
            hop.method,
            &hop.addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>(),
        )
        .map_err(auth)?;
        self.check_needs(&needs)?; // Release guard BEFORE host-selected store access.
        let origin = url.origin().ascii_serialization();
        let provider = self
            .factory
            .credentials
            .as_mut()
            .ok_or_else(|| denied("no native credential backend bound"))?;
        let output = provider.bound_headers(
            &self.factory.principal,
            handle,
            &origin,
            &self.factory.quota,
        )?;
        self.check_needs(&needs)?; // Lookup cannot survive revoke/stop/deadline.
        if output.principal != self.factory.principal
            || output.handle != handle
            || output.origin != origin
            || !output.quota.shares_root(&self.factory.quota)
        {
            return Err(denied("native credential selected binding mismatch"));
        }
        validate_credential_headers(&output.headers)?;
        let headers = output.headers.clone(); // Original admitted authority covers bounded returned header copy.
        self.credential_output = Some(output);
        Ok(headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{EngineLimits, ServiceAuthority, ServiceBudget, ServicePhase, ServiceValue},
        permissions::{
            CallPhase, Capability, Ceiling, Demand, PermissionPlan, PermissionRequest, Right,
            Scope, UserChoice,
        },
    };
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, JobCost, JobOutcome, JobPoll, Lane, LaneConfig,
        QuotaLimits, Retained, ShutdownMode,
    };
    use std::time::{Duration, Instant};
    struct Fixture {
        execution: Execution,
        client: Client,
        broker: Arc<Mutex<PermissionBroker>>,
        channel: Channel,
        ticket: Arc<OperationTicket>,
        request: HostRequest,
        quota: QuotaGroup,
        net: Right,
    }
    impl Fixture {
        fn new(local: bool, combined: bool) -> Self {
            let quota = QuotaGroup::new(QuotaLimits {
                clients: 2,
                jobs: 2,
                service_jobs: 0,
                input_bytes: 2 * 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
                worker_threads: 2,
                worker_bytes: 4 * 1024 * 1024,
            });
            let zero = LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            };
            let execution = Execution::start(
                quota.clone(),
                ExecutionConfig {
                    cpu: zero,
                    service: zero,
                    io: LaneConfig {
                        threads: 1,
                        queue_slots: 2,
                        priority: None,
                        resident_bytes_per_thread: 1024,
                    },
                },
            )
            .unwrap();
            let client = execution
                .client(ClientLimits {
                    jobs: 2,
                    service_jobs: 0,
                    input_bytes: 1024 * 1024,
                    result_bytes: 1024 * 1024,
                })
                .unwrap();
            let net = Right {
                id: Capability::NetworkHttp,
                scope: Scope::Network {
                    origins: BTreeSet::from([
                        "https://example.org".into(),
                        "https://other.example.org".into(),
                    ]),
                    methods: BTreeSet::from([HttpMethod::Get]),
                },
            };
            let local_right = Right {
                id: Capability::NetworkLocal,
                scope: net.scope.clone(),
            };
            let mut rights = vec![net.clone()];
            if local {
                rights.push(local_right.clone());
            }
            let ceiling = Ceiling {
                permissions: rights.clone(),
            };
            let mut broker = PermissionBroker::new(
                PackageIdentity::unverified(
                    "http_fixture".into(),
                    b"synthetic-native-http-contract",
                )
                .unwrap(),
                ceiling.clone(),
                ceiling,
            )
            .unwrap();
            let requests = rights
                .iter()
                .enumerate()
                .map(|(n, right)| PermissionRequest {
                    request_id: Some(if n == 0 { "net" } else { "local" }.into()),
                    id: right.id,
                    scope: right.scope.clone(),
                    required: false,
                    reason: "Synthetic offline native fixture".into(),
                })
                .collect();
            let mut ids = BTreeSet::from(["net".into()]);
            if local && combined {
                ids.insert("local".into());
            }
            let review = broker
                .prepare(
                    1,
                    1,
                    PermissionPlan {
                        permissions: requests,
                        demands: vec![Demand {
                            demand_id: "source".into(),
                            request_ids: ids,
                        }],
                    },
                    BTreeMap::new(),
                )
                .unwrap();
            let mut answers = BTreeMap::from([("net".into(), UserChoice::AllowSession)]);
            if local {
                answers.insert("local".into(), UserChoice::AllowSession);
            }
            let active = broker.resolve(review, answers).unwrap().activation.unwrap();
            let authority = ServiceAuthority {
                instance_id: active.plan.instance_id,
                plan_generation: active.plan.plan_revision,
                authorization_epoch: active.plan.authorization_epoch,
            };
            let limits = EngineLimits::default();
            let payload = ServiceValue::copy_request_from_host(
                &serde_json::json!({}),
                &[],
                &BTreeMap::new(),
                &limits,
                quota.clone(),
                &ServiceBudget::new(&limits),
            )
            .unwrap();
            let request = HostRequest::from_transport(
                1,
                "http.request".into(),
                30000,
                "f".repeat(64),
                authority,
                ServicePhase::Async,
                payload,
            )
            .unwrap();
            let ticket = Arc::new(
                broker
                    .dispatch(
                        &active.channel,
                        CallPhase::Async,
                        "source",
                        vec![OperationNeed::http_preflight(
                            "https://example.org/data",
                            HttpMethod::Get,
                        )
                        .unwrap()],
                    )
                    .unwrap(),
            );
            Self {
                execution,
                client,
                broker: Arc::new(Mutex::new(broker)),
                channel: active.channel,
                ticket,
                request,
                quota,
                net,
            }
        }
        fn factory(
            &self,
            credentials: Option<Box<dyn HostCredentialAdapter>>,
        ) -> Result<NativeHttpAuthorityFactory> {
            NativeHttpAuthorityFactory::from_native(
                Arc::clone(&self.broker),
                self.channel.clone(),
                Arc::clone(&self.ticket),
                self.request.clone(),
                self.quota.clone(),
                credentials,
            )
        }
        fn issue(
            &self,
            committed: bool,
            credentials: Option<Box<dyn HostCredentialAdapter>>,
        ) -> Retained<Result<NativeHttpAuthority>> {
            let factory = self.factory(credentials).unwrap();
            let original_client = self.client.clone();
            let body = move |context: JobContext| factory.enter(&context, &original_client);
            let submit = || {
                self.client
                    .try_submit(
                        Lane::Io,
                        JobCost {
                            input_bytes: 128 * 1024,
                            result_bytes: 128 * 1024,
                        },
                        body,
                    )
                    .map_err(Box::new)
            };
            // ACTUAL finite IO submission is the committed effect; never commit(||()).
            let mut receipt = if committed {
                self.broker
                    .lock()
                    .unwrap()
                    .commit(&self.ticket, submit)
                    .unwrap()
                    .unwrap()
            } else {
                submit().unwrap()
            };
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match receipt.try_take() {
                    JobPoll::Ready(outcome) => {
                        return outcome.map(|outcome| match outcome {
                            JobOutcome::Finished(value) => value,
                            _ => panic!("fixture original native IO did not finish"),
                        })
                    }
                    JobPoll::Pending if Instant::now() < deadline => std::thread::yield_now(),
                    _ => panic!("fixture original native IO outcome missing"),
                }
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.execution.request_shutdown(ShutdownMode::Drain);
            let report = self
                .execution
                .join_until_background(Instant::now() + Duration::from_secs(2));
            let report = report.unwrap();
            assert_eq!(report.remaining_workers, 0);
            assert_eq!(
                report
                    .health
                    .lanes
                    .iter()
                    .map(|lane| lane.joined)
                    .sum::<usize>(),
                1
            ); // Actual platform join, not callback exit.
        }
    }
    fn public() -> Vec<SocketAddr> {
        vec!["93.184.216.34:443".parse().unwrap()]
    }
    fn private() -> Vec<SocketAddr> {
        vec!["10.0.0.1:443".parse().unwrap()]
    }
    #[test]
    fn queued_ticket_refuses_and_real_original_io_submission_allows() {
        let fixture = Fixture::new(false, false);
        let unissued = fixture.issue(false, None);
        assert!(unissued.view().is_err());
        drop(unissued);
        let issued = fixture.issue(true, None).map(|authority| {
            let mut authority = authority.unwrap();
            let url = Url::parse("https://example.org/data").unwrap();
            authority
                .authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])
                .unwrap();
            authority
                .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &public())
                .unwrap();
            authority
                .authorize(HttpPhase::Delivery, HttpMethod::Get, &url, &public())
                .unwrap();
            authority
                .with_body_delivery(|| {
                    assert!(matches!(
                        fixture.broker.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ));
                    Ok(())
                })
                .unwrap();
        });
        drop(issued);
        fixture
            .broker
            .lock()
            .unwrap()
            .settle_without_delivery(&fixture.ticket)
            .unwrap();
    }
    #[test]
    fn foreign_native_ticket_and_poisoned_owner_are_refused() {
        let fixture = Fixture::new(false, false);
        let foreign = Fixture::new(false, false);
        assert!(NativeHttpAuthorityFactory::from_native(
            Arc::clone(&fixture.broker),
            fixture.channel.clone(),
            Arc::clone(&fixture.ticket),
            fixture.request.clone(),
            foreign.quota.clone(),
            None
        )
        .is_err()); // Equal configured ceilings are NOT the same original root.
        assert!(NativeHttpAuthorityFactory::from_native(
            Arc::clone(&foreign.broker),
            fixture.channel.clone(),
            Arc::clone(&fixture.ticket),
            fixture.request.clone(),
            fixture.quota.clone(),
            None
        )
        .is_err());
        let issued = fixture.issue(true, None).map(|authority| {
            let mut authority = authority.unwrap();
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = fixture.broker.lock().unwrap();
                panic!("synthetic task-local authority poison");
            }));
            assert!(panic.is_err());
            assert!(authority
                .authorize(
                    HttpPhase::Preflight,
                    HttpMethod::Get,
                    &Url::parse("https://example.org/data").unwrap(),
                    &[]
                )
                .is_err());
        });
        drop(issued);
    }
    #[test]
    fn redirect_method_pin_and_current_epoch_are_rechecked() {
        let fixture = Fixture::new(false, false);
        let outcome = fixture.issue(true, None).map(|authority| {
            let mut authority = authority.unwrap();
            let url = Url::parse("https://example.org/data").unwrap();
            assert!(authority
                .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &public())
                .is_err());
            authority
                .authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])
                .unwrap();
            assert!(authority
                .authorize(
                    HttpPhase::Dispatch,
                    HttpMethod::Get,
                    &url,
                    &["93.184.216.34:444".parse().unwrap()]
                )
                .is_err());
            authority
                .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &public())
                .unwrap();
            assert!(authority
                .authorize(HttpPhase::Delivery, HttpMethod::Get, &url, &private())
                .is_err());
            let next = Url::parse("https://other.example.org/data").unwrap();
            authority
                .authorize(HttpPhase::Preflight, HttpMethod::Get, &next, &[])
                .unwrap();
            authority
                .authorize(HttpPhase::Dispatch, HttpMethod::Get, &next, &public())
                .unwrap();
            assert!(authority
                .authorize(HttpPhase::Preflight, HttpMethod::Post, &next, &[])
                .is_err());
            let _invalidation = fixture
                .broker
                .lock()
                .unwrap()
                .revoke(fixture.net.clone())
                .unwrap();
            assert!(authority
                .authorize(HttpPhase::Delivery, HttpMethod::Get, &next, &public())
                .is_err());
            assert!(authority.with_body_delivery(|| Ok(())).is_err());
        });
        drop(outcome);
        fixture
            .broker
            .lock()
            .unwrap()
            .settle_without_delivery(&fixture.ticket)
            .unwrap();
    }
    #[test]
    fn local_addresses_require_exact_combined_original_demand() {
        for (has_local, combined, expected) in [
            (false, false, false),
            (true, false, false),
            (true, true, true),
        ] {
            let fixture = Fixture::new(has_local, combined);
            let outcome = fixture.issue(true, None).map(|authority| {
                let mut authority = authority.unwrap();
                let url = Url::parse("https://example.org/data").unwrap();
                authority
                    .authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])
                    .unwrap();
                assert_eq!(
                    authority
                        .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &private())
                        .is_ok(),
                    expected
                );
            });
            drop(outcome);
            fixture
                .broker
                .lock()
                .unwrap()
                .settle_without_delivery(&fixture.ticket)
                .unwrap();
        }
    }
    struct TrapDns;
    impl DnsResolver for TrapDns {
        fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>> {
            panic!("DNS MUST NOT RUN after origin denial");
        }
    }
    #[test]
    fn actual_raw_source_transport_rejects_denied_origin_before_dns() {
        let fixture = Fixture::new(false, false);
        let outcome = fixture.issue(true, None).map(|authority| {
            let mut authority = authority.unwrap();
            let options = HttpOptions {
                url: "https://denied.invalid/data".into(),
                method: "GET".into(),
                headers: BTreeMap::new(),
                body: None,
                response: "bytes".into(),
                max_bytes: 16,
                timeout_ms: 100,
                credential: None,
            };
            assert!(authority
                .request_source_response(&options, &TrapDns, &AtomicBool::new(false))
                .is_err());
        });
        drop(outcome);
        fixture
            .broker
            .lock()
            .unwrap()
            .settle_without_delivery(&fixture.ticket)
            .unwrap();
    }
    struct RevokingCredentialFixture {
        broker: Arc<Mutex<PermissionBroker>>,
        right: Right,
    }
    impl HostCredentialAdapter for RevokingCredentialFixture {
        fn bound_headers(
            &mut self,
            principal: &PackageIdentity,
            handle: &str,
            origin: &str,
            quota: &QuotaGroup,
        ) -> Result<BoundCredentialHeaders> {
            // Synthetic admitted memory provider, NO actual secret/backend lookup.
            let mut broker = self
                .broker
                .try_lock()
                .expect("credential lookup cannot hold authority guard");
            let _invalidation = broker.revoke(self.right.clone()).unwrap();
            drop(broker);
            BoundCredentialHeaders::from_host_selection(
                principal,
                handle,
                origin,
                &BTreeMap::from([("Authorization".into(), "synthetic fixture".into())]),
                quota,
            )
        }
    }
    #[test]
    fn credential_lookup_runs_unlocked_and_revoke_withholds_headers() {
        let fixture = Fixture::new(false, false);
        let provider = RevokingCredentialFixture {
            broker: Arc::clone(&fixture.broker),
            right: fixture.net.clone(),
        };
        let outcome = fixture
            .issue(true, Some(Box::new(provider)))
            .map(|authority| {
                let mut authority = authority.unwrap();
                let url = Url::parse("https://example.org/data").unwrap();
                authority
                    .authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])
                    .unwrap();
                authority
                    .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &public())
                    .unwrap();
                assert!(authority
                    .credential_headers("selected_fixture", &url)
                    .is_err());
            });
        drop(outcome);
        fixture
            .broker
            .lock()
            .unwrap()
            .settle_without_delivery(&fixture.ticket)
            .unwrap();
    }
    #[test]
    fn native_credential_headers_cannot_replace_transport_or_expand_bounds() {
        let fixture = Fixture::new(false, false);
        let principal = fixture.broker.lock().unwrap().identity().clone();
        for headers in [
            BTreeMap::from([("Host".into(), "evil.example".into())]),
            BTreeMap::from([("Authorization".into(), "line\r\nbreak".into())]),
            BTreeMap::from([("Authorization".into(), "x".repeat(8193))]),
        ] {
            assert!(BoundCredentialHeaders::from_host_selection(
                &principal,
                "fixture",
                "https://example.org",
                &headers,
                &fixture.quota
            )
            .is_err());
        }
    }
    #[test]
    fn original_request_cancellation_prevents_later_hops_and_body_delivery() {
        let fixture = Fixture::new(false, false);
        let outcome = fixture.issue(true, None).map(|authority| {
            let mut authority = authority.unwrap();
            let url = Url::parse("https://example.org/data").unwrap();
            authority
                .authorize(HttpPhase::Preflight, HttpMethod::Get, &url, &[])
                .unwrap();
            authority
                .authorize(HttpPhase::Dispatch, HttpMethod::Get, &url, &public())
                .unwrap();
            fixture.request.stop_token().stop();
            assert!(authority
                .authorize(HttpPhase::Delivery, HttpMethod::Get, &url, &public())
                .is_err());
            assert!(authority.with_body_delivery(|| Ok(())).is_err());
        });
        drop(outcome);
        fixture
            .broker
            .lock()
            .unwrap()
            .settle_without_delivery(&fixture.ticket)
            .unwrap();
    }
}
