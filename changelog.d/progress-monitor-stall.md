### Fixed
- Progress monitors no longer show "no progress reported" for healthy long jobs whose probe keeps reporting the same percent and message (builds, test runs). The warning now means the probe produced no accepted report for ten intervals (at least ten minutes) and reads "no probe report".

### Added
- Probes may set `"indeterminate": true` for tasks with no measurable progress; the footer then shows no ETA and the popover says progress is not measurable.
- Agent probe instructions now ask for a real counter or phase signal first and `indeterminate` only when none exists.
