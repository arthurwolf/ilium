### Fixed
- Fixed a race where session discovery that started before `/resume` or `/clear` could rebind the session ID the command had just invalidated; the quarantine is now re-checked when the result is applied.
