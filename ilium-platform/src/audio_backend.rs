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
}
