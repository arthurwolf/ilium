use super::*;

fn shared_with_extra(bytes: usize) -> Shared {
    let quota = QuotaGroup::new(ilium_execution::QuotaLimits {
        clients: 0,
        jobs: 0,
        service_jobs: 0,
        input_bytes: 0,
        result_bytes: 0,
        worker_threads: 0,
        worker_bytes: input_storage_bytes() + bytes,
    });
    let admission = quota
        .reserve_external_storage(input_storage_bytes())
        .unwrap();
    Shared {
        state: Mutex::new(State {
            events: VecDeque::with_capacity(INPUT_QUEUE_CAPACITY),
            failure: None,
            producer_done: false,
            receiver_open: true,
            retiring: false,
            queue_excess: None,
            native: None,
        }),
        available: Condvar::new(),
        ready: Notify::new(),
        stop: StopToken::default(),
        quota,
        storage: Arc::new(InputStorage {
            active: Mutex::new(0),
            available: Condvar::new(),
            _admission: admission,
        }),
    }
}
fn native_storage(shared: &Shared) -> RootNativeStorage {
    RootNativeStorage {
        mandatory_bytes: input_storage_bytes(),
        last_refusal: Mutex::new(None),
        quota: shared.quota.clone(),
        storage: Arc::clone(&shared.storage),
        stop: shared.stop.clone(),
    }
}

#[test]
fn root_admits_full_old_and_new_backings_without_changing_fifo_policy() {
    let shared = shared_with_extra(768);
    let storage = native_storage(&shared);
    let old = storage.reserve(256).unwrap();
    let new = storage.reserve(512).unwrap();
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 768
    );
    assert!(matches!(
        storage.reserve(1),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Busy,
            requested: 1
        })
    ));
    drop(old);
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 512
    );
    drop(new);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
    assert_eq!(INPUT_QUEUE_CAPACITY, 256);
    assert_eq!(crate::terminal_input::MAX_BYTES, 64 * 1024 * 1024);
}

#[test]
fn leased_original_moves_pointer_without_second_payload_admission() {
    let shared = shared_with_extra(256);
    let lease = native_storage(&shared).reserve(256).unwrap();
    let mut text = String::with_capacity(256);
    text.push_str("original semantic Paste");
    let pointer = text.as_ptr();
    let slot = shared.query_slot().unwrap();
    assert!(enqueue_leased_original(
        &shared,
        slot,
        Event::Paste(text),
        Some(lease),
        None
    ));
    let original = shared.state.lock().unwrap().events.pop_front().unwrap();
    assert_eq!(original.unadmitted_payload_bytes(), 0);
    assert!(original.payload.is_none());
    original.dispatch_paste(|text| {
        assert_eq!(text.as_ptr(), pointer);
        assert_eq!(
            shared.quota.snapshot().worker_bytes,
            input_storage_bytes() + 256
        );
        drop(text);
        assert_eq!(
            shared.quota.snapshot().worker_bytes,
            input_storage_bytes() + 256
        );
    });
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn early_query_paste_keeps_native_lease_and_precedes_reply() {
    let shared = shared_with_extra(512);
    let admission = native_storage(&shared);
    let paste_lease = admission.reserve(256).unwrap();
    let reply_lease = admission.reserve(256).unwrap();
    let mut text = String::with_capacity(256);
    text.push_str("early original");
    let mut items = VecDeque::from([
        ReadItem::Event(Event::Paste(text), Some(paste_lease)),
        ReadItem::Reply(NativeReply {
            bytes: b"\x1b[?1u".to_vec(),
            _storage: Some(reply_lease),
        }),
    ]);
    assert!(collect_owned_keyboard_query(
        &shared,
        |_| Ok(items.pop_front()),
        Instant::now() + Duration::from_secs(1)
    )
    .unwrap());
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 256
    );
    let original = shared.state.lock().unwrap().events.pop_front().unwrap();
    assert!(matches!(original.view(), Event::Paste(text) if text == "early original"));
    drop(original);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn cancellation_rejects_new_growth_but_keeps_original_lease() {
    let shared = shared_with_extra(256);
    let storage = native_storage(&shared);
    let lease = storage.reserve(256).unwrap();
    shared.close(true);
    assert!(matches!(
        storage.reserve(1),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Closed,
            requested: 1
        })
    ));
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 256
    );
    drop(lease);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn completion_policy_compacts_only_capacity_slack_that_blocks_a_valid_semantic_body() {
    let shared = shared_with_extra(0);
    let storage = native_storage(&shared);
    let cap = crate::terminal_input::MAX_BYTES;
    assert_eq!(
        storage.paste_completion(40 * 1024 * 1024, cap),
        NativePasteCompletion::Compact
    );
    assert_eq!(
        storage.paste_completion(5, 256),
        NativePasteCompletion::KeepBacking
    );
    assert_eq!(
        storage.paste_completion(cap, 2 * cap),
        NativePasteCompletion::KeepBacking
    );
    assert_eq!(
        storage.paste_completion(cap - PASTE_REQUEST_WIRE_BYTES, cap),
        NativePasteCompletion::Compact
    );
    assert_eq!(
        storage.paste_completion(cap - PASTE_REQUEST_WIRE_BYTES + 1, cap),
        NativePasteCompletion::KeepBacking
    );
    assert_eq!(
        storage.paste_completion(5, cap - PASTE_DELIMITER_BYTES),
        NativePasteCompletion::KeepBacking
    );
    assert_eq!(crate::terminal_input::MAX_BYTES, 64 * 1024 * 1024);
    assert_eq!(INPUT_QUEUE_CAPACITY, 256);
}

