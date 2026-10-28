### Changed
- Existing Git worktrees accept multiple agents. Shared worktrees appear as fixed leaf-icon groups under their project; single agents retain the compact branch line.

### Fixed
- Keep shared-worktree agents together across tree moves, splits, restructuring, undo, and snapshot restore. Closing an agent preserves the other worktree members.
- Keep deep server request and repository-facts polling on cancellation-owned tasks to avoid debug-runtime stack overflow.
