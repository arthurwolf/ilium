//! Qualification-only native thread failure gate. It deliberately remains
//! ignored in ordinary test suites and must be invoked by its exact name.

#[cfg(target_os = "linux")]
mod linux {
    //! Ignored, isolated qualification of an actual kernel thread-creation refusal.
    //! Run this test alone: `cargo test -p ilium-platform --test native_spawn_failure
    //! -- --ignored --exact linux::native_spawn_failure_releases_slot_and_custody --test-threads=1`.
    //! The harness parent creates a child copy of this test executable; only that
    //! child changes its own RLIMIT_NPROC soft limit, and always restores it.

    use ilium_platform::owned_worker::{
        initialize_supervisor, reserve_owned_worker, supervisor_status, StopToken, WorkerExit,
        WorkerKind,
    };
    use std::io;
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    const CHILD_MARKER: &str = "ILIUM_NATIVE_SPAWN_FAILURE_322_CHILD";
    const TEST_NAME: &str = "linux::native_spawn_failure_releases_slot_and_custody";
    const DECLARED_STACK_BYTES: usize = 256 * 1024;
    const PROOF_MARKER: &str = "ILIUM_KERNEL_EAGAIN_AND_ROLLBACK_PROVED";

    /// Represents the caller's already-admitted physical quota. The platform
    /// remains independent of ilium-execution; the execution integration test
    /// separately covers its real QuotaGroup debit and failed-Builder rollback.
    struct GenericAdmissionLease(Arc<AtomicUsize>);

    impl Drop for GenericAdmissionLease {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// The soft limit is changed only in the throwaway child process. Keeping
    /// rlim_max unchanged lets an unprivileged child restore its original soft
    /// value. Drop is a final safeguard for any early return or panic.
    struct ChildNprocLimit {
        original: libc::rlimit,
        active: bool,
    }

