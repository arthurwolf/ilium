//! Session-wide limits shared by every plugin of a session (reference-errors-and-limits.md HST-40x).

/// The session limits after applying the user's settings (reference-errors-and-limits.md HST-406).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLimits {
    /// Memory of every plugin instance together, in MiB.
    pub session_memory_mib: u64,
    /// Helper processes.
    pub session_helpers: u64,
    /// Child processes.
    pub session_processes: u64,
    /// Open HTTP streams.
    pub session_http_streams: u64,
    /// Decorations.
    pub session_decorations: u64,
    /// Pending requests.
    pub session_pending_requests: u64,
    /// Key grabs.
    pub session_key_grabs: u64,
    /// Virtual children.
    pub session_children: u64,
}

impl Default for SessionLimits {
    /// The documented defaults.
    fn default() -> SessionLimits {
        SessionLimits {
            session_memory_mib: 1_024,
            session_helpers: 16,
            session_processes: 16,
            session_http_streams: 64,
            session_decorations: 65_536,
            session_pending_requests: 4_096,
            session_key_grabs: 32,
            // Assumption: the manual gives no number for virtual children per session.
            session_children: 256,
        }
    }
}

impl SessionLimits {
    /// The defaults overridden by the user's `(key, value)` settings; higher values are clamped.
    pub fn from_user(settings: &[(&str, u64)]) -> SessionLimits {
        let mut limits = SessionLimits::default();
        for (key, value) in settings {
            // Zero means "not set"; values above the default are clamped to it (HST-406, LIF-143).
            if *value == 0 {
                continue;
            }
            let field = match *key {
                "session_memory_mib" => &mut limits.session_memory_mib,
                "session_helpers" => &mut limits.session_helpers,
                "session_processes" => &mut limits.session_processes,
                "session_http_streams" => &mut limits.session_http_streams,
                "session_decorations" => &mut limits.session_decorations,
                _ => continue,
            };
            *field = (*field).min(*value);
        }
        limits
    }
}

#[cfg(test)]
mod tests {
    use super::SessionLimits;

    /// One session key: its name, the getter of its field and its documented default.
    type SessionKeyRow = (&'static str, fn(&SessionLimits) -> u64, u64);

    /// The five session keys a user may set, with the getter of each field and its documented default.
    fn session_key_table() -> [SessionKeyRow; 5] {
        [
            (
                "session_memory_mib",
                |limits| limits.session_memory_mib,
                1_024,
            ),
            ("session_helpers", |limits| limits.session_helpers, 16),
            ("session_processes", |limits| limits.session_processes, 16),
            (
                "session_http_streams",
                |limits| limits.session_http_streams,
                64,
            ),
            (
                "session_decorations",
                |limits| limits.session_decorations,
                65_536,
            ),
        ]
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_session_defaults_match_the_documented_values() {
        let defaults = SessionLimits::default();
        assert_eq!(
            (
                defaults.session_memory_mib,
                defaults.session_helpers,
                defaults.session_processes,
                defaults.session_http_streams,
                defaults.session_decorations,
            ),
            (1_024, 16, 16, 64, 65_536)
        );
    }

    /// REQ: API-070
    #[test]
    fn api_070_fixed_caps_pending_requests_and_key_grabs_have_the_documented_defaults() {
        let defaults = SessionLimits::default();
        assert_eq!(
            (
                defaults.session_pending_requests,
                defaults.session_key_grabs
            ),
            (4_096, 32)
        );
    }

    /// REQ: HST-497
    #[test]
    fn hst_497_session_processes_default_allows_sixteen_children() {
        assert_eq!(SessionLimits::default().session_processes, 16);
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_empty_user_settings_return_the_defaults() {
        assert_eq!(SessionLimits::from_user(&[]), SessionLimits::default());
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_user_value_lower_than_the_default_is_applied_for_session_memory_mib() {
        let limits = SessionLimits::from_user(&[("session_memory_mib", 512)]);
        assert_eq!(limits.session_memory_mib, 512);
        assert_eq!(limits.session_helpers, 16);
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_lower_user_value_is_applied_for_each_of_the_five_session_keys() {
        let lowered = [8_u64, 4, 32, 1_000, 512];
        for ((key, read, _default), value) in session_key_table().into_iter().zip(lowered) {
            let limits = SessionLimits::from_user(&[(key, value)]);
            assert_eq!(read(&limits), value, "key {key}");
        }
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_user_value_equal_to_the_default_keeps_the_default_for_each_key() {
        for (key, read, default) in session_key_table() {
            let limits = SessionLimits::from_user(&[(key, default)]);
            assert_eq!(read(&limits), default, "key {key}");
        }
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_user_value_higher_than_the_default_is_clamped_for_each_key() {
        for (key, read, default) in session_key_table() {
            let higher = SessionLimits::from_user(&[(key, default + 1)]);
            let maximum = SessionLimits::from_user(&[(key, u64::MAX)]);
            assert_eq!(read(&higher), default, "key {key}, default + 1");
            assert_eq!(read(&maximum), default, "key {key}, u64::MAX");
        }
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_clamping_one_key_does_not_change_a_lower_value_for_another() {
        let limits =
            SessionLimits::from_user(&[("session_memory_mib", 99_999), ("session_helpers", 8)]);
        assert_eq!(limits.session_memory_mib, 1_024);
        assert_eq!(limits.session_helpers, 8);
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_several_lower_session_keys_in_one_call_apply_together() {
        let limits = SessionLimits::from_user(&[
            ("session_memory_mib", 512),
            ("session_helpers", 8),
            ("session_processes", 4),
            ("session_http_streams", 32),
            ("session_decorations", 1_000),
        ]);
        assert_eq!(
            (
                limits.session_memory_mib,
                limits.session_helpers,
                limits.session_processes,
                limits.session_http_streams,
                limits.session_decorations,
            ),
            (512, 8, 4, 32, 1_000)
        );
    }

    /// REQ: LIF-143
    #[test]
    fn lif_143_zero_user_value_means_not_set_and_keeps_the_default() {
        for (key, read, default) in session_key_table() {
            let limits = SessionLimits::from_user(&[(key, 0)]);
            assert_eq!(read(&limits), default, "key {key}");
        }
    }

    /// REQ: LIF-143
    #[test]
    fn lif_143_smallest_nonzero_user_value_is_applied_for_each_key() {
        for (key, read, _default) in session_key_table() {
            let limits = SessionLimits::from_user(&[(key, 1)]);
            assert_eq!(read(&limits), 1, "key {key}");
        }
    }

    /// REQ: API-070
    #[test]
    fn api_070_fixed_caps_are_not_user_settable() {
        let limits =
            SessionLimits::from_user(&[("session_pending_requests", 1), ("session_key_grabs", 1)]);
        assert_eq!(limits.session_pending_requests, 4_096);
        assert_eq!(limits.session_key_grabs, 32);
    }

    /// REQ: HST-406
    #[test]
    fn hst_406_keys_that_are_not_session_keys_are_ignored() {
        let limits = SessionLimits::from_user(&[("not_a_session_key", 1), ("", 1)]);
        assert_eq!(limits, SessionLimits::default());
    }
}
