### Fixed
- Bound redraw retries during temporary frame-storage pressure, preserving pending input and graphics effects, with rate-limited admission diagnostics.
- Preserve the visible terminal recovery reserve through encoding and retirement metadata, even when ordinary event storage is full.
- Report prolonged server storage waits with content-free ledger diagnostics and a bounded retry fallback.
