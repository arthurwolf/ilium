//! Requestable limits, defaults and tier ceilings (reference-errors-and-limits.md
//! "Requestable limits", reference-manifest.md PKG-031).
//!
//! The five session-wide budgets (`session_memory_mib`, `session_helpers`, `session_processes`,
//! `session_http_streams`, `session_decorations`) are host settings, not manifest keys, so they
//! are not a [`LimitKey`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Code, UnknownName};
use crate::manifest::Diagnostic;
use crate::tier::Tier;

/// One key of the manifest `limits` object, in the order of the manual table
/// (reference-errors-and-limits.md "Requestable limits").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LimitKey {
    /// `memory_mib` (reference-errors-and-limits.md "Requestable limits").
    MemoryMib,
    /// `table_elements` (reference-errors-and-limits.md "Requestable limits").
    TableElements,
    /// `event_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    EventDeadlineMs,
    /// `command_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    CommandDeadlineMs,
    /// `render_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    RenderDeadlineMs,
    /// `annotate_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    AnnotateDeadlineMs,
    /// `frame_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    FrameDeadlineMs,
    /// `detector_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    DetectorDeadlineMs,
    /// `activate_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    ActivateDeadlineMs,
    /// `deactivate_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    DeactivateDeadlineMs,
    /// `migrate_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    MigrateDeadlineMs,
    /// `job_deadline_ms` (reference-errors-and-limits.md "Requestable limits").
    JobDeadlineMs,
    /// `job_wall_ms` (reference-errors-and-limits.md "Requestable limits").
    JobWallMs,
    /// `job_memory_mib` (reference-errors-and-limits.md "Requestable limits").
    JobMemoryMib,
    /// `job_io_mib` (reference-errors-and-limits.md "Requestable limits").
    JobIoMib,
    /// `concurrent_jobs` (reference-errors-and-limits.md "Requestable limits").
    ConcurrentJobs,
    /// `job_queue` (reference-errors-and-limits.md "Requestable limits").
    JobQueue,
    /// `event_queue` (reference-errors-and-limits.md "Requestable limits").
    EventQueue,
    /// `pending_requests` (reference-errors-and-limits.md "Requestable limits").
    PendingRequests,
    /// `timers` (reference-errors-and-limits.md "Requestable limits").
    Timers,
    /// `output_filters` (reference-errors-and-limits.md "Requestable limits").
    OutputFilters,
    /// `watches` (reference-errors-and-limits.md "Requestable limits").
    Watches,
    /// `key_grabs` (reference-errors-and-limits.md "Requestable limits").
    KeyGrabs,
    /// `ui_tree_nodes` (reference-errors-and-limits.md "Requestable limits").
    UiTreeNodes,
    /// `ui_tree_kib` (reference-errors-and-limits.md "Requestable limits").
    UiTreeKib,
    /// `decorations` (reference-errors-and-limits.md "Requestable limits").
    Decorations,
    /// `node_metadata_kib` (reference-errors-and-limits.md "Requestable limits").
    NodeMetadataKib,
    /// `storage_bucket_mib` (reference-errors-and-limits.md "Requestable limits").
    StorageBucketMib,
    /// `disk_mib` (reference-errors-and-limits.md "Requestable limits").
    DiskMib,
    /// `fs_read_all_mib` (reference-errors-and-limits.md "Requestable limits").
    FsReadAllMib,
    /// `fs_write_mib` (reference-errors-and-limits.md "Requestable limits").
    FsWriteMib,
    /// `http_response_mib` (reference-errors-and-limits.md "Requestable limits").
    HttpResponseMib,
    /// `process_output_mib` (reference-errors-and-limits.md "Requestable limits").
    ProcessOutputMib,
    /// `log_lines_per_second` (reference-errors-and-limits.md "Requestable limits").
    LogLinesPerSecond,
    /// `open_videos` (reference-errors-and-limits.md "Requestable limits").
    OpenVideos,
    /// `video_buffer_mib` (reference-errors-and-limits.md "Requestable limits").
    VideoBufferMib,
    /// `audio_captures` (reference-errors-and-limits.md "Requestable limits").
    AudioCaptures,
    /// `http_streams` (reference-errors-and-limits.md "Requestable limits").
    HttpStreams,
    /// `children` (reference-errors-and-limits.md "Requestable limits").
    Children,
    /// `durable_timers` (reference-errors-and-limits.md "Requestable limits").
    DurableTimers,
    /// `vector_indexes` (reference-errors-and-limits.md "Requestable limits").
    VectorIndexes,
    /// `vector_entries` (reference-errors-and-limits.md "Requestable limits").
    VectorEntries,
    /// `shared_cache_mib` (reference-errors-and-limits.md "Requestable limits").
    SharedCacheMib,
    /// `journal_entries` (reference-errors-and-limits.md "Requestable limits").
    JournalEntries,
    /// `toasts_pending` (reference-errors-and-limits.md "Requestable limits").
    ToastsPending,
    /// `package_assets_mib` (reference-errors-and-limits.md "Requestable limits").
    PackageAssetsMib,
}

