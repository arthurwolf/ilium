---
layout: page
title: Troubleshooting for authors
permalink: /pages/guide-troubleshooting/
---

## Troubleshooting for authors

This page lists problems an author meets, by the symptom seen and by the phase of work: build, install, activation, run time, update and release. Each row gives the likely cause and the fix. Users who meet a problem with a plugin they installed should read [Troubleshooting for users](/pages/user-troubleshooting/) and [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/).

Requirement ids of this page are in the range `AUT-800` to `AUT-899`.

### First steps for every problem

1. Run `ilium plugin check` and `ilium plugin lint`. They find many causes without running the plugin.
2. Read the log: `ilium plugin logs <id> --tail 100`.
3. Read the state: `ilium plugin status <id>`.
4. For a slow plugin, run `ilium plugin stats <id>` and `ilium plugin profile <id>`.
5. Reproduce it in a test on the fake host. A bug without a test returns.

An author SHOULD turn every fixed run-time bug into a test before the next release. [AUT-801]

## Build

| Symptom | Likely cause | Fix |
|---|---|---|
| `component.missing` from `check` | A declared side was not built | Run `ilium plugin build`, and check `side` in the manifest |
| `component.invalid` | The file is not a WebAssembly component (a plain module) | Use the SDK build command, not `rustc` alone |
| `component.world-mismatch` | The component was built for another world than the manifest names | Make `world` and the SDK entry agree |
| `manifest.api-version` | The SDK that built the component is another version than `api` | Rebuild with the SDK of the contract you target |
| `manifest.exports-mismatch` | The exports differ from the manifest list | Update `exports` or the code, then rebuild |
| `component.import-ungranted` | Code calls a function whose grant the side does not request | Add the grant to the correct side, or remove the call |
| `component.import-unknown` | The code imports something outside the contract | Use SDK imports only. Check for a host-specific crate |
| `component.size-warning` | A component is over 8 MiB | Strip dependencies, build `--release`. Above 32 MiB it is an error |
| Build passes, `lint.debug-info-in-release` | A debug build was packed | Pack after `build --release` |

## Install

The install stages are listed in [Package format](/pages/reference-package-format/). Run `ilium plugin verify <file>` first. It runs the stages offline. The complete code list is the install code index in [Errors and limits reference](/pages/reference-errors-and-limits/).

| Symptom | Likely cause | Fix |
|---|---|---|
| `manifest.unknown-key` | A key not in the schema, often a typo | Compare with [Manifest reference](/pages/reference-manifest/) |
| `manifest.key-missing` | A packed manifest has no `files` | Run `ilium plugin pack`, which writes it |
| `manifest.id-invalid` or `manifest.id-reserved` | Bad id pattern, or an `ilium.` id | Use a reverse-domain id you control |
| `manifest.file-unlisted`, `manifest.file-missing` | The package and `files` disagree | Run `pack` again after the last change |
| `manifest.file-length`, `manifest.file-hash` | A file changed after `pack` | Run `pack` again |
| `manifest.signature-invalid` | The manifest changed after signing | Sign last |
| `manifest.publisher-mismatch` or `manifest.id-publisher` | The `publisher` or the id prefix differs from the key | Use the publisher id of the key as the id prefix |
| `manifest.grant-unknown` | A misspelled or removed grant | Check [Capabilities](/pages/capabilities/) |
| `manifest.grant-tier` | A grant only for bundled plugins | Remove it, or ship another way |
| `manifest.grant-dependency` | A grant that needs a companion | Request both (for example `process.long-lived` needs `process.run:<program>`) |
| `package.version-reused` | Same version, other bytes | Raise the version |
| `package.downgrade` | An older version than the installed one | Raise the version, or use `--allow-downgrade` in testing |
| `package.api-unsupported` | `api` is outside the window of this Ilium | Build for a contract this Ilium supports. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) |
| `package.platform-unsupported` | T2 or T3 on Windows or macOS | Use Linux for these tiers |
| Install asks to trust a key | The key is not in the trust store | `ilium plugin trust add publisher.pub` after checking the fingerprint |
| Install refused without a message | The user declined the review dialog | Run again and read the dialog |

An author looks the code in an install error up in the error table above and changes only the field or file that row names, because each code names one cause. [AUT-802]

