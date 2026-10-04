//! Resource limits installed before an isolated helper executes any code.
use std::{io, process::Command};

/// Linux enforces address-space admission before exec, including allocations
/// advertised by an external clipboard owner. Other implementations must not
/// claim a bound until an equivalent job/pre-exec mechanism is verified.
#[cfg(target_os = "linux")]
pub fn configure_child_address_space_limit(command: &mut Command, bytes: usize) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let bytes = libc::rlim_t::try_from(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "child address-space limit is not representable",
        )
    })?;
    if bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child address-space limit must be nonzero",
        ));
    }
    // SAFETY: only async-signal-safe setrlimit calls run between fork and exec;
    // numeric values were checked before spawning, no allocation/logging here.
    unsafe {
        command.pre_exec(move || {
            let memory = libc::rlimit {
                rlim_cur: bytes,
                rlim_max: bytes,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0 {
                return Err(io::Error::last_os_error());
            }
            let core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn configure_child_address_space_limit(
    _command: &mut Command,
    _bytes: usize,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "pre-exec clipboard address-space limits are not implemented on this platform",
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn limit_is_installed_before_helper_code_runs() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "ulimit -v"]);
        configure_child_address_space_limit(&mut command, 64 * 1024 * 1024).unwrap();
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "65536");
    }
    #[test]
    fn zero_limit_is_rejected_before_spawn() {
        assert_eq!(
            configure_child_address_space_limit(&mut Command::new("/bin/sh"), 0)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

/// Validate the inherited bound before initializing a native library. A hidden
/// helper invoked directly must not claim its parent installed a memory limit.
#[cfg(target_os = "linux")]
pub fn verify_current_address_space_limit(bytes: usize) -> std::io::Result<()> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes only the initialized, correctly sized structure.
    if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let expected = libc::rlim_t::try_from(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "address space limit overflow",
        )
    })?;
    if limit.rlim_cur > expected || limit.rlim_max > expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "clipboard helper requires an inherited hard address-space limit",
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn verify_current_address_space_limit(_bytes: usize) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "hard child address-space limits are unavailable on this platform",
    ))
}
