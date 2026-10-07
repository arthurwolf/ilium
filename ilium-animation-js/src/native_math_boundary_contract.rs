#![cfg(all(feature = "v8-runtime", feature = "native-host"))] // Exercise the real production binary engine and existing finite native CPU bank together.
use crate::{
    engine::{
        CompletionState, CreateState, Engine, EngineLimits, ServiceAuthority, TypedArrayKind,
    },
    native_math_bridge::NativeMathBridge,
    package::{Package, PackageLimits},
    permissions::{
        Activation, CallPhase, Ceiling, PackageIdentity, PermissionBroker, PermissionPlan,
    },
}; // No synthetic compute registry or permission ticket replaces the supplied native APIs.
use ilium_ambient::resources::AmbientResources; // Reuse the original composition adapter around one admitted execution client.
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, JobContext, JobCost, JobOutcome, JobPoll, Lane,
    LaneConfig, QuotaGroup,
}; // Use real receipts and bounded completion hints without Tokio.
use serde_json::json; // Fixture metadata is small; actual mathematical input/output crosses binary planes.
use sha2::{Digest, Sha256}; // Bind the native permission principal to the exact same fixture archive bytes.
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
}; // Serialize process-global V8 ownership and bound every synchronization point.
fn serial() -> MutexGuard<'static, ()> {
    crate::engine::inventory_contracts::fixture_lock().0
}
fn release_signal() -> &'static crate::engine::inventory_contracts::ReleaseSignal {
    crate::engine::inventory_contracts::release_signal()
}
fn wait_snapshot(quota: &QuotaGroup, expected: ilium_execution::QuotaSnapshot) {
    // Observe actual retirement without assuming that Drop or a completion wake proves physical exit.
    let deadline = Instant::now() + Duration::from_secs(3);
    let receiver = release_signal().1.lock().unwrap(); // Keep the same bounded three-second qualification deadline.
    loop {
        let observed = quota.snapshot();
        if observed == expected {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "original root release deadline: {observed:?}, expected {expected:?}"
        );
        receiver
            .recv_timeout(remaining)
            .expect("actual original-root release hint before deadline");
    } // Recheck only after actual release hints; never spin or repeatedly poll a Receipt.
} // Failure to reach the real retained-owner state is a failing test, never fabricated zero accounting.
fn quota() -> QuotaGroup {
    crate::engine::inventory_contracts::quota()
}
#[test]
fn boundary_math_storage_refusal_precedes_cpu_launch_without_partial_custody() {
    use crate::native_math::{parameters_from_wire, MathInput, MathRequest, NativeMath};
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let (execution, resources, _wake) = bank(&quota);
    let mut native = NativeMath::new(resources.clone(), quota.clone(), 1).unwrap();
    let input = MathInput::from_host(&[3., 4., 0.], quota.clone()).unwrap();
    let request = MathRequest::new(
        input.clone(),
        parameters_from_wire(
            "transform",
            &BTreeMap::from([("operation".into(), 1.), ("components".into(), 3.)]),
        )
        .unwrap(),
        1024,
        1000,
    )
    .unwrap();
    let original = quota.snapshot();
    // Synthetic quota pressure leaves one byte less than this real kernel's
    // 4096 scratch bytes plus its 12-byte output and 512-byte output metadata.
    // The pressure lease allocates no fabricated production payload.
    let pressure = quota
        .reserve_external_storage(
            original.limits.worker_bytes - original.worker_bytes - (4096 + 12 + 512 - 1),
        )
        .unwrap();
    let before = quota.snapshot();
    assert!(
        native.submit(request, 1).is_err(),
        "insufficient output storage must refuse before CPU launch"
    );
    assert_eq!(
        quota.snapshot(),
        before,
        "refusal must release partial scratch admission and publish no job"
    );
    drop(pressure);
    assert_eq!(quota.snapshot(), original);
    drop(input);
    drop(native);
    drop(resources);
    drop(execution);
    wait_snapshot(&quota, baseline);
}
fn bank(quota: &QuotaGroup) -> (Execution, AmbientResources, mpsc::Receiver<()>) {
    // Start one task-local finite CPU bank with the supplied native-math fixture sizes.
    let zero = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    }; // No service or I/O bank is invented for CPU math.
    let execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: LaneConfig {
                threads: 1,
                queue_slots: 4,
                priority: None,
                resident_bytes_per_thread: 1024 * 1024,
            },
            io: zero,
            service: zero,
        },
    )
    .unwrap(); // Actual worker admission precedes native work.
    let (sender, receiver) = mpsc::sync_channel(1); // One bounded nonblocking completion hint, not a second result channel.
    let client = execution
        .client(ClientLimits {
            jobs: 8,
            service_jobs: 0,
            input_bytes: 32 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
        })
        .unwrap()
        .with_completion_wake(move || {
            let _ = sender.try_send(());
        }); // Preserve original client limits and never block a bank callback on delivery.
    (execution, AmbientResources::new(client), receiver) // Return every actual bank/client owner for explicit lifetime assertions.
} // No Tokio dependency or timer-based receipt polling is used.
fn limits() -> EngineLimits {
    EngineLimits {
        pending_requests: 1,
        ..EngineLimits::default()
    }
} // Force descriptor ACK to release raw submit custody before the one permitted result request can be admitted.
struct Fixture {
    engine: Engine,
    broker: Arc<Mutex<PermissionBroker>>,
    activation: Activation,
    authority: ServiceAuthority,
    digest: String,
}
fn fixture(source: &str, quota: &QuotaGroup) -> Fixture {
    // Build one genuine package and consent-free native CPU activation.
    let manifest = json!({"api_version":1,"id":"math-boundary-contract","name":"Boundary","version":"1.0.0","entry":"entry.mjs","modes":["live"],"settings":{"type":"object","properties":{}},"files":[{"path":"entry.mjs","bytes":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes()))}]}); // Use the exact supplied Manifest schema; no capability, trust, or native identity is fabricated.
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new())); // Follow the exact stored-ZIP package fixture pattern from the supplied tests.
    for (name, bytes) in [
        ("entry.mjs", source.as_bytes().to_vec()),
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
    ] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
    } // Include the complete declared source and manifest.
    let archive = zip.finish().unwrap().into_inner(); // Both native identity and validated Package consume these exact bytes.
    let package = Arc::new(Package::from_bytes(&archive, PackageLimits::default()).unwrap()); // Exercise the actual package validator without inventing a trust verifier constructor.
    let identity = PackageIdentity::unverified(package.manifest().id.clone(), &archive).unwrap(); // This fixture is unsigned and gains no privileged capability.
    let mut broker = PermissionBroker::new(
        identity,
        Ceiling {
            permissions: vec![],
        },
        Ceiling {
            permissions: vec![],
        },
    )
    .unwrap(); // Native CPU math is baseline admitted work, not a fictional permission.
    let review = broker
        .prepare(
            1,
            1,
            PermissionPlan {
                permissions: vec![],
                demands: vec![],
            },
            BTreeMap::new(),
        )
        .unwrap(); // Use the actual native review/activation state machine.
    let activation = broker
        .resolve(review, BTreeMap::new())
        .unwrap()
        .activation
        .unwrap(); // No synthetic accepted channel is reconstructed from numeric fields.
    assert!(broker
        .dispatch(&activation.channel, CallPhase::Create, "compute", vec![])
        .is_err()); // Force the real broker's empty-OperationNeed refusal rather than claiming an empty grant authorizes CPU work.
    let authority = ServiceAuthority {
        instance_id: activation.plan.instance_id,
        plan_generation: activation.plan.plan_revision,
        authorization_epoch: activation.plan.authorization_epoch,
    }; // Bind only the actual native activation returned above.
    let digest = package.digest().to_owned(); // Capture the validated immutable package identity before moving its Arc.
    let mut engine = Engine::new(package, limits(), quota.clone()).unwrap(); // Keep the engine on the same original root as the real bank.
    engine.install_bootstrap(crate::TRUSTED_BOOTSTRAP).unwrap();
    engine.load().unwrap();
    engine.bind_service_authority(&digest, authority).unwrap(); // Use the complete production facade and explicit native activation.
    Fixture {
        engine,
        broker: Arc::new(Mutex::new(broker)),
        activation,
        authority,
        digest,
    } // The caller independently owns native activation and V8 execution.
} // No HTTP/provider, source-provenance or UI acceptance is represented by this fixture.
const SOURCE: &str = r#"export async function create(host) { // Invoke the actual production SDK compute facade.
    globalThis.original_input = new Float32Array([99,3,4,0,88]); // Sentinel neighbors must not cross the logical subview boundary.
    const pending = host.compute.submit({kernel:'transform',input:original_input.subarray(1,4),parameters:{operation:1,components:3},max_bytes:1024,timeout_ms:1000}); // Use the actual native normalization kernel and its original deadline.
    original_input.fill(77); // Native ingress must already own a copy before the package mutates its backing buffer.
    const submitted = await pending; if (!submitted.ok) throw Error(submitted.error.code); globalThis.job = submitted.value; // Retain only the genuine returned compute wrapper.
    const first = await job.result(); const again = await job.result(); if (!first.ok || !again.ok) throw Error('result_failed'); // Retrieval must reuse one terminal result rather than resubmit the job.
    globalThis.result = first.value; globalThis.same_result = first.value === again.value; globalThis.result_kind = first.value instanceof Float32Array; // The mathematical output remains a typed V8-owned buffer.
    return { render(){}, dispose(){} }; // No drawing, UI emission or package side effect is claimed by this fixture.
}"#; // End complete fixture module.
     // Only a genuine original private broker/channel may protect a bounded issue
     // or inert copy/ACK. Release this guard before every JS pump or worker wait.
