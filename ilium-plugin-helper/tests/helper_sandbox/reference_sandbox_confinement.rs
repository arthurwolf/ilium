//! reference-sandbox.md: topology, process confinement, seccomp, cgroup limits and
//! sandbox availability (SEC-020 to SEC-029, SEC-055 to SEC-057, SEC-065, SEC-067).

use crate::probe_child::{parent_ns, run_probe, Probe, SYSCALL_GROUPS};
use crate::support::*;

const PAGE: &str = "reference-sandbox";

fn applied(name: &str, env: &[(&str, String)]) -> Probe {
    let probe = run_probe(name, env);
    assert!(
        probe.completed(),
        "sandbox was not applied for probe {name}: {:?}",
        probe.one("apply_error")
    );
    probe
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ilium-helper-sandbox-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    dir
}

// ------------------------------------------------------------------ SEC-020

/// REQ: SEC-020
#[test]
fn sec_020_two_plugins_run_in_two_helper_processes() {
    let mut first = Helper::spawn();
    let mut second = Helper::spawn();
    assert_ne!(first.pid(), second.pid());
    assert_eq!(first.ready("plugin-a"), Ok(()));
    assert_eq!(second.ready("plugin-b"), Ok(()));
}

/// REQ: SEC-020
#[test]
fn sec_020_server_side_and_scene_side_of_one_plugin_use_separate_helpers() {
    let mut server_side = Helper::spawn();
    let mut scene_side = Helper::spawn();
    assert_ne!(server_side.pid(), scene_side.pid());
    assert_eq!(server_side.ready("notes"), Ok(()));
    assert_eq!(scene_side.ready("notes"), Ok(()));
}

/// REQ: SEC-020
#[test]
fn sec_020_one_helper_refuses_a_second_plugin() {
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready("plugin-a"), Ok(()));
    let fingerprint = helper.hello.clone().expect("hello").fingerprint;
    helper.send(&msg_instantiate(
        "plugin-b",
        &fingerprint,
        EMPTY_COMPONENT,
        &[],
    ));
    // The refusal must be an explicit InstantiateFailed, never Ready and never just silence
    // (a hang or crash would otherwise pass). manual does not pin the refusal message text.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1_500);
    let mut refused = false;
    while !refused {
        let left = deadline
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        match helper.recv(left.max(1)) {
            Recv::Frame(frame) => {
                assert_ne!(frame_tag(&frame), Some(tag::READY), "second plugin served");
                refused = frame_tag(&frame) == Some(tag::INSTANTIATE_FAILED);
            }
            Recv::Timeout | Recv::Closed => break,
        }
    }
    assert!(refused, "no InstantiateFailed for the second plugin");
}

/// REQ: SEC-020
#[test]
fn sec_020_hostile_guest_cannot_read_another_plugins_data_folder() {
    let wasm = require_fixture("conformance-probe");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("plugin-a", &wasm), Ok(()));
    let (result, _) = run_call(
        &mut helper,
        1,
        "read-other-plugin-data",
        b"plugin-b",
        2_000,
        3_000,
        |_| (Vec::new(), None),
    );
    assert_eq!(
        String::from_utf8_lossy(&result.expect("call result").result),
        "not-found"
    );
}

// ------------------------------------------------------------------ SEC-021

/// REQ: SEC-021
#[test]
fn sec_021_user_mount_ipc_uts_and_net_namespaces_differ_from_the_server() {
    let probe = applied("ns", &[]);
    let inner = probe.map("ns");
    for kind in ["user", "mnt", "ipc", "uts", "net"] {
        let inner_ns = inner
            .get(kind)
            .unwrap_or_else(|| panic!("{kind} namespace not reported by the probe"));
        assert_ne!(inner_ns, &parent_ns(kind), "{kind} namespace is shared");
    }
}

