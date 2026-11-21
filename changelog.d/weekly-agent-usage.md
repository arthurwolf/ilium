### Added
- Add a full-screen weekly agent usage view, available from the tree context menu and as an optional tree toolbar button, with model-coloured workload bars and local quota provenance.

### Fixed
- Read statistics from server-verified custom Claude and Codex transcript stores, with session/project validation and separate cache identity.
- Allow receipt-verified installation to read artifacts from the configured Cargo target directory.
- Retain verified weekly ownership periods across session changes and binding renewals, with bounded history, conservative overlap exclusion, and explicit lost-coverage reporting.
- Keep prior-provider usage in separate selectable rows after a pane switches provider, and show tiny nonzero workload values without rounding them to zero.
- Keep weekly usage content above the persistent voice control in compact terminals.
- Fix weekly agent usage showing mostly empty rows: oversized transcript lines (over 4 MiB) no longer make Claude, Claude subagent or Codex statistics unavailable, same-session history before the first observed ownership appears as a labelled historical estimate, ambiguous sessions are counted once as unassigned, configured price overrides and gpt-6 model names are weighted, unknown weights never show as 0.000, rows show a short status with errors in the selected details, and bars use full blocks.
- Rework the usage view as a framed, scrollable screen with a clickable range selector (6h, 12h, 1d, 3d, 7d, 14d, 1m) instead of a fixed week; rows open and close on click or Space; bars are sized by estimated cost and coloured cool to hot by model strength, with a legend ordered the same way.
- Count Codex sub-agent threads (separate rollout files linked to their parent) in session statistics and usage, alongside the Claude sub-agent and workflow transcripts that were already included.
