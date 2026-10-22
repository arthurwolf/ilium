---
layout: page
title: Failure modes of the dev command
permalink: /pages/reference-plugin-cli-dev-failure-modes/
---

## Failure modes of the dev command

This page defines how `ilium plugin dev` behaves when a build fails, when the file watcher cannot do its job, when rebuilds arrive in bursts and when a plugin keeps crashing during a session. The normal reload rules are in [Plugin command-line reference](/pages/reference-plugin-cli/). This page describes target behaviour; it does not claim the behaviour exists today.

Requirement ids of this page are in the range `DEV-879` to `DEV-881`. The complete list is in [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/).

### Rules

- **[DEV-879]** When a rebuild fails, `dev` MUST keep the last good instance running, print the build diagnostics as `diagnostic` records, emit one `reload` record with `outcome` `build-failed`, and wait for the next change. A later successful build MUST reload as usual. A failed build MUST NOT end the session.

- **[DEV-880]** `dev` MUST wait for 300 ms without a file change before it starts a rebuild and MUST start at most one rebuild at a time. Changes that arrive during a rebuild MUST cause exactly one follow-up rebuild. Files that the build writes (`dist/`, `target/`, `node_modules/`) MUST NOT trigger a rebuild.

- **[DEV-881]** When the file watcher cannot be created or loses events (inotify limit reached, network or remote file system, event overflow), `dev` MUST print a `warning` with the code `dev.watcher-degraded` that names the cause and the fix, and MUST fall back to polling every 1,000 ms. When a plugin crashes on activation 3 times in 60 seconds, `dev` MUST stop reloading it, emit `dev.crash-loop`, and resume only after the next source change.

### Failure behaviour

Each rule names its failure code. A failure prints a `diagnostic` record in JSONL or one line on standard error in text, and the process exit code follows [Plugin command-line reference](/pages/reference-plugin-cli/#exit-codes).

## See also

- [Plugin command-line reference](/pages/reference-plugin-cli/)
- [Requirements of the developer tooling lane](/pages/reference-requirements-dev-tooling/)

API checked against `ilium:plugin@0.1.0`.