/// REQ: SEC-021
#[test]
fn sec_021_pid_namespace_differs_from_the_server() {
    let probe = applied("ns", &[]);
    let inner = probe.map("ns");
    // The plugin's own process must be in a new PID namespace (not only its children).
    let own_pid_ns = inner
        .get("pid")
        .expect("pid namespace not reported by the probe");
    assert_ne!(own_pid_ns, &parent_ns("pid"), "pid namespace is shared");
}

/// REQ: SEC-021
#[test]
fn sec_021_no_capabilities_outside_the_user_namespace() {
    let probe = applied("privs", &[]);
    let status = probe.map("status");
    assert_eq!(
        status.get("CapEff").map(String::as_str),
        Some("0000000000000000")
    );
    assert_eq!(
        status.get("CapPrm").map(String::as_str),
        Some("0000000000000000")
    );
}

/// REQ: SEC-021
#[test]
fn doc_sec_021_manual_lists_the_six_namespaces() {
    assert_page_has(
        PAGE,
        "The helper runs in new user, mount, PID, IPC, UTS and network namespaces",
    );
}

// ------------------------------------------------------------------ SEC-022

/// REQ: SEC-022
#[test]
fn sec_022_connect_to_loopback_fails() {
    let probe = applied("net", &[]);
    assert_eq!(probe.one("tcp_loopback").as_deref(), Some("false"));
}

/// REQ: SEC-022
#[test]
fn sec_022_connect_to_a_public_address_fails() {
    let probe = applied("net", &[]);
    assert_eq!(probe.one("tcp_public").as_deref(), Some("false"));
}

/// REQ: SEC-022
#[test]
fn sec_022_creating_a_udp_socket_fails() {
    let probe = applied("net", &[]);
    assert_eq!(probe.one("udp_bind").as_deref(), Some("false"));
}

/// REQ: SEC-022
#[test]
fn sec_022_only_loopback_interface_exists() {
    let probe = applied("net", &[]);
    let interfaces = probe.all("interface");
    assert_eq!(interfaces, vec!["lo".to_string()]);
}

// ------------------------------------------------------------------ SEC-023

const ALLOWED_ROOT: [&str; 14] = [
    "usr",
    "bin",
    "lib",
    "lib64",
    "etc",
    "proc",
    "dev",
    "tmp",
    "package",
    "data",
    "cache",
    "project",
    "worktrees",
    "picked",
];

/// REQ: SEC-023
#[test]
fn sec_023_root_holds_only_the_specified_paths() {
    let probe = applied("mounts", &[]);
    let entries = probe.all("root_entry");
    for entry in &entries {
        assert!(
            ALLOWED_ROOT.contains(&entry.as_str()),
            "unexpected /{entry}"
        );
    }
    for required in ["usr", "bin", "etc", "proc", "dev", "tmp"] {
        assert!(entries.iter().any(|e| e == required), "missing /{required}");
    }
}

/// REQ: SEC-023
#[test]
fn sec_023_dev_holds_only_null_zero_urandom_full() {
    let probe = applied("mounts", &[]);
    let mut entries = probe.all("dev_entry");
    entries.sort();
    assert_eq!(entries, vec!["full", "null", "urandom", "zero"]);
}

/// REQ: SEC-023
#[test]
fn sec_023_sys_is_not_mounted() {
    let probe = applied("mounts", &[]);
    assert_eq!(probe.one("sys_present").as_deref(), Some("false"));
}

/// REQ: SEC-023
#[test]
fn sec_023_tmp_is_a_64_mib_tmpfs() {
    let probe = applied("mounts", &[]);
    let line = probe
        .all("mountinfo")
        .into_iter()
        .find(|l| l.split(' ').nth(4) == Some("/tmp"))
        .expect("a /tmp mount");
    assert!(line.contains(" - tmpfs "), "{line}");
    assert!(line.contains("size=65536k"), "{line}");
}

