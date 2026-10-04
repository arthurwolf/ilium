use super::*;
use sha2::Digest; // Enable hashing the exact native helper fixture archive. // Exercise actual packet construction, request custody, and helper retirement latches.
fn root() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 1,
        worker_bytes: 1024 * 1024,
    })
} // A small independent pure-protocol fixture does not initialize a V8 platform or raise production limits.
fn active() -> ServiceAuthority {
    ServiceAuthority {
        instance_id: 1,
        plan_generation: 2,
        authorization_epoch: 3,
    }
} // This native test stamp is separate from the immutable IPC envelope below.
fn authority() -> HelperAuthority {
    HelperAuthority {
        package_digest: "a".repeat(64),
        instance_id: 1,
        plan_generation: 1,
        authorization_epoch: 1,
    }
} // The launch stamp intentionally differs from the accepted active epoch.
fn request(quota: &QuotaGroup, budget: &Arc<ServiceBudget>, id: u64) -> HostRequest {
    // Construct only through the real admitted native ingress constructor.
    let arrays = [ArraySpec {
        name: "b0".into(),
        kind: TypedArrayKind::U8,
        elements: 3,
    }];
    let planes = BTreeMap::from([("b0".into(), vec![7, 8, 9])]); // Exactly one canonical declared plane has an exact logical byte length.
    let payload = ServiceValue::copy_request_from_host(
        &json!({"data":{"$ilium_binary":"b0"}}),
        &arrays,
        &planes,
        &EngineLimits::default(),
        quota.clone(),
        budget,
    )
    .unwrap(); // Original-root storage and aggregate request leases precede the retained copy.
    HostRequest::from_transport(
        id,
        "fixture.bytes".into(),
        1000,
        authority().package_digest,
        active(),
        ServicePhase::Create,
        payload,
    )
    .unwrap() // Native phase and active identity are not taken from metadata JSON.
} // End immutable request fixture.
fn session_without_process(quota: &QuotaGroup) -> (HelperSession, mpsc::Receiver<Option<Packet>>) {
    // Pure negative-path fixture: it contains no physical child and cannot establish retirement success.
    let (writer, writes) = mpsc::sync_channel(1);
    let (_responses, reader) = mpsc::sync_channel(1);
    let limits = HelperLimits::default(); // Keep the original one-command/one-response channel bounds.
    (
        HelperSession {
            authority: authority(),
            child: None,
            writer,
            reader,
            workers: Vec::new(),
            sequence: 0,
            requests: Vec::new(),
            status: None,
            pending: BTreeMap::new(),
            cancelled: VecDeque::new(),
            active: Some(active()),
            creation: Some(CreateState::Ready),
            service_budget: ServiceBudget::new(&limits.engine),
            last_request_id: 0,
            limits,
            quota: quota.clone(),
            _storage: Some(Arc::new(quota.reserve_external_storage(1024).unwrap())),
            _physical: None,
            closed: false,
            retirement_failed: false,
            physically_retired: false,
            native_publication: false,
            deferred_retirement: false,
            pending_seed: None,
        },
        writes,
    ) // No fake child, successful join or physical-retirement flag is supplied.
} // Real successful retirement remains an explicit actual-helper qualification.
#[test] // Queued immutable service planes must keep the same original allocation and admission.
fn boundary_queued_service_packet_shares_input_and_retains_original_root() {
    // Dropping producer/request owners is not permission to uncharge an escaped pipe packet.
    let quota = root();
    let budget = ServiceBudget::new(&EngineLimits::default());
    let request = request(&quota, &budget, 1);
    let pointer = request.payload.planes()["b0"].as_ptr();
    let retained = quota.snapshot().worker_bytes; // Observe genuine shared allocation identity and original-root storage.
    let mut packet = Packet::new(
        1,
        authority(),
        "response",
        json!({"requests":[RequestRecord::from(&request)]}),
        BTreeMap::new(),
    );
    packet.attach_service(request.payload.clone()).unwrap(); // Attach ServiceValue custody instead of deep-cloning a plane map.
    assert_eq!(packet.plane("b0").unwrap().as_ptr(), pointer);
    let (sender, receiver) = mpsc::sync_channel(1);
    assert!(sender.send(packet).is_ok());
    drop(request); // The actual queued packet becomes the remaining immutable payload owner.
    assert_eq!(quota.snapshot().worker_bytes, retained);
    let packet = receiver.recv().unwrap();
    assert_eq!(packet.plane("b0").unwrap(), [7, 8, 9]); // Queue custody outlives the original local request record.
    let mut bytes = Vec::new();
    write_packet(&mut bytes, &packet).unwrap();
    let decoded = read_packet(&mut bytes.as_slice()).unwrap(); // Use actual metadata/binary framing rather than a JSON-byte-array roundtrip.
    assert_eq!(decoded.envelope.authority, authority());
    assert_eq!(decoded.planes["b0"], [7, 8, 9]);
    assert_eq!(
        decoded.envelope.payload["requests"][0]["authority"]["authorization_epoch"],
        3
    ); // Immutable session epoch 1 remains distinct from native active epoch 3.
    drop(decoded);
    drop(packet);
    assert_eq!(quota.snapshot().worker_bytes, 0); // Only actual final immutable payload destruction releases its original-root admission.
} // Local fixture serialization buffers are not claimed to be native worker outcomes.
#[test] // The complete packet must be preflighted before either sequence advancement or pipe output.
fn boundary_whole_envelope_refusal_preserves_sequence_and_emits_no_prefix() {
    // This failure is independent of service-level individual payload admission.
    let quota = root();
    let (mut session, writes) = session_without_process(&quota); // No real pipe worker exists in this pure negative-path test.
    let result = session.command(Command::Plan {
        settings: json!({"large":"x".repeat(MAX_JSON)}),
        mode: AnimationMode::Live,
        environment: json!({}),
    }); // The metadata fits no full command envelope under the unchanged bound.
    assert!(result.is_err());
    assert_eq!(session.sequence, 0);
    assert!(matches!(writes.try_recv(), Err(mpsc::TryRecvError::Empty))); // A refused packet cannot consume correlation or place even a prefix on the outbound queue.
    assert!(!session.closed);
    session.retirement_failed = true;
    session._storage.take(); // Dispose only fixture scratch; no physical process was ever created or claimed to have exited.
} // The production owner still retires the transport after uncertainty following an actual committed send.
#[test] // Notification and logical closure must never erase a failed physical retirement.
fn boundary_failed_retirement_latches_across_cancel_dispose_and_retained_requests() {
    // This is a negative state-machine test, not simulated successful sandbox isolation.
    let quota = root();
    let (mut session, _writes) = session_without_process(&quota);
    let request = request(&quota, &session.service_budget, 1);
    session.pending.insert(1, request.clone());
    session.requests.push(request.clone()); // Retain an actual admitted request alongside the failed owner state.
    session.closed = true;
    session.retirement_failed = true;
    session.service_budget.close();
    request.stop_token().stop();
    let held = quota.snapshot().worker_bytes; // Model a previously recorded failure without inventing a child join result.
    assert!(session.cancel().is_err());
    assert!(session.dispose().is_err());
    assert!(session.cancel().is_err());
    assert!(!session.is_physically_retired()); // Repeated calls cannot promote failed prior joins, EOF or cancellation to physical success.
    assert_eq!(quota.snapshot().worker_bytes, held);
    assert!(request.is_cancelled());
    assert_eq!(request.payload.planes()["b0"], [7, 8, 9]); // Failure retains all actual original payload custody.
    session.pending.clear();
    session.requests.clear();
    session._storage.take();
    drop(session);
    assert!(quota.snapshot().worker_bytes > 0); // Only explicitly owned fixture scratch is removed; the escaped request still owns its real guard.
    drop(request);
    assert_eq!(quota.snapshot().worker_bytes, 0); // No actual native physical admission was constructed by this negative-only fixture.
} // The separate ignored helper test must establish actual positive process and worker retirement.
#[test] // Only create/pump are acquiring commands; result-copy ACK and draining stay native-only.
fn boundary_completion_cancel_and_drain_commands_are_nonacquiring() {
    // Prevent a future compatibility branch from reopening an implicit microtask boundary.
    for command in [
        Command::BindAuthority {
            authority: active().into(),
        },
        Command::DrainRequests,
        Command::CompleteService {
            id: 1,
            authority: active().into(),
            result: Value::Null,
            arrays: vec![],
        },
        Command::CancelService {
            id: 1,
            authority: active().into(),
        },
        Command::CompleteRequest {
            id: 1,
            result: Value::Null,
        },
    ] {
        assert!(!command.permits_acquisition());
    } // Every listed command must reject fresh guest service acquisition.
    assert!(Command::Pump.permits_acquisition());
    assert!(Command::StartCreate {
        settings: json!({}),
        accepted_plan: json!({})
    }
    .permits_acquisition()); // Preserve the two explicit authorized continuation entrypoints.
    for state in [
        CompletionState::Delivered,
        CompletionState::Unknown,
        CompletionState::TimedOut,
        CompletionState::Cancelled,
    ] {
        let record = completion_record(7, active(), Ok(state));
        assert_eq!(completed_state(record, 7, active()).unwrap(), state);
    } // Correlation preserves every distinct native terminal outcome.
    assert!(completed_state(
        completion_record(7, active(), Ok(CompletionState::Delivered)),
        8,
        active()
    )
    .is_err()); // A different request cannot consume an otherwise successful ACK.
    assert!(completed_state(
        completion_record(7, active(), Ok(CompletionState::Delivered)),
        7,
        ServiceAuthority {
            authorization_epoch: 4,
            ..active()
        }
    )
    .is_err()); // A different active epoch cannot consume the ACK either.
} // Actual no-checkpoint behavior is also forced through the real engine and actual helper qualification.
#[test] // Two individually legal 48-plane requests must be paged rather than flattened into one illegal packet.
fn boundary_actual_engine_requests_page_without_plane_collision_or_checkpoint() {
    // Use the real engine owner with a pure in-memory serialization sink.
    let (_serial, quota) = crate::engine::boundary_tests::fixture_lock(); // Share the same unit-test platform/root as all private engine tests.
    let mut engine=crate::engine::boundary_tests::engine("export async function create(){await Promise.all([__ilium_dispatch('fixture.one',Array.from({length:48},(_,i)=>new Uint8Array([i]))),__ilium_dispatch('fixture.two',Array.from({length:48},(_,i)=>new Uint8Array([255-i])))]);return {render(){},dispose(){}}}",crate::engine::boundary_tests::SIMPLE,EngineLimits::default(),quota); // Actual native traversal owns each immutable plane before paging.
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Pending
    );
    let mut services = ChildServices::default();
    services.collect(&mut engine).unwrap(); // Move only the real admitted engine requests into the helper's bounded queue.
    let (first, first_guard) = services
        .page(&mut engine, &authority(), active(), 1)
        .unwrap();
    let (second, second_guard) = services
        .page(&mut engine, &authority(), active(), 2)
        .unwrap(); // Each one-event page may independently use all 48 canonical names.
    assert_eq!(first.envelope.planes.len(), 48);
    assert_eq!(second.envelope.planes.len(), 48);
    assert_eq!(first.plane("b0").unwrap(), [0]);
    assert_eq!(second.plane("b0").unwrap(), [255]); // Equal per-request names must never overwrite a peer's plane bytes.
    assert_eq!(first.envelope.payload["more"], true);
    assert_eq!(second.envelope.payload["more"], false);
    assert_eq!(
        first.envelope.payload["requests"].as_array().unwrap().len(),
        1
    ); // Full-envelope pagination is bounded and makes progress.
    let mut bytes = Vec::new();
    write_packet(&mut bytes, &first).unwrap();
    let decoded = read_packet(&mut bytes.as_slice()).unwrap();
    assert_eq!(decoded.planes["b47"], [47]); // Actual wire serialization preserves the last declared plane too.
    assert!(engine.take_requests().unwrap().is_empty());
    assert!(services.queued.is_empty());
    assert_eq!(services.issued.len(), 2); // Paging cannot execute package continuations or acquire additional requests.
    engine.cancel();
    drop(decoded);
    drop(first);
    drop(second);
    drop(first_guard);
    drop(second_guard); // Retain guards until all actual page writes and native cancellation decisions are complete.
} // This test is a real codec/engine check, not evidence of physical pipe or sandbox behavior.
#[test]
// Requires actual V8 process isolation and original owned-worker shutdown, never a mock success flag.
#[ignore = "requires ILIUM_ANIMATION_HELPER and delegated Linux sandbox; run explicitly for native qualification"] // An ordinary component run cannot establish physical process retirement or terminal UI emission.
fn boundary_actual_helper_retirement_preserves_native_activation_until_actual_emission_recheck() {
    use crate::{
        manifest::AnimationMode,
        permissions::Ceiling,
        runtime::{InstancePreparation, PackageInstance},
        trust::TrustVerifier,
    };
    let bytes = super::isolation_qualification::archive(
        "export function plan(){return {format:'gray32',fps:30,inputs:{}}} export async function create(){if(fixture_seed_calls!==1)throw Error('missing_fixture_seed_activation');return {render(c,f){f.gray.fill(0.25);f.present()},dispose(){}}}",
    );
    // Zero inputs and this legacy renderer consume no host payload. Validate
    // the real host-only seed ABI; do not substitute frame/provider data.
    let trusted_bootstrap = format!(
        "{}\n{}",
        super::isolation_qualification::BOOTSTRAP,
        r#"
        globalThis.fixture_seed_calls = 0;
        globalThis.__ilium_seed_frame = (metadata, planes) => {
            if (metadata.frame !== null || !metadata.host ||
                !Array.isArray(metadata.host.permissions) || metadata.host.permissions.length !== 0 ||
                metadata.host.cancelled !== false || metadata.host.package.id !== 'helper-qualification' ||
                !Number.isSafeInteger(metadata.host.selection.generation) || metadata.host.selection.generation <= 0 ||
                !Number.isSafeInteger(metadata.host.selection.authorization_epoch) || metadata.host.selection.authorization_epoch <= 0 ||
                Reflect.ownKeys(planes).length !== 0) throw Error('unexpected_fixture_host_seed');
            if (++fixture_seed_calls !== 1) throw Error('repeated_fixture_seed_activation');
        };
    "#
    );
    let quota = super::isolation_qualification::quota();
    let executable =
        std::env::var("ILIUM_ANIMATION_HELPER").expect("actual built helper path required");
    let verifier = TrustVerifier::from_release_inventory(vec![]).unwrap();
    let settings = json!({});
    let environment = json!({});
    let verified = PackageInstance::verify(InstancePreparation {
        archive: &bytes,
        verifier: &verifier,
        helper_executable: Path::new(&executable),
        trusted_bootstrap: &trusted_bootstrap,
        settings: &settings,
        mode: AnimationMode::Live,
        environment: &environment,
        host_policy: Ceiling {
            permissions: vec![],
        },
        instance_id: 73,
        limits: HelperLimits::default(),
        quota: quota.clone(),
    })
    .unwrap();
    let (mut instance, initial_review) = verified.prepare_without_rights().unwrap();
    drop(initial_review);
    // The genuine native review consumes a newer revision. Immutable helper
    // launch coordinates remain revision one; no guest stamp is substituted.
    let review = instance.review_selected(73, 3, BTreeMap::new()).unwrap();
    let pending = instance.begin_resolution(review, BTreeMap::new()).unwrap();
    let resolution = instance.finish_resolution(pending).unwrap();
    assert_eq!(resolution.accepted_creation(), Some(CreateState::Ready));
    let expected = instance.frame_authority().unwrap();
    assert_eq!(expected.plan_generation, 3);
    let frame = instance
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 4,
            }],
        )
        .unwrap();
    instance.accept_frame(true).unwrap();
    let mut emitted = 0;
    assert!(instance
        .with_playback_authority(&expected, || {
            emitted += 1;
        })
        .is_err());
    assert_eq!(emitted, 0);
    instance.retire_helper().unwrap();
    assert!(instance.is_physically_retired());
    assert_eq!(instance.frame_authority(), Some(expected.clone()));
    instance
        .with_playback_authority(&expected, || {
            assert_eq!(
                frame.output.planes["gray"],
                0.25_f32.to_ne_bytes().repeat(4)
            );
            emitted += 1;
        })
        .unwrap();
    assert_eq!(emitted, 1);
    let invalidation = instance.revoke_activation().unwrap().unwrap();
    assert!(invalidation.instance_ids.contains(&expected.instance_id));
    assert!(instance
        .with_playback_authority(&expected, || {
            emitted += 1;
        })
        .is_err());
    assert_eq!(emitted, 1);
    drop(resolution);
    drop(instance);
    assert_eq!(quota.snapshot().worker_threads, 0);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(frame);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
