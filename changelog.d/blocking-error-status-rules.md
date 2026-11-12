### Added
- Custom status rules gain the `blocking-error` (this pane has a retained blocking error) and `session-error` (the session has one) conditions, and a shipped "Blocking error" default rule.
- Settings -> Agent Monitoring -> Blocking errors in Custom mode: Rules only (default), Overlay (the previous triangle over the icon) or Second icon (an extra cell right of the custom icon with its own tooltip).

### Fixed
- One session-level error no longer marks every pane with the warning triangle; it now shows only on the session row.
- In Custom mode the error triangle no longer ignores rule order unless Overlay is chosen.
