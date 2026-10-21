//! Spawning and talking to `ilium-plugin-helper` for T2 and T3 plugins (reference-host-architecture.md).

use crate::error::HostError;

/// A connection to one helper process.
#[derive(Debug)]
pub struct HelperClient;

impl HelperClient {
    /// Spawns the helper through `ilium-platform` and waits for `Ready`.
    pub fn spawn(_helper_path: std::path::PathBuf) -> Result<HelperClient, HostError> {
        unimplemented!("host::helper_client::HelperClient::spawn")
    }

    /// Sends one framed request and waits for the reply.
    pub fn request(&mut self, _payload: &[u8]) -> Result<Vec<u8>, HostError> {
        unimplemented!("host::helper_client::HelperClient::request")
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
        let mut client = HelperClient;

        let outcome = client.request(&[0x01, 0x02, 0x03]);

        assert!(outcome.is_err(), "request to a helper that is not running must not return a reply");
    }

    /// REQ: HST-169
    #[test]
    fn request_with_empty_payload_without_a_running_helper_returns_an_error() {
        let mut client = HelperClient;

        let outcome = client.request(&[]);

        assert!(outcome.is_err(), "empty request to a helper that is not running must not return a reply");
    }
}
