//! Custody declarations for CPAL 0.18.1's default hosts used by Ilium.
//! These are source-qualified declarations, not CPAL API guarantees or
//! measured native memory/thread upper bounds. Requalify if the backend changes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeStreamCustody {
    /// Known Rust threads spawned by a duplex input/output stream pair.
    /// Native OS audio service/callback threads can be additional and opaque.
    pub declared_threads: usize,
    /// The inspected backend synchronously joins its Rust stream threads
    /// when both stream handles are dropped. This says nothing about OS pools.
    pub stream_drop_joins: bool,
    pub backend: &'static str,
}

pub fn native_stream_custody() -> NativeStreamCustody {
    if cfg!(target_os = "linux") {
        // cpal host/alsa/mod.rs:1311,1354 spawn; :1379 joins in Stream::drop.
        NativeStreamCustody {
            declared_threads: 2,
            stream_drop_joins: true,
            backend: "CPAL 0.18.1 default ALSA",
        }
    } else if cfg!(windows) {
        // cpal host/wasapi/stream.rs:357,427 spawn; :467 joins in Stream::drop.
        NativeStreamCustody {
            declared_threads: 2,
            stream_drop_joins: true,
            backend: "CPAL 0.18.1 default WASAPI",
        }
    } else if cfg!(target_os = "macos") {
        // Input DisconnectManager starts two monitors. Output selects either
        // DisconnectManager or DefaultOutputMonitor, each two, not both.
        // Their JoinHandles are discarded (coreaudio/macos/mod.rs:132,181,
        // :72,247; device.rs:804,916). AudioUnit callback threads are opaque.
        NativeStreamCustody {
            declared_threads: 4,
            stream_drop_joins: false,
            backend: "CPAL 0.18.1 CoreAudio monitors; native callbacks opaque",
        }
    } else {
        // Preserve the backend's existing availability without inventing a
        // thread count or claiming its uninspected destruction proves exit.
        NativeStreamCustody {
            declared_threads: 0,
            stream_drop_joins: false,
            backend: "unqualified CPAL default host",
        }
    }
}

/// A Linux Pulse source observed by the native host. The source index and name
/// are kept together; a name-only configured default is never accepted.
#[derive(Debug, Clone)]
pub struct SelectedPulseSource {
    endpoint: String,
    identity: String,
    helper_program: std::path::PathBuf,
}
impl SelectedPulseSource {
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn helper_program(&self) -> &std::path::Path {
        &self.helper_program
    }
}

/// Called in an admitted dedicated I/O job after explicit user selection.
/// This inspects the audio server's source inventory; it opens no PCM stream.
/// The capture owner must call it again immediately before the actual open.
pub fn qualify_selected_pulse_source(
    endpoint: &str,
    expect_monitor: bool,
) -> std::io::Result<SelectedPulseSource> {
    #[cfg(target_os = "linux")]
    {
        linux_selected_pulse_source(endpoint, expect_monitor)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (endpoint, expect_monitor);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no qualified selected native capture adapter for this OS",
        ))
    }
}

