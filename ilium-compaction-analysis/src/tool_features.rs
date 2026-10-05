//! Compact per-request tool-call features, the input of the measured rework.
//!
//! After a compaction an agent re-reads files and re-runs commands that the
//! summary dropped. Measuring that needs to know, for every request, how many
//! file-read-like tool calls it made, whether it edited anything, and which
//! paths and commands it touched, but never their text. Paths, search
//! arguments and commands are therefore stored as 32-bit hashes only (privacy:
//! no raw path or command ever reaches a trace or its cache).
//!
//! Layout: one [`TurnTools`] row per request (parallel to
//! [`SessionTrace::turns`](crate::trace::SessionTrace::turns)) plus two side
//! vectors of hashes; each row says how many of those hashes are its own.

use serde::{Deserialize, Serialize};

/// The request edits or writes through a dedicated tool (Claude
/// `Edit`/`Write`/`MultiEdit`/`NotebookEdit`; Codex `apply_patch`).
pub const TOOL_FLAG_EDIT: u8 = 1;
/// A shell command of the request looks like a write (heredoc, `sed -i`,
/// `tee`, `>` redirection, `git apply`/`commit`, `patch`). Flagged separately
/// from [`TOOL_FLAG_EDIT`] because it is a heuristic.
pub const TOOL_FLAG_WRITE_HEURISTIC: u8 = 1 << 1;

/// Most read hashes stored per request (the call count is exact regardless).
pub const MAX_READ_HASHES_PER_TURN: usize = 16;
/// Most command hashes stored per request.
pub const MAX_COMMAND_HASHES_PER_TURN: usize = 8;

/// Mask of the two low bits of a read hash that encode its kind.
pub const HASH_KIND_MASK: u32 = 3;
/// Read hash kind: a file path that was read (Read, `cat`, `head`, `sed -n`).
pub const HASH_KIND_PATH: u32 = 0;
/// Read hash kind: a search or listing (Grep, Glob, `rg`, `grep`, `ls`,
/// `find`, `git log`...), hashed over the whole argument.
pub const HASH_KIND_SEARCH: u32 = 1;

/// Tool features of one request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "TurnToolsRow", into = "TurnToolsRow")]
pub struct TurnTools {
    /// Every tool call of the request.
    pub tool_calls: u16,
    /// File-read-like calls (reads, searches, listings).
    pub read_calls: u16,
    /// Shell/exec commands (Claude `Bash`, Codex `exec_command` and friends).
    pub command_calls: u16,
    /// `TOOL_FLAG_*` bits.
    pub flags: u8,
    /// Hashes of this request in [`ToolFeatures::read_hashes`].
    pub stored_reads: u8,
    /// Hashes of this request in [`ToolFeatures::command_hashes`].
    pub stored_commands: u8,
}

/// Serialized shape: `[tool_calls, read_calls, command_calls, flags,
/// stored_reads, stored_commands]`.
type TurnToolsRow = (u16, u16, u16, u8, u8, u8);

impl From<TurnToolsRow> for TurnTools {
    fn from(row: TurnToolsRow) -> Self {
        Self {
            tool_calls: row.0,
            read_calls: row.1,
            command_calls: row.2,
            flags: row.3,
            stored_reads: row.4,
            stored_commands: row.5,
        }
    }
}

impl From<TurnTools> for TurnToolsRow {
    fn from(tools: TurnTools) -> Self {
        (
            tools.tool_calls,
            tools.read_calls,
            tools.command_calls,
            tools.flags,
            tools.stored_reads,
            tools.stored_commands,
        )
    }
}

impl TurnTools {
    /// Whether the request edited through a dedicated edit tool.
    pub fn has_edit(&self) -> bool {
        self.flags & TOOL_FLAG_EDIT != 0
    }

    /// Whether the request edited (dedicated tool) or looks like it wrote
    /// through the shell.
    pub fn has_any_write(&self) -> bool {
        self.flags & (TOOL_FLAG_EDIT | TOOL_FLAG_WRITE_HEURISTIC) != 0
    }
}

