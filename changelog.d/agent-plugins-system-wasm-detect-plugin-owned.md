### Added
- The `detector` plugin export gains `identify-tree` (names the agent process in a pane's process tree) and `classify-screen` (one pane's full screen verdict: turn, evidence line, goal state, fresh screen, interstitial answer, prompt readiness), and the Agent detect plugin implements both.
- An agent-setup plugin holding read and propose grants for an agent's configuration can now take a lock on that configuration scope; the scope stays read-only.

### Changed
- When a detector plugin is running, agent activity and goal state come from its verdicts: the detection loop queues each identified pane's screen without waiting, one call per pane runs on the plugin thread within a 50 ms budget per cycle starting at a rotating pane, and a pane without a verdict keeps its previous activity. Interstitial answers are sent only from bundled detectors. Without a detector plugin the built-in table classifies as before.
