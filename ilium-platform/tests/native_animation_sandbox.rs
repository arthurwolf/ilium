//! Execute only in a disposable native Linux account with a delegated cgroup-v2
//! ancestor and bubblewrap. The ordinary suite must not acquire host cgroup state.

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a disposable delegated cgroup-v2 scope and /usr/bin/bwrap"]
fn owned_domain_kills_descendants_and_releases_tasks() {
    use ilium_platform::animation_sandbox::{spawn_video_decoder, SandboxLimits};
    use std::{
        io::{BufRead, BufReader, Read},
        path::Path,
        sync::mpsc,
        time::Duration,
    };

    let limits = SandboxLimits::default();
    let mut child = spawn_video_decoder(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "sleep 30 & descendant=$!; kill -0 \"$descendant\" || exit 1; printf 'owned-descendant:%s\\n' \"$descendant\"; wait".into(),
        ],
        limits,
    )
    .expect("the disposable account must have a delegated cgroup and bubblewrap");
    let cancel = child.cancel_handle();
    let stdout = child.take_stdout().expect("owned target stdout pipe");
    let (sender, receiver) = mpsc::sync_channel(1);
    // Command::spawn proves only launcher exec. Require a marker produced by
    // the shell after it has actually started and observed its live descendant.
    // The reader owns this pipe, has a byte bound, and is joined after retirement
    // on every timeout/error path so failed startup cannot leak a test thread.
    let reader = std::thread::spawn(move || {
        let mut marker = String::new();
        let result = BufReader::new(stdout.take(96))
            .read_line(&mut marker)
            .map(|_| marker);
        let _ = sender.send(result);
    });
    let readiness = receiver.recv_timeout(Duration::from_secs(3));
    let valid_marker = matches!(
        &readiness,
        Ok(Ok(marker)) if marker.strip_prefix("owned-descendant:")
            .and_then(|value| value.strip_suffix('\n'))
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|process_id| process_id > 0)
    );
    if !valid_marker {
        let termination = cancel.terminate();
        let retirement = child.shutdown();
        let joined = reader.join();
        panic!(
            "owned target/descendant never became ready: {readiness:?}; \
             terminate={termination:?}, reap={retirement:?}, reader={joined:?}"
        );
    }
    reader.join().expect("readiness reader completed");
    let active = cancel
        .resource_usage()
        .expect("read the owned kernel domain");
    assert_eq!(active.maximum_tasks, limits.maximum_tasks);
    assert!(
        active.current_tasks >= 2,
        "target and its ready descendant are not present in the owned cgroup"
    );
    assert!(active.current_memory_bytes <= limits.memory_bytes);

    cancel
        .terminate()
        .expect("kill and drain the original cgroup");
    child.shutdown().expect("reap the original launcher");
    let retired = cancel.resource_usage().expect("read retained exact domain");
    assert_eq!(
        retired.current_tasks, 0,
        "owned descendants survived cancellation"
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a disposable delegated cgroup-v2 scope and /usr/bin/bwrap"]
fn kernel_rejects_tasks_beyond_the_owned_domain_limit() {
    use ilium_platform::animation_sandbox::{spawn_video_decoder, SandboxLimits};
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    let limits = SandboxLimits::default();
    let mut child = spawn_video_decoder(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "i=0; while [ \"$i\" -lt 40 ]; do /usr/bin/sleep 10 & i=$((i+1)); done; wait".into(),
        ],
        limits,
    )
    .expect("launch the shell in its private cgroup");
    let cancel = child.cancel_handle();
    let deadline = Instant::now() + Duration::from_secs(3);
    let usage = loop {
        let usage = cancel.resource_usage().expect("read kernel task counters");
        if usage.task_limit_events > 0 || Instant::now() >= deadline {
            break usage;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        usage.task_limit_events > 0,
        "kernel never denied an excess task: {usage:?}"
    );
    assert!(
        usage.current_tasks <= limits.maximum_tasks,
        "task cap was exceeded: {usage:?}"
    );
    cancel.terminate().expect("kill the original domain");
    child.shutdown().expect("reap the launcher");
    assert_eq!(
        cancel
            .resource_usage()
            .expect("retained domain")
            .current_tasks,
        0
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a disposable delegated cgroup-v2 scope, /usr/bin/python3 and /usr/bin/bwrap"]
fn kernel_oom_kills_a_real_physical_allocation() {
    use ilium_platform::animation_sandbox::{spawn_video_decoder, SandboxLimits};
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    let limits = SandboxLimits {
        memory_bytes: 64 * 1024 * 1024,
        ..SandboxLimits::default()
    };
    // bytearray initializes each page; reserving address space alone would not
    // prove enforcement of the cgroup's physical memory limit.
    let mut child = spawn_video_decoder(
        Path::new("/usr/bin/python3"),
        &[
            "-c".into(),
            "resident = bytearray(128 * 1024 * 1024)".into(),
        ],
        limits,
    )
    .expect("launch a real allocating process in its private cgroup");
    let cancel = child.cancel_handle();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().expect("observe owned child").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            cancel.terminate().expect("retire a stalled allocator");
            panic!("allocator did not terminate within the native test deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = child
        .wait_for_exit()
        .expect("reap and drain the owned domain");
    let usage = cancel
        .resource_usage()
        .expect("read retained kernel OOM counter");
    assert!(
        !status.success(),
        "allocation unexpectedly survived the cap"
    );
    assert!(
        usage.memory_oom_kill_events > 0,
        "kernel did not record an OOM kill: {usage:?}"
    );
    assert!(
        usage.memory_limit_events > 0,
        "allocation never reached the cgroup memory limit: {usage:?}"
    );
    assert_eq!(
        usage.current_tasks, 0,
        "allocator left descendants: {usage:?}"
    );
}