impl LimitKey {
    /// Every key in table order (reference-errors-and-limits.md "Requestable limits").
    pub const ALL: [LimitKey; 46] = [
        LimitKey::MemoryMib,
        LimitKey::TableElements,
        LimitKey::EventDeadlineMs,
        LimitKey::CommandDeadlineMs,
        LimitKey::RenderDeadlineMs,
        LimitKey::AnnotateDeadlineMs,
        LimitKey::FrameDeadlineMs,
        LimitKey::DetectorDeadlineMs,
        LimitKey::ActivateDeadlineMs,
        LimitKey::DeactivateDeadlineMs,
        LimitKey::MigrateDeadlineMs,
        LimitKey::JobDeadlineMs,
        LimitKey::JobWallMs,
        LimitKey::JobMemoryMib,
        LimitKey::JobIoMib,
        LimitKey::ConcurrentJobs,
        LimitKey::JobQueue,
        LimitKey::EventQueue,
        LimitKey::PendingRequests,
        LimitKey::Timers,
        LimitKey::OutputFilters,
        LimitKey::Watches,
        LimitKey::KeyGrabs,
        LimitKey::UiTreeNodes,
        LimitKey::UiTreeKib,
        LimitKey::Decorations,
        LimitKey::NodeMetadataKib,
        LimitKey::StorageBucketMib,
        LimitKey::DiskMib,
        LimitKey::FsReadAllMib,
        LimitKey::FsWriteMib,
        LimitKey::HttpResponseMib,
        LimitKey::ProcessOutputMib,
        LimitKey::LogLinesPerSecond,
        LimitKey::OpenVideos,
        LimitKey::VideoBufferMib,
        LimitKey::AudioCaptures,
        LimitKey::HttpStreams,
        LimitKey::Children,
        LimitKey::DurableTimers,
        LimitKey::VectorIndexes,
        LimitKey::VectorEntries,
        LimitKey::SharedCacheMib,
        LimitKey::JournalEntries,
        LimitKey::ToastsPending,
        LimitKey::PackageAssetsMib,
    ];

