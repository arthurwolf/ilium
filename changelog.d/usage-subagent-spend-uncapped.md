### Fixed
- Usage statistics no longer drop a session that used many background tasks (sub-agents). A session with more than 65,536 sub-agent records used to fail the retained-record limit and lose its spend; sub-agent calls now fold into per-minute totals, and the record limit is gone.
- Usage statistics now read every sub-agent and Codex child transcript to its end in the pane path, instead of only the first 16 MiB of each file.
