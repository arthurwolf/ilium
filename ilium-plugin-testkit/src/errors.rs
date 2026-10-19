//! Harness diagnostics (reference-test-harness.md "Complete error code list").

/// Dotted codes of the harness, fixtures, replay, toolchain and test warnings.
pub mod codes {
    /// Builder call after activation.
    pub const SETUP_CLOSED: &str = "harness.setup-closed";
    /// Grant not in the manifest.
    pub const GRANT_NOT_DECLARED: &str = "harness.grant-not-declared";
    /// Grant name not in the catalogue.
    pub const GRANT_UNKNOWN: &str = "harness.grant-unknown";
    /// More than 256 events in one `deliver`.
    pub const BATCH_TOO_LARGE: &str = "harness.batch-too-large";
    /// Queue not empty after 100 rounds.
    pub const SETTLE_LIMIT: &str = "harness.settle-limit";
    /// `cell` outside the screen.
    pub const CELL_OUT_OF_RANGE: &str = "harness.cell-out-of-range";
    /// Render size outside 10 to 500 by 1 to 200.
    pub const RENDER_SIZE: &str = "harness.render-size";
    /// `set_clock` moved time back.
    pub const CLOCK_BACKWARDS: &str = "harness.clock-backwards";
    /// Golden file absent.
    pub const GOLDEN_MISSING: &str = "harness.golden-missing";
    /// Golden update requested when `CI` is set.
    pub const GOLDEN_UPDATE_IN_CI: &str = "harness.golden-update-in-ci";
    /// Second activation.
    pub const ALREADY_ACTIVE: &str = "harness.already-active";
    /// Call before activation.
    pub const NOT_ACTIVE: &str = "harness.not-active";
    /// Unknown code in `inject_failure`.
    pub const ERROR_UNKNOWN: &str = "harness.error-unknown";
    /// Event does not belong to the side.
    pub const EVENT_NOT_FOR_SIDE: &str = "harness.event-not-for-side";
    /// A fake process or network call had no expectation.
    pub const UNEXPECTED_CALL: &str = "harness.unexpected-call";
    /// Fake file system path is absolute or contains `..`.
    pub const FS_PATH_INVALID: &str = "harness.fs-path-invalid";
    /// `render` target is not one of the five `ViewTarget` cases.
    pub const VIEW_TARGET_UNKNOWN: &str = "harness.view-target-unknown";
    /// `advance_clock` argument is invalid.
    pub const CLOCK_ARGUMENT: &str = "harness.clock-argument";
    /// `deactivate(crash-restart)` was requested.
    pub const REASON_RESERVED: &str = "harness.reason-reserved";
    /// `install_core` name is not a core plugin.
    pub const CORE_UNKNOWN: &str = "harness.core-unknown";
    /// `session.server` or `session.client` names a plugin or side that is not installed.
    pub const PLUGIN_UNKNOWN: &str = "harness.plugin-unknown";
    /// `run_hook` on a plugin without a hook handler.
    pub const NOT_A_HOOK_HANDLER: &str = "harness.not-a-hook-handler";
    /// Storage snapshot file has an unknown shape.
    pub const SNAPSHOT_INVALID: &str = "harness.snapshot-invalid";
    /// Fixture key not in the schema.
    pub const FIXTURE_UNKNOWN_KEY: &str = "fixture.unknown-key";
    /// Fixture value has the wrong shape.
    pub const FIXTURE_BAD_VALUE: &str = "fixture.bad-value";
    /// Fixture reuses an id.
    pub const FIXTURE_DUPLICATE_ID: &str = "fixture.duplicate-id";
    /// Fixture state does not match.
    pub const FIXTURE_STATE_MISMATCH: &str = "fixture.state-mismatch";
    /// Replay diverged from the recording.
    pub const REPLAY_DIVERGED: &str = "replay.diverged";
    /// Replay file version is not 1.
    pub const REPLAY_VERSION_UNSUPPORTED: &str = "replay.version-unsupported";
    /// `--coverage` needs `cargo llvm-cov` or `bun test --coverage`.
    pub const COVERAGE_TOOL_MISSING: &str = "toolchain.coverage-tool-missing";
    /// Warning: a fixture has no expectation file.
    pub const NO_EXPECTATION: &str = "test.no-expectation";
    /// Trap codes (Errors and limits "Trap codes").
    pub const TRAP_CODES: &[&str] = &[
        "trap.deadline",
        "trap.memory",
        "trap.table",
        "trap.submission-batch",
        "trap.invalid-ui-tree",
        "trap.abi",
        "trap.panic",
    ];
}

/// A harness failure with its dotted code (reference-test-harness.md).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct HarnessError {
    /// Dotted code, one of [`codes`].
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl HarnessError {
    /// Builds an error from a code and a message.
    pub fn new(code: &str, message: impl Into<String>) -> HarnessError {
        HarnessError {
            code: code.to_string(),
            message: message.into(),
        }
    }
}
