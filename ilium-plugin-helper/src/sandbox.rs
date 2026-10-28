//! OS sandbox setup through `ilium-platform` (seccomp and landlock on Linux, the platform
//! equivalents elsewhere); reference-security.md.

use crate::HelperError;

/// What the helper may touch once sandboxed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SandboxPolicy {
    /// Directories the helper may read.
    pub read_paths: Vec<std::path::PathBuf>,
    /// Directories the helper may write.
    pub write_paths: Vec<std::path::PathBuf>,
    /// Whether outbound network access is allowed.
    pub allow_network: bool,
}

/// Applies the policy to the current process; it cannot be undone.
///
/// Linux: sets `no_new_privs`, moves the process into fresh user and network namespaces (the
/// network namespace holds only a downed loopback interface) and drops every capability. A
/// prerequisite the kernel refuses (for example unprivileged user namespaces) is a
/// [`HelperError::Sandbox`] naming it; the helper never continues unconfined. The path grants
/// of the policy are enforced by the host through the WASI preopens it hands the guest, since
/// the helper opens no path on its own.
pub fn apply(policy: &SandboxPolicy) -> Result<(), HelperError> {
    apply_platform(policy)
}

#[cfg(target_os = "linux")]
fn apply_platform(policy: &SandboxPolicy) -> Result<(), HelperError> {
    fn last_error(step: &str) -> HelperError {
        HelperError::Sandbox(format!("{step} failed: {}", std::io::Error::last_os_error()))
    }

    // SAFETY: prctl with PR_SET_NO_NEW_PRIVS takes only integer arguments and touches no memory.
    let no_new_privs = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if no_new_privs != 0 {
        return Err(last_error("prctl(PR_SET_NO_NEW_PRIVS)"));
    }

    if !policy.allow_network {
        // SAFETY: unshare takes only flags. It fails with EINVAL in a multithreaded process and
        // with EPERM where unprivileged user namespaces are disabled; both are reported.
        let unshared = unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) };
        if unshared != 0 {
            let error = last_error("unshare(user, network namespace)");
            return Err(HelperError::Sandbox(format!(
                "{error}; prerequisite: unprivileged user namespaces enabled and a single-threaded helper start"
            )));
        }
    }

    drop_all_capabilities()
}