    impl ChildNprocLimit {
        fn lower_to_zero() -> io::Result<Self> {
            let mut original = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: original points to a live libc::rlimit value, owned here.
            if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &mut original) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let lowered = libc::rlimit {
                rlim_cur: 0,
                rlim_max: original.rlim_max,
            };
            // SAFETY: lowered is a valid in-process soft-limit request. The hard
            // limit and every other process's rlimits remain untouched.
            if unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &lowered) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let guard = Self {
                original,
                active: true,
            };
            let mut observed = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: observed is writable local storage for the child-only
            // limit readback. An error drops guard and restores the original.
            if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &mut observed) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if observed.rlim_cur != 0 || observed.rlim_max != guard.original.rlim_max {
                return Err(io::Error::other("RLIMIT_NPROC soft-zero readback failed"));
            }
            Ok(guard)
        }

        fn restore(&mut self) -> io::Result<()> {
            if !self.active {
                return Ok(());
            }
            // SAFETY: original was returned by getrlimit in this same process;
            // only our soft limit changed, so restoration needs no privilege.
            if unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &self.original) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.active = false;
            Ok(())
        }
    }

    impl Drop for ChildNprocLimit {
        fn drop(&mut self) {
            if !self.active {
                return;
            }
            if let Err(error) = self.restore() {
                eprintln!("UNQUALIFIED: child RLIMIT_NPROC restore failed: {error}");
                // The child must not continue under an unexpectedly low limit.
                // This exits only the isolated test-executable child.
                std::process::exit(91);
            }
        }
    }

    fn child_qualification() {
        // Linux exempts real root and suitably capable processes from NPROC.
        // A bypass would make this test unable to prove kernel EAGAIN.
        // SAFETY: these libc calls only read the calling process's UID values.
        if unsafe { libc::getuid() } == 0 || unsafe { libc::geteuid() } == 0 {
            panic!("UNQUALIFIED: RLIMIT_NPROC does not constrain root credentials");
        }

        assert!(
            initialize_supervisor().expect("start the existing supervisor before limiting NPROC")
        );
        let before = supervisor_status().expect("supervisor installed");
        assert_eq!(before.registered_workers, 0);
        assert_eq!(before.reserved_workers, 0);

        let active_leases = Arc::new(AtomicUsize::new(1));
        let lease = GenericAdmissionLease(Arc::clone(&active_leases));
        let reservation = reserve_owned_worker(Some(DECLARED_STACK_BYTES), lease)
            .expect("valid named worker reserves its original registry slot");
        let admitted = supervisor_status().expect("reservation observed");
        assert_eq!(admitted.thread_id, before.thread_id);
        assert_eq!(admitted.registered_workers, 0);
        assert_eq!(admitted.reserved_workers, 1);
        assert_eq!(active_leases.load(Ordering::Acquire), 1);

        let body_ran = Arc::new(AtomicBool::new(false));
        let body_flag = Arc::clone(&body_ran);
        let mut limit = ChildNprocLimit::lower_to_zero()
            .expect("UNQUALIFIED: child could not set its own RLIMIT_NPROC soft limit to zero");
        let attempted = reservation.spawn(
            "ilium-native-eagain-probe",
            WorkerKind::Cooperative,
            StopToken::default(),
            || {},
            move |_| {
                body_flag.store(true, Ordering::Release);
            },
        );
        limit
            .restore()
            .expect("UNQUALIFIED: child could not restore its original RLIMIT_NPROC");

        let error = match attempted {
            Err(error) => error,
            Ok(owner) => {
                let ticket = owner.ticket();
                drop(owner);
                let joined = ticket.join_until(Instant::now() + Duration::from_secs(5));
                panic!(
                    "UNQUALIFIED: valid pthread creation succeeded despite child RLIMIT_NPROC=0; \
                     possible privilege/capability bypass; cleanup join: {joined:?}"
                );
            }
        };
        assert_eq!(
            error.raw_os_error(),
            Some(libc::EAGAIN),
            "UNQUALIFIED: expected the kernel pthread EAGAIN, got {error}"
        );
        assert!(!body_ran.load(Ordering::Acquire), "refused native body ran");
        assert_eq!(
            active_leases.load(Ordering::Acquire),
            0,
            "original caller custody was retained"
        );
        let after = supervisor_status().expect("same supervisor remains available");
        assert_eq!(after.thread_id, before.thread_id);
        assert_eq!(
            after.reserved_workers, 0,
            "failed native spawn stranded a slot"
        );
        assert_eq!(
            after.registered_workers, 0,
            "failed native spawn published a handle"
        );

        // A restored limit must permit another ordinary valid request. This
        // confirms the failed reservation did not poison the sole supervisor.
        let subsequent = reserve_owned_worker(Some(DECLARED_STACK_BYTES), ())
            .expect("reserve after native refusal")
            .spawn(
                "ilium-native-recovery-probe",
                WorkerKind::Cooperative,
                StopToken::default(),
                || {},
                |_| {},
            )
            .expect("spawn after restoring child's original limit");
        let ticket = subsequent.ticket();
        drop(subsequent);
        assert_eq!(
            ticket.join_until(Instant::now() + Duration::from_secs(5)),
            Ok(WorkerExit::Joined),
            "supervisor must still retire a later real worker"
        );
        println!("{PROOF_MARKER}");
    }

    fn require_child_proof(output: Output) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains(PROOF_MARKER),
            "native kernel-refusal gate failed or is unqualified; child status: {}; \
             stdout: {stdout}; stderr: {stderr}",
            output.status
        );
        // Export only a proof already validated from the isolated child's
        // successful exit. The outer qualification runner also requires it.
        println!("{PROOF_MARKER}");
    }

    #[test]
    #[ignore = "Linux-only qualification: child RLIMIT_NPROC must yield real pthread EAGAIN"]
    fn native_spawn_failure_releases_slot_and_custody() {
        if let Some(marker) = std::env::var_os(CHILD_MARKER) {
            let parent_id: u32 = marker
                .to_string_lossy()
                .parse()
                .expect("UNQUALIFIED: malformed isolated-child marker");
            // A stray environment variable must never lower a normal test
            // process's limit. Only the direct child created below is eligible.
            // SAFETY: getppid only reads this process's parent PID.
            assert_eq!(
                unsafe { libc::getppid() } as u32,
                parent_id,
                "UNQUALIFIED: marker does not identify this test's direct child"
            );
            child_qualification();
            return;
        }
        let test_exe = std::env::current_exe().expect("the current integration-test executable");
        let mut child = Command::new(test_exe)
            .arg("--ignored")
            .arg("--exact")
            .arg(TEST_NAME)
            .arg("--test-threads=1")
            .arg("--nocapture")
            .env(CHILD_MARKER, std::process::id().to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("launch an isolated copy of this test executable");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match child.try_wait().expect("inspect only our isolated child") {
                Some(_) => {
                    let output = child
                        .wait_with_output()
                        .expect("collect completed child proof");
                    require_child_proof(output);
                    break;
                }
                None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                None => {
                    let _ = child.kill();
                    let output = child
                        .wait_with_output()
                        .expect("reap timed-out owned child");
                    panic!(
                        "UNQUALIFIED: isolated native-spawn child exceeded 15 s; status: {}; \
                         stdout: {}; stderr: {}",
                        output.status,
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    #[test]
    #[ignore = "Linux RLIMIT_NPROC qualification is unavailable on this target"]
    fn native_spawn_failure_releases_slot_and_custody() {
        panic!("UNQUALIFIED: this native EAGAIN gate requires Linux RLIMIT_NPROC");
    }
}