/// Tool features being collected for one request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolAccumulator {
    /// Every tool call.
    pub tool_calls: u16,
    /// File-read-like calls.
    pub read_calls: u16,
    /// Shell/exec commands.
    pub command_calls: u16,
    /// `TOOL_FLAG_*` bits.
    pub flags: u8,
    /// Read hashes (kind in the low two bits).
    pub read_hashes: Vec<u32>,
    /// Command hashes.
    pub command_hashes: Vec<u32>,
}

impl ToolAccumulator {
    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.tool_calls == 0
            && self.read_calls == 0
            && self.command_calls == 0
            && self.flags == 0
            && self.read_hashes.is_empty()
            && self.command_hashes.is_empty()
    }

    /// Records a read hash (the count is kept by the caller via `read_calls`).
    pub fn push_read_hash(&mut self, hash: u32) {
        if self.read_hashes.len() < MAX_READ_HASHES_PER_TURN {
            self.read_hashes.push(hash);
        }
    }

    /// Records a command hash.
    pub fn push_command_hash(&mut self, hash: u32) {
        if self.command_hashes.len() < MAX_COMMAND_HASHES_PER_TURN {
            self.command_hashes.push(hash);
        }
    }
}

/// Tool features of a whole session, parallel to its requests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFeatures {
    /// One row per request.
    pub turns: Vec<TurnTools>,
    /// Read hashes of all requests, in request order.
    pub read_hashes: Vec<u32>,
    /// Command hashes of all requests, in request order.
    pub command_hashes: Vec<u32>,
}

/// Start offsets of each request's hashes (one entry per request plus a final
/// end offset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOffsets {
    /// Offsets into [`ToolFeatures::read_hashes`].
    pub read: Vec<usize>,
    /// Offsets into [`ToolFeatures::command_hashes`].
    pub command: Vec<usize>,
}

impl ToolFeatures {
    /// Whether there is exactly one row per request and the stored hash counts
    /// add up (a cache written by this crate version always is).
    pub fn is_consistent_with(&self, turns: usize) -> bool {
        let reads: usize = self
            .turns
            .iter()
            .map(|row| usize::from(row.stored_reads))
            .sum();
        let commands: usize = self
            .turns
            .iter()
            .map(|row| usize::from(row.stored_commands))
            .sum();
        self.turns.len() == turns
            && reads == self.read_hashes.len()
            && commands == self.command_hashes.len()
    }

    /// Appends the features of a new request.
    pub fn push(&mut self, accumulator: &ToolAccumulator) {
        let row = TurnTools {
            tool_calls: accumulator.tool_calls,
            read_calls: accumulator.read_calls,
            command_calls: accumulator.command_calls,
            flags: accumulator.flags,
            stored_reads: accumulator.read_hashes.len().min(MAX_READ_HASHES_PER_TURN) as u8,
            stored_commands: accumulator
                .command_hashes
                .len()
                .min(MAX_COMMAND_HASHES_PER_TURN) as u8,
        };
        self.read_hashes.extend(
            accumulator
                .read_hashes
                .iter()
                .take(MAX_READ_HASHES_PER_TURN),
        );
        self.command_hashes.extend(
            accumulator
                .command_hashes
                .iter()
                .take(MAX_COMMAND_HASHES_PER_TURN),
        );
        self.turns.push(row);
    }

    /// Adds features to the most recent request (Claude writes one line per
    /// content block, so a request's tool calls arrive over several lines).
    /// Returns `false` when there is no request yet.
    pub fn merge_into_last(&mut self, accumulator: &ToolAccumulator) -> bool {
        let Some(row) = self.turns.last_mut() else {
            return false;
        };
        row.tool_calls = row.tool_calls.saturating_add(accumulator.tool_calls);
        row.read_calls = row.read_calls.saturating_add(accumulator.read_calls);
        row.command_calls = row.command_calls.saturating_add(accumulator.command_calls);
        row.flags |= accumulator.flags;
        let read_room = MAX_READ_HASHES_PER_TURN - usize::from(row.stored_reads);
        let reads: Vec<u32> = accumulator
            .read_hashes
            .iter()
            .copied()
            .take(read_room)
            .collect();
        row.stored_reads += reads.len() as u8;
        self.read_hashes.extend(reads);
        let command_room = MAX_COMMAND_HASHES_PER_TURN - usize::from(row.stored_commands);
        let commands: Vec<u32> = accumulator
            .command_hashes
            .iter()
            .copied()
            .take(command_room)
            .collect();
        row.stored_commands += commands.len() as u8;
        self.command_hashes.extend(commands);
        true
    }

