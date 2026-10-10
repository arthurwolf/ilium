#![cfg(feature = "native-host")]
use ilium_ambient::{resources::AmbientResources, voxel_landscape::noise};
use ilium_animation_js::native_math::{
    cross3, execute, identity4, inverse4, multiply4, parameters_from_wire, transform4,
    ComputeStatus, FixedSteps, KernelParameters, MathInput, MathRequest, NativeMath, OutputLayout,
    SeededRandom,
};
use ilium_execution::{
    ClientLimits, Execution, ExecutionConfig, JobContext, JobCost, Lane, LaneConfig, QuotaGroup,
    QuotaLimits,
};
use ilium_platform::owned_worker::StopToken;
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc},
    time::Duration,
};
fn quota() -> QuotaGroup {
    QuotaGroup::new(QuotaLimits {
        clients: 8,
        jobs: 8,
        service_jobs: 0,
        input_bytes: 32 * 1024 * 1024,
        result_bytes: 16 * 1024 * 1024,
        worker_threads: 4,
        worker_bytes: 128 * 1024 * 1024,
    })
}
fn request(input: &[f32], parameters: KernelParameters, quota: &QuotaGroup) -> MathRequest {
    MathRequest::new(
        MathInput::from_host(input, quota.clone()).unwrap(),
        parameters,
        4 * 1024 * 1024,
        1000,
    )
    .unwrap()
}
fn transform(operation: u8, components: u8) -> KernelParameters {
    KernelParameters::Transform {
        operation,
        components,
        matrix: identity4(),
        project: false,
    }
}
#[test]
fn noise_reuses_the_actual_native_algorithm_and_content_provenance() {
    let quota = quota();
    let baseline = quota.snapshot().worker_bytes;
    let params = KernelParameters::Noise {
        seed: 7,
        period: 64,
        octaves: 4,
    };
    let a = execute(
        request(&[-8., 24., 0., 0.], params.clone(), &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    let b = execute(
        request(&[-8., 24., 0., 0.], params, &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(
        a.values(),
        &[
            noise::fbm2(7, -8, 24, 64, 4) as f32,
            noise::fbm2(7, 0, 0, 64, 4) as f32
        ]
    );
    assert_eq!(a.values(), b.values());
    assert_eq!(a.request_digest(), b.request_digest());
    let c = execute(
        request(
            &[-8., 24., 0., 0.],
            KernelParameters::Noise {
                seed: 8,
                period: 64,
                octaves: 4,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_ne!(a.request_digest(), c.request_digest());
    assert_ne!(a.values(), c.values());
    assert_eq!(a.algorithm(), "ilium-native-math-v1");
    assert_eq!(a.layout(), OutputLayout::Scalars);
    let retained = Arc::clone(&a);
    drop(a);
    drop(b);
    drop(c);
    assert!(quota.snapshot().worker_bytes > baseline);
    drop(retained);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn matrix_vector_batches_obey_row_major_projection_and_singular_refusal() {
    let quota = quota();
    let mut matrix = identity4();
    matrix[3] = 2.;
    matrix[7] = -3.;
    matrix[11] = 4.;
    matrix[15] = 2.;
    assert_eq!(
        transform4(&matrix, &[1., 2., 3., 1.]).unwrap(),
        [3., -1., 7., 2.]
    );
    let output = execute(
        request(
            &[1., 2., 3.],
            KernelParameters::Transform {
                operation: 0,
                components: 3,
                matrix,
                project: true,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(output.values(), &[1.5, -0.5, 3.5]);
    let inverse = inverse4(&matrix).unwrap();
    let product = multiply4(&matrix, &inverse).unwrap();
    for (actual, expected) in product.iter().zip(identity4()) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert!(inverse4(&[0.; 16]).is_err());
    let mut zero_w = identity4();
    zero_w[15] = 0.;
    assert!(execute(
        request(
            &[1., 2., 3.],
            KernelParameters::Transform {
                operation: 0,
                components: 3,
                matrix: zero_w,
                project: true
            },
            &quota
        ),
        &quota,
        &StopToken::default()
    )
    .is_err());
    let normalized = execute(
        request(&[3., 4., 0.], transform(1, 3), &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(normalized.values(), &[0.6, 0.8, 0.]);
    assert_eq!(cross3([1., 0., 0.], [0., 1., 0.]).unwrap(), [0., 0., 1.]);
    let crossed = execute(
        request(&[1., 0., 0., 0., 1., 0.], transform(2, 3), &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(crossed.values(), &[0., 0., 1.]);
    let dot = execute(
        request(&[1., 2., 3., 4., 5., 6.], transform(3, 3), &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(dot.values(), &[32.]);
    let matrices: Vec<f32> = matrix
        .iter()
        .chain(inverse.iter())
        .map(|value| *value as f32)
        .collect();
    let multiplied = execute(
        request(&matrices, transform(4, 4), &quota),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    for (actual, expected) in multiplied.values().iter().zip(identity4()) {
        assert!((f64::from(*actual) - expected).abs() < 1e-6);
    }
    let inverted = execute(
        request(
            &identity4().map(|value| value as f32),
            transform(5, 4),
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(inverted.values(), &identity4().map(|value| value as f32));
}
#[test]
fn native_fft_forward_inverse_windows_and_shape_checks_are_real() {
    let quota = quota();
    let forward = execute(
        request(
            &[1., 0., 0., 0.],
            KernelParameters::Fft {
                complex: false,
                inverse: false,
                window: 0,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(forward.values(), &[1., 0., 1., 0., 1., 0., 1., 0.]);
    let inverse = execute(
        request(
            forward.values(),
            KernelParameters::Fft {
                complex: true,
                inverse: true,
                window: 0,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(inverse.values(), &[1., 0., 0., 0., 0., 0., 0., 0.]);
    let hann = execute(
        request(
            &[1.; 8],
            KernelParameters::Fft {
                complex: false,
                inverse: false,
                window: 1,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert!((hann.values()[0] - 4.).abs() < 1e-5);
    assert!(hann.values().iter().all(|value| value.is_finite()));
    let blackman = execute(
        request(
            &[1.; 8],
            KernelParameters::Fft {
                complex: false,
                inverse: false,
                window: 2,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert!((blackman.values()[0] - 3.36).abs() < 1e-5);
    let invalid = MathInput::from_host(&[1.; 3], quota.clone()).unwrap();
    assert!(MathRequest::new(
        invalid,
        KernelParameters::Fft {
            complex: false,
            inverse: false,
            window: 0
        },
        1024,
        1000
    )
    .is_err());
}
#[test]
fn mesh_uses_native_camera_depth_and_returns_no_fake_source_receipt() {
    let quota = quota();
    let vertices = [
        0., 0., 0.25, 4., 0., 0.25, 0., 4., 0.25, 0., 0., 0.75, 4., 0., 0.75, 0., 4., 0.75,
    ];
    let output = execute(
        request(
            &vertices,
            KernelParameters::Mesh {
                width: 4,
                height: 4,
            },
            &quota,
        ),
        &quota,
        &StopToken::default(),
    )
    .unwrap();
    assert_eq!(
        output.layout(),
        OutputLayout::Mesh {
            width: 4,
            height: 4
        }
    );
    assert_eq!(output.values().len(), 32);
    assert_eq!(&output.values()[..2], &[0.75, 1.]);
    assert!(output.values().chunks_exact(2).any(|pair| pair == [0., 0.]));
    assert!(output.values().iter().all(|value| value.is_finite()));
    let work: Vec<f32> = (0..2048)
        .flat_map(|_| [0., 0., 1., 1024., 0., 1., 0., 1024., 1.])
        .collect();
    let input = MathInput::from_host(&work, quota.clone()).unwrap();
    assert!(MathRequest::new(
        input,
        KernelParameters::Mesh {
            width: 1024,
            height: 1024
        },
        4 * 1024 * 1024,
        1000
    )
    .is_err());
}
#[test]
fn malformed_input_unknown_parameters_foreign_quota_and_cancel_fail_before_publication() {
    let quota = quota();
    let baseline = quota.snapshot().worker_bytes;
    assert!(MathInput::from_host(&[f32::NAN], quota.clone()).is_err());
    assert!(MathInput::from_host(&vec![0.; 262145], quota.clone()).is_err());
    assert!(parameters_from_wire("arbitrary_code", &BTreeMap::new()).is_err());
    assert!(parameters_from_wire("fft", &BTreeMap::from([("execute".into(), 1.)])).is_err());
    assert!(parameters_from_wire("noise", &BTreeMap::from([("octaves".into(), 9.)])).is_err());
    assert!(parameters_from_wire("transform", &BTreeMap::from([("m99".into(), 1.)])).is_err());
    assert!(
        parameters_from_wire("mesh", &BTreeMap::from([("width".into(), f64::INFINITY)])).is_err()
    );
    let noise = parameters_from_wire("noise", &BTreeMap::new()).unwrap();
    let input = MathInput::from_host(&[0., 0.], quota.clone()).unwrap();
    assert!(MathRequest::new(input.clone(), noise.clone(), 1, 1000).is_err());
    assert!(MathRequest::new(input.clone(), noise.clone(), 1024, 0).is_err());
    let stop = StopToken::default();
    stop.stop();
    assert!(execute(
        MathRequest::new(input.clone(), noise.clone(), 1024, 1000).unwrap(),
        &quota,
        &stop
    )
    .is_err());
    let foreign = QuotaGroup::new(quota.snapshot().limits);
    assert!(execute(
        MathRequest::new(input.clone(), noise, 1024, 1000).unwrap(),
        &foreign,
        &StopToken::default()
    )
    .is_err());
    assert!(execute(
        request(&[0., 0., 0.], transform(1, 3), &quota),
        &quota,
        &StopToken::default()
    )
    .is_err());
    drop(input);
    assert_eq!(quota.snapshot().worker_bytes, baseline);
}
#[test]
fn seeded_random_reset_and_fixed_steps_preserve_deterministic_history() {
    let mut random = SeededRandom::new(7);
    let sequence: Vec<_> = (0..16).map(|_| random.next_unit()).collect();
    assert!(sequence.iter().all(|value| *value >= 0. && *value < 1.));
    random.reset();
    assert_eq!(
        sequence,
        (0..16).map(|_| random.next_unit()).collect::<Vec<_>>()
    );
    for _ in 0..256 {
        assert!((-3..=7).contains(&random.integer(-3, 7).unwrap()));
    }
    assert_eq!(random.integer(4, 4).unwrap(), 4);
    assert!(random.integer(2, 1).is_err());
    let mut steps = FixedSteps::new(0.1, 2., 2).unwrap();
    let first = steps.advance(0.5).unwrap();
    assert_eq!(first.steps, 2);
    assert!((first.pending_seconds - 0.3).abs() < 1e-12);
    let next = steps.advance(0.).unwrap();
    assert_eq!(next.steps, 2);
    let last = steps.advance(0.).unwrap();
    assert_eq!(last.steps, 1);
    assert!(last.pending_seconds < 1e-12);
    assert!(steps.advance(3.).is_err());
    assert_eq!(steps.advance(0.).unwrap().steps, 0);
    assert!(steps.advance(f64::NAN).is_err());
    assert!(FixedSteps::new(0., 1., 1).is_err());
    let mut exact = FixedSteps::new(0.1, 1., 10).unwrap();
    assert_eq!(exact.advance(0.3).unwrap().steps, 3);
}
fn resources(quota: &QuotaGroup) -> (Execution, AmbientResources, mpsc::Receiver<()>) {
    let zero = LaneConfig {
        threads: 0,
        queue_slots: 0,
        priority: None,
        resident_bytes_per_thread: 0,
    };
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
    .unwrap();
    let (sender, receiver) = mpsc::sync_channel(4);
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
        });
    (execution, AmbientResources::new(client), receiver)
}
#[test]
fn actual_shared_bank_handles_are_instance_bound_and_retained_results_outlive_service() {
    let quota = quota();
    let (execution, resources, wake) = resources(&quota);
    let before = quota.snapshot().worker_bytes;
    let mut service = NativeMath::new(resources.clone(), quota.clone(), 1).unwrap();
    let mut foreign = NativeMath::new(resources.clone(), quota.clone(), 1).unwrap();
    let input = MathInput::from_host(&[0., 0., 1., 1.], quota.clone()).unwrap();
    let handle = service
        .submit(
            MathRequest::new(
                input,
                KernelParameters::Noise {
                    seed: 1,
                    period: 64,
                    octaves: 4,
                },
                1024,
                1000,
            )
            .unwrap(),
            1,
        )
        .unwrap();
    assert!(foreign.poll(&handle, 1).is_err());
    assert!(foreign.cancel(&handle).is_err());
    // Completion hint, not sleep or repeated receipt polling.
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    let ComputeStatus::Ready(output) = service.poll(&handle, 1).unwrap() else {
        panic!("real math completion");
    };
    assert!(service.poll(&handle, 1).is_err());
    assert_eq!(output.values().len(), 2);
    drop(service);
    drop(foreign);
    assert!(quota.snapshot().worker_bytes > before);
    let copy = Arc::clone(&output);
    drop(output);
    assert!(quota.snapshot().worker_bytes > before);
    drop(copy);
    assert_eq!(quota.snapshot().worker_bytes, before);
    assert_eq!(quota.snapshot().jobs, 0);
    drop(resources);
    drop(execution);
}
#[test]
fn cancellation_retains_queued_job_until_its_actual_terminal_event() {
    let quota = quota();
    let (execution, resources, wake) = resources(&quota);
    let blocker = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
        })
        .unwrap();
    let (started_sender, started) = mpsc::sync_channel(1);
    let (release_sender, release) = mpsc::sync_channel(1);
    // Task-local bounded synchronization fixture deliberately occupies the one
    // CPU worker, proving cancel is a request rather than early job retirement.
    let blocker_receipt = blocker
        .try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: 256,
                result_bytes: 256,
            },
            move |_: JobContext| -> Result<(), ()> {
                started_sender.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            },
        )
        .unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut service = NativeMath::new(resources.clone(), quota.clone(), 1).unwrap();
    let handle = service
        .submit(
            request(
                &[0., 0.],
                KernelParameters::Noise {
                    seed: 1,
                    period: 64,
                    octaves: 4,
                },
                &quota,
            ),
            1,
        )
        .unwrap();
    service.cancel(&handle).unwrap();
    assert!(matches!(
        service.poll(&handle, 1).unwrap(),
        ComputeStatus::Cancelling
    ));
    assert!(quota.snapshot().jobs >= 2);
    release_sender.send(()).unwrap();
    drop(blocker_receipt);
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        service.poll(&handle, 1).unwrap(),
        ComputeStatus::Cancelled
    ));
    assert!(service.poll(&handle, 1).is_err());
    assert_eq!(quota.snapshot().jobs, 0);
    drop(service);
    drop(blocker);
    drop(resources);
    drop(execution);
}
#[test]
fn stale_epoch_withholds_completed_payload_and_queue_delay_counts_toward_deadline() {
    let quota = quota();
    let (execution, resources, wake) = resources(&quota);
    let mut service = NativeMath::new(resources.clone(), quota.clone(), 1).unwrap();
    let handle = service
        .submit(
            request(
                &[0., 0.],
                KernelParameters::Noise {
                    seed: 1,
                    period: 64,
                    octaves: 4,
                },
                &quota,
            ),
            1,
        )
        .unwrap();
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        service.poll(&handle, 2).unwrap(),
        ComputeStatus::Cancelled
    ));
    assert!(service
        .submit(
            request(
                &[0., 0.],
                KernelParameters::Noise {
                    seed: 1,
                    period: 64,
                    octaves: 4
                },
                &quota
            ),
            1
        )
        .is_err());
    drop(service);
    let mut service = NativeMath::new(resources.clone(), quota.clone(), 2).unwrap();
    let blocker = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 1024,
            result_bytes: 1024,
        })
        .unwrap();
    let (started_sender, started) = mpsc::sync_channel(1);
    let (release_sender, release) = mpsc::sync_channel(1);
    let blocker_receipt = blocker
        .try_submit(
            Lane::Cpu,
            JobCost {
                input_bytes: 256,
                result_bytes: 256,
            },
            move |_: ilium_execution::JobContext| -> Result<(), ()> {
                started_sender.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            },
        )
        .unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let expired = MathRequest::new(
        MathInput::from_host(&[0., 0.], quota.clone()).unwrap(),
        KernelParameters::Noise {
            seed: 1,
            period: 64,
            octaves: 4,
        },
        1024,
        100,
    )
    .unwrap();
    let handle = service.submit(expired, 2).unwrap();
    // Keep the real CPU worker occupied past this accepted request's deadline.
    std::thread::sleep(Duration::from_millis(120));
    release_sender.send(()).unwrap();
    drop(blocker_receipt);
    wake.recv_timeout(Duration::from_secs(2)).unwrap();
    let ComputeStatus::Failed(message) = service.poll(&handle, 2).unwrap() else {
        panic!("expired job must fail");
    };
    assert!(message.contains("deadline"));
    service.close();
    assert!(service
        .submit(
            request(
                &[0., 0.],
                KernelParameters::Noise {
                    seed: 1,
                    period: 64,
                    octaves: 4
                },
                &quota
            ),
            1
        )
        .is_err());
    drop(service);
    drop(blocker);
    drop(resources);
    drop(execution);
}
