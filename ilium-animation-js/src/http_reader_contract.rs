//! Pure/native task-local fixtures; no sockets, DNS acquisition or user state.
use super::*;
use std::{io::Cursor, sync::Arc};

struct Authority {
    revoked: Arc<AtomicBool>,
    checks: usize,
}
impl HttpAuthority for Authority {
    fn authorize(
        &mut self,
        phase: HttpPhase,
        _: HttpMethod,
        _: &Url,
        _: &[SocketAddr],
    ) -> Result<()> {
        assert_eq!(phase, HttpPhase::Delivery);
        self.checks += 1;
        if self.revoked.load(Ordering::Acquire) {
            return Err(AnimationError::PermissionDenied(
                "native grant revoked".into(),
            ));
        }
        Ok(())
    }
    fn credential_headers(&mut self, _: &str, _: &Url) -> Result<BTreeMap<String, String>> {
        panic!("reader cannot acquire credentials")
    }
}
fn head() -> HttpResponseHead {
    HttpResponseHead {
        status: 200,
        headers: BTreeMap::new(),
        final_url: "https://example.org/fixture".into(),
    }
}
fn consume<T>(
    raw: &mut dyn Read,
    authority: &mut Authority,
    stop: &AtomicBool,
    bound: usize,
    consumer: impl FnOnce(HttpResponseHead, &mut HttpBodyReader<'_>) -> Result<T>,
) -> Result<T> {
    let url = Url::parse("https://example.org/fixture").unwrap();
    let addresses = ["1.1.1.1:443".parse().unwrap()];
    consume_response(
        head(),
        raw,
        ReadContext {
            authority,
            method: HttpMethod::Get,
            url: &url,
            addresses: &addresses,
            stop,
            deadline: Instant::now() + Duration::from_secs(2),
            max_bytes: bound,
        },
        consumer,
    )
}
fn authority() -> Authority {
    Authority {
        revoked: Arc::new(AtomicBool::new(false)),
        checks: 0,
    }
}
struct MutatingReader<'a> {
    bytes: Cursor<&'a [u8]>,
    revoke: Option<Arc<AtomicBool>>,
    cancel: Option<&'a AtomicBool>,
    reads: usize,
}
impl Read for MutatingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        let count = self.bytes.read(buffer)?;
        if let Some(revoke) = &self.revoke {
            revoke.store(true, Ordering::Release);
        }
        if let Some(cancel) = self.cancel {
            cancel.store(true, Ordering::Release);
        }
        Ok(count)
    }
}

#[test]
fn native_refusal_does_not_read_or_eagerly_parse_invalid_json() {
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = MutatingReader {
        bytes: Cursor::new(b"not JSON"),
        revoke: None,
        cancel: None,
        reads: 0,
    };
    let result: Result<()> = consume(&mut raw, &mut auth, &stop, 32, |metadata, _| {
        assert_eq!(metadata.status, 200);
        Err(AnimationError::Budget(
            "native output admission refused".into(),
        ))
    });
    assert!(matches!(result, Err(AnimationError::Budget(_))));
    assert_eq!(raw.reads, 0);
    assert_eq!(auth.checks, 2); // Before consumer and before returning its result.
}

#[test]
fn revoked_before_consumer_exposes_neither_metadata_nor_body() {
    let mut auth = authority();
    auth.revoked.store(true, Ordering::Release);
    let stop = AtomicBool::new(false);
    let mut raw = Cursor::new(b"secret");
    let mut called = false;
    let result = consume(&mut raw, &mut auth, &stop, 32, |_, _| {
        called = true;
        Ok(())
    });
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert!(!called);
    assert_eq!(raw.position(), 0);
}

#[test]
fn revocation_during_native_read_never_copies_staged_bytes_to_consumer() {
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = MutatingReader {
        bytes: Cursor::new(b"secret"),
        revoke: Some(Arc::clone(&auth.revoked)),
        cancel: None,
        reads: 0,
    };
    let mut output = [b'x'; 16];
    let result = consume(&mut raw, &mut auth, &stop, 32, |_, reader| {
        assert!(reader.read(&mut output).is_err());
        // Swallowing a reader error cannot publish consumer output.
        Ok(7)
    });
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert_eq!(output, [b'x'; 16]);
    assert_eq!(raw.reads, 1);
}

