# Reference, plugin scope and sessions

## Reference, plugin scope and sessions

A user may run several Ilium sessions, and may open projects with different needs. This page defines where a plugin is installed, where it is enabled, how a change reaches the other sessions, and which failure state is shared.

Contract version: `ilium:plugin@0.1.0`.

### Install scope

In 0.1.0 a plugin installs globally, for the user. There is no project-scope install. A package in a project folder could run code the moment a repository is opened, so the platform leaves it out until project trust exists. The shared package store, `installed.json`, grants and the global storage bucket serve every session of the user.

- [LIF-160] The host MUST accept installs only into the user scope in 0.1.0, and MUST refuse `--scope project` for install with `install.scope-unsupported`. Test: run the command.

### Enable scope

A plugin is enabled or disabled at three levels. A lower level overrides a higher one.

| Order | Level | Where stored | Set by |
|---|---|---|---|
| 1 | global | `installed.json` | `ilium plugin enable <id>` |
| 2 | project | `<project>/.ilium/plugins.toml` | `ilium plugin enable <id> --scope project` |
| 3 | session | session state | the Plugins screen toggle with "this session only" |

The effective value is the session value if set, else the project value if set, else the global value. A plugin disabled by policy or by safe mode stays off whatever the levels say.

The WIT call is `set-enabled(id, enabled, scope)` with `scope` one of `global`, `project`, `session`.

- [LIF-161] The effective enabled value MUST follow the order session, project, global. Test: set the three levels in all eight combinations.
- [LIF-162] `plugins.toml` MUST be read as a request only. It MUST NOT install, grant or raise a tier. Test: list a package that is not installed.

### Propagation to other sessions

A global change reaches running sessions within 2 seconds. Each session server watches `installed.json`. A session that is mid-call finishes the call, then applies the change. The CLI command fans out: it prints one JSONL line per reachable session with its result. An unreachable session gets the change at its next start.

```text
{"type":"session-result","session":"default","project":"/home/u/app","ok":true}
{"type":"session-result","session":"review","project":"/home/u/lib","ok":true}
```

- [LIF-163] A global enable, disable or uninstall MUST reach every running session of the user within 2 seconds. Test: two fake sessions and a changed file.
- [LIF-164] The CLI MUST print one `session-result` line per session and exit 0 only when all succeeded. Test: make one session unreachable.

### Locks

Writes to `installed.json` take the lock `<data>/plugins/.lock`. A second writer waits up to 5 seconds, then fails with `conflict`. An update that is in progress holds a per-plugin lock, so two sessions never run the same trial.

### Failure scope

The failure window is per session, per plugin side. A plugin that fails three times in one session is disabled in that session only, and a note is written to the shared registry so that other sessions show the warning. It is not disabled elsewhere.

- [LIF-165] A failure disable MUST apply to the session where it happened and MUST NOT disable the plugin in other sessions. Test: fail a plugin in session A and call it in B.

### Grants and the "Not now" counter

Grants are global and are stored in `$XDG_STATE_HOME/ilium/grants.json`. The counter of "Not now" answers for a bundled plugin's first-start prompt is stored with the grants, so the prompt does not repeat in each session.

- [LIF-166] A grant change MUST apply to every session and the "Not now" counter MUST be shared. Test: answer in one session and open another.

### See also

- [Install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Lifecycle](/pages/lifecycle/)
- [Requirements, lifecycle and operations](/pages/reference-requirements-lifecycle-ops/)

API version checked: `ilium:plugin@0.1.0`.