#[test]
// Force binary request/result copying and the no-checkpoint ACK rule through an actual sandbox helper.
#[ignore = "requires ILIUM_ANIMATION_HELPER and delegated Linux sandbox; run explicitly for native qualification"] // Do not replace native process admission with a facade fixture or remove this prerequisite.
fn boundary_actual_helper_copy_ack_never_runs_the_next_acquiring_continuation() {
    // A premature checkpoint would leave an undrained child request before the next pump.
    let source="export async function create(){const first=await __ilium_dispatch('fixture.first',{data:new Uint8Array([7,8,9])});if(!(first instanceof Uint8Array))throw Error('binary_result_kind');await __ilium_dispatch('fixture.after_ack',{data:first});return {render(c,f){f.gray.fill(0.25);f.present()},dispose(){}}}"; // A second actual native request can arise only from the first completion's continuation.
    let bytes = super::isolation_qualification::archive(source);
    let quota = super::isolation_qualification::quota();
    let executable =
        std::env::var("ILIUM_ANIMATION_HELPER").expect("actual helper executable required"); // Preserve real helper fixture limits and native launch requirements.
    let identity = HelperAuthority {
        package_digest: format!("{:x}", sha2::Sha256::digest(&bytes)),
        instance_id: 73,
        plan_generation: 3,
        authorization_epoch: 9,
    };
    let stamp = ServiceAuthority {
        instance_id: 73,
        plan_generation: 3,
        authorization_epoch: 9,
    }; // These are native engine-only fixture coordinates, not broker grants.
    let mut helper = HelperSession::launch(
        Path::new(&executable),
        &bytes,
        super::isolation_qualification::BOOTSTRAP,
        identity,
        HelperLimits::default(),
        quota.clone(),
    )
    .unwrap();
    helper.bind_service_authority(stamp).unwrap(); // Bind before the actual helper executes an acquiring phase.
    assert_eq!(
        helper.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Pending
    );
    let first = helper.take_requests();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].method, "fixture.first");
    assert_eq!(first[0].payload.planes()["b0"], [7, 8, 9]); // Actual request planes cross native V8 traversal, child framing, pipe I/O and parent admitted ingress.
    let result = ServiceValue::copy_from_host(
        &json!({"$ilium_binary":"b0"}),
        first[0].payload.arrays(),
        first[0].payload.planes(),
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap(); // Build a separately admitted native typed result from immutable borrowed input.
    assert_eq!(
        helper
            .complete_service_request(first[0].id, stamp, result)
            .unwrap(),
        CompletionState::Delivered
    );
    assert!(helper.take_requests().is_empty()); // The complete copy/resolve ACK must not perform a microtask checkpoint or diagnostic hook.
    assert_eq!(helper.pump().unwrap(), CreateState::Pending);
    let second = helper.take_requests();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].method, "fixture.after_ack");
    assert_eq!(second[0].payload.planes()["b0"], [7, 8, 9]); // Only the next explicit pump may acquire and page the continuation's new request.
    let result = ServiceValue::copy_from_host(
        &Value::Null,
        &[],
        &BTreeMap::new(),
        &EngineLimits::default(),
        quota.clone(),
    )
    .unwrap();
    assert_eq!(
        helper
            .complete_service_request(second[0].id, stamp, result)
            .unwrap(),
        CompletionState::Delivered
    ); // Terminal second result uses the same native no-checkpoint boundary.
    assert_eq!(helper.pump().unwrap(), CreateState::Ready);
    let output = helper
        .render(
            &json!({}),
            &[ArraySpec {
                name: "gray".into(),
                kind: TypedArrayKind::F32,
                elements: 4,
            }],
        )
        .unwrap();
    assert_eq!(output.planes["gray"], 0.25_f32.to_ne_bytes().repeat(4));
    helper.accept_frame(true).unwrap(); // Synchronous rendering remains usable after explicitly authorized service continuation pumping.
    helper.dispose().unwrap();
    assert!(helper.is_physically_retired());
    drop(output);
    drop(helper);
    assert!(quota.snapshot().worker_bytes > 0);
    drop(first);
    drop(second);
    assert_eq!(quota.snapshot().worker_bytes, 0); // Escaped parent request aliases retain their own admission after the actual child and pipe workers retire.
} // This is actual binary/helper qualification, not HTTP/provider/UI success or a fabricated service registry.
