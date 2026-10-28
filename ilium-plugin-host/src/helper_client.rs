//! Spawning and talking to `ilium-plugin-helper` for T2 and T3 plugins (reference-host-architecture.md).
//!
//! Frames on the channel are a 4-byte big-endian length followed by the payload. The first frame
//! the helper sends is its `Hello`; a missing `Hello` within 5,000 ms is `helper.spawn-failed`
//! (HST-154). After the helper dies every request completes as an error (HST-169).

use std::borrow::Cow;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use ilium_plugin_api::{Code, Diagnostic};

use crate::error::HostError;

/// Longest time the helper may take to send `Hello` (HST-154).
const HELLO_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Largest frame the host accepts from the helper.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// A running helper process with its channel.
#[derive(Debug)]
struct Running {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

/// A connection to one helper process.
#[derive(Debug)]
pub struct HelperClient {
    running: Option<Running>,
}

fn spawn_failed(step: &str, detail: impl std::fmt::Display) -> HostError {
    HostError::Diagnostic(Diagnostic::error(
        Code(Cow::Borrowed("helper.spawn-failed")),
        "",
        format!("{step}: {detail}"),
    ))
}

fn cancelled(detail: &str) -> HostError {
    HostError::Diagnostic(Diagnostic::error(
        Code(Cow::Borrowed("helper.cancelled")),
        "",
        detail.to_string(),
    ))
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> std::io::Result<()> {
    let length = u32::try_from(payload.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn read_frame(reader: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut length_bytes = [0_u8; 4];
    reader.read_exact(&mut length_bytes)?;
    let length = u32::from_be_bytes(length_bytes) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame length above the message cap",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

impl HelperClient {
    /// A client without a helper; every request fails (HST-169).
    pub fn disconnected() -> HelperClient {
        HelperClient { running: None }
    }

    /// Spawns the helper through `ilium-platform` and waits for `Ready`.
    pub fn spawn(helper_path: std::path::PathBuf) -> Result<HelperClient, HostError> {
        if helper_path.as_os_str().is_empty() || !helper_path.exists() {
            return Err(HostError::HelperMissing);
        }
        let mut child = Command::new(&helper_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| spawn_failed("exec", error))?;
        let (Some(input), Some(mut output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(spawn_failed("channel", "the helper pipes are missing"));
        };
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let hello = read_frame(&mut output);
            let _ = sender.send(hello.map(|_| output));
        });
        let outcome = receiver.recv_timeout(HELLO_TIMEOUT);
        match outcome {
            Ok(Ok(output)) => {
                let _ = reader.join();
                Ok(HelperClient {
                    running: Some(Running {
                        child,
                        input,
                        output,
                    }),
                })
            }
            Ok(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                Err(spawn_failed("hello", error))
            }
            Err(_) => {
                // Killing the helper closes its stdout, which ends the reader thread.
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                Err(spawn_failed("hello", "no Hello within 5000 ms"))
            }
        }
    }

    /// Sends one framed request and waits for the reply.
    pub fn request(&mut self, payload: &[u8]) -> Result<Vec<u8>, HostError> {
        let Some(running) = self.running.as_mut() else {
            return Err(cancelled("the helper is not running"));
        };
        let outcome = write_frame(&mut running.input, payload)
            .and_then(|()| read_frame(&mut running.output));
        match outcome {
            Ok(reply) => Ok(reply),
            Err(error) => {
                if let Some(mut dead) = self.running.take() {
                    let _ = dead.child.kill();
                    let _ = dead.child.wait();
                }
                Err(cancelled(&format!("the helper channel failed: {error}")))
            }
        }
    }
}

impl Drop for HelperClient {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HelperClient;
    use crate::error::HostError;
    use ilium_plugin_api::Severity;
    use std::path::PathBuf;

    /// Runs `spawn` on a path that must fail and returns the error, failing the test if it succeeds.
    fn spawn_error(helper_path: PathBuf) -> HostError {
        match HelperClient::spawn(helper_path) {
            Ok(_) => panic!("spawn unexpectedly returned a helper client"),
            Err(host_error) => host_error,
        }
    }

    /// REQ: HST-151
    #[test]
    fn spawn_with_missing_helper_binary_reports_helper_missing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let helper_path = directory.path().join("ilium-plugin-helper");

        let host_error = spawn_error(helper_path);

        assert!(matches!(host_error, HostError::HelperMissing));
        assert_eq!(host_error.code().0, "host.helper-missing");
    }

    /// REQ: HST-151
    #[test]
    fn spawn_with_empty_path_reports_helper_missing() {
        let host_error = spawn_error(PathBuf::new());

        assert!(matches!(host_error, HostError::HelperMissing));
        assert_eq!(host_error.code().0, "host.helper-missing");
    }

    /// REQ: HST-154
    #[test]
    fn spawn_that_never_sends_hello_reports_spawn_failed_diagnostic() {
        // A regular file without the execute bit fails at the exec step, before any `Hello` can arrive.
        let directory = tempfile::tempdir().expect("temporary directory");
        let helper_path = directory.path().join("ilium-plugin-helper");
        std::fs::write(&helper_path, b"not a program").expect("write fake helper file");

        let host_error = spawn_error(helper_path);

        match host_error {
            HostError::Diagnostic(diagnostic) => {
                assert_eq!(diagnostic.code.0, "helper.spawn-failed");
                assert_eq!(diagnostic.severity, Severity::Error);
            }
            other => panic!("expected a helper.spawn-failed diagnostic, got {other:?}"),
        }
    }

    /// REQ: HST-169
    #[test]
    fn request_without_a_running_helper_returns_an_error() {
        let mut client = HelperClient::disconnected();

        let outcome = client.request(&[0x01, 0x02, 0x03]);

        assert!(
            outcome.is_err(),
            "request to a helper that is not running must not return a reply"
        );
    }

    /// REQ: HST-169
    #[test]
    fn request_with_empty_payload_without_a_running_helper_returns_an_error() {
        let mut client = HelperClient::disconnected();

        let outcome = client.request(&[]);

        assert!(
            outcome.is_err(),
            "empty request to a helper that is not running must not return a reply"
        );
    }
}