    /// The manifest key, for example `memory_mib` (reference-manifest.md PKG-031).
    pub fn as_key(self) -> &'static str {
        match self {
            LimitKey::MemoryMib => "memory_mib",
            LimitKey::TableElements => "table_elements",
            LimitKey::EventDeadlineMs => "event_deadline_ms",
            LimitKey::CommandDeadlineMs => "command_deadline_ms",
            LimitKey::RenderDeadlineMs => "render_deadline_ms",
            LimitKey::AnnotateDeadlineMs => "annotate_deadline_ms",
            LimitKey::FrameDeadlineMs => "frame_deadline_ms",
            LimitKey::DetectorDeadlineMs => "detector_deadline_ms",
            LimitKey::ActivateDeadlineMs => "activate_deadline_ms",
            LimitKey::DeactivateDeadlineMs => "deactivate_deadline_ms",
            LimitKey::MigrateDeadlineMs => "migrate_deadline_ms",
            LimitKey::JobDeadlineMs => "job_deadline_ms",
            LimitKey::JobWallMs => "job_wall_ms",
            LimitKey::JobMemoryMib => "job_memory_mib",
            LimitKey::JobIoMib => "job_io_mib",
            LimitKey::ConcurrentJobs => "concurrent_jobs",
            LimitKey::JobQueue => "job_queue",
            LimitKey::EventQueue => "event_queue",
            LimitKey::PendingRequests => "pending_requests",
            LimitKey::Timers => "timers",
            LimitKey::OutputFilters => "output_filters",
            LimitKey::Watches => "watches",
            LimitKey::KeyGrabs => "key_grabs",
            LimitKey::UiTreeNodes => "ui_tree_nodes",
            LimitKey::UiTreeKib => "ui_tree_kib",
            LimitKey::Decorations => "decorations",
            LimitKey::NodeMetadataKib => "node_metadata_kib",
            LimitKey::StorageBucketMib => "storage_bucket_mib",
            LimitKey::DiskMib => "disk_mib",
            LimitKey::FsReadAllMib => "fs_read_all_mib",
            LimitKey::FsWriteMib => "fs_write_mib",
            LimitKey::HttpResponseMib => "http_response_mib",
            LimitKey::ProcessOutputMib => "process_output_mib",
            LimitKey::LogLinesPerSecond => "log_lines_per_second",
            LimitKey::OpenVideos => "open_videos",
            LimitKey::VideoBufferMib => "video_buffer_mib",
            LimitKey::AudioCaptures => "audio_captures",
            LimitKey::HttpStreams => "http_streams",
            LimitKey::Children => "children",
            LimitKey::DurableTimers => "durable_timers",
            LimitKey::VectorIndexes => "vector_indexes",
            LimitKey::VectorEntries => "vector_entries",
            LimitKey::SharedCacheMib => "shared_cache_mib",
            LimitKey::JournalEntries => "journal_entries",
            LimitKey::ToastsPending => "toasts_pending",
            LimitKey::PackageAssetsMib => "package_assets_mib",
        }
    }
}

impl std::fmt::Display for LimitKey {
    /// The manifest key (reference-manifest.md PKG-031).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_key())
    }
}

impl std::str::FromStr for LimitKey {
    type Err = UnknownName;

    /// Parses a manifest limit key (reference-manifest.md PKG-031).
    fn from_str(text: &str) -> Result<LimitKey, UnknownName> {
        LimitKey::ALL
            .into_iter()
            .find(|key| key.as_key() == text)
            .ok_or_else(|| UnknownName::new("limit key", text))
    }
}

/// The `limits` object of a manifest as written: key to requested value (reference-manifest.md PKG-031).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Limits(pub BTreeMap<String, u64>);

/// Defaults and tier ceilings from reference-manifest.md "Limits object".
fn table_values(key: LimitKey) -> (u64, u64, u64) {
    match key {
        LimitKey::MemoryMib => (32, 256, 128),
        LimitKey::TableElements => (10000, 50000, 20000),
        LimitKey::EventDeadlineMs => (20, 50, 20),
        LimitKey::CommandDeadlineMs => (50, 100, 50),
        LimitKey::RenderDeadlineMs => (10, 20, 10),
        LimitKey::AnnotateDeadlineMs => (10, 20, 10),
        LimitKey::FrameDeadlineMs => (33, 50, 50),
        LimitKey::DetectorDeadlineMs => (20, 50, 20),
        LimitKey::ActivateDeadlineMs => (2000, 5000, 2000),
        LimitKey::DeactivateDeadlineMs => (500, 1000, 500),
        LimitKey::MigrateDeadlineMs => (30000, 60000, 30000),
        LimitKey::JobDeadlineMs => (2000, 60000, 10000),
        LimitKey::JobWallMs => (20000, 600000, 100000),
        LimitKey::JobMemoryMib => (64, 512, 256),
        LimitKey::JobIoMib => (16, 64, 16),
        LimitKey::ConcurrentJobs => (2, 8, 2),
        LimitKey::JobQueue => (16, 64, 16),
        LimitKey::EventQueue => (1024, 4096, 1024),
        LimitKey::PendingRequests => (64, 256, 64),
        LimitKey::Timers => (64, 256, 64),
        LimitKey::OutputFilters => (32, 128, 32),
        LimitKey::Watches => (64, 256, 64),
        LimitKey::KeyGrabs => (8, 32, 8),
        LimitKey::UiTreeNodes => (4096, 16384, 4096),
        LimitKey::UiTreeKib => (1024, 4096, 1024),
        LimitKey::Decorations => (4096, 16384, 4096),
        LimitKey::NodeMetadataKib => (64, 256, 64),
        LimitKey::StorageBucketMib => (16, 256, 16),
        LimitKey::DiskMib => (256, 4096, 256),
        LimitKey::FsReadAllMib => (64, 512, 64),
        LimitKey::FsWriteMib => (16, 64, 16),
        LimitKey::HttpResponseMib => (8, 64, 8),
        LimitKey::ProcessOutputMib => (4, 32, 4),
        LimitKey::LogLinesPerSecond => (200, 1000, 200),
        LimitKey::OpenVideos => (2, 8, 2),
        LimitKey::VideoBufferMib => (64, 256, 64),
        LimitKey::AudioCaptures => (2, 4, 2),
        LimitKey::HttpStreams => (4, 16, 4),
        LimitKey::Children => (2, 8, 2),
        LimitKey::DurableTimers => (64, 256, 64),
        LimitKey::VectorIndexes => (4, 16, 4),
        LimitKey::VectorEntries => (10000, 200000, 50000),
        LimitKey::SharedCacheMib => (64, 256, 128),
        LimitKey::JournalEntries => (5000, 20000, 5000),
        LimitKey::ToastsPending => (3, 3, 3),
        LimitKey::PackageAssetsMib => (64, 256, 64),
    }
}