#[test]
fn cancellation_during_native_read_is_sticky_and_keeps_consumer_buffer_clean() {
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = MutatingReader {
        bytes: Cursor::new(b"secret"),
        revoke: None,
        cancel: Some(&stop),
        reads: 0,
    };
    let mut output = [b'x'; 16];
    let result = consume(&mut raw, &mut auth, &stop, 32, |_, reader| {
        assert!(reader.read(&mut output).is_err());
        assert!(reader.read(&mut output).is_err());
        Ok(())
    });
    assert!(matches!(result, Err(AnimationError::Runtime(message)) if message=="HTTP cancelled"));
    assert_eq!(output, [b'x'; 16]);
    assert_eq!(raw.reads, 1);
}

#[test]
fn exact_body_bound_requires_eof_and_overflow_cannot_be_swallowed() {
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut exact = Cursor::new(b"four");
    let result = consume(&mut exact, &mut auth, &stop, 4, |_, reader| {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
    .unwrap();
    assert_eq!(result, b"four");
    let mut extra = Cursor::new(b"four!");
    let mut bytes = [b'x'; 4];
    let result = consume(&mut extra, &mut auth, &stop, 4, |_, reader| {
        assert_eq!(reader.read(&mut bytes).unwrap(), 4);
        let mut overflow = [b'x'; 1];
        assert!(reader.read(&mut overflow).is_err());
        assert_eq!(overflow, [b'x']);
        Ok(())
    });
    assert!(
        matches!(result, Err(AnimationError::Runtime(message)) if message=="HTTP response budget")
    );
}

struct OutputWitness(Arc<AtomicBool>);
impl Drop for OutputWitness {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
#[test]
fn post_consumer_revocation_discards_already_constructed_private_output() {
    let mut auth = authority();
    let revoked = Arc::clone(&auth.revoked);
    let stop = AtomicBool::new(false);
    let dropped = Arc::new(AtomicBool::new(false));
    let witness = Arc::clone(&dropped);
    let mut raw = Cursor::new(b"ok");
    let result = consume(&mut raw, &mut auth, &stop, 32, |_, reader| {
        let mut buffer = [0; 2];
        reader.read_exact(&mut buffer)?;
        revoked.store(true, Ordering::Release);
        Ok(OutputWitness(witness))
    });
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn every_line_rechecks_current_authority_even_when_one_chunk_contains_both() {
    let mut auth = authority();
    let revoked = Arc::clone(&auth.revoked);
    let stop = AtomicBool::new(false);
    let mut raw = Cursor::new(b"first\nsecond\n");
    let mut delivered = 0;
    let mut callback = |line: &[u8]| {
        assert_eq!(line, b"first");
        delivered += 1;
        revoked.store(true, Ordering::Release);
        Ok(true)
    };
    let result = consume(&mut raw, &mut auth, &stop, 128, |head, reader| {
        line_response(
            head,
            reader,
            LineSink {
                limits: StreamLimits {
                    max_line_bytes: 16,
                    max_records: 8,
                },
                records: 0,
                callback: &mut callback,
            },
        )
    });
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert_eq!(delivered, 1);
    // The first truthful delivery is not claimed undone by the later revocation.
}

struct PrefixOnly {
    reads: usize,
}
impl Read for PrefixOnly {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        assert_eq!(self.reads, 0, "finite prefix must not wait for source EOF");
        self.reads += 1;
        let bytes = b"record\r\nnext\n";
        buffer[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
}
#[test]
fn line_prefix_stops_without_an_eof_read_and_retains_crlf_contract() {
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = PrefixOnly { reads: 0 };
    let mut lines = Vec::new();
    let mut callback = |line: &[u8]| {
        lines.push(line.to_vec());
        Ok(true)
    };
    let response = consume(&mut raw, &mut auth, &stop, 128, |head, reader| {
        line_response(
            head,
            reader,
            LineSink {
                limits: StreamLimits {
                    max_line_bytes: 16,
                    max_records: 1,
                },
                records: 0,
                callback: &mut callback,
            },
        )
    })
    .unwrap();
    assert_eq!(response.body, Value::Null);
    assert_eq!(lines, vec![b"record".to_vec()]);
    assert_eq!(raw.reads, 1);
}

#[test]
fn existing_buffered_bytes_text_json_and_stream_line_bounds_remain_real_consumers() {
    for (format, input, expected) in [
        ("bytes", b"ok".as_slice(), serde_json::json!([111, 107])),
        ("text", b"ok".as_slice(), serde_json::json!("ok")),
        ("json", b"{\"n\":1}".as_slice(), serde_json::json!({"n":1})),
    ] {
        let mut auth = authority();
        let stop = AtomicBool::new(false);
        let mut raw = Cursor::new(input);
        let response = consume(&mut raw, &mut auth, &stop, 64, |head, reader| {
            buffered_response(head, reader, format)
        })
        .unwrap();
        assert_eq!(response.body, expected);
    }
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = Cursor::new(b"too-long\n");
    let mut callback = |_: &[u8]| panic!("oversized record must not be delivered");
    let result = consume(&mut raw, &mut auth, &stop, 64, |head, reader| {
        line_response(
            head,
            reader,
            LineSink {
                limits: StreamLimits {
                    max_line_bytes: 2,
                    max_records: 8,
                },
                records: 0,
                callback: &mut callback,
            },
        )
    });
    assert!(result.is_err());
}

struct DeniedPreflight;
impl HttpAuthority for DeniedPreflight {
    fn authorize(
        &mut self,
        phase: HttpPhase,
        _: HttpMethod,
        _: &Url,
        addresses: &[SocketAddr],
    ) -> Result<()> {
        assert_eq!(phase, HttpPhase::Preflight);
        assert!(addresses.is_empty());
        Err(AnimationError::PermissionDenied("origin denied".into()))
    }
    fn credential_headers(&mut self, _: &str, _: &Url) -> Result<BTreeMap<String, String>> {
        panic!("no credentials before preflight approval")
    }
}
struct NoDns(AtomicBool);
impl DnsResolver for NoDns {
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<SocketAddr>> {
        self.0.store(true, Ordering::Release);
        panic!("no DNS before preflight approval")
    }
}
#[test]
fn actual_raw_request_preflight_rejects_before_dns_credentials_and_native_consumer() {
    let options:HttpOptions=serde_json::from_value(serde_json::json!({"url":"https://example.org/","response":"json","max_bytes":32,"timeout_ms":1000,"credential":"opaque"})).unwrap();
    let dns = NoDns(AtomicBool::new(false));
    let mut called = false;
    let result = request_with_reader(
        &options,
        &mut DeniedPreflight,
        &dns,
        &AtomicBool::new(false),
        |_, _| {
            called = true;
            Ok(())
        },
    );
    assert!(matches!(result, Err(AnimationError::PermissionDenied(_))));
    assert!(!called);
    assert!(!dns.0.load(Ordering::Acquire));
}

#[test]
fn caller_reads_under_actual_original_io_job_and_retains_output_admission() {
    use ilium_execution::{
        ClientLimits, Execution, ExecutionConfig, Job, JobContext, JobCost, JobOutcome, JobPoll,
        Lane, LaneConfig, QuotaGroup, QuotaLimits, ShutdownMode, StorageAdmission,
    };
    struct Output {
        bytes: Vec<u8>,
        _storage: StorageAdmission,
    }
    struct CopyJob {
        quota: QuotaGroup,
    }
    impl Job for CopyJob {
        type Output = Output;
        type Error = AnimationError;
        fn run(self, context: JobContext) -> Result<Output> {
            let storage = self
                .quota
                .reserve_external_storage(64 * 1024)
                .map_err(|_| AnimationError::Budget("native output admission".into()))?;
            let mut auth = authority();
            let stop = AtomicBool::new(context.stop_requested());
            let mut raw = Cursor::new(b"not JSON");
            let bytes = consume(&mut raw, &mut auth, &stop, 64, |_, reader| {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                Ok(bytes)
            })?;
            Ok(Output {
                bytes,
                _storage: storage,
            })
        }
    }
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 2,
        jobs: 2,
        service_jobs: 0,
        input_bytes: 1024 * 1024,
        result_bytes: 1024 * 1024,
        worker_threads: 2,
        worker_bytes: 2 * 1024 * 1024,
    });
    let lane = || LaneConfig {
        threads: 1,
        queue_slots: 2,
        priority: None,
        resident_bytes_per_thread: 64 * 1024,
    };
    let mut execution = Execution::start(
        quota.clone(),
        ExecutionConfig {
            cpu: lane(),
            io: lane(),
            service: LaneConfig {
                threads: 0,
                queue_slots: 0,
                priority: None,
                resident_bytes_per_thread: 0,
            },
        },
    )
    .unwrap();
    // A bounded wake channel coalesces completion without introducing an async
    // runtime. Receipt ownership, not a wake message, proves actual completion.
    let (wake, ready) = std::sync::mpsc::sync_channel(1);
    let client = execution
        .client(ClientLimits {
            jobs: 2,
            service_jobs: 0,
            input_bytes: 1024 * 1024,
            result_bytes: 1024 * 1024,
        })
        .unwrap()
        .with_completion_wake(move || {
            let _ = wake.try_send(()); // A full slot already holds the wake; no worker blocks.
        });
    let cost = JobCost {
        input_bytes: 64 * 1024,
        result_bytes: 64 * 1024,
    };
    // Original bank reservation precedes capture; original result/storage survive
    // actual native exit and are released only with the retained typed output.
    let reservation = client.try_reserve(Lane::Io, cost).unwrap();
    let mut receipt = reservation
        .submit(CopyJob {
            quota: quota.clone(),
        })
        .map_err(|_| ())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let retained = loop {
        match receipt.try_take() {
            JobPoll::Ready(value) => break value,
            JobPoll::Pending => ready
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap(),
            _ => panic!("native original receipt missing"),
        }
    };
    assert!(
        matches!(retained.view(),JobOutcome::Finished(Ok(output)) if output.bytes==b"not JSON")
    );
    execution.request_shutdown(ShutdownMode::Drain);
    assert_eq!(
        execution
            .join_until_background(Instant::now() + Duration::from_secs(3))
            .unwrap()
            .remaining_workers,
        0
    );
    assert_eq!(quota.snapshot().input_bytes, cost.input_bytes);
    assert_eq!(quota.snapshot().result_bytes, cost.result_bytes);
    let retained_bytes = quota.snapshot().worker_bytes;
    assert!(retained_bytes >= 64 * 1024);
    drop(retained);
    // The returned output releases its own bytes; the empty-but-live original
    // Receipt independently retains job input/result charges by contract.
    assert_eq!(quota.snapshot().input_bytes, cost.input_bytes);
    assert_eq!(quota.snapshot().result_bytes, cost.result_bytes);
    assert_eq!(quota.snapshot().worker_bytes, retained_bytes - 64 * 1024);
    drop(receipt);
    assert_eq!(quota.snapshot().input_bytes, 0);
    assert_eq!(quota.snapshot().result_bytes, 0);
    // Bank/client metadata legitimately remains charged after worker join.
    // Release every actual owner before asserting a zero original-root balance.
    drop(client);
    drop(execution);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn actual_source_response_wrapper_reads_raw_under_supplied_original_admission() {
    use ilium_execution::{QuotaGroup, QuotaLimits};
    let quota = QuotaGroup::new(QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 1,
        worker_bytes: 64 * 1024,
    });
    let mut auth = authority();
    let stop = AtomicBool::new(false);
    let mut raw = Cursor::new(b"not JSON");
    let response = consume(&mut raw, &mut auth, &stop, 32, |head, reader| {
        crate::sources::SourceHttpResponse::read(head.status, reader, &quota, 32, &stop)
    })
    .unwrap();
    // Invalid JSON succeeds here because this is genuinely raw Source custody;
    // the response owns its admitted private bytes until the service parses it.
    assert_eq!(response.status(), 200);
    assert_eq!(response.as_bytes(), b"not JSON");
    assert!(quota.snapshot().worker_bytes >= 33);
    drop(response);
    assert_eq!(quota.snapshot().worker_bytes, 0);
}

#[test]
fn source_adapter_refuses_unsupported_output_bound_before_dns_or_consumer() {
    let options:HttpOptions=serde_json::from_value(serde_json::json!({"url":"https://example.org/","response":"json","max_bytes":32_000_001,"timeout_ms":1000})).unwrap();
    let quota = ilium_execution::QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 1,
        jobs: 1,
        service_jobs: 0,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 1,
        worker_bytes: 1024,
    });
    let dns = NoDns(AtomicBool::new(false));
    let result = request_source_response(
        &options,
        &mut DeniedPreflight,
        &dns,
        &AtomicBool::new(false),
        &quota,
    );
    assert!(matches!(result, Err(AnimationError::Budget(_))));
    assert!(!dns.0.load(Ordering::Acquire));
    assert_eq!(quota.snapshot().worker_bytes, 0);
}