/// REQ: SEC-023
#[test]
fn sec_023_writable_scope_mounts_are_noexec_nosuid_nodev() {
    let dir = temp_dir("scope");
    let probe = applied(
        "mounts",
        &[("ILIUM_PROBE_WRITE", dir.display().to_string())],
    );
    let line = probe
        .all("mountinfo")
        .into_iter()
        .find(|l| l.split(' ').nth(4) == Some(dir.to_str().expect("utf8")))
        .expect("the granted scope is mounted");
    for flag in ["noexec", "nosuid", "nodev"] {
        assert!(
            line.split(' ')
                .nth(5)
                .unwrap_or("")
                .split(',')
                .any(|f| f == flag),
            "{flag} missing: {line}"
        );
    }
}

/// REQ: SEC-023
#[test]
fn sec_023_executing_a_binary_from_a_writable_scope_fails_with_eacces() {
    let dir = temp_dir("exec");
    let probe = applied(
        "mounts",
        &[("ILIUM_PROBE_WRITE", dir.display().to_string())],
    );
    assert_eq!(
        probe.one("exec"),
        Some(format!("{}=errno13", dir.display()))
    );
}

// ------------------------------------------------------------------ SEC-024

/// REQ: SEC-024
#[test]
fn sec_024_no_new_privs_is_set() {
    let probe = applied("privs", &[]);
    assert_eq!(
        probe.map("status").get("NoNewPrivs").map(String::as_str),
        Some("1")
    );
}

/// REQ: SEC-024
#[test]
fn sec_024_seccomp_filter_mode_is_installed() {
    let probe = applied("privs", &[]);
    assert_eq!(
        probe.map("status").get("Seccomp").map(String::as_str),
        Some("2")
    );
}

/// REQ: SEC-024
#[test]
fn sec_024_every_capability_set_is_empty() {
    let probe = applied("privs", &[]);
    let status = probe.map("status");
    for key in ["CapEff", "CapPrm", "CapBnd", "CapAmb"] {
        assert_eq!(
            status.get(key).map(String::as_str),
            Some("0000000000000000"),
            "{key}"
        );
    }
}

// ------------------------------------------------------------------ SEC-025

/// REQ: SEC-025
#[test]
fn sec_025_escape_probe_suite_all_attempts_are_denied() {
    let wasm = require_fixture("hostile-escape");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("hostile", &wasm), Ok(()));
    let (result, _) = run_call(
        &mut helper,
        1,
        "run-escape-suite",
        b"",
        10_000,
        15_000,
        |_| (Vec::new(), None),
    );
    assert_eq!(
        String::from_utf8_lossy(&result.expect("call result").result),
        "all-denied"
    );
}

/// REQ: SEC-025
#[test]
fn doc_sec_025_manual_states_defence_in_depth() {
    assert_page_has(PAGE, "The sandbox is defence in depth");
}

// ------------------------------------------------------------------ SEC-026

/// REQ: SEC-026
#[test]
fn sec_026_the_servers_proc_entry_does_not_exist() {
    let probe = applied("proc", &[]);
    assert_eq!(probe.one("outer_pid_visible").as_deref(), Some("false"));
}

/// REQ: SEC-026
#[test]
fn sec_026_the_servers_environ_cannot_be_read() {
    let probe = applied("proc", &[]);
    assert_eq!(
        probe.one("outer_environ_readable").as_deref(),
        Some("false")
    );
}

/// REQ: SEC-026
#[test]
fn sec_026_sys_class_is_absent() {
    let probe = applied("proc", &[]);
    assert_eq!(probe.one("sys_class_net").as_deref(), Some("false"));
}

// ------------------------------------------------------------------ SEC-027

const FORBIDDEN_ENV: [&str; 10] = [
    "SSH_AUTH_SOCK",
    "DBUS_SESSION_BUS_ADDRESS",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_RUNTIME_DIR",
    "HTTPS_PROXY",
    "http_proxy",
    "SERVICE_TOKEN",
    "SERVICE_KEY",
    "SERVICE_SECRET",
];
const ALLOWED_ENV: [&str; 5] = ["HOME", "TMPDIR", "LANG", "PATH", "ILIUM_PLUGIN_ID"];