## Activation

| Symptom | Likely cause | Fix |
|---|---|---|
| Plugin never starts | Lazy activation, and nothing used a contribution | Invoke a command, or add a trigger to `activation` |
| `activate` times out (2 s) | Slow work in `activate` | Return at once. Start the work as a request. See [Lifecycle](/pages/lifecycle/) |
| `activate` traps | A call failed and the code unwrapped the error | Handle every `Result` |
| Plugin starts but timers are gone | Timers are lost on restart | Arm timers in `activate` every time |
| `not-permitted` at start | The user declined an optional grant | Run with a reduced feature set |
| Server side active, client side not | Each side activates on its own | Check `activation` and the log of each side |
| `pane-not-ready` | A call on a pane that is still starting | Wait for the pane event and retry |
| Migration fails and the old version returns | `migrator` returned an error or trapped | Fix `migrator`. The backup was restored. See [Data migration](/pages/guide-data-migration/) |
| `migration.data-newer` | An older plugin met data from a newer version | Install the newer version, or run the command again with `--restore-backup` or `--discard-data` |

## Run time

| Symptom | Likely cause | Fix |
|---|---|---|
| Plugin disabled after three failures in 10 minutes | Repeated traps | Fix the first trap in the log. Then `ilium plugin reset-failures <id>`, which clears the count without starting the plugin; turn it on again in Settings > Plugins or with `ilium plugin enable <id>` |
| Event handler trap on time | A handler ran over the 20 ms default (50 ms is only the T1 ceiling of `event_deadline_ms`; T2 and T3 stop at 20 ms. A hook handler may use 5 to 50 ms at any tier) | Move slow work to jobs or requests |
| Command trap on time | Over 50 ms (default for all tiers; a T1 plugin may raise `command_deadline_ms` to 100 ms) | Return a pending state and finish later |
| Render trap | `render` over 10 ms (default 10 ms; only a T1 plugin may raise `render_deadline_ms`, up to 20 ms; T2 and T3 stay at 10 ms), or a tree over its limit | Cut the tree. See [Performance](/pages/performance/) |
| Tree over 4,096 nodes or 1,024 KiB | Too many rows | Window the list. See [Patterns cookbook](/pages/guide-patterns-cookbook/) |
| `quota-exceeded` | A limit is full (storage, timers, requests) | Free space, or lower use |
| `rate-limited` | Toast, status segment or request rate | Coalesce updates |
| `busy` | The host is serving too many requests | Retry with backoff |
| `timed-out` | A request ran past its limit | Make the work smaller or use a job |
| `conflict` on a submission | The state changed since it was read | Read again and retry |
| `duplicate` outcome | The same `dedup-key` was seen in 10 minutes | This is correct for a retry. Use a new key for new work |
| `unsupported` | The host or the target does not offer it | Check before use and degrade |
| Memory trap | Over `memory_mib` (32 MiB by default) | Free large buffers. Raise the limit in `limits` only inside the tier ceiling |
| Widget loses its selection on every render | Missing or changing `key` | Give a stable `key` |
| A list shows all rows after scroll | Reading `selected` and `offset` as your own state | These are host-owned. Read with `ui.widget-state` |
| Timer never fires in a test | The fake host does not run timers by itself | Call `advance_clock(ms)`, then `settle()` |
| State gone after a crash | State was only in memory | Write restore bytes or storage |
| Text misaligned for wide characters | Width counted in characters | Use `display-width`. See [Internationalisation](/pages/guide-internationalisation/) |

## Update

| Symptom | Likely cause | Fix |
|---|---|---|
| Update stays on the old version | A failed stage rolled back | Read `ilium plugin logs <id>` and the update result |
| Old settings lost | A settings key was renamed | Keep old keys, or migrate them |
| Old data unreadable | Layout changed with no `migrator` | Raise `data_version` and add `migrator`. See [Compatibility and upgrades](/pages/guide-compatibility-and-upgrades/) |
| Users asked to review grants again | A new grant was added | Expected. Say so in the release notes |
| A command no longer works for users | Its id changed | Keep the old id as a hidden alias |

## Release

