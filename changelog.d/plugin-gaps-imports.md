### Added
- Plugins can run child processes, read, write and watch files, call web services and start background jobs through the host imports `process`, `fs`, `http` and `jobs`, each limited to what the plugin's grants allow: `process.run:<program>` and `process.long-lived`, the `fs.read`, `fs.write` and `fs.watch` grants of the `project` scope, `http:<host>`, and `jobs.run` with the kinds declared under `contributes.jobs`.
- Plugins receive the output and exit of their child processes, folder changes, web stream chunks and job progress as events, and a job runs in its own plugin instance under the deadline and memory limit its manifest entry declares.
- Plugins can reach `https` hosts, with the host checked against their `http:` grants.

### Changed
- Every host import now keeps its state per plugin instance instead of in process-wide storage, so one plugin's files, children, streams, jobs, audio, fonts, sessions and key grabs are never visible to another, and stopping a plugin stops what it started.
- Request numbers of one session no longer repeat, and the limit of 16 processes at once is shared by every plugin of the session.
