//! Operating systems and process kinds (reference-host-architecture.md).

use crate::error::UnknownName;

/// The operating system the host runs on (reference-host-architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Linux (reference-host-architecture.md).
    Linux,
    /// macOS (reference-host-architecture.md).
    MacOs,
    /// Windows (reference-host-architecture.md).
    Windows,
}

impl Platform {
    /// Every platform (reference-host-architecture.md).
    pub const ALL: [Platform; 3] = [Platform::Linux, Platform::MacOs, Platform::Windows];

    /// Lower-case name: `linux`, `macos`, `windows` (reference-host-architecture.md).
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::MacOs => "macos",
            Platform::Windows => "windows",
        }
    }
}

impl std::fmt::Display for Platform {
    /// `linux`, `macos` or `windows` (reference-host-architecture.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = UnknownName;

    /// Parses `linux`, `macos` or `windows` (reference-host-architecture.md).
    fn from_str(text: &str) -> Result<Platform, UnknownName> {
        Platform::ALL
            .into_iter()
            .find(|platform| platform.as_str() == text)
            .ok_or_else(|| UnknownName::new("platform", text))
    }
}

/// The Ilium process a plugin host runs in (reference-host-architecture.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessKind {
    /// `ilium-server`: server sides (reference-host-architecture.md).
    Server,
    /// `ilium-client`: client and scene sides (reference-host-architecture.md).
    Client,
    /// The `ilium` CLI (reference-host-architecture.md).
    Cli,
    /// The T2/T3 sandbox helper (reference-host-helper.md).
    Helper,
}

impl ProcessKind {
    /// Every process kind (reference-host-architecture.md).
    pub const ALL: [ProcessKind; 4] = [
        ProcessKind::Server,
        ProcessKind::Client,
        ProcessKind::Cli,
        ProcessKind::Helper,
    ];

    /// Lower-case name: `server`, `client`, `cli`, `helper` (reference-host-architecture.md).
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessKind::Server => "server",
            ProcessKind::Client => "client",
            ProcessKind::Cli => "cli",
            ProcessKind::Helper => "helper",
        }
    }
}

impl std::fmt::Display for ProcessKind {
    /// `server`, `client`, `cli` or `helper` (reference-host-architecture.md).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for ProcessKind {
    type Err = UnknownName;

    /// Parses `server`, `client`, `cli` or `helper` (reference-host-architecture.md).
    fn from_str(text: &str) -> Result<ProcessKind, UnknownName> {
        ProcessKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| UnknownName::new("process kind", text))
    }
}

/// What `ilium plugin doctor` reports about the runtime (ABI-070).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    /// The exact pinned Wasmtime version.
    pub wasmtime_version: String,
    /// The canonical ABI revision.
    pub canonical_abi_revision: String,
}

impl DoctorReport {
    /// The report as the text of the command (ABI-070).
    pub fn render(&self) -> String {
        unimplemented!("api::platform::DoctorReport::render")
    }
}

/// ABI-070: the runtime facts that `ilium plugin doctor` shows.
pub fn doctor_report() -> DoctorReport {
    unimplemented!("api::platform::doctor_report")
}
