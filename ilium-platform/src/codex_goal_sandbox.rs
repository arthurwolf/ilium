//! Owned Linux sandbox process for the bounded Codex SQLite query.
//!
//! Launch is only a containment mechanism. A caller must still verify process
//! and transcript identity, bound the pipes and deadline, validate the response,
//! and check the pinned files before accepting an observation.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::codex_goal_storage::PinnedCodexGoalFiles;

pub struct CodexGoalSandbox {
    child: Child,
    files: PinnedCodexGoalFiles,
    reaped: bool,
}

impl CodexGoalSandbox {
    pub fn spawn(provider_home: &Path) -> io::Result<Self> {
        Self::spawn_program(provider_home, include_str!("codex_goal_query.py"))
    }

    fn spawn_program(provider_home: &Path, program: &str) -> io::Result<Self> {
        let files = PinnedCodexGoalFiles::open(provider_home)?;
        let descriptors = files.descriptors();
        let mut command = Command::new("/usr/bin/bwrap");
        command.env_clear().args([
            "--unshare-user",
            "--unshare-ipc",
            "--unshare-pid",
            "--unshare-net",
            "--unshare-uts",
            "--unshare-cgroup",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "--setenv",
            "LANG",
            "C",
            "--ro-bind",
            "/usr",
            "/usr",
            "--dir",
            "/goal",
            "--tmpfs",
            "/tmp",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ]);
        for library_root in ["/lib", "/lib64"] {
            if Path::new(library_root).exists() {
                command.args(["--ro-bind", library_root, library_root]);
            }
        }
        for (descriptor, name) in
            descriptors
                .iter()
                .zip(["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"])
        {
            command
                .arg("--ro-bind-fd")
                .arg(descriptor.to_string())
                .arg(format!("/goal/{name}"));
        }
        command
            .args([
                "--chdir",
                "/tmp",
                "--",
                "/usr/bin/python3",
                "-I",
                "-c",
                program,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: the forked closure uses only async-signal-safe syscalls and
        // the three borrowed descriptors remain open until spawn returns.
        unsafe {
            command.pre_exec(move || {
                let core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                let cpu = libc::rlimit {
                    rlim_cur: 5,
                    rlim_max: 5,
                };
                let memory = libc::rlimit {
                    rlim_cur: 512 * 1024 * 1024,
                    rlim_max: 512 * 1024 * 1024,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0
                    || libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0
                    || libc::setrlimit(libc::RLIMIT_AS, &memory) != 0
                    || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 4_u32) != 0
                {
                    return Err(io::Error::last_os_error());
                }
                // Only the three O_RDONLY, inode-pinned files may cross exec.
                for descriptor in descriptors {
                    if libc::fcntl(descriptor, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            child,
            files,
            reaped: false,
        })
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Run one short request on an I/O worker. A timeout or error drops and
    /// reaps the sandbox, and no output is accepted after a file identity change.
    pub fn query(mut self, request: &[u8], timeout: Duration) -> io::Result<Vec<u8>> {
        const MAX_REQUEST: usize = 1024;
        const MAX_OUTPUT: usize = 8 * 1024 * 1024;
        if request.len() > MAX_REQUEST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "goal request too large",
            ));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid goal deadline"))?;
        let mut input = self
            .take_stdin()
            .ok_or_else(|| io::Error::other("missing goal input"))?;
        input.write_all(request)?;
        drop(input);
        let mut stdout = self
            .take_stdout()
            .ok_or_else(|| io::Error::other("missing goal output"))?;
        let mut output = Vec::new();
        let mut eof = false;
        let mut exit = None;
        loop {
            if exit.is_none() {
                exit = self.try_wait()?;
            }
            if eof && exit.is_some() {
                break;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "goal query timed out"))?;
            let wait_ms = remaining.as_millis().clamp(1, 50) as i32;
            let mut poll_descriptor = libc::pollfd {
                fd: if eof { -1 } else { stdout.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            };
            // The descriptor is owned by stdout for the entire poll call.
            let ready = unsafe { libc::poll(&mut poll_descriptor, 1, wait_ms) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if ready == 0 {
                continue;
            }
            if poll_descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(io::Error::other("goal output pipe failed"));
            }
            if poll_descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                let mut buffer = [0; 8192];
                let count = stdout.read(&mut buffer)?;
                if count == 0 {
                    eof = true;
                    continue;
                }
                if output.len().saturating_add(count) > MAX_OUTPUT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "goal output too large",
                    ));
                }
                output.extend_from_slice(&buffer[..count]);
            }
        }
        if !exit.is_some_and(|status| status.success()) {
            return Err(io::Error::other("goal query failed"));
        }
        self.files_unchanged()?;
        Ok(output)
    }

    /// Call only on the owned I/O worker after the child has exited. A caller
    /// must apply a deadline and drop this object on timeout or cancellation.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.reaped = true;
        }
        Ok(status)
    }

    /// Blocking wait for a caller that owns an independently enforced deadline.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    pub fn files_unchanged(&mut self) -> io::Result<()> {
        if !self.reaped {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "query has not exited",
            ));
        }
        self.files.unchanged()
    }
}