/// The host default of a limit (reference-errors-and-limits.md "Requestable limits").
///
/// The frame default is the documented 30 fps baseline (33 ms). The runtime must
/// resolve an unset frame deadline to `1000 / effective_frame_rate`, rounded down,
/// and cap that budget at the tier ceiling before scheduling a scene frame.
pub fn default_of(key: LimitKey) -> u64 {
    table_values(key).0
}

/// The ceiling of a limit for a tier (reference-errors-and-limits.md "Requestable limits").
pub fn ceiling(key: LimitKey, tier: Tier) -> u64 {
    let (_, first_party, third_party) = table_values(key);
    match tier {
        Tier::T1 => first_party,
        Tier::T2 | Tier::T3 => third_party,
    }
}

/// PKG-031: a request above the ceiling is clamped, never refused.
///
/// Returns every requestable key, including defaults for absent or zero requests.
/// Unknown keys and values outside the manifest's unsigned 32-bit range remain
/// schema errors (PKG-070, PKG-072). The caller owns logging raised requests once;
/// this pure contract module has no logging side effects.
pub fn clamp(requested: &Limits, tier: Tier) -> Result<BTreeMap<LimitKey, u64>, Diagnostic> {
    let mut effective: BTreeMap<_, _> = LimitKey::ALL
        .into_iter()
        .map(|key| (key, default_of(key).min(ceiling(key, tier))))
        .collect();
    for (name, &value) in &requested.0 {
        let pointer = format!("/limits/{}", name.replace('~', "~0").replace('/', "~1"));
        let key = name.parse::<LimitKey>().map_err(|_| {
            Diagnostic::error(
                Code::from_static("manifest.unknown-key"),
                &pointer,
                format!("Unknown limit key: {name}"),
            )
        })?;
        if value > u64::from(u32::MAX) {
            return Err(Diagnostic::error(
                Code::from_static("manifest.schema"),
                pointer,
                "Limit values must be integers from 0 to 4,294,967,295",
            ));
        }
        if value != 0 {
            effective.insert(key, value.min(ceiling(key, tier)));
        }
    }
    Ok(effective)
}

/// ABI-068: the refusal code of the table row for `key`.
pub fn row_refusal_code(_key: LimitKey) -> &'static str {
    // Every row of the size table is refused with the one ABI code for a decoded value over its
    // limit (reference-errors-and-limits.md, ABI-063).
    "abi.limit-exceeded"
}

#[cfg(test)]
mod tests {
    use super::{ceiling, clamp, default_of, LimitKey, Limits};
    use crate::error::Severity;
    use crate::tier::Tier;