fn polluted_env() -> Vec<(&'static str, String)> {
    FORBIDDEN_ENV
        .iter()
        .map(|k| (*k, "leak".to_string()))
        .collect()
}

/// REQ: SEC-027
#[test]
fn sec_027_forbidden_variables_are_absent() {
    let probe = applied("env", &polluted_env());
    let env = probe.map("env");
    for key in FORBIDDEN_ENV {
        assert!(!env.contains_key(key), "{key} leaked");
    }
}

/// REQ: SEC-027
#[test]
fn sec_027_only_allowlist_variables_remain() {
    let probe = applied("env", &polluted_env());
    for key in probe.map("env").keys() {
        assert!(
            ALLOWED_ENV.contains(&key.as_str()),
            "unexpected variable {key}"
        );
    }
}

/// REQ: SEC-027
#[test]
fn sec_027_fixed_values_of_tmpdir_lang_and_path() {
    let probe = applied("env", &polluted_env());
    let env = probe.map("env");
    assert_eq!(env.get("TMPDIR").map(String::as_str), Some("/tmp"));
    assert_eq!(env.get("LANG").map(String::as_str), Some("C.UTF-8"));
    assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
    assert!(env.contains_key("HOME"));
}

// ------------------------------------------------------------------ SEC-028

/// REQ: SEC-028
#[test]
fn sec_028_only_stdio_and_one_channel_descriptor_remain() {
    let probe = applied("fds", &[]);
    let numbers: Vec<u32> = probe
        .all("fd")
        .iter()
        .filter_map(|l| l.split_once('=').and_then(|(n, _)| n.parse().ok()))
        .collect();
    let extra: Vec<&u32> = numbers.iter().filter(|n| **n > 2).collect();
    assert_eq!(extra.len(), 1, "descriptors above 2: {extra:?}");
}

/// REQ: SEC-028
#[test]
fn sec_028_helper_has_its_own_session_and_no_controlling_terminal() {
    let probe = applied("fds", &[]);
    assert_eq!(probe.one("tty_nr").as_deref(), Some("0"));
    assert_eq!(probe.one("session"), probe.one("pid"));
}

/// REQ: SEC-028
#[test]
fn sec_028_terminal_injection_ioctls_fail() {
    let probe = applied("fds", &[]);
    for name in ["TIOCSTI", "TIOCLINUX", "TIOCSCTTY"] {
        let line = probe
            .all("ioctl")
            .into_iter()
            .find(|l| l.starts_with(name))
            .expect(name);
        let ret: i64 = line
            .split_once('=')
            .expect("value")
            .1
            .parse()
            .expect("number");
        assert!(ret < 0, "{name} returned {ret}");
    }
}

// ------------------------------------------------------------------ SEC-029

/// REQ: SEC-029
#[test]
fn doc_sec_029_manual_states_the_cgroup_limits() {
    assert_page_has(
        PAGE,
        "`memory.max` = plugin `memory_mib` plus 32 MiB overhead",
    );
    assert_page_has(PAGE, "`pids.max` = 64 plus 8 per allowed child");
    assert_page_has(PAGE, "`cpu.weight` = 10 (low priority)");
    assert_page_has(PAGE, "`cpu.max` = the manifest `cpu_ms_per_s` quota");
}

/// REQ: SEC-029
#[test]
fn sec_029_allocating_past_the_memory_limit_kills_the_helper() {
    let wasm = require_fixture("hostile-escape");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("hostile", &wasm), Ok(()));
    helper.send(&msg_call_export(1, "allocate-past-limit", b"", 10_000));
    let status = helper
        .wait_exit(15_000)
        .expect("helper must be killed at memory.max");
    // The cgroup OOM killer ends the helper with SIGKILL (a clean exit would not be the limit).
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(9),
        "{status:?}"
    );
}

