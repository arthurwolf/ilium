---
layout: page
title: Install, update and uninstall reference
permalink: /pages/reference-install-and-update/
---

## Install, update and uninstall reference

This page specifies what the host does when a plugin is installed, updated, rolled back or removed, and where it keeps the result. A developer can implement the install transaction from it. A test author can derive crash, concurrency and update tests from it.

An **install** adds a plugin version to the machine. An **update** replaces an installed version with a higher one. A **downgrade** installs a lower one. A **rollback** returns to the version that was active before. An **uninstall** removes the plugin. A **transaction** is a change that either completes fully or leaves the earlier state intact. The **active version** is the one the host loads. A **session** is one running Ilium server with its clients.

Requirement ids such as `PKG-260` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The validation stages: [Package format reference](/pages/reference-package-format/).
- Trust and tiers: [Signing and trust](/pages/reference-signing-and-trust/).
- The runtime states: [Lifecycle](/pages/lifecycle/) and [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/).

## Install sources

| Source | Form | Notes |
|---|---|---|
| Local file | `ilium plugin install ./notes-1.2.0.iliumpkg` | The default |
| URL | `ilium plugin install https://example.org/notes-1.2.0.iliumpkg` | HTTPS only |
| Registry id | `ilium plugin install com.example.notes[@range]` | Needs a configured registry |
| Bundle | `ilium plugin install --from-bundle <dir> <id>` | Offline. See [Registry reference](/pages/reference-registry/#air-gapped-installs) |
| Project recommendation | A prompt from `.ilium/plugins.toml` | Never automatic. See [Finding and installing plugins](/pages/user-plugin-store/) |
| Development | `ilium plugin dev` | Tier T3, channel `dev` |

| Id | Rule |
|---|---|
| [PKG-260] | A URL install downloads the file into the cache folder (`<cache dir>/ilium/plugins/downloads/`), then treats it as a local file. A URL with the scheme `http` is refused with `install.insecure-url`. The download follows at most 3 redirects, all to HTTPS. |
| [PKG-261] | The download is at most 300 MiB. A larger response is aborted with `install.size`. The server's content type is ignored. The file is judged only by the package format. |
| [PKG-262] | A URL install of an unsigned package needs `--allow-unsigned`, as a file does. A URL does not change the trust rules. |
| [PKG-263] | A registry id with a range resolves through [Versions and dependencies](/pages/reference-dependency-resolution/) to one version and then installs that package. The recorded `source` is `registry` and the record keeps the registry URL. |
| [PKG-264] | A file named in a project recommendation is treated like any other source after the user confirms it. The host MUST NOT download or install a recommended plugin without an explicit user action. |

## On-disk state

The folder layout, the registry file `installed.json`, the lock `installed.lock`, the immutable store `store/<id>/<version>/` and the staging area are owned by [Reference, install, update and uninstall](/pages/reference-install-update-uninstall/) and listed for users in [Plugin files on disk](/pages/reference-plugin-files/). This page uses those names and does not repeat them. It adds the distribution fields that a source, a channel and a signature need.

| Id | Rule |
|---|---|
| [PKG-265] | Each plugin entry of `installed.json` follows the one schema of [Install, update and uninstall](/pages/reference-install-update-uninstall/#the-registry-file): `source` is `bundled`, `file`, `url`, `registry` or `dev`; `channel` is `stable`, `beta`, `dev` or `file`; `key_id` is 64 lowercase hexadecimal characters or null; `package_sha256` and `manifest_sha256` are 64 lowercase hexadecimal characters; `skip` and `declined` are lists of version strings; `installed_at` is RFC 3339 in UTC with the suffix `Z`; `update` is null or `{ candidate, state, attempts, started_at }`. The page that owns the schema is the only place that lists the fields. |
| [PKG-266] | The package file is kept as `store/<id>/<version>/package.iliumpkg` next to the unpacked files, so that `verify` and `rollback` need no network. Files in `store/` are written once and made read-only. An update writes a new folder and never changes an old one. |
| [PKG-267] | The user trust store is `$XDG_STATE_HOME/ilium/trust.json` (Linux `~/.local/state/ilium/trust.json` when `XDG_STATE_HOME` is unset), mode 0600, written atomically. The variable `ILIUM_TRUST_STORE` replaces it and holds an absolute file path. The format is that of [Signing and trust](/pages/reference-signing-and-trust/#the-trust-store). Downloads in progress live in `<cache dir>/ilium/plugins/downloads/`. Cached registry metadata lives in `<cache dir>/ilium/plugins/registry/<hash-of-url>/`. |
| [PKG-268] | The compiled cache `<cache dir>/ilium/plugins/compile/` is derived data, keyed by the component SHA-256 and an engine id (the Wasmtime version and settings). The host MUST be able to delete it at any time and recompile. |
| [PKG-269] | A crash leaves `installed.json` unchanged, because the registry file is the single commit point (steps 6 below). Orphans in `staging/` are deleted at the next start. |

## The install transaction

An install is the following steps in order. A failure at any step leaves the earlier state.

1. **Acquire** the lock `installed.lock`.
2. **Fetch**: read the file, or download it (URL, registry).
3. **Validate**: run stages 1 to 10 of the [validation order](/pages/reference-package-format/#the-validation-order).
4. **Review**: show the review dialog, or read `--grant` and `--yes` flags.
5. **Stage**: copy the package to `staging/<uuid>/`, unpack it, make files read-only, then rename the folder to `store/<id>/<version>/`.
6. **Commit**: write the new `installed.json` (atomic rename). This is the single point at which the install becomes true.
7. **Notify**: tell running servers that the state changed.
8. **Release** the lock.

| Id | Rule |
|---|---|
| [PKG-270] | A second install, update or uninstall that starts while the lock is held polls every 100 ms for up to 30 s and then fails with `install.locked`, exit code 4 (LIF-066). The lock is an exclusive OS advisory lock, so a crashed holder releases it by process exit and no takeover rule exists. |
| [PKG-271] | A crash before step 6 leaves the previous `installed.json`. The next start deletes the `staging/` directories that `installed.json` does not reference. A crash after step 6 and before step 7 leaves a complete install that the running servers see when they watch `installed.json` (LIF-089). |
| [PKG-272] | The host MUST NOT instantiate any component before step 6 has finished and step 7 has been sent. |
| [PKG-273] | The platform gate is stage 9: on Windows and macOS an install of a T2 or T3 package is refused with `package.platform-unsupported` before the review dialog. T1 bundled plugins are not installed by this path. |
| [PKG-274] | An install of an id that is already installed at the same version with the same manifest SHA-256 is a no-op: it exits 0 and prints `install.already-installed` as a warning. With `--reinstall` it rewrites `store/<id>/<version>/` from the package. A different manifest SHA-256 at the same version is refused at stage 9 with `package.version-reused`, exit 1. The manifest hash is compared, not the package hash, because the signature document changes the package bytes (LIF-072). |
| [PKG-275] | `--dry-run` runs stages 1 to 10 of the [validation order](/pages/reference-package-format/#the-validation-order) without writing anything; stage 11 (the review) is printed as records and never asked, as [PKG-427] says. It prints the plan and exits with the code the real install would give (0 if all stages pass). |

### Running servers and several sessions

| Id | Rule |
|---|---|
| [PKG-276] | The installed set is per user, not per session. Every session of the same user sees the same `installed.json`. |
| [PKG-277] | Step 7 sends the message `plugins-changed` to every server of the user. A server that cannot be reached reads `installed.json` when it next starts or when its watcher sees the file change. |
| [PKG-278] | A new plugin is registered and activated by its triggers in each server that gets the notice. An update does not touch running instances: the candidate waits in `store/<id>/<version>/` with `update.state: staged`, and the old version keeps running until it deactivates with reason `upgrade`, then the candidate activates. The server does this at the next safe point: when the plugin is idle, or at the next server start. The user can force it with `ilium plugin restart <id>`. |
| [PKG-279] | When two sessions run the same plugin, each switches independently. The order across sessions is not defined, and no plugin may rely on two sessions running the same version at one moment. State kept in `data/` is shared, so a plugin that changes its data format MUST handle both versions during the switch. |

## Update

An update is an install of a higher version of an installed id.

| Id | Rule |
|---|---|
| [PKG-280] | Versions compare with the precedence of [Versions and dependencies](/pages/reference-dependency-resolution/#plugin-versions). A prerelease is lower than its release. An equal version is not an update. |
| [PKG-281] | Grants: new grants (strings that the active version did not request) are asked in the review. Removed grants are dropped from the record. Grants that the user declined before stay declined and are not asked again, unless the new manifest requests them again in a different form (a different argument is a different grant). |
| [PKG-282] | A new high-risk grant, a new `replaces` entry or a new high-risk extension point contribution always needs confirmation. `--yes` does not confirm it. The flag `--grant <grant>` for that exact grant does. |
| [PKG-283] | A publisher key change follows [Signing and trust](/pages/reference-signing-and-trust/#publisher-key-change-and-rotation). |
| [PKG-284] | Settings values, plugin data and storage are kept. The old version deactivates with reason `upgrade`, which lets it save state, and the new version activates and reads the same data. See [Lifecycle](/pages/lifecycle/). |
| [PKG-285] | After a successful update the host keeps the previous version folder in `store/` and deletes older ones, which matches LIF-076: exactly one previous version stays on disk. |
| [PKG-286] | The candidate becomes `active_version` only after its first `activate` returns `ok` on the server side. If that first `activate` fails, traps or misses its deadline, the host rolls back at once to the previous version and its pre-update data snapshot, marks the candidate `skip`, logs `update.rolled-back` and does not count the failure in the failure window (LIF-075). Failures after a successful first activation follow the normal failure window and never roll back by themselves. |

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin update example.notes
<span class="t-dim">step</span>   resolve    1.2.0 > 1.3.0
<span class="t-ok">ok</span>     verify     signature, key 5647 5aa7
<span class="t-warn">new</span>    grant      http:api.example.com
<span class="t-dim">asked</span>  kept       tree.read ui.panels
<span class="t-ok">ok</span>     commit     1.3.0 active at restart
<span class="t-hi">done</span>   example.notes 1.3.0
</pre>
<figcaption>Rendered at 48 columns. Illustrative. An update that asks for one new grant.</figcaption>
</figure>

## Downgrade

| Id | Rule |
|---|---|
| [PKG-287] | A package whose version is lower than the active version is refused with `package.downgrade` unless `--allow-downgrade` is given. |
| [PKG-288] | A downgrade or rollback to a version whose `data_version` is lower than the stamp of any bucket in scope is refused with `migration.data-newer`, exit 1, unless `--restore-backup` or `--discard-data` is given (LIF-079). The host never converts newer data downward. |
| [PKG-289] | A downgrade keeps the replaced newer version as the one previous version on disk, so a later update to it needs no download. |

## Rollback

| Id | Rule |
|---|---|
| [PKG-290] | `ilium plugin rollback <id>` makes the one previous version in `store/` active. It runs the same transaction as an update, but without the version comparison, and keeps the current grants where the old manifest still asks for them. |
| [PKG-291] | Rollback is refused with `install.no-previous` when no earlier version is on disk. |
| [PKG-292] | After a rollback the rolled-back version is marked `skip` and is not offered by update until a still newer version exists or the user clears the mark with `--unskip`. |

## Uninstall

| Id | Rule |
|---|---|
| [PKG-293] | `ilium plugin uninstall <id>` deactivates the plugin (reason `disabled`), withdraws all of its contributions, removes the record from `installed.json` and deletes `store/<id>/`. |
| [PKG-294] | Without `--purge` the folders `data/<id>/` and `storage/<id>/`, the stored settings and `backups/<id>/` remain, so a later reinstall finds its configuration. No prompt is shown: the command prints the kept paths as `warning` lines (LIF-082). Grants, first-use answers, durable timers, failure counters and decorations are deleted on every uninstall (LIF-083), so a reinstall asks for grants again. |
| [PKG-295] | `--purge` also deletes `data/<id>/`, `storage/<id>/`, `backups/<id>/`, `locks/<id>.*`, the plugin log, the stored settings values of the plugin (including project-scoped ones in `.ilium/` of the current project only), the plugin cache and the compiled cache entries. It never deletes anything outside these paths. It prints the number of bytes freed. The grant entry is deleted without `--purge` as well. |
| [PKG-296] | The trusted key stays in the trust store. Uninstall never edits trust. |
| [PKG-297] | Dependants follow [Versions and dependencies](/pages/reference-dependency-resolution/#effects-on-update-and-uninstall). A plugin installed by a system administrator as read-only is refused with `policy.managed-plugin`. |
| [PKG-298] | The uninstall of a plugin that is currently `failed` or `disabled` works the same way. |
| [PKG-299] | A T1 bundled plugin cannot be uninstalled. It can be disabled. The command is refused with `install.bundled`. |

## Auto-update

| Id | Rule |
|---|---|
| [PKG-300] | The setting `[plugins] auto_update` has the values `off` (default), `notify` and `install`. With `off` the host never checks the network by itself. With `notify` it checks and shows a notice. With `install` it checks and installs updates that need no new grants. |
| [PKG-301] | The check runs at server start, then every 24 hours while the server runs, with a random delay of up to 1 hour. `[plugins] auto_update_interval_hours` (1 to 168) changes it. |
| [PKG-302] | Auto-install never installs an update that adds grants, adds `replaces`, changes the publisher key, or crosses to a channel the user did not select. Those updates produce a notice and wait. |
| [PKG-303] | When the network is marked metered (the operating system says so, or `[plugins] metered = true`), the host does not check. |
| [PKG-304] | A notice lists the plugin, the old and new version, the new grants and the link to the changelog. The user can **Update**, **Later** (which hides the notice for that version for 7 days) or **Skip this version**. |
| [PKG-305] | Auto-update is a policy item. An administrator can set `off` and lock it. |

## Codes and JSONL lines

| Code | Cause |
|---|---|
| `install.locked` | Another transaction holds the lock for more than 30 s |
| `install.insecure-url` | A URL used `http` |
| `install.download-failed` | The network failed during a download |
| `install.size` | A download or upload was too large |
| `install.state-corrupt` | `installed.json` was unreadable |
| `install.no-space` | The disk is full |
| `install.needs-input` | A decision is needed and no person can answer |
| `install.already-installed` | The same version is installed |
| `install.not-installed` | The id is not installed |
| `install.no-previous` | There is no earlier version for a rollback |
| `install.bundled` | The target is a bundled plugin |
| `install.cancelled` | The user closed the review dialog |
| `package.downgrade` | A lower version without `--allow-downgrade` |
| `package.version-reused` | The same version with a different hash |
| `package.platform-unsupported` | T2 or T3 on a platform with no sandbox |

Each step prints one JSONL line with `type` `progress`, `step` (the step name, one of `acquire`, `fetch`, `validate`, `review`, `stage`, `commit`, `notify`; these progress steps are their own scheme, not the seven rows of the install pipeline table in [Install, update and uninstall](/pages/reference-install-update-uninstall/), where row 5 is the rename and row 6 the `installed.json` update), `message` (human text), `current` (the number of the step, from 1) and `total` (the number of steps, 7). The final line has `type` `result` with `id`, `version`, `previous`, `state`, `tier`, `action` (`installed`, `updated`, `downgraded`, `rolled-back`, `uninstalled`) and `active_from` (`now` or `restart`). Exit codes are in [Plugin command line](/pages/reference-plugin-cli/#exit-codes) and [Distribution commands](/pages/reference-plugin-distribution-cli/).

## Worked example: a crash during an update

The user updates `example.notes` from 1.2.0 to 1.3.0 and the machine loses power at progress step 5 (`stage`), while the new folder is being written.

1. `installed.json` still says `active: 1.2.0`, because progress step 6 (`commit`) did not run.
2. `staging/<uuid>/` exists.
3. At the next start, the host deletes the orphan, loads 1.2.0, and shows no error.
4. The user repeats the update. It succeeds.

If the power fails after progress step 6, `installed.json` says 1.3.0 and `store/example.notes/1.3.0/` is complete, because the rename of the folder came first. The server loads 1.3.0.

## Edge cases

| Situation | Result |
|---|---|
| The disk is full at step 5 | The staging folder is removed. The error is `install.no-space`, with the free space |
| Two `install` commands for different ids | The second waits for the lock, up to 30 s |
| The package file is deleted during install | The host works on its own copy from step 2 |
| `--yes` without `--grant` flags | Installs with the default grants: normal grants on, high-risk grants off |
| An id that is both bundled and in a file | Refused with `manifest.id-reserved` |

## What the user sees

The user sees a progress line per step, the review dialog and the result. After an update the plugin shows **restart pending** in Settings > Plugins if the old version is still running. See [Finding and installing plugins](/pages/user-plugin-store/).

## See also

- [Package format reference](/pages/reference-package-format/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
- [Registry reference](/pages/reference-registry/)
- [Plugin command line](/pages/reference-plugin-cli/)
- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Reference, install, update and uninstall](/pages/reference-install-update-uninstall/)
- [Lifecycle](/pages/lifecycle/)
