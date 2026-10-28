### Fixed
- Read terminal history and recover lost output without blocking shared pane state behind native journal locks; report capture failures with the pane and operation.
- Retain and retry a deferred output repair after the terminal becomes silent or closes, while rejecting output from replaced terminal sessions.
