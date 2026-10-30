### Fixed
- An agent that starts working on its own in a pane you are not viewing (goal continuation, queued or scheduled prompt, background-task notification) now shows its activity spinner within a couple of seconds, instead of up to 45 seconds later or never for a short turn. Idle and finished agent panes are rechecked shortly after they print output.
- Claude Code's six-point `✶` and ASCII `*` spinner frames are recognised as a working turn, so a running turn no longer registers as idle and drops into the slow check interval.
- Panes holding an idle or finished agent are now checked at least every 15 seconds (or your `idle_poll_seconds` if lower); plain shells keep the 45 second default.