/// Host feature availability only; no endpoint or live PCM is implied.
pub fn selected_pulse_capture_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        trusted_linux_audio_program("pactl").is_ok() && trusted_linux_audio_program("parec").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn linux_selected_pulse_source(
    endpoint: &str,
    expect_monitor: bool,
) -> std::io::Result<SelectedPulseSource> {
    use std::{
        io::{self, Read},
        os::fd::AsRawFd,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // A failed poll/read must not detach a child whose source query may still
    // be running. This guard remains in the admitted IO preparation job.
    struct ReapOnDrop(std::process::Child);
    impl Drop for ReapOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    if endpoint.is_empty()
        || endpoint.len() > 256
        || !endpoint.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid selected audio source name",
        ));
    }
    let query = trusted_linux_audio_program("pactl")?;
    let helper_program = trusted_linux_audio_program("parec")?;
    let mut child = ReapOnDrop(
        Command::new(query)
            .args(["--format=json", "list", "sources"])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = child
        .0
        .stdout
        .as_ref()
        .ok_or_else(|| io::Error::other("audio source enumeration stdout missing"))?;
    let descriptor = stdout.as_raw_fd();
    // Read while the process runs. Waiting before reading can deadlock when
    // a valid JSON inventory fills the pipe, falsely timing out a live host.
    // SAFETY: stdout remains owned by child for both calls; only its pipe
    // status flags change, and this accepted job has no other pipe reader.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut inventory = Vec::with_capacity(64 * 1024);
    let mut scratch = [0u8; 8192];
    let mut eof = false;
    let status = loop {
        while !eof {
            let stdout = child
                .0
                .stdout
                .as_mut()
                .ok_or_else(|| io::Error::other("audio source enumeration stdout missing"))?;
            match stdout.read(&mut scratch) {
                Ok(0) => eof = true,
                Ok(count) => {
                    if inventory.len().saturating_add(count) > 64 * 1024 {
                        return Err(io::Error::other("audio source enumeration exceeded 64 KiB"));
                    }
                    inventory.extend_from_slice(&scratch[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        if let Some(status) = child.0.try_wait()? {
            if eof {
                break status;
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "audio source enumeration deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(io::Error::other("audio source enumeration failed"));
    }
    let identity = selected_pulse_source_identity(&inventory, endpoint, expect_monitor)?;
    Ok(SelectedPulseSource {
        endpoint: endpoint.to_owned(),
        identity,
        helper_program,
    })
}

#[cfg(target_os = "linux")]
fn trusted_linux_audio_program(name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::{
        io,
        os::unix::fs::{MetadataExt, PermissionsExt},
        path::PathBuf,
    };
    let candidate = PathBuf::from("/usr/bin").join(name);
    let canonical = candidate.canonicalize()?;
    let metadata = canonical.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "audio helper is not a trusted installed executable",
        ));
    }
    Ok(candidate)
}

#[cfg(target_os = "linux")]
fn selected_pulse_source_identity(
    inventory: &[u8],
    selected: &str,
    expect_monitor: bool,
) -> std::io::Result<String> {
    use std::io;
    let values: serde_json::Value = serde_json::from_slice(inventory)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let sources = values.as_array().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "audio source inventory is not an array",
        )
    })?;
    if sources.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "audio source inventory exceeds bound",
        ));
    }
    let mut matches = Vec::new();
    for source in sources {
        if source.get("name").and_then(serde_json::Value::as_str) != Some(selected) {
            continue;
        }
        let monitor_source = source
            .get("monitor_source")
            .and_then(serde_json::Value::as_str);
        let properties = source
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "selected audio source has no properties",
                )
            })?;
        let media_class = properties
            .get("media.class")
            .and_then(serde_json::Value::as_str);
        let monitor_class = properties
            .get("device.class")
            .and_then(serde_json::Value::as_str);
        let matches_kind = if expect_monitor {
            monitor_source.is_some_and(|name| !name.is_empty())
                && media_class == Some("Audio/Sink")
                && monitor_class == Some("monitor")
        } else {
            monitor_source == Some("")
                && media_class == Some("Audio/Source")
                && monitor_class != Some("monitor")
        };
        if !matches_kind {
            continue;
        }
        let index = source
            .get("index")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "selected audio source has no index",
                )
            })?;
        let serial = properties
            .get("object.serial")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "selected audio source has no serial",
                )
            })?;
        let object_id = properties
            .get("object.id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "selected audio source has no object id",
                )
            })?;
        if serial.len() > 32
            || object_id.len() > 32
            || !serial.bytes().all(|byte| byte.is_ascii_digit())
            || !object_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid selected audio source identity",
            ));
        }
        matches.push(format!("{index}:{selected}:{serial}:{object_id}"));
    }
    if matches.len() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "selected audio endpoint absent, ambiguous, or wrong capture kind",
        ));
    }
    Ok(matches.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_distinguish_inspected_join_from_opaque_native_ownership() {
        let policy = native_stream_custody();
        if cfg!(any(target_os = "linux", windows)) {
            assert_eq!(policy.declared_threads, 2);
            assert!(policy.stream_drop_joins);
        } else if cfg!(target_os = "macos") {
            assert_eq!(policy.declared_threads, 4);
            assert!(!policy.stream_drop_joins);
        } else {
            assert_eq!(policy.declared_threads, 0);
            assert!(!policy.stream_drop_joins);
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn selected_pulse_source_requires_exact_kind_name_and_unique_index() {
        let monitor = serde_json::json!({"index":5,"name":"sink.monitor","monitor_source":"sink","properties":{"media.class":"Audio/Sink","device.class":"monitor","object.serial":"5","object.id":"2"}});
        let microphone = serde_json::json!({"index":8,"name":"mic","monitor_source":"","properties":{"media.class":"Audio/Source","device.class":"sound","object.serial":"8","object.id":"3"}});
        let listing = serde_json::to_vec(&vec![monitor.clone(), microphone.clone()]).unwrap();
        assert_eq!(
            selected_pulse_source_identity(&listing, "sink.monitor", true).unwrap(),
            "5:sink.monitor:5:2"
        );
        assert_eq!(
            selected_pulse_source_identity(&listing, "mic", false).unwrap(),
            "8:mic:8:3"
        );
        assert!(selected_pulse_source_identity(&listing, "sink.monitor", false).is_err());
        assert!(selected_pulse_source_identity(&listing, "missing", true).is_err());
        let duplicate = serde_json::to_vec(&vec![microphone.clone(), microphone]).unwrap();
        assert!(selected_pulse_source_identity(&duplicate, "mic", false).is_err());
    }
}
