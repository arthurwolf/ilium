### Changed
- The generic agent CLIs opencode and aider are now recognised through the Agent detect plugin's manifest signatures instead of a table built into the server, so disabling that plugin also stops their detection.

### Added
- The Agent detect plugin serves `ilium.core.agent-detect/screen-verdicts` (`classify-batch`), which classifies the screens of many panes in one call and reports the turn, the matching evidence line, goal state with its rule, fresh-conversation and interstitial answers, and composer readiness.

### Fixed
- The Agent detect plugin's classifier treats Claude Code's "Waiting for 1 dynamic workflow to finish" as a blocked turn, matching the server's detector.