/// REQ: SEC-029
#[test]
fn sec_029_fork_bomb_fails_with_eagain_and_helper_stays_responsive() {
    let wasm = require_fixture("hostile-escape");
    let mut helper = Helper::spawn();
    assert_eq!(helper.ready_with("hostile", &wasm), Ok(()));
    let (result, _) = run_call(&mut helper, 1, "fork-bomb", b"", 10_000, 15_000, |_| {
        (Vec::new(), None)
    });
    assert_eq!(
        String::from_utf8_lossy(&result.expect("call result").result),
        "EAGAIN"
    );
    assert!(helper.heartbeat_echo(3, 2_000));
}

// ------------------------------------------------------------------ SEC-055 to SEC-057

/// REQ: SEC-055
#[test]
fn sec_055_a_helper_that_serves_is_sandboxed_before_the_first_plugin_byte() {
    let mut helper = Helper::spawn();
    let served = helper.recv_tag(tag::HELLO, 5_000).is_some();
    if served {
        let status =
            std::fs::read_to_string(format!("/proc/{}/status", helper.pid())).unwrap_or_default();
        assert!(
            status.contains("NoNewPrivs:\t1"),
            "served without no_new_privs"
        );
        assert!(
            status.contains("Seccomp:\t2"),
            "served without a seccomp filter"
        );
    } else {
        // manual does not pin how the helper process reports `sandbox.unavailable` (the code
        // belongs to the host's load refusal); here it must end with a failure status.
        let exit = helper
            .wait_exit(3_000)
            .expect("an unavailable sandbox ends the helper, never serves unconfined");
        assert!(!exit.success());
    }
}

/// REQ: SEC-055
#[test]
fn doc_sec_055_manual_names_the_error_code_and_prerequisites() {
    assert_page_has(PAGE, "refuse to load with error code `sandbox.unavailable`");
    assert_page_has(
        PAGE,
        "There is no silent fallback to in-process or unconfined execution",
    );
    for needle in [
        "`user namespaces`",
        "`cgroup v2`",
        "`seccomp`",
        "`procfs mount`",
    ] {
        assert_page_has(PAGE, needle);
    }
}

/// REQ: SEC-056
#[test]
fn sec_056_root_user_never_gets_a_served_helper() {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let is_root = status
        .lines()
        .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(2) == Some("0"));
    let mut helper = Helper::spawn();
    let served = helper.recv_tag(tag::HELLO, 5_000).is_some();
    if is_root {
        assert!(
            !served,
            "running as root is not supported for third-party plugins"
        );
        let exit = helper.wait_exit(3_000).expect("a refused helper ends");
        assert!(!exit.success());
    } else if !served {
        // Without a served helper the sandbox is unavailable and the helper must have failed.
        let exit = helper.wait_exit(3_000).expect("an unserved helper ends");
        assert!(!exit.success());
    }
}

/// REQ: SEC-056
#[test]
fn doc_sec_056_manual_states_the_root_message() {
    assert_page_has(
        PAGE,
        "`running as root is not supported for third-party plugins`",
    );
}

/// REQ: SEC-057
#[test]
fn doc_sec_057_platform_matrix_rows() {
    for row in [
        "| Linux, native | Yes | Yes, with the prerequisites above |",
        "| WSL 1 | Yes | No |",
        "| macOS | Yes | No (decision D5) |",
        "| Windows | Yes | No (decision D5) |",
    ] {
        assert_page_has(PAGE, row);
    }
}

/// REQ: SEC-057
#[test]
fn sec_057_on_linux_the_helper_binary_starts() {
    assert!(cfg!(target_os = "linux"));
    let mut helper = Helper::spawn();
    helper.expect_hello();
}

// ------------------------------------------------------------------ SEC-065