fn with_current<T>(
    broker: &Arc<Mutex<PermissionBroker>>,
    activation: &Activation,
    operation: impl FnOnce(ServiceAuthority) -> crate::error::Result<T>,
) -> crate::error::Result<T> {
    let owner = broker.lock().map_err(|_| {
        crate::error::AnimationError::PermissionDenied("poisoned native owner".into())
    })?;
    owner
        .grant(&activation.channel, "_host_channel_check")
        .map_err(|error| {
            crate::error::AnimationError::PermissionDenied(format!("native channel: {error:?}"))
        })?;
    operation(ServiceAuthority {
        instance_id: activation.plan.instance_id,
        plan_generation: activation.plan.plan_revision,
        authorization_epoch: activation.plan.authorization_epoch,
    })
}
#[test]
fn boundary_real_math_uses_original_bank_and_keeps_typed_result_without_completion_checkpoint() {
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let (execution, resources, wake) = bank(&quota);
    let Fixture {
        mut engine,
        broker,
        activation,
        authority,
        digest,
    } = fixture(SOURCE, &quota);
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Pending
    );
    let requests = engine.take_requests().unwrap();
    assert_eq!(requests.len(), 1);
    let submitted = requests[0].clone();
    assert_eq!(submitted.method, "compute.submit");
    assert_eq!(submitted.payload.arrays()[0].kind, TypedArrayKind::F32);
    assert_eq!(
        submitted.payload.planes()["b0"],
        [3.0_f32, 4.0, 0.0]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect::<Vec<_>>()
    );
    let mut bridge =
        NativeMathBridge::new(resources.clone(), quota.clone(), &digest, authority).unwrap();
    let mut foreign =
        NativeMathBridge::new(resources.clone(), quota.clone(), &digest, authority).unwrap();
    let handle = with_current(&broker, &activation, |current| {
        bridge.submit(&submitted, current)
    })
    .unwrap();
    assert!(foreign.poll(&handle, authority).is_err());
    assert!(foreign.close_handle(&handle, authority).is_err());
    assert!(foreign.observe_retained_output(&handle, authority).is_err());
    assert!(bridge
        .observe_retained_output(&handle, authority)
        .unwrap()
        .is_none());
    assert!(bridge
        .observe_retained_output(
            &handle,
            ServiceAuthority {
                authorization_epoch: authority.authorization_epoch + 1,
                ..authority
            }
        )
        .is_err());
    let wire_id = with_current(&broker, &activation, |current| {
        let value = bridge.descriptor(&handle, current, &limits())?;
        let id = value.metadata()["value"]["id"].as_str().unwrap().to_owned();
        let actual_ack = engine.complete_service_request(submitted.id, current, value)?;
        assert_eq!(actual_ack, CompletionState::Delivered);
        bridge.acknowledge_submission(&handle, submitted.id, actual_ack, current)?;
        Ok(id)
    })
    .unwrap();
    assert!(engine.take_requests().unwrap().is_empty());
    assert_eq!(engine.service_usage().0, 1);
    drop(requests);
    drop(submitted);
    assert_eq!(engine.service_usage(), (0, 0));
    assert_eq!(engine.pump().unwrap(), CreateState::Pending);
    let results = engine.take_requests().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].method, "compute.result");
    assert_eq!(
        results[0].payload.metadata(),
        &json!({"id":wire_id,"kind":"compute"})
    );
    wake.recv_timeout(Duration::from_secs(3)).unwrap();
    let output = with_current(&broker, &activation, |current| {
        assert!(bridge.poll(&handle, current)?);
        let output = bridge.observe_retained_output(&handle, current)?;
        assert!(
            output.is_some(),
            "original native math terminal status: {}",
            bridge.descriptor(&handle, current, &limits())?.metadata()
        );
        Ok(output.unwrap())
    })
    .unwrap();
    assert_eq!(output.values(), &[0.6_f32, 0.8, 0.0]);
    assert_eq!(output.values().len(), 3);
    assert_eq!(output.algorithm(), "ilium-native-math-v1");
    assert!(output.request_digest().iter().any(|byte| *byte != 0));
    assert_eq!(
        with_current(&broker, &activation, |current| {
            let value = bridge.copy_result(
                &handle,
                &results[0],
                current,
                TypedArrayKind::F32,
                &limits(),
            )?;
            engine.complete_service_request(results[0].id, current, value)
        })
        .unwrap(),
        CompletionState::Delivered
    );
    assert!(engine.take_requests().unwrap().is_empty());
    assert!(engine.evaluate_json("result_kind").is_err());
    assert_eq!(engine.pump().unwrap(), CreateState::Ready);
    assert_eq!(
        engine
            .evaluate_json("[result_kind,same_result,Array.from(result)]")
            .unwrap(),
        json!([true, true, [0.6000000238418579, 0.800000011920929, 0]])
    );
    assert!(engine.take_requests().unwrap().is_empty());
    assert_eq!(quota.snapshot().jobs, baseline.jobs);
    drop(bridge);
    drop(foreign);
    assert_eq!(output.values(), &[0.6_f32, 0.8, 0.0]);
    engine.cancel();
    drop(engine);
    drop(results);
    drop(resources);
    drop(execution);
    let mut retained = baseline;
    retained.worker_bytes += 3 * 4 + 512;
    wait_snapshot(&quota, retained);
    drop(output);
    wait_snapshot(&quota, baseline);
}
#[test]
fn boundary_math_revoke_prevents_publication_but_retains_actual_terminal_cleanup() {
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let (execution, resources, wake) = bank(&quota);
    let Fixture {
        mut engine,
        broker,
        activation,
        authority,
        digest,
    } = fixture(SOURCE, &quota);
    assert_eq!(
        engine.start_create(&json!({}), &json!({})).unwrap(),
        CreateState::Pending
    );
    let request = engine.take_requests().unwrap().remove(0);
    let mut bridge =
        NativeMathBridge::new(resources.clone(), quota.clone(), &digest, authority).unwrap();
    let handle = with_current(&broker, &activation, |current| {
        bridge.submit(&request, current)
    })
    .unwrap();
    let invalidation = broker.lock().unwrap().retire(authority.instance_id);
    assert!(invalidation.instance_ids.contains(&authority.instance_id));
    let mut publications = 0;
    assert!(with_current(&broker, &activation, |current| {
        let value = bridge.descriptor(&handle, current, &limits())?;
        publications += 1;
        engine.complete_service_request(request.id, current, value)
    })
    .is_err());
    assert_eq!(publications, 0);
    bridge.revoke();
    assert!(bridge.observe_retained_output(&handle, authority).is_err());
    wake.recv_timeout(Duration::from_secs(3)).unwrap();
    bridge.collect_retirement_on_wake().unwrap();
    assert!(bridge.is_drained());
    bridge.collect_retirement_on_wake().unwrap();
    assert!(bridge.is_drained());
    assert!(bridge.descriptor(&handle, authority, &limits()).is_err());
    engine.cancel();
    drop(engine);
    drop(bridge);
    drop(request);
    drop(resources);
    drop(execution);
    wait_snapshot(&quota, baseline);
}
#[test] // The real execution Receipt retains both budget dimensions even when it is empty.
fn boundary_empty_receipt_retains_debit_after_typed_guarded_output_is_dropped() {
    // This oracle preserves the parent's corrected original job.rs lifetime semantics.
    let _serial = serial();
    let quota = quota();
    let baseline = quota.snapshot();
    let (execution, resources, wake) = bank(&quota); // No additional bank, unbounded wake or Tokio runtime is introduced.
    let input = crate::native_math::MathInput::from_host(&[3.0, 4.0, 0.0], quota.clone()).unwrap(); // Use an actual admitted native input.
    let request = crate::native_math::MathRequest::new(
        input,
        crate::native_math::parameters_from_wire(
            "transform",
            &BTreeMap::from([("operation".into(), 1.0), ("components".into(), 3.0)]),
        )
        .unwrap(),
        1024,
        1000,
    )
    .unwrap(); // Preserve real kernel and deadline validation.
    let original_root = quota.clone();
    let cost = JobCost {
        input_bytes: 3 * 4 + 1024 + 4096,
        result_bytes: 3 * 4 + 4096,
    }; // Match the supplied NativeMath transform job's exact cooperative cost.
    let mut receipt = resources
        .finite()
        .try_submit(Lane::Cpu, cost, move |context: JobContext| {
            crate::native_math::execute(request, &original_root, &context.stop_token())
        })
        .unwrap(); // Execute the actual native kernel on the original finite CPU lane.
    wake.recv_timeout(Duration::from_secs(3)).unwrap();
    let JobPoll::Ready(retained) = receipt.try_take() else {
        panic!("actual typed math completion required");
    }; // A completion hint is not itself a typed result or a delivery acknowledgement.
    let JobOutcome::Finished(Ok(output)) = retained.view() else {
        panic!("actual native guarded result required");
    };
    let output = Arc::clone(output); // Share only the already admitted immutable mathematical output.
    drop(retained);
    assert!(matches!(receipt.try_take(), JobPoll::Taken)); // The receipt is empty but still owns its original JobHold.
    let before_drop = quota.snapshot().worker_bytes;
    drop(output); // Dropping the physical output does not release the still-live receipt's input/result debit.
    assert_eq!(quota.snapshot().worker_bytes, before_drop - (3 * 4 + 512)); // Assert the exact real MathOutput admission, not the HTTP fixture's unrelated 64-KiB allocation.
    assert_eq!(quota.snapshot().jobs, baseline.jobs + 1);
    assert_eq!(
        quota.snapshot().input_bytes,
        baseline.input_bytes + cost.input_bytes
    );
    assert_eq!(
        quota.snapshot().result_bytes,
        baseline.result_bytes + cost.result_bytes
    ); // Empty-but-live receipt custody is the mandatory accounting oracle.
    drop(receipt);
    assert_eq!(quota.snapshot().jobs, baseline.jobs);
    assert_eq!(quota.snapshot().input_bytes, baseline.input_bytes);
    assert_eq!(quota.snapshot().result_bytes, baseline.result_bytes); // Only actual receipt destruction releases its independent debit.
    drop(resources);
    drop(execution);
    wait_snapshot(&quota, baseline); // Original clients and bank metadata remain charged until their actual owners drop.
} // This test never calls cancellation or pipe closure physical worker proof.
