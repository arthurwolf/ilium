### Fixed

- Fixed a session identity that `/resume` or `/clear` had invalidated coming back when detection switched from an agent's launcher process to the agent process itself; the hand-off now keeps the quarantine and the session's owning process.