    /// The Limits object of reference-manifest.md, in manual order. `None` default means the
    /// manual gives no single number (`frame_deadline_ms` defaults to the frame budget).
    const MANUAL_LIMITS: [(LimitKey, Option<u64>, u64, u64); 46] = [
        (LimitKey::MemoryMib, Some(32), 256, 128),
        (LimitKey::TableElements, Some(10_000), 50_000, 20_000),
        (LimitKey::EventDeadlineMs, Some(20), 50, 20),
        (LimitKey::CommandDeadlineMs, Some(50), 100, 50),
        (LimitKey::RenderDeadlineMs, Some(10), 20, 10),
        (LimitKey::AnnotateDeadlineMs, Some(10), 20, 10),
        (LimitKey::FrameDeadlineMs, None, 50, 50),
        (LimitKey::DetectorDeadlineMs, Some(20), 50, 20),
        (LimitKey::ActivateDeadlineMs, Some(2_000), 5_000, 2_000),
        (LimitKey::DeactivateDeadlineMs, Some(500), 1_000, 500),
        (LimitKey::MigrateDeadlineMs, Some(30_000), 60_000, 30_000),
        (LimitKey::JobDeadlineMs, Some(2_000), 60_000, 10_000),
        (LimitKey::JobWallMs, Some(20_000), 600_000, 100_000),
        (LimitKey::JobMemoryMib, Some(64), 512, 256),
        (LimitKey::JobIoMib, Some(16), 64, 16),
        (LimitKey::ConcurrentJobs, Some(2), 8, 2),
        (LimitKey::JobQueue, Some(16), 64, 16),
        (LimitKey::EventQueue, Some(1_024), 4_096, 1_024),
        (LimitKey::PendingRequests, Some(64), 256, 64),
        (LimitKey::Timers, Some(64), 256, 64),
        (LimitKey::OutputFilters, Some(32), 128, 32),
        (LimitKey::Watches, Some(64), 256, 64),
        (LimitKey::KeyGrabs, Some(8), 32, 8),
        (LimitKey::UiTreeNodes, Some(4_096), 16_384, 4_096),
        (LimitKey::UiTreeKib, Some(1_024), 4_096, 1_024),
        (LimitKey::Decorations, Some(4_096), 16_384, 4_096),
        (LimitKey::NodeMetadataKib, Some(64), 256, 64),
        (LimitKey::StorageBucketMib, Some(16), 256, 16),
        (LimitKey::DiskMib, Some(256), 4_096, 256),
        (LimitKey::FsReadAllMib, Some(64), 512, 64),
        (LimitKey::FsWriteMib, Some(16), 64, 16),
        (LimitKey::HttpResponseMib, Some(8), 64, 8),
        (LimitKey::ProcessOutputMib, Some(4), 32, 4),
        (LimitKey::LogLinesPerSecond, Some(200), 1_000, 200),
        (LimitKey::OpenVideos, Some(2), 8, 2),
        (LimitKey::VideoBufferMib, Some(64), 256, 64),
        (LimitKey::AudioCaptures, Some(2), 4, 2),
        (LimitKey::HttpStreams, Some(4), 16, 4),
        (LimitKey::Children, Some(2), 8, 2),
        (LimitKey::DurableTimers, Some(64), 256, 64),
        (LimitKey::VectorIndexes, Some(4), 16, 4),
        (LimitKey::VectorEntries, Some(10_000), 200_000, 50_000),
        (LimitKey::SharedCacheMib, Some(64), 256, 128),
        (LimitKey::JournalEntries, Some(5_000), 20_000, 5_000),
        (LimitKey::ToastsPending, Some(3), 3, 3),
        (LimitKey::PackageAssetsMib, Some(64), 256, 64),
    ];

    /// Manifest key names in manual order (reference-manifest.md "Limits object").
    const MANUAL_KEY_NAMES: [&str; 46] = [
        "memory_mib",
        "table_elements",
        "event_deadline_ms",
        "command_deadline_ms",
        "render_deadline_ms",
        "annotate_deadline_ms",
        "frame_deadline_ms",
        "detector_deadline_ms",
        "activate_deadline_ms",
        "deactivate_deadline_ms",
        "migrate_deadline_ms",
        "job_deadline_ms",
        "job_wall_ms",
        "job_memory_mib",
        "job_io_mib",
        "concurrent_jobs",
        "job_queue",
        "event_queue",
        "pending_requests",
        "timers",
        "output_filters",
        "watches",
        "key_grabs",
        "ui_tree_nodes",
        "ui_tree_kib",
        "decorations",
        "node_metadata_kib",
        "storage_bucket_mib",
        "disk_mib",
        "fs_read_all_mib",
        "fs_write_mib",
        "http_response_mib",
        "process_output_mib",
        "log_lines_per_second",
        "open_videos",
        "video_buffer_mib",
        "audio_captures",
        "http_streams",
        "children",
        "durable_timers",
        "vector_indexes",
        "vector_entries",
        "shared_cache_mib",
        "journal_entries",
        "toasts_pending",
        "package_assets_mib",
    ];