#[test]
fn cumulative_worker_credit_is_busy_then_releases_without_allocating_on_refusal() {
    let shared = shared_with_extra(512);
    let storage = native_storage(&shared);
    let other_owner = shared.quota.reserve_external_storage(512).unwrap();
    assert!(matches!(
        storage.reserve(256),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Busy,
            requested: 256
        })
    ));
    assert_eq!(
        *storage.last_refusal.lock().unwrap(),
        Some(RejectReason::WorkerBytes)
    );
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 512
    );
    drop(other_owner);
    let original = storage.reserve(256).unwrap();
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 256
    );
    drop(original);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn inherently_impossible_native_cost_is_limit_and_cancel_remains_closed() {
    let shared = shared_with_extra(256);
    let storage = native_storage(&shared);
    assert!(matches!(
        storage.reserve(257),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Limit,
            requested: 257
        })
    ));
    assert_eq!(
        *storage.last_refusal.lock().unwrap(),
        Some(RejectReason::WorkerBytes)
    );
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
    shared.close(true);
    assert!(matches!(
        storage.reserve(1),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Closed,
            requested: 1
        })
    ));
    assert_eq!(
        *storage.last_refusal.lock().unwrap(),
        Some(RejectReason::Closed)
    );
}

#[test]
fn intrinsic_old_and_new_peak_is_hard_but_other_completed_credit_can_release() {
    let shared = shared_with_extra(768);
    let storage = native_storage(&shared);
    let raw = storage.reserve(256).unwrap();
    assert!(matches!(
        storage.reserve_replacement(513, 256),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Limit,
            requested: 513
        })
    ));
    assert_eq!(
        *storage.last_refusal.lock().unwrap(),
        Some(RejectReason::WorkerBytes)
    );
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 256
    );
    let completed = storage.reserve(512).unwrap();
    assert!(matches!(
        storage.reserve_replacement(5, 256),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Busy,
            requested: 5
        })
    ));
    drop(completed);
    let replacement = storage.reserve_replacement(5, 256).unwrap();
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 261
    );
    drop(raw);
    drop(replacement);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn blocked_credit_release_retries_one_owned_fixture_original_and_preserves_key_order() {
    // Synthetic partial fixture exercises the real root classifier and wait
    // used by InputDriver. It is not a real NativeInputReader/PTY custody proof.
    let shared = Arc::new(shared_with_extra(768));
    let storage = Arc::new(native_storage(&shared));
    let raw_guard = storage.reserve(256).unwrap();
    let mut raw = String::with_capacity(256);
    raw.push_str("\x1b[200~hello\x1b[201");
    let pointer = raw.as_ptr() as usize;
    let other_owner = shared.quota.reserve_external_storage(512).unwrap();
    let (blocked_sender, blocked_receiver) = std::sync::mpsc::channel();
    let (done_sender, done_receiver) = std::sync::mpsc::channel();
    let body = shared.clone();
    let admission = storage.clone();
    let producer = std::thread::spawn(move || {
        let tail = b"~x";
        let mut first = true;
        let deadline = Instant::now() + Duration::from_secs(5);
        let lease = loop {
            match admission.reserve_replacement(5, raw.capacity()) {
                Ok(lease) => break lease,
                Err(refusal) => {
                    assert_eq!(refusal.kind, NativeRefusalKind::Busy);
                    assert_eq!(raw.as_ptr() as usize, pointer);
                    assert_eq!(raw, "\x1b[200~hello\x1b[201");
                    assert_eq!(tail, b"~x");
                    if first {
                        blocked_sender.send(()).unwrap();
                        first = false;
                    }
                    assert!(await_native_credit(&body, deadline));
                }
            }
        };
        let mut text = String::with_capacity(5);
        text.push_str(&raw[6..11]);
        drop(raw);
        drop(raw_guard);
        assert!(enqueue_leased_original(
            &body,
            body.query_slot().unwrap(),
            Event::Paste(text),
            Some(lease),
            None
        ));
        assert!(enqueue_leased_original(
            &body,
            body.query_slot().unwrap(),
            Event::Key(crossterm::event::KeyCode::Char('x').into()),
            None,
            None
        ));
        done_sender.send(()).unwrap();
    });
    blocked_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(shared.state.lock().unwrap().events.is_empty());
    assert_eq!(
        *storage.last_refusal.lock().unwrap(),
        Some(RejectReason::WorkerBytes)
    );
    drop(other_owner); // Only100ms fallback can observe this foreign-root release.
    done_receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    producer.join().unwrap();
    let mut state = shared.state.lock().unwrap();
    assert_eq!(state.events.len(), 2);
    let paste = state.events.pop_front().unwrap();
    assert!(matches!(paste.view(), Event::Paste(text) if text=="hello"));
    let key = state.events.pop_front().unwrap();
    assert!(
        matches!(key.view(), Event::Key(key) if key.code==crossterm::event::KeyCode::Char('x'))
    );
    assert!(state.events.is_empty());
    drop(state);
    drop(paste);
    drop(key);
    assert_eq!(shared.quota.snapshot().worker_bytes, input_storage_bytes());
}

#[test]
fn credit_wait_cancellation_wakes_without_consuming_partial_fixture() {
    let shared = Arc::new(shared_with_extra(256));
    let storage = native_storage(&shared);
    let lease = storage.reserve(256).unwrap();
    let (started_sender, started_receiver) = std::sync::mpsc::channel();
    let body = shared.clone();
    let waiter = std::thread::spawn(move || {
        started_sender.send(()).unwrap();
        await_native_credit(&body, Instant::now() + Duration::from_secs(5))
    });
    started_receiver
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    shared.close(true);
    assert!(!waiter.join().unwrap());
    assert!(matches!(
        storage.reserve_replacement(1, 256),
        Err(NativeStorageRefusal {
            kind: NativeRefusalKind::Closed,
            requested: 1
        })
    ));
    assert_eq!(
        shared.quota.snapshot().worker_bytes,
        input_storage_bytes() + 256
    );
    drop(lease);
}
