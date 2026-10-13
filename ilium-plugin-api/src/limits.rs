//! Requestable limits, defaults and tier ceilings (reference-errors-and-limits.md
//! "Requestable limits", reference-manifest.md PKG-031).
//!
//! The five session-wide budgets (`session_memory_mib`, `session_helpers`, `session_processes`,
//! `session_http_streams`, `session_decorations`) are host settings, not manifest keys, so they
//! are not a [`LimitKey`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::UnknownName;
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

/// The host default of a limit (reference-errors-and-limits.md "Requestable limits").
pub fn default_of(key: LimitKey) -> u64 {
    unimplemented!("api::limits::default_of")
}

/// The ceiling of a limit for a tier (reference-errors-and-limits.md "Requestable limits").
pub fn ceiling(key: LimitKey, tier: Tier) -> u64 {
    unimplemented!("api::limits::ceiling")
}

/// PKG-031: a request above the ceiling is clamped, never refused.
pub fn clamp(requested: &Limits, tier: Tier) -> Result<BTreeMap<LimitKey, u64>, Diagnostic> {
    unimplemented!("api::limits::clamp")
}