| Symptom | Likely cause | Fix |
|---|---|---|
| Users cannot verify the package | No SHA-256 or fingerprint published | Publish both. See [Publishing and support](/pages/guide-publishing-and-support/) |
| `sign.key-permissions` | The key file is readable by others | `chmod 600` the key |
| `trust.key-exists` | `keygen` found existing key files | Choose another `--out` folder |
| Users report a problem you cannot see | Traces are redacted | Ask for `logs`, `status` and `stats`, not unredacted traces |

### Privacy requests about your plugin's data

A user may ask you for a copy of their data, or for its erasure. You never receive the user's data, so the work runs on the user's machine with the host's commands, and you help by naming the right command and by checking the result. The user-facing rules are in [Plugin privacy](/pages/user-privacy/).

1. **Find out which request it is.** A copy is an export. Erasure removes the data. Ask which plugin id is meant, and whether the user wants one plugin or all of them.
2. **Ask for a copy.** The user runs `ilium plugin export-data`. It writes one folder with a `manifest.json` (format version `1`, the Ilium version, the UTC time and a SHA-256 for each file) and one subfolder per plugin. It refuses a folder that is not empty, with `export.target-not-empty`, so the user names a new folder. It covers plugin settings, grants, plugin logs, audit records and crash records. The plugin's own storage is copied by `ilium plugin data export <id> <dir>`, which writes each bucket as JSON lines. The command needs no network. [USR-245], [USR-323], [HST-656]
3. **Do not ask for the files themselves.** When the user needs to show you a problem, ask for `ilium plugin support-bundle <id> --out <file>` instead. It holds the newest 200 lines of the plugin log, the stats and the crash records, and no plugin data folder and no grant secrets. It is 8 MiB or smaller, and its summary states any lines it dropped. [USR-221], [USR-223], [USR-224]
4. **Erase one plugin.** The user runs `ilium plugin erase-data <id>`. It shows the plugin name and the total size, and the default answer is No. It removes the plugin's settings, grants, data, storage, logs, crash records, pre-migration backups, cache and consent records. It reports what it kept. [USR-246], [USR-326], [USR-327]
5. **Erase every plugin.** The user runs `ilium plugin erase-data` without an id. The command asks the user to type `erase`, or accepts `--yes-erase-all` for a script. [USR-327]
6. **Uninstall first.** Recommend `ilium plugin uninstall <id>` before erasure, so the plugin cannot write new data while it is erased. Erasure works on a plugin in any state, even when its code does not load. [USR-328]
7. **Check the result.** The command prints one line per data kind, `erased <kind>: <n> files, <bytes> bytes, remaining 0`, and exits with 0 when every kind it did not keep is empty. If it exits with 3, it lists the paths it could not delete under `erase.incomplete`. The user sends you those lines, not the files. [USR-325]
8. **Name what is kept.** The audit log is kept for `audit.keep_days` (365 days by default) even after erasure. The erase report names it. Copies the user wrote elsewhere, such as a file from `ilium plugin export <id>`, are not deleted by the command, and the user must delete them. [SEC-224], [USR-248], [HST-977]
9. **Handle the data you hold.** If your plugin sent the user's data to a host or service you run, erasure on the user's machine does not reach it. You must erase that copy yourself, and you should tell the user that you did. Each party answers for the data it holds ([Plugin privacy](/pages/user-privacy/)).

Requirement ids of the commands are in [Plugin privacy](/pages/user-privacy/) and [Install, update and uninstall](/pages/reference-install-update-uninstall/).

## When it still fails

`ilium plugin bisect` halves the set of enabled third-party plugins, so it finds a plugin that clashes with yours. Start Ilium in safe mode to see whether a third-party plugin is the cause. See [Safe mode and troubleshooting](/pages/user-safe-mode-and-troubleshooting/). A report to the host project SHOULD contain the Ilium version, the plugin id and version, the manifest, the last 100 log lines and a test that reproduces it. [AUT-803]

## See also

- [Debugging](/pages/debugging/) and [Debugging tools](/pages/reference-debugging-tools/)
- [Errors and limits](/pages/reference-errors-and-limits/)
- [Quality checklist](/pages/guide-quality-checklist/)
- [FAQ](/pages/guide-faq/)

API version checked: `ilium:plugin@0.1.0`.