    fn limits_of(entries: &[(&str, u64)]) -> Limits {
        Limits(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_owned(), *value))
                .collect(),
        )
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_all_lists_the_forty_six_manifest_keys_in_manual_order() {
        let names = LimitKey::ALL.map(|key| key.as_key());
        assert_eq!(names, MANUAL_KEY_NAMES);
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_every_key_parses_back_from_its_manifest_name() {
        for key in LimitKey::ALL {
            let parsed: LimitKey = key.as_key().parse().expect("manifest key must parse");
            assert_eq!(parsed, key);
        }
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_display_is_the_manifest_key_name() {
        assert_eq!(LimitKey::MemoryMib.to_string(), "memory_mib");
        assert_eq!(LimitKey::PackageAssetsMib.to_string(), "package_assets_mib");
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_unknown_key_name_is_rejected_with_its_kind_and_text() {
        let error = "not_a_limit"
            .parse::<LimitKey>()
            .expect_err("a key outside the table must be refused");
        assert_eq!(error.kind, "limit key");
        assert_eq!(error.name, "not_a_limit");
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_empty_key_name_is_rejected() {
        assert!("".parse::<LimitKey>().is_err());
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_key_names_are_case_sensitive() {
        assert!("Memory_MiB".parse::<LimitKey>().is_err());
        assert!("MEMORY_MIB".parse::<LimitKey>().is_err());
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_session_keys_are_not_manifest_limit_keys() {
        for name in [
            "session_memory_mib",
            "session_helpers",
            "session_processes",
            "session_http_streams",
            "session_decorations",
        ] {
            assert!(
                name.parse::<LimitKey>().is_err(),
                "{name} is a host setting, not a manifest key"
            );
        }
    }

    /// REQ: PKG-031
    #[test]
    fn pkg_031_default_of_matches_the_manual_default_column() {
        for (key, default, _, _) in MANUAL_LIMITS {
            let Some(expected) = default else { continue };
            assert_eq!(default_of(key), expected, "default of {key}");
        }
    }

    /// REQ: PKG-031
    #[test]
    fn pkg_031_ceiling_matches_the_manual_tier_columns() {
        for (key, _, t1, t23) in MANUAL_LIMITS {
            assert_eq!(ceiling(key, Tier::T1), t1, "T1 ceiling of {key}");
            assert_eq!(ceiling(key, Tier::T2), t23, "T2 ceiling of {key}");
            assert_eq!(ceiling(key, Tier::T3), t23, "T3 ceiling of {key}");
        }
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_no_default_exceeds_its_ceiling() {
        for key in LimitKey::ALL {
            for tier in Tier::ALL {
                assert!(
                    default_of(key) <= ceiling(key, tier),
                    "default of {key} must not exceed its {tier} ceiling"
                );
            }
        }
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_frame_deadline_ceiling_is_fifty_ms_for_every_tier() {
        for tier in Tier::ALL {
            assert_eq!(ceiling(LimitKey::FrameDeadlineMs, tier), 50);
        }
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_memory_mib_has_the_documented_default_and_ceilings() {
        assert_eq!(default_of(LimitKey::MemoryMib), 32);
        assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T1), 256);
        assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T2), 128);
        assert_eq!(ceiling(LimitKey::MemoryMib, Tier::T3), 128);
    }

    /// REQ: LIF-143
    #[test]
    fn lif_143_empty_request_yields_every_key_at_its_default() {
        let effective = clamp(&Limits::default(), Tier::T3).expect("an empty request is valid");
        assert_eq!(effective.len(), LimitKey::ALL.len());
        for key in LimitKey::ALL {
            if key == LimitKey::FrameDeadlineMs {
                continue;
            }
            assert_eq!(effective[&key], default_of(key), "{key}");
        }
    }

    /// REQ: LIF-143
    #[test]
    fn lif_143_requested_value_below_ceiling_is_kept() {
        let requested = limits_of(&[("memory_mib", 64)]);
        let effective = clamp(&requested, Tier::T3).expect("valid request");
        assert_eq!(effective[&LimitKey::MemoryMib], 64);
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_requested_value_equal_to_ceiling_is_kept() {
        let ceiling_of_t3 = ceiling(LimitKey::MemoryMib, Tier::T3);
        let requested = limits_of(&[("memory_mib", ceiling_of_t3)]);
        let effective = clamp(&requested, Tier::T3).expect("valid request");
        assert_eq!(effective[&LimitKey::MemoryMib], ceiling_of_t3);
    }

    /// REQ: HST-404
    #[test]
    fn hst_404_request_above_tier_ceiling_is_clamped_not_refused() {
        // Manual example: a T3 plugin asking for memory_mib 200 gets 128.
        let requested = limits_of(&[("memory_mib", 200)]);
        let effective =
            clamp(&requested, Tier::T3).expect("a request above the ceiling is never refused");
        assert_eq!(effective[&LimitKey::MemoryMib], 128);
    }

    /// REQ: HST-404
    #[test]
    fn hst_404_the_same_request_is_clamped_to_the_tier_own_ceiling() {
        let requested = limits_of(&[("memory_mib", 200)]);
        let effective = clamp(&requested, Tier::T1).expect("valid request");
        assert_eq!(effective[&LimitKey::MemoryMib], 200, "T1 ceiling is 256");
        let effective = clamp(&requested, Tier::T2).expect("valid request");
        assert_eq!(effective[&LimitKey::MemoryMib], 128, "T2 ceiling is 128");
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_zero_means_the_default_not_zero() {
        let requested = limits_of(&[("memory_mib", 0)]);
        let effective = clamp(&requested, Tier::T3).expect("0 is a valid request");
        assert_eq!(
            effective[&LimitKey::MemoryMib],
            default_of(LimitKey::MemoryMib)
        );
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_zero_is_the_default_for_every_key() {
        let entries: Vec<(&str, u64)> = LimitKey::ALL.iter().map(|key| (key.as_key(), 0)).collect();
        let effective = clamp(&limits_of(&entries), Tier::T1).expect("zeros are valid requests");
        for key in LimitKey::ALL {
            if key == LimitKey::FrameDeadlineMs {
                continue;
            }
            assert_eq!(effective[&key], default_of(key), "{key}");
        }
    }

    /// REQ: PKG-070
    #[test]
    fn pkg_070_largest_schema_value_is_clamped_to_the_ceiling() {
        let requested = limits_of(&[("job_wall_ms", 4_294_967_295)]);
        let effective = clamp(&requested, Tier::T2).expect("clamps, never refuses");
        assert_eq!(
            effective[&LimitKey::JobWallMs],
            ceiling(LimitKey::JobWallMs, Tier::T2)
        );
    }

    /// REQ: PKG-071
    #[test]
    fn pkg_071_every_key_at_its_ceiling_is_kept_as_given() {
        let entries: Vec<(&str, u64)> = LimitKey::ALL
            .iter()
            .map(|key| (key.as_key(), ceiling(*key, Tier::T1)))
            .collect();
        let effective = clamp(&limits_of(&entries), Tier::T1).expect("ceilings are valid");
        for key in LimitKey::ALL {
            assert_eq!(effective[&key], ceiling(key, Tier::T1), "{key}");
        }
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_unknown_key_refuses_the_whole_request_with_unknown_key_code() {
        let requested = limits_of(&[("memory_mib", 64), ("not_a_limit", 1)]);
        let diagnostic = clamp(&requested, Tier::T3).expect_err("an unknown key must be refused");
        assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
        assert_eq!(diagnostic.severity, Severity::Error);
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_session_key_in_manifest_is_refused_as_unknown() {
        let requested = limits_of(&[("session_memory_mib", 512)]);
        let diagnostic =
            clamp(&requested, Tier::T1).expect_err("session keys are not manifest keys");
        assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_key_names_are_matched_case_sensitively_in_requests() {
        let requested = limits_of(&[("Memory_MiB", 64)]);
        let diagnostic = clamp(&requested, Tier::T3).expect_err("names are case-sensitive");
        assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
    }

    /// REQ: PKG-072
    #[test]
    fn pkg_072_empty_key_name_is_refused_as_unknown() {
        let requested = limits_of(&[("", 1)]);
        let diagnostic = clamp(&requested, Tier::T3).expect_err("an empty key is not in the table");
        assert_eq!(diagnostic.code.as_str(), "manifest.unknown-key");
    }
}
