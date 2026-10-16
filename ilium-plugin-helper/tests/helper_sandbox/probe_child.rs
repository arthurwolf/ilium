//! Observing the OS sandbox from the inside (reference-sandbox.md, SEC-020 to SEC-029, SEC-067).
//!
//! `apply` confines the calling process for good, so a probe runs in a re-executed copy of this
//! test binary: the parent calls [`run_probe`], the child runs [`sec_025_probe_child_entry`]
//! (which does nothing unless `ILIUM_PROBE` is set), applies the policy, performs the probe and
//! prints `key|value` lines on stdout. The parent parses the lines and asserts on them.

#![allow(dead_code)]

use ilium_plugin_helper::sandbox::{apply, SandboxPolicy};
use std::collections::BTreeMap;
use std::process::{Command, Stdio};

pub const ENTRY: &str = "probe_child::sec_025_probe_child_entry";

/// Result of one probe run: the printed `key|value` lines in order.
pub struct Probe {
    pub lines: Vec<(String, String)>,
    pub exit_ok: bool,
}

impl Probe {
    pub fn all(&self, key: &str) -> Vec<String> {
        self.lines.iter().filter(|(k, _)| k == key).map(|(_, v)| v.clone()).collect()
    }
    pub fn one(&self, key: &str) -> Option<String> {
        self.all(key).into_iter().next()
    }
    /// True when the sandbox was applied and the probe printed its `done` marker.
    pub fn completed(&self) -> bool {
        self.one("done").is_some() && self.one("apply_error").is_none()
    }
    pub fn map(&self, key: &str) -> BTreeMap<String, String> {
        self.all(key)
            .into_iter()
            .filter_map(|v| v.split_once('=').map(|(a, b)| (a.to_string(), b.to_string())))
            .collect()
    }
}

