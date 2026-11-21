### Fixed
- Count the advisor tool in agent usage: each advisor call (a separate response from a stronger model such as Fable, recorded only inside a reply's `usage.iterations`) is now priced at the advisor model's own rate and shown under that model, in the session statistics and in the Agent usage view.