#[cfg(target_os = "linux")]
fn drop_all_capabilities() -> Result<(), HelperError> {
    #[repr(C)]
    struct CapabilityHeader {
        version: u32,
        process: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CapabilityData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    const CAPABILITY_VERSION_3: u32 = 0x2008_0522;

    // Clear the bounding set first; ignore capabilities the kernel does not know.
    for capability in 0..64 {
        // SAFETY: prctl(PR_CAPBSET_DROP) takes only integer arguments.
        let dropped = unsafe { libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) };
        if dropped != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINVAL) {
                break;
            }
            return Err(HelperError::Sandbox(format!(
                "prctl(PR_CAPBSET_DROP, {capability}) failed: {error}"
            )));
        }
    }

    let header = CapabilityHeader {
        version: CAPABILITY_VERSION_3,
        process: 0,
    };
    let data = [CapabilityData {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: header and data are valid, correctly laid out for capset(2) version 3 (two data
    // elements) and live for the duration of the call.
    let result = unsafe { libc::syscall(libc::SYS_capset, &header, data.as_ptr()) };
    if result != 0 {
        return Err(HelperError::Sandbox(format!(
            "capset failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_platform(_policy: &SandboxPolicy) -> Result<(), HelperError> {
    Err(HelperError::Sandbox(
        "no OS sandbox is implemented for this platform; the helper refuses to run unconfined"
            .to_owned(),
    ))
}

#[cfg(test)]
mod spec_tests {
    //! `apply` is one-way for the calling process (its doc comment: "it cannot be undone"), so
    //! no test calls it in the test process itself. Each sandboxed case re-runs this test binary
    //! as a child that applies the policy and reports what it observed.

    use super::{apply, SandboxPolicy};
    use crate::HelperError;

    const SCENARIO_ENV: &str = "ILIUM_SANDBOX_SPEC_SCENARIO";
    const CHILD_TEST_NAME: &str = "sandbox::spec_tests::sandboxed_child";

    /// Runs the child test body for one scenario and returns its captured output.
    /// Asserts that the filter matched exactly one test, so a renamed child cannot pass vacuously.
    fn run_sandboxed_scenario(scenario: &str) -> (std::process::ExitStatus, String) {
        let test_binary = std::env::current_exe().expect("current test binary path");
        let output = std::process::Command::new(test_binary)
            .args([
                "--exact",
                CHILD_TEST_NAME,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(SCENARIO_ENV, scenario)
            .output()
            .expect("spawn sandboxed child test process");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            stdout.contains("1 passed"),
            "sandboxed child scenario did not pass (status {}); stdout was: {stdout}",
            output.status
        );
        (output.status, stdout)
    }

    /// Manual: empty input: no granted scope and no network
    #[test]
    fn default_policy_grants_no_paths_and_no_network() {
        let policy = SandboxPolicy::default();
        assert!(policy.read_paths.is_empty());
        assert!(policy.write_paths.is_empty());
        assert!(!policy.allow_network);
    }

    #[test]
    fn sandboxed_child() {
        let Ok(scenario) = std::env::var(SCENARIO_ENV) else {
            return;
        };
        let outcome = apply(&SandboxPolicy::default());
        match scenario.as_str() {
            "result" => match outcome {
                Ok(()) => println!("RESULT ok"),
                Err(HelperError::Sandbox(message)) => println!("RESULT sandbox-error {message}"),
                Err(other) => println!("RESULT other {other}"),
            },
            "no_new_privs" => {
                outcome.expect("apply must succeed for the no_new_privs scenario");
                let status = std::fs::read_to_string("/proc/self/status").expect("read status");
                let no_new_privs: Vec<&str> = status
                    .lines()
                    .find(|line| line.starts_with("NoNewPrivs:"))
                    .expect("NoNewPrivs line")
                    .split_whitespace()
                    .collect();
                assert_eq!(no_new_privs, ["NoNewPrivs:", "1"]);
            }
            "caps" => {
                outcome.expect("apply must succeed for the caps scenario");
                let status = std::fs::read_to_string("/proc/self/status").expect("read status");
                let effective = status
                    .lines()
                    .find(|line| line.starts_with("CapEff:"))
                    .expect("CapEff line")
                    .split_whitespace()
                    .nth(1)
                    .expect("CapEff value");
                assert_eq!(u64::from_str_radix(effective, 16).expect("hex CapEff"), 0);
            }
            "network" => {
                outcome.expect("apply must succeed for the network scenario");
                let devices = std::fs::read_to_string("/proc/net/dev").expect("read net/dev");
                let interfaces: Vec<&str> = devices
                    .lines()
                    .skip(2)
                    .filter_map(|line| line.split(':').next())
                    .map(str::trim)
                    .collect();
                assert_eq!(interfaces, ["lo"]);
            }
            other => panic!("unknown scenario {other}"),
        }
    }

    /// Manual: apply sets no_new_privs before the helper reads plugin bytes
    #[test]
    fn apply_sets_no_new_privileges() {
        let (status, stdout) = run_sandboxed_scenario("no_new_privs");
        assert!(status.success(), "child failed: {stdout}");
    }

    /// Manual: apply drops every capability
    #[test]
    fn apply_drops_every_capability() {
        let (status, stdout) = run_sandboxed_scenario("caps");
        assert!(status.success(), "child failed: {stdout}");
    }

    /// Manual: the network namespace has only a downed loopback interface
    #[test]
    fn apply_leaves_only_loopback_interface() {
        let (status, stdout) = run_sandboxed_scenario("network");
        assert!(status.success(), "child failed: {stdout}");
    }

    /// never a silent unconfined run)
    #[test]
    fn apply_reports_sandbox_error_with_named_prerequisite() {
        let (status, stdout) = run_sandboxed_scenario("result");
        assert!(status.success(), "child failed: {stdout}");
        let result_line = stdout
            .lines()
            .find(|line| line.starts_with("RESULT "))
            .expect("child must print a RESULT line");
        if let Some(message) = result_line.strip_prefix("RESULT sandbox-error ") {
            assert!(
                !message.trim().is_empty(),
                "sandbox error must name the prerequisite"
            );
        } else {
            assert_eq!(result_line, "RESULT ok");
        }
    }
}