/// Re-executes this test binary as the probe child with extra environment variables.
pub fn run_probe(name: &str, extra_env: &[(&str, String)]) -> Probe {
    let exe = std::env::current_exe().expect("current_exe");
    let mut command = Command::new(exe);
    command
        .args(["--exact", ENTRY, "--nocapture", "--test-threads=1"])
        .env("ILIUM_PROBE", name)
        .env("ILIUM_PROBE_OUTER_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    for kind in ["user", "mnt", "pid", "ipc", "uts", "net"] {
        let link = std::fs::read_link(format!("/proc/self/ns/{kind}"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        command.env(format!("ILIUM_PROBE_PARENT_NS_{kind}"), link);
    }
    let output = command.output().expect("spawn probe child");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let lines = text
        .lines()
        .filter_map(|l| l.strip_prefix("PROBE|"))
        .filter_map(|l| l.split_once('|'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Probe { lines, exit_ok: output.status.success() }
}

pub fn parent_ns(kind: &str) -> String {
    std::fs::read_link(format!("/proc/self/ns/{kind}"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn out(key: &str, value: impl std::fmt::Display) {
    println!("PROBE|{key}|{value}");
}

#[cfg(target_arch = "x86_64")]
unsafe fn raw_syscall(number: i64, a: i64, b: i64, c: i64) -> i64 {
    let ret: i64;
    // SAFETY: the caller passes arguments that the kernel or the seccomp filter rejects.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") number => ret,
            in("rdi") a, in("rsi") b, in("rdx") c,
            lateout("rcx") _, lateout("r11") _,
            options(nostack)
        );
    }
    ret
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn raw_syscall(_number: i64, _a: i64, _b: i64, _c: i64) -> i64 {
    -38
}

/// Syscall groups of the always-denied table of SEC-067 with x86_64 numbers.
pub const SYSCALL_GROUPS: &[(&str, &[(&str, i64)])] = &[
    ("debug", &[("ptrace", 101), ("process_vm_readv", 310), ("process_vm_writev", 311), ("kcmp", 312)]),
    (
        "keys",
        &[
            ("keyctl", 250), ("add_key", 248), ("request_key", 249), ("bpf", 321),
            ("perf_event_open", 298), ("userfaultfd", 323), ("io_uring_setup", 425),
            ("io_uring_enter", 426), ("io_uring_register", 427),
        ],
    ),
    (
        "mounts",
        &[
            ("mount", 165), ("umount2", 166), ("pivot_root", 155), ("chroot", 161),
            ("unshare", 272), ("setns", 308), ("open_by_handle_at", 304),
        ],
    ),
    (
        "modules",
        &[
            ("init_module", 175), ("finit_module", 313), ("delete_module", 176),
            ("kexec_load", 246), ("kexec_file_load", 320), ("reboot", 169),
        ],
    ),
    (
        "sockets",
        &[
            ("socket", 41), ("socketpair", 53), ("connect", 42), ("bind", 49),
            ("listen", 50), ("accept", 43), ("accept4", 288),
        ],
    ),
    (
        "privilege",
        &[("setuid", 105), ("setgid", 106), ("setresuid", 117), ("capset", 126), ("personality", 135)],
    ),
];

pub const TIOCSTI: i64 = 0x5412;
pub const TIOCLINUX: i64 = 0x541C;
pub const TIOCSCTTY: i64 = 0x540E;
const SYS_IOCTL: i64 = 16;

/// Entry of the re-executed probe child. Without `ILIUM_PROBE` it returns at once.
///
/// REQ: SEC-025
#[test]
fn sec_025_probe_child_entry() {
    let Ok(name) = std::env::var("ILIUM_PROBE") else {
        return;
    };
    let mut policy = SandboxPolicy::default();
    if let Ok(paths) = std::env::var("ILIUM_PROBE_READ") {
        policy.read_paths = paths.split(':').filter(|p| !p.is_empty()).map(Into::into).collect();
    }
    if let Ok(paths) = std::env::var("ILIUM_PROBE_WRITE") {
        policy.write_paths = paths.split(':').filter(|p| !p.is_empty()).map(Into::into).collect();
    }
    if let Err(error) = apply(&policy) {
        out("apply_error", error);
        return;
    }
    match name.as_str() {
        "ns" => probe_ns(),
        "net" => probe_net(),
        "mounts" => probe_mounts(),
        "privs" => probe_privs(),
        "proc" => probe_proc(),
        "env" => probe_env(),
        "fds" => probe_fds(),
        "limits" => probe_limits(),
        "childstatus" => probe_child_status(),
        group if group.starts_with("syscalls:") => probe_syscalls(&group["syscalls:".len()..]),
        other => out("unknown_probe", other),
    }
    out("done", "1");
}

fn probe_ns() {
    for kind in ["user", "mnt", "pid", "ipc", "uts", "net"] {
        let link = std::fs::read_link(format!("/proc/self/ns/{kind}"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|e| format!("error:{e}"));
        out("ns", format!("{kind}={link}"));
        let parent = std::env::var(format!("ILIUM_PROBE_PARENT_NS_{kind}")).unwrap_or_default();
        out("parent_ns", format!("{kind}={parent}"));
    }
    if let Ok(link) = std::fs::read_link("/proc/self/ns/pid_for_children") {
        out("ns_pid_for_children", link.display());
    }
}

fn probe_net() {
    use std::net::{SocketAddr, TcpStream, UdpSocket};
    use std::time::Duration;
    let loopback: SocketAddr = "127.0.0.1:9".parse().expect("addr");
    let public: SocketAddr = "1.1.1.1:53".parse().expect("addr");
    out("tcp_loopback", TcpStream::connect_timeout(&loopback, Duration::from_millis(500)).is_ok());
    out("tcp_public", TcpStream::connect_timeout(&public, Duration::from_millis(500)).is_ok());
    out("udp_bind", UdpSocket::bind("127.0.0.1:0").is_ok());
    let devices = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    for line in devices.lines().skip(2) {
        if let Some((name, _)) = line.split_once(':') {
            out("interface", name.trim());
        }
    }
}

fn probe_mounts() {
    for entry in std::fs::read_dir("/").into_iter().flatten().flatten() {
        out("root_entry", entry.file_name().to_string_lossy());
    }
    for entry in std::fs::read_dir("/dev").into_iter().flatten().flatten() {
        out("dev_entry", entry.file_name().to_string_lossy());
    }
    out("sys_present", std::fs::read_dir("/sys").map(|mut d| d.next().is_some()).unwrap_or(false));
    for line in std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default().lines() {
        out("mountinfo", line);
    }
    for path in std::env::var("ILIUM_PROBE_WRITE").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
        let target = format!("{path}/probe-binary");
        let copied = std::fs::copy("/bin/true", &target);
        if copied.is_err() {
            out("exec", format!("{path}=copy-failed"));
            continue;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        match Command::new(&target).status() {
            Ok(_) => out("exec", format!("{path}=ran")),
            Err(e) => out("exec", format!("{path}=errno{}", e.raw_os_error().unwrap_or(-1))),
        }
    }
}

fn probe_privs() {
    for line in std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines() {
        if let Some((key, value)) = line.split_once(':') {
            if ["NoNewPrivs", "CapEff", "CapBnd", "CapPrm", "CapInh", "CapAmb", "Seccomp"].contains(&key) {
                out("status", format!("{key}={}", value.trim()));
            }
        }
    }
}

fn probe_proc() {
    let outer = std::env::var("ILIUM_PROBE_OUTER_PID").unwrap_or_default();
    out("outer_pid_visible", std::path::Path::new(&format!("/proc/{outer}")).exists());
    out(
        "outer_environ_readable",
        std::fs::read(format!("/proc/{outer}/environ")).is_ok(),
    );
    out("own_pid", std::process::id());
    let init_environ = std::fs::read("/proc/1/environ");
    out("proc1_environ", if init_environ.is_ok() { "readable" } else { "denied" });
    let pids: Vec<String> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.chars().all(|c| c.is_ascii_digit()))
        .collect();
    out("visible_pids", pids.len());
    out("sys_class_net", std::path::Path::new("/sys/class/net").exists());
}

fn probe_env() {
    for (key, value) in std::env::vars() {
        if key.starts_with("ILIUM_PROBE") || key.starts_with("RUST_") {
            continue;
        }
        out("env", format!("{key}={value}"));
    }
}

fn probe_fds() {
    let own = std::process::id();
    for entry in std::fs::read_dir("/proc/self/fd").into_iter().flatten().flatten() {
        let number = entry.file_name().to_string_lossy().into_owned();
        let target = std::fs::read_link(entry.path()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        if target == format!("/proc/{own}/fd") || target.ends_with("/fd") {
            continue;
        }
        out("fd", format!("{number}={target}"));
    }
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    if let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r.trim().to_string()) {
        let fields: Vec<&str> = rest.split(' ').collect();
        out("session", fields.get(3).copied().unwrap_or("?"));
        out("tty_nr", fields.get(4).copied().unwrap_or("?"));
    }
    out("pid", own);
    let mut byte = 0u8;
    let ptr = &mut byte as *mut u8 as i64;
    for (name, request) in [("TIOCSTI", TIOCSTI), ("TIOCLINUX", TIOCLINUX), ("TIOCSCTTY", TIOCSCTTY)] {
        // SAFETY: a plain ioctl with a valid pointer argument; the sandbox denies it.
        let ret = unsafe { raw_syscall(SYS_IOCTL, 0, request, ptr) };
        out("ioctl", format!("{name}={ret}"));
    }
}

fn probe_limits() {
    for line in std::fs::read_to_string("/proc/self/limits").unwrap_or_default().lines() {
        out("limit", line);
    }
}

fn probe_syscalls(group: &str) {
    let Some((_, calls)) = SYSCALL_GROUPS.iter().find(|(name, _)| *name == group) else {
        out("unknown_group", group);
        return;
    };
    for (name, number) in calls.iter() {
        // SAFETY: every call passes null or invalid arguments, so even an unfiltered kernel
        // rejects it with EFAULT, EINVAL or EBADF; the sandbox must answer EPERM instead.
        let ret = unsafe { raw_syscall(*number, 0, 0, 0) };
        out("syscall", format!("{name}={ret}"));
    }
    if group == "privilege" || group == "sockets" {
        // setuid(0) style calls with arguments 0 are harmless for an unprivileged user.
    }
}

/// A child of the confined process (SEC-032): its status shows the same confinement.
fn probe_child_status() {
    let output = Command::new("/bin/sh")
        .args(["-c", "grep -E '^(NoNewPrivs|Seccomp|CapEff):' /proc/self/status"])
        .output();
    match output {
        Ok(output) => {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some((key, value)) = line.split_once(':') {
                    out("child_status", format!("{key}={}", value.trim()));
                }
            }
        }
        Err(error) => out("child_error", error),
    }
    let connect = Command::new("/bin/sh")
        .args(["-c", "exec 3<>/dev/tcp/127.0.0.1/9"])
        .status();
    out("child_net_ok", connect.map(|s| s.success()).unwrap_or(false));
}