/// REQ: SEC-065
#[test]
fn sec_065_without_cgroup_delegation_rlimits_bound_address_space_processes_and_cpu() {
    let probe = applied(
        "limits",
        &[("ILIUM_PLUGIN_FORCE_NO_CGROUP", "1".to_string())],
    );
    let limits = probe.all("limit");
    for name in ["Max address space", "Max processes", "Max cpu time"] {
        let line = limits
            .iter()
            .find(|l| l.starts_with(name))
            .unwrap_or_else(|| panic!("{name} missing"));
        assert!(!line.contains("unlimited"), "{name} is unlimited: {line}");
    }
}

/// REQ: SEC-065
#[test]
fn doc_sec_065_manual_states_setrlimit_fallback_and_audit_entry() {
    assert_page_has(
        PAGE,
        "`setrlimit` (`RLIMIT_AS`, `RLIMIT_NPROC`, `RLIMIT_CPU`)",
    );
    assert_page_has(
        PAGE,
        "records `sandbox-degraded` with the controller list once per start",
    );
}

// ------------------------------------------------------------------ SEC-067

fn syscall_results(group: &str) -> Vec<(String, i64)> {
    let probe = applied(&format!("syscalls:{group}"), &[]);
    probe
        .all("syscall")
        .into_iter()
        .map(|l| {
            let (name, ret) = l.split_once('=').expect("name=ret");
            (name.to_string(), ret.parse().expect("number"))
        })
        .collect()
}

fn assert_group_denied(group: &str) {
    let expected = SYSCALL_GROUPS
        .iter()
        .find(|(g, _)| *g == group)
        .expect("group")
        .1;
    let results = syscall_results(group);
    assert_eq!(
        results.len(),
        expected.len(),
        "the child was killed or skipped calls: {results:?}"
    );
    for (name, ret) in results {
        assert_eq!(ret, -1, "{name} must return EPERM");
    }
}

/// REQ: SEC-067
#[test]
fn sec_067_debugging_and_foreign_memory_calls_are_denied() {
    assert_group_denied("debug");
}

/// REQ: SEC-067
#[test]
fn sec_067_keys_and_kernel_object_calls_are_denied() {
    assert_group_denied("keys");
}

/// REQ: SEC-067
#[test]
fn sec_067_namespace_and_mount_calls_are_denied() {
    assert_group_denied("mounts");
}

/// REQ: SEC-067
#[test]
fn sec_067_module_and_boot_calls_are_denied() {
    assert_group_denied("modules");
}

/// REQ: SEC-067
#[test]
fn sec_067_socket_calls_are_denied() {
    assert_group_denied("sockets");
}

/// REQ: SEC-067
#[test]
fn sec_067_privilege_calls_are_denied() {
    assert_group_denied("privilege");
}

/// REQ: SEC-067
#[test]
fn doc_sec_067_every_denied_call_of_the_manual_is_in_the_probe_table() {
    let text = page(PAGE);
    let start = text
        .find("These calls are always denied")
        .expect("denied table");
    let table = &text[start..];
    let probe_names: Vec<&str> = SYSCALL_GROUPS
        .iter()
        .flat_map(|(_, calls)| calls.iter().map(|(n, _)| *n))
        .collect();
    for name in probe_names {
        assert!(
            table.contains(&format!("`{name}`")),
            "{name} not in the manual table"
        );
    }
    for name in [
        "ptrace",
        "process_vm_readv",
        "bpf",
        "io_uring_setup",
        "pivot_root",
        "reboot",
        "accept4",
        "personality",
    ] {
        assert!(
            SYSCALL_GROUPS
                .iter()
                .any(|(_, calls)| calls.iter().any(|(n, _)| *n == name)),
            "{name} missing from the probe table"
        );
    }
}

/// REQ: SEC-067
#[test]
fn doc_sec_067_manual_requires_a_published_generated_list() {
    assert_page_has(
        PAGE,
        "MUST publish the list as a generated file with the release",
    );
}
