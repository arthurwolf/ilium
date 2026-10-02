use super::*;
use std::sync::mpsc;

fn request(generation: u64) -> Request {
    Request {
        generation,
        region_directory: "/test/region".into(),
        chunks: [[0, 0]].into(),
        limits: loader::Limits::default(),
    }
}

fn admit<T: Send + Sync + 'static>(worker: &PreparationWorker<T>, request: &Request) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match worker.submit(request).unwrap() {
            Admission::Accepted => return,
            Admission::Busy if std::time::Instant::now() < deadline => std::thread::yield_now(),
            admission => panic!("request not admitted: {admission:?}"),
        }
    }
}

#[test]
fn newer_generation_cancels_inflight_and_stale_completion_is_not_published() {
    let (started, start_events) = mpsc::channel();
    let (release, releases) = mpsc::channel();
    let worker = PreparationWorker::start_with(move |request, cancelled| {
        started.send(request.generation).unwrap();
        if request.generation == 1 {
            releases.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(cancelled());
        }
        Ok(request.generation)
    })
    .unwrap();
    admit(&worker, &request(1));
    assert_eq!(
        start_events.recv_timeout(Duration::from_secs(3)).unwrap(),
        1
    );
    admit(&worker, &request(2));
    release.send(()).unwrap();
    assert_eq!(
        start_events.recv_timeout(Duration::from_secs(3)).unwrap(),
        2
    );
    assert_eq!(worker.submit(&request(1)).unwrap(), Admission::Stale);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut prepared = None;
    while prepared.is_none() && std::time::Instant::now() < deadline {
        worker.update(&mut prepared).unwrap();
        std::thread::yield_now();
    }
    let prepared = prepared.unwrap();
    assert_eq!(prepared.generation, 2);
    assert_eq!(*prepared.result.as_ref().unwrap(), 2);
}

#[test]
fn pending_slot_coalesces_to_the_latest_request() {
    let (started, starts) = mpsc::channel();
    let (release, releases) = mpsc::channel();
    let worker = PreparationWorker::start_with(move |request, _| {
        started.send(request.generation).unwrap();
        if request.generation == 1 {
            releases.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        Ok(request.generation)
    })
    .unwrap();
    admit(&worker, &request(1));
    assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
    admit(&worker, &request(2));
    admit(&worker, &request(3));
    release.send(()).unwrap();
    assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
}

#[test]
fn invalid_requests_do_not_supersede_valid_generation() {
    let worker = PreparationWorker::start_with(|request, _| Ok(request.generation)).unwrap();
    let mut invalid = request(2);
    invalid.chunks = (0..129).map(|x| [x, 0]).collect();
    assert!(matches!(
        worker.submit(&invalid),
        Err(Error::InvalidRequest)
    ));
    admit(&worker, &request(1));
}

#[test]
fn swap_retires_old_output_on_the_worker_thread() {
    struct Dropped(mpsc::Sender<String>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0
                .send(
                    std::thread::current()
                        .name()
                        .unwrap_or("unnamed")
                        .to_owned(),
                )
                .unwrap();
        }
    }
    let (dropped, drops) = mpsc::channel();
    let worker = PreparationWorker::start_with(move |_, _| Ok(Dropped(dropped.clone()))).unwrap();
    let mut prepared = None;
    for generation in [1, 2] {
        admit(&worker, &request(generation));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while prepared
            .as_ref()
            .map(|p: &Arc<Prepared<Dropped>>| p.generation)
            != Some(generation)
            && std::time::Instant::now() < deadline
        {
            worker.update(&mut prepared).unwrap();
            std::thread::yield_now();
        }
        assert_eq!(prepared.as_ref().unwrap().generation, generation);
    }
    assert!(drops
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .starts_with("ilium-ambient-minecraft-saved"));
    // Keep the current object owned by the worker when retiring the scene.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match worker.retire_current(&mut prepared) {
            Ok(()) => break,
            Err(Error::Busy) if std::time::Instant::now() < deadline => std::thread::yield_now(),
            result => panic!("retirement failed: {result:?}"),
        }
    }
}

#[test]
fn busy_admission_cancels_old_generation_without_blocking_the_caller() {
    let worker = PreparationWorker::start_with(|request, _| Ok(request.generation)).unwrap();
    let pending = worker.shared.pending.lock().unwrap();
    assert_eq!(worker.submit(&request(2)).unwrap(), Admission::Busy);
    assert_eq!(worker.shared.desired.load(Ordering::Acquire), 2);
    assert_eq!(worker.submit(&request(1)).unwrap(), Admission::Stale);
    drop(pending);
    admit(&worker, &request(2));
}

#[test]
fn preparation_error_is_bounded_and_published_with_its_generation() {
    let worker = PreparationWorker::<()>::start_with(|_, _| Err("é".repeat(10_000))).unwrap();
    admit(&worker, &request(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut prepared = None;
    while prepared.is_none() && std::time::Instant::now() < deadline {
        worker.update(&mut prepared).unwrap();
        std::thread::yield_now();
    }
    let prepared = prepared.unwrap();
    assert_eq!(prepared.generation, 1);
    assert_eq!(prepared.result.as_ref().unwrap_err().chars().count(), 512);
}

#[test]
fn callback_panic_becomes_explicit_fault_without_publishing_partial_output() {
    let worker =
        PreparationWorker::<()>::start_with(|_, _| panic!("test preparation panic")).unwrap();
    admit(&worker, &request(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while worker.shared.fault.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(matches!(worker.update(&mut None), Err(Error::Panicked)));
    assert!(matches!(worker.submit(&request(2)), Err(Error::Panicked)));
    assert!(worker.shared.latest.lock().unwrap().is_none());
}

#[test]
fn dropping_worker_signals_inflight_cancellation_and_owned_cleanup() {
    let (started, starts) = mpsc::channel();
    let (ended, ends) = mpsc::channel();
    let worker = PreparationWorker::start_with(move |_, cancelled| {
        started.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !cancelled() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(cancelled());
        ended.send(()).unwrap();
        Ok(())
    })
    .unwrap();
    admit(&worker, &request(1));
    starts.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(worker);
    ends.recv_timeout(Duration::from_secs(3)).unwrap();
}

#[test]
fn faulted_worker_still_retires_published_output_off_the_caller_thread() {
    struct Dropped(mpsc::Sender<String>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            let _ = self.0.send(
                std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .to_owned(),
            );
        }
    }
    let (dropped, drops) = mpsc::channel();
    let worker = PreparationWorker::start_with(move |request, _| {
        assert_ne!(request.generation, 2, "test second preparation panic");
        Ok(Dropped(dropped.clone()))
    })
    .unwrap();
    admit(&worker, &request(1));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut prepared = None;
    while prepared.is_none() && std::time::Instant::now() < deadline {
        worker.update(&mut prepared).unwrap();
        std::thread::yield_now();
    }
    assert!(prepared.is_some());
    admit(&worker, &request(2));
    while worker.shared.fault.load(Ordering::Acquire) == 0 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(matches!(worker.health(), Err(Error::Panicked)));
    loop {
        match worker.retire_current(&mut prepared) {
            Ok(()) => break,
            Err(Error::Busy) if std::time::Instant::now() < deadline => std::thread::yield_now(),
            result => panic!("fault retirement failed: {result:?}"),
        }
    }
    drop(worker);
    assert!(drops
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .starts_with("ilium-ambient-minecraft-saved"));
}