    /// Start offsets of every request's hashes.
    pub fn offsets(&self) -> ToolOffsets {
        let mut read = Vec::with_capacity(self.turns.len() + 1);
        let mut command = Vec::with_capacity(self.turns.len() + 1);
        let (mut read_at, mut command_at) = (0, 0);
        for row in &self.turns {
            read.push(read_at);
            command.push(command_at);
            read_at += usize::from(row.stored_reads);
            command_at += usize::from(row.stored_commands);
        }
        read.push(read_at);
        command.push(command_at);
        ToolOffsets { read, command }
    }

    /// Keeps only the requests whose `keep` entry is true (hashes follow).
    pub fn retain_turns(&mut self, keep: &[bool]) {
        let offsets = self.offsets();
        let mut turns = Vec::new();
        let mut read_hashes = Vec::new();
        let mut command_hashes = Vec::new();
        for (index, row) in self.turns.iter().enumerate() {
            if !keep.get(index).copied().unwrap_or(true) {
                continue;
            }
            turns.push(*row);
            read_hashes
                .extend_from_slice(&self.read_hashes[offsets.read[index]..offsets.read[index + 1]]);
            command_hashes.extend_from_slice(
                &self.command_hashes[offsets.command[index]..offsets.command[index + 1]],
            );
        }
        self.turns = turns;
        self.read_hashes = read_hashes;
        self.command_hashes = command_hashes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accumulator(reads: &[u32], commands: &[u32], flags: u8) -> ToolAccumulator {
        ToolAccumulator {
            tool_calls: (reads.len() + commands.len()) as u16,
            read_calls: reads.len() as u16,
            command_calls: commands.len() as u16,
            flags,
            read_hashes: reads.to_vec(),
            command_hashes: commands.to_vec(),
        }
    }

    #[test]
    fn rows_serialize_flat_and_offsets_follow_the_counts() {
        let mut features = ToolFeatures::default();
        features.push(&accumulator(&[4, 8], &[100], TOOL_FLAG_EDIT));
        features.push(&accumulator(&[], &[], 0));
        features.push(&accumulator(&[12], &[200, 300], 0));
        assert!(features.is_consistent_with(3));
        let offsets = features.offsets();
        assert_eq!(offsets.read, vec![0, 2, 2, 3]);
        assert_eq!(offsets.command, vec![0, 1, 1, 3]);
        assert_eq!(
            serde_json::to_string(&features.turns[0]).unwrap(),
            "[3,2,1,1,2,1]"
        );
        let back: ToolFeatures =
            serde_json::from_str(&serde_json::to_string(&features).unwrap()).unwrap();
        assert_eq!(back, features);
    }

    #[test]
    fn merge_appends_to_the_last_request_within_the_caps() {
        let mut features = ToolFeatures::default();
        features.push(&accumulator(&[4], &[], 0));
        let many: Vec<u32> = (0..40).map(|value| value * 4).collect();
        assert!(features.merge_into_last(&accumulator(&many, &[7], TOOL_FLAG_WRITE_HEURISTIC)));
        let row = features.turns[0];
        assert_eq!(usize::from(row.read_calls), 41);
        assert_eq!(usize::from(row.stored_reads), MAX_READ_HASHES_PER_TURN);
        assert!(row.has_any_write() && !row.has_edit());
        assert!(features.is_consistent_with(1));
        assert!(!ToolFeatures::default().merge_into_last(&accumulator(&[], &[], 0)));
    }

    #[test]
    fn retaining_requests_keeps_their_hashes() {
        let mut features = ToolFeatures::default();
        features.push(&accumulator(&[4], &[10], 0));
        features.push(&accumulator(&[8, 12], &[20], 0));
        features.push(&accumulator(&[16], &[30], 0));
        features.retain_turns(&[true, false, true]);
        assert_eq!(features.read_hashes, vec![4, 16]);
        assert_eq!(features.command_hashes, vec![10, 30]);
        assert!(features.is_consistent_with(2));
    }
}