impl Drop for CodexGoalSandbox {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    fn query_fixture(program: &str) -> (tempfile::TempDir, CodexGoalSandbox) {
        let directory = tempfile::tempdir().unwrap();
        for name in ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"] {
            std::fs::write(directory.path().join(name), b"fixture").unwrap();
        }
        let sandbox = CodexGoalSandbox::spawn_program(directory.path(), program).unwrap();
        (directory, sandbox)
    }

    #[test]
    fn sandbox_query_enforces_deadline_and_returns_complete_output() {
        let (_directory, sandbox) = query_fixture("import time; time.sleep(30)");
        let started = Instant::now();
        let error = sandbox
            .query(b"{}", Duration::from_millis(250))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));

        let (_directory, sandbox) = query_fixture("import sys; print(sys.stdin.read())");
        assert_eq!(
            sandbox.query(b"synthetic", Duration::from_secs(5)).unwrap(),
            b"synthetic\n"
        );
    }

    #[test]
    fn sandbox_query_rejects_oversized_output() {
        let (_directory, sandbox) =
            query_fixture("import sys; sys.stdout.write('x' * (8 * 1024 * 1024 + 1))");
        let error = sandbox.query(b"{}", Duration::from_secs(10)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn dropping_sandbox_stops_its_payload_and_descendant() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"] {
            std::fs::write(directory.path().join(name), b"fixture").unwrap();
        }
        let program = r#"
import os, time
os.fork()
print('ready', flush=True)
time.sleep(30)
"#;
        let mut sandbox = CodexGoalSandbox::spawn_program(directory.path(), program).unwrap();
        let mut output = BufReader::new(sandbox.take_stdout().unwrap());
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        assert_eq!(ready.trim(), "ready");
        drop(sandbox);
        let mut descriptor = libc::pollfd {
            fd: output.get_ref().as_raw_fd(),
            events: libc::POLLHUP,
            revents: 0,
        };
        // The process tree must close the pipe; bound the test even if it does not.
        let observed = unsafe { libc::poll(&mut descriptor, 1, 5000) };
        assert_eq!(observed, 1);
        assert_ne!(descriptor.revents & libc::POLLHUP, 0);
    }

    #[test]
    fn confined_query_cannot_use_an_inherited_writable_descriptor() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"] {
            std::fs::write(directory.path().join(name), b"fixture").unwrap();
        }
        let sentinel_path = directory.path().join("sentinel");
        let sentinel = std::fs::File::create(&sentinel_path).unwrap();
        // Use a high descriptor so the sandbox payload cannot confuse a reused
        // standard stream or startup descriptor with the inherited handle.
        let raw = unsafe { libc::fcntl(sentinel.as_raw_fd(), libc::F_DUPFD, 1000) };
        assert!(raw >= 1000);
        let inherited = unsafe { OwnedFd::from_raw_fd(raw) };
        let program = format!(
            r#"
import errno, os
try:
    os.write({raw}, b'changed')
except OSError as error:
    assert error.errno == errno.EBADF
else:
    raise AssertionError('writable descriptor survived exec')
print('blocked')
"#
        );
        let mut sandbox = CodexGoalSandbox::spawn_program(directory.path(), &program).unwrap();
        drop(inherited);
        let mut output = String::new();
        sandbox
            .take_stdout()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        assert!(sandbox.wait().unwrap().success());
        sandbox.files_unchanged().unwrap();
        assert_eq!(output.trim(), "blocked");
        assert_eq!(std::fs::read(sentinel_path).unwrap(), b"");
    }

    #[test]
    fn confined_goal_query_reads_wal_without_changing_provider_files() {
        let directory = tempfile::tempdir().unwrap();
        let writer_script = r#"
import os, sqlite3, sys
d = sqlite3.connect(os.path.join(sys.argv[1], 'goals_1.sqlite'))
d.execute('pragma journal_mode=wal')
d.execute('create table _sqlx_migrations (version integer, success integer, checksum blob)')
d.execute('insert into _sqlx_migrations values (1, 1, ?)', (b'synthetic',))
d.execute('create table thread_goals (thread_id text primary key, status text, objective text)')
d.execute('insert into thread_goals values (?, ?, ?)', ('thread', 'paused', 'synthetic objective'))
d.commit()
print('ready', flush=True)
sys.stdin.read()
"#;
        let mut writer = Command::new("python3")
            .args(["-u", "-c", writer_script])
            .arg(directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(writer.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        let names = ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"];
        let before: Vec<_> = names
            .iter()
            .map(|name| std::fs::read(directory.path().join(name)).unwrap())
            .collect();
        let times: Vec<_> = names
            .iter()
            .map(|name| {
                let metadata = std::fs::metadata(directory.path().join(name)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let mut sandbox = CodexGoalSandbox::spawn(directory.path()).unwrap();
        assert_eq!(
            sandbox.files_unchanged().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        sandbox
            .take_stdin()
            .unwrap()
            .write_all(br#"{"thread_id":"thread"}"#)
            .unwrap();
        let mut output = Vec::new();
        sandbox
            .take_stdout()
            .unwrap()
            .read_to_end(&mut output)
            .unwrap();
        assert!(sandbox.wait().unwrap().success());
        sandbox.files_unchanged().unwrap();
        let result: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            result["rows"],
            serde_json::json!([["thread", "paused", "synthetic objective"]])
        );
        let after_times: Vec<_> = names
            .iter()
            .map(|name| {
                let metadata = std::fs::metadata(directory.path().join(name)).unwrap();
                (metadata.accessed().ok(), metadata.modified().ok())
            })
            .collect();
        let after: Vec<_> = names
            .iter()
            .map(|name| std::fs::read(directory.path().join(name)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert_eq!(times, after_times);
        drop(writer.stdin.take());
        assert!(writer.wait().unwrap().success());
    }
}
