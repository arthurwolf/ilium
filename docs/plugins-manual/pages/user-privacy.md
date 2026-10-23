---
layout: page
title: Plugin privacy
permalink: /pages/user-privacy/
---

## Plugin privacy

This page answers three questions about plugins and your privacy. What can a plugin see? Where can it send what it sees? What does Ilium itself send about plugins? It also tells you where plugin data is stored on your machine and how to delete it.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. The statements here are the specified behaviour. A plugin you install is software written by someone else. The permission system limits it, but it cannot make a dishonest plugin honest. Install only what you trust.</p>
</div>

### Contents

- The short version
- What a plugin can see
- Where a plugin can send data
- What Ilium itself sends
- What is stored on your machine
- How long each kind of data is kept
- Delete a plugin's data
- Crash records and pre-migration backups
- Core plugins and AI providers
- Errors and edge cases
- Who is responsible for your data
- Requirements

### The short version

- A plugin sees only what you grant, plus its own folders. See [Permissions explained](/pages/user-permissions-explained/).
- A plugin can send data only to hosts you granted, over HTTPS. Plugins cannot open a raw network connection.
- Ilium does not report plugins, their ids or their use to anyone. It has no plugin telemetry.
- Everything about a plugin is stored in your own folders. You can delete it all.

- [USR-060] Ilium MUST NOT send the list of installed plugins, plugin use, plugin logs or plugin errors to any remote party. Test: run Ilium for one hour with five plugins and request logging, and assert that no request carries a plugin id.

### What a plugin can see

| A plugin can see | Only if |
|---|---|
| The names and structure of your project tree | you granted `tree.read` |
| Titles, sizes and focus of panes | you granted `panes.read` |
| The text in a terminal pane | you granted `terminal.read-screen` or `terminal.read-cells` |
| Older terminal lines | you granted `terminal.history` (HIGH) |
| What you typed to a coding agent | you granted `agent.read-prompts` (HIGH) |
| An agent's saved conversation | you granted `agent.transcript` or `fs.read:agent-sessions:<agent>` (HIGH) |
| Files in the project folder | you granted `fs.read:project` |
| Files in other folders | you granted a read of that specific folder |
| What you copied | you granted `clipboard.read` (HIGH) |
| Your voice | you granted `audio.capture` (HIGH) |
| Sound your computer plays | you granted `audio.loopback` (HIGH) |
| Your approximate location | you granted `location.observer` (HIGH) |
| Your key presses | you granted `keys.intercept` (HIGH) |
| Another plugin's data | never directly. Only through a service or a message that the other plugin chose to offer, and you granted |

A plugin never sees the process environment, your home folder as a whole, other users' files, or the memory of Ilium or of other plugins. It sees what Ilium passes to it and nothing else.

Terminal text may contain passwords, keys and private notes. Think twice about terminal permissions for a plugin whose job does not need them.

- [USR-061] A plugin MUST NOT read terminal text, history, agent prompts, transcripts, the clipboard, audio or location without the matching grant. Test: for each item call the function without the grant and assert `not-permitted`.

### Where a plugin can send data

A third-party plugin runs in a sandbox with no network of its own. The only ways data can leave are listed here.

| Way out | What guards it |
|---|---|
| `http:<host>` | You granted that exact host or pattern. Only HTTPS, except for the loopback hosts `localhost`, `127.0.0.1` and `[::1]` when the grant names them. A redirect to another host needs its own grant |
| `http:setting:<key>` | The host comes from a setting. Ilium asks you the first time for each new host |
| `inference` | The plugin uses the AI providers you set up. What it sends goes to the provider you chose, as described in [Inference providers](/pages/core-plugin-inference/) |
| `location.geocode` | A place name goes to an online lookup service, Photon, with Nominatim as fallback. Ilium asks the first time |
| `open-external` | Ilium asks before it opens each site in your browser. The browser then contacts the site |
| `process.run:<program>` | The program runs in the same sandbox, with no network |
| `notify.desktop`, `sound.play` | Local only |
| Messages to another plugin | Needs `messages.peer:<plugin-id>`. The other plugin may have its own grants for the network |

A plugin that holds both a read permission and a network permission can send what it reads to that host. The dialog does not warn you about the combination. Think about it yourself: a notes plugin that reads terminal text and can talk to `*.example.com` can pass that text to example.com.

- [USR-102] A T2 or T3 plugin MUST have no network access except through `http:` grants and the providers named above. Test: from a test plugin, try a raw socket and a request to an ungranted host, and assert failure.
- [USR-103] Ilium MUST refuse a request to a host that the plugin has no grant for, including a redirect target. Test: redirect from a granted host to an ungranted host and assert `not-permitted`.

The `location.geocode` service sends the query to the provider. A search sends the query text, between 2 and 200 characters, to Photon, with Nominatim as fallback. A reverse lookup sends the coordinates of the point. The host keeps each result for 24 hours, keyed by query text, so a repeated search does not contact the provider again. The provider's own terms and retention apply to what it receives. Ilium does not control them.

- [USR-957] The host MUST send to the geocode provider only the query text of a search or the coordinates of a reverse lookup, and MUST NOT keep a cached result longer than 24 hours. Test: run one search and one reverse lookup against a capture proxy and assert that each request carries only those fields; assert that a cached result is gone 24 hours later.

### What Ilium itself sends

Ilium does not contact any server on its own to check, list, download or update plugins. The only exception is a registry you set up yourself: the store then fetches that registry's metadata when you open it (see [the plugin store](/pages/user-plugin-store/)), and nothing else is fetched. Installing a plugin does not use the network. Reading a package does not use the network. Verifying a signature does not use the network, because the keys you trust are stored on your machine.

Ilium's own features that use the network are unchanged: for example, AI providers you configure. Core plugins contact the same services as before. Their page in Settings lists them. See [Inference providers](/pages/core-plugin-inference/).

- [USR-062] Install, verify, update, uninstall and list of local packages MUST NOT use the network; only the store's registry fetch from a registry the user configured (the refresh sequence of PKG-370 in [the registry reference](/pages/reference-registry/)) may, and never with no registry configured (PKG-532). Test: run each in a network namespace without interfaces and assert success.

A registry you configure receives each request you make to it, as any web server does. Each request carries your IP address and the `User-Agent` header. The registry operator decides how long its access logs keep them. Ilium does not control that period.

- [USR-956] A registry operator MUST publish a privacy notice that states how long its access logs keep the IP address and `User-Agent` of each request, and that period MUST NOT exceed 30 days (proposed, owner to confirm). Test: pending: test to be written.

### What is stored on your machine

| What | Where | Who can read it |
|---|---|---|
| Installed packages | your plugin store | you |
| Your permissions and answers | `grants.json` and `consents.json`, one file each for all plugins | you |
| Plugin settings | your `config.toml` and the project's `.ilium` folder | you, and anyone who gets those files |
| Plugin data and notes | one folder and database per plugin | you and that plugin |
| Plugin logs | one file per plugin | you |
| Crash records | the server state folder `<state>`, never a plugin's folder | you |
| Pre-migration backups | `<data>/plugins/backups/<id>/<version>-<utc timestamp>/`, copies of the plugin's data and storage buckets | you |
| Audit log | always on; nothing to enable | you and the administrator |

A project's `.ilium` folder may hold plugin settings and the plugin's project storage. If you share or commit that folder, you share them. Permissions are never stored there. The exact paths are in [Plugin files on disk](/pages/reference-plugin-files/).

Plugin logs are plain text. Plugins are told not to write secrets there, but Ilium cannot check. Read a log before you share it.

### How long each kind of data is kept

Each kind of plugin data that Ilium keeps is listed here with its default retention and the command that deletes it. The locations are in the table above. [USR-348] requires a test that compares each retention value with the code constants.

| What | Default retention | Deleted by |
|---|---|---|
| Installed packages | Until you uninstall the plugin or update it | `ilium plugin uninstall <id>` removes the package ([USR-208]) |
| Permissions and answers (`grants.json`, `consents.json`) | Until you revoke them or uninstall the plugin | A default uninstall removes grants ([USR-208]). `ilium plugin erase-data` removes grants and consent records ([USR-246], [USR-326]) |
| Plugin settings | Until you delete them | `ilium plugin uninstall <id> --purge`, `erase-data`, or Recovery page > Forget settings. A default uninstall keeps them ([USR-208]) |
| Plugin data and notes | Until you delete them | `ilium plugin uninstall <id> --purge` or `erase-data`. An uninstall without `--purge` leaves them inert |
| Workspace search index: one vector per pane, made from pane titles and screen snippets (`create-index`, see [Workspace search](/pages/core-plugin-search/#index-contents-redaction-and-retention)) | The search plugin's data folder | Until the plugin removes a pane's entry (`remove-entries`), replaces it when the text changes, or drops the index (`drop-index`). The manual gives no fixed period (owner to confirm) | `ilium plugin uninstall <id> --purge` or `erase-data`. An uninstall without `--purge` leaves it inert |
| Recap text: the latest short and long recap of each pane, used for the `stale` state (see [Recap](/pages/core-plugin-recap/)) | The Recap plugin's data. The Recap page does not name the file | Until the pane is recapped again. The manual gives no fixed period (owner to confirm) | `ilium plugin uninstall <id> --purge` or `erase-data`. An uninstall without `--purge` leaves it inert |
| Spend per day: one key `day/<day>` per day in project storage (see [Cost](/pages/core-plugin-cost/)) | The project's `.ilium` folder | Until deleted. The Cost page gives no pruning rule (owner to confirm) | `--purge` while the project is open, or delete the plugin's files in the `.ilium` folder. See [Delete a plugin's data](#delete-a-plugins-data) |

The search index, and the recap of a shell pane, are made from text on a terminal screen, and a screen can show a token or a typed password. The search index does not filter secrets before it embeds them, as [Workspace search](/pages/core-plugin-search/#index-contents-redaction-and-retention) explains, so treat that index as holding screen text. A default uninstall keeps the search index and the recap text on disk, inert, until `--purge` or `erase-data` removes them. The Cost day totals hold spend figures, not screen text.
| Plugin logs | The active file and 4 rotated files. Rotated files are deleted after 14 days at server start. After an uninstall, logs are deleted 7 days later | `--purge` deletes them at once, and `erase-data` deletes them ([USR-246]) |
| Crash records | 30 days and at most 200 records ([USR-236]; both values are proposed, owner to confirm, see [SEC-955](/pages/reference-audit-log/)) | `ilium plugin uninstall <id>`, with or without `--purge`, deletes all crash records of that plugin. `erase-data` deletes them ([USR-326]) |
| Pre-migration backups | Only the newest one per plugin, kept after each committed update ([USR-237]) | `--purge` deletes all of them. An uninstall without `--purge` keeps them ([USR-238]). `erase-data` removes them ([USR-326]) |
| Audit log | `audit.keep_days`, default 365 days ([SEC-224]) | `erase-data` does not delete it before the period ends. Erasure names the kept records ([USR-248]) |
| Plugin cache | No fixed period. The host may evict it at any time | A default uninstall ([USR-208]) and `erase-data` ([USR-326]) |
| Support-bundle staging files | Not stated in the source pages (owner to confirm) | `erase-data` ([USR-326]) |
| `installed.json` extras that hold settings values | Until erased | `erase-data` ([USR-326]) |
| Geocode results | 24 hours, keyed by query text ([USR-957]) | Not in the erase list of [USR-326] (owner to confirm) |
| Registry metadata cache | Until a newer verified copy replaces it, for offline use (see [the registry reference](/pages/reference-registry/)) | Not stated in the source pages (owner to confirm) |

- [USR-955] Every kind of plugin data the host keeps MUST have one row in the retention table, with its default retention and the command that deletes it. A kind with no deleting command MUST be named as kept. Test: pending: test to be written.

### Delete a plugin's data

| I want to delete | Do this |
|---|---|
| Everything of one plugin | `ilium plugin uninstall <id> --purge` |
| A plugin's settings only | Recovery page > Forget settings |
| A plugin's logs | `--purge` |
| A plugin's pre-migration backups | `ilium plugin uninstall <id> --purge` |
| A plugin's crash records | `ilium plugin uninstall <id>`, with or without `--purge`, or `ilium plugin erase-data` (see [Crash records and pre-migration backups](#crash-records-and-pre-migration-backups)) |
| One project's plugin data | Delete the plugin's files in the project's `.ilium` folder, or use `--purge` while the project is open |
| My trusted publisher keys | `ilium plugin trust remove <key_id>` |
| Everything | Remove the plugin folders listed in [Plugin files on disk](/pages/reference-plugin-files/) with Ilium stopped |

`--purge` also records a purge note in Ilium's data folder (`<data>/plugins/purged.json`, holding the plugin id and the purge time). The next time you open a project that holds data of that plugin, Ilium deletes the plugin's project and session data and then removes that project from the note. A project you never reopen keeps its data, and the note stays until every known project has been cleaned. A plugin that was uninstalled without `--purge` leaves its data inert: no other plugin can read it, and the same plugin reads it again if you reinstall.

### Crash records and pre-migration backups

Two kinds of plugin data are kept outside the plugin's own folder. Both appear in the tables above.

Crash records are written to the server state folder `<state>` when a plugin helper crashes, is killed or runs out of memory. Each record holds the time in UTC, the plugin id and version, the side (`t1`, `t2` or `t3`), the reason (`crash`, `kill` or `out-of-memory`), the exit status and the name of the export that was running. A record holds no payloads, stack memory or file contents. Run `ilium plugin support-bundle <id> --out <file>` to copy the records into the bundle's `crashes.txt` before you remove anything.

- [USR-236] A crash record MUST be removed once it is more than 30 days old. When more than 200 records exist, the oldest records MUST be removed first until 200 remain. Test: doc check in ilium/tests/plugin_user_flows that this section and the storage table give the 30-day and 200-record limits.

Pre-migration backups are copies of a plugin's data and storage buckets, made before an update migrates them, so that a failed migration can be undone. They are kept in `<data>/plugins/backups/<id>/`. Set `migration_backup_mib = 0` in the `[plugins]` table of `config.toml`, or pass `--no-backup` to the update, and Ilium makes no copy. Without a copy, a failed migration cannot be undone.

- [USR-237] After each committed update, Ilium MUST keep only the newest pre-migration backup of that plugin and delete the older ones. `ilium plugin uninstall <id> --purge` MUST delete every pre-migration backup of that plugin. Test: doc check in ilium/tests/plugin_user_flows that the backup rule and the purge row of the delete table name the newest-copy rule and `--purge`.
- [USR-238] `ilium plugin uninstall <id>` without `--purge` MUST keep the pre-migration backups of that plugin, so that a reinstall finds them. Test: doc check in ilium/tests/plugin_user_flows that this rule matches the uninstall rules of the install and uninstall reference.

Crash records of an uninstalled plugin are deleted at once. `ilium plugin uninstall <id>`, with or without `--purge`, deletes every crash record of that plugin and leaves the records of other plugins alone. `--keep-data` does not keep crash records. This is the rule of [SEC-955](/pages/reference-audit-log/). Run the support bundle command above first if you need the records.

### Crash-record retention: one number

One retention applies to crash data: **30 days and at most 200 records** ([USR-236], [SEC-955](/pages/reference-audit-log/)). A record is deleted when it is more than 30 days old, and when more than 200 exist the oldest go first until 200 remain. Both values are proposed, owner to confirm. The words "crash record" (this page, [USR-229]) and "crash report" ([SEC-447] in [Audit log](/pages/reference-audit-log/)) name the same thing: one JSON file in `<state>/crashes/`, mode 0600, in a 0700 folder. The 14 days in the text of [SEC-447] are superseded by [SEC-955]; the owner must change that text so the three statements agree: [USR-236], the retention table above (row "Crash records") and [USR-348] all say 30 days / 200 records, and the doc check of [USR-348] compares the table with the same two constants.

What deletes a record, and what the audit log shows:

| Cause | Deleted | Audit record |
|---|---|---|
| Older than 30 days, or the 201st-oldest and beyond | At the next server start and at the 24-hour sweep, oldest first | One `audit-retention-deleted` record per file, written before the deletion ([SEC-954](/pages/reference-admin-policy/)); `rule` is `keep-days` for age, `storage-ceiling` when the count or size ceiling applied |
| `ilium plugin uninstall <id>`, with or without `--purge` | At once, all records of that plugin | The uninstall record. No `audit-retention-deleted`, because no retention rule applied |
| `ilium plugin erase-data` | At once ([USR-326]) | The erase record of [USR-248]. No `audit-retention-deleted` |

Host crash records (`host-host-<time>.json`) belong to no plugin. Only age and count remove them, never an uninstall.

### Files a plugin writes outside Ilium's folders

A plugin granted `fs.write` for a `project`, home or custom scope writes ordinary files in that place. No Ilium command exports, erases, backs up or purges them. `export-data` ([USR-245]), `erase-data` ([USR-246]), `backup` ([HST-962]), `uninstall --purge` ([USR-209]) and `purge-user` ([HST-689]) cover only the plugin's `data`, `cache`, settings, logs, grants and the other kinds listed in [Delete a plugin's data](#delete-a-plugins-data). A file a plugin wrote to your project folder stays there after every one of them, and Ilium cannot tell which files in that folder the plugin wrote.

Ilium therefore tells you what it cannot reach:

- `ilium plugin data-report` ([USR-322]) and `ilium plugin erase-data` list, per plugin, every `fs.write` grant that allows writes outside `data` and `cache`, with its scope. They print the line `not cleaned: fs.write:<scope> (files written by the plugin stay in place)` for each. `erase-data` repeats the list in its kept-locations report ([USR-326]).
- `ilium plugin uninstall <id> --purge` and `ilium plugin admin purge-user <user> --yes` print the same `not cleaned` line for each such scope the plugin held.
- The record of what was written is the log. Every write, remove, rename and create-dir outside `data` and `cache` is logged at level `info` ([fs.write](/pages/capability-fs-write/), [Logging](/pages/api-log/)). Read it with the plugin's log file, or with `ilium plugin audit --plugin <id> --kind fs-write` (the `--kind` filter is proposed, owner to confirm). The log is deleted by `--purge` and `erase-data`, so copy the lines you need first.

To clean up, read those lines, delete the files yourself, then run the purge. Rotated logs older than 14 days are gone, so writes older than that may have no record.

### Removing Ilium itself: what stays on disk

Uninstalling the Ilium binaries (package manager, `cargo uninstall`, deleting the app) removes executables only. Every row below stays. `<data>`, `<state>`, `<cache>` and `<config>` are the roots of [Plugin files on disk](/pages/reference-plugin-files/).

| What stays | Where | Holds | Removed by a package uninstall? |
|---|---|---|---|
| Plugin store, data, storage, backups, purge notes | `<data>/plugins/` (`store`, per-plugin `data` and `storage`, `backups/`, `purged.json`) | Packages, plugin notes and settings data, pre-migration copies | No |
| Grants, consents, trust, audit, crash records, logs | `<state>/` (`grants.json`, `consents.json`, trust files, `audit/`, `crashes/`, plugin logs, `secrets.json` if no keyring) | Your answers, what happened, screen-related error context | No |
| Plugin cache | `<cache>/plugins/` | Compile cache and per-plugin caches | No |
| Plugin settings and tables | `<config>/config.toml`, tables `[plugins]` and `[plugins."<id>"]` | Settings values, limits, extension order | No |
| Per-project data | `<project>/.ilium/plugins/` and `<project>/.ilium/plugins.toml`, in every project folder you opened | Project storage, project plugin settings | No |
| Secrets in the OS keyring | Secret Service, Keychain or Credential Manager, written for [SEC-240] secrets | API keys, webhook tokens | No |
| Notification and protocol registrations | Desktop notification entries and protocol handlers the OS holds | Names and handlers | Only if the package removes them; not guaranteed |
| Files written by plugins with `fs.write` outside `data`/`cache` | Wherever the grant pointed | Anything the plugin wrote | No, and never by Ilium ([above](#files-a-plugin-writes-outside-iliums-folders)) |

**One command for a whole profile.** With the server stopped or not, run before you remove Ilium: `ilium plugin purge-all --yes` (proposed, owner to confirm). It deletes the `<data>/plugins/`, `<state>` plugin files, `<cache>/ilium/plugins/` and plugin tables of `config.toml` listed above, forgets the keyring secrets of [SEC-248], and prints counts per kind. It refuses with `purge.server-running` while a session server runs and without `--yes` it only prints the plan. The same rules as [SEC-194] apply per OS user: it touches only the calling user's roots. It does not clean project folders, plugin-written files or keyring entries it could not reach, and it names each as not cleaned.

**Manual steps** (no Ilium binary left): stop every Ilium server, delete the four folders above (the plugin parts of `<data>`, `<state>`, `<cache>`; remove the `[plugins]` tables from `config.toml` or the file), then the project folders and the keyring.

- **Project folders.** The host keeps no list of your projects, only the purge notes of [HST-675] for plugins you purged. Delete `.ilium/plugins/` and `.ilium/plugins.toml` in each project folder you used, one by one. Searching your disk for folders named `.ilium` finds them.
- **Keyring.** Ilium cannot reach the keyring once it is gone. Use the keyring's own tool: Seahorse or `secret-tool` on Linux, Keychain Access on macOS, Credential Manager on Windows, and delete the entries whose service name is `ilium`. Run `ilium plugin purge-all --yes` first when you can, since it forgets them for you.
- **Images and shared machines.** For a managed machine, the administrator runs `ilium plugin admin purge-user <user> --yes` ([HST-689]) for each user before the image is retired. Archives from `ilium plugin backup` are separate files that nothing here removes ([HST-962]).

This is the only way to make [USR-246] and [USR-248] true for a machine that no longer runs Ilium: audit records are kept for `audit.keep_days` unless the audit folder is deleted by hand, which removes them without a record.

### Core plugins and AI providers

Core plugins are made by the Ilium project and follow the same permission rules for high-risk abilities. Some of them send data to services you configure: the Inference plugin sends prompts to the AI provider you pick, the Voice plugin may use a speech service you set up (audio is sent only when that provider is remote, stays in memory, and transcripts are dropped when the voice session ends; see [Voice data and privacy](/pages/core-plugin-voice/#data-and-privacy)), and the Cost plugin may read agents' usage files. These behave as documented in the user documentation. A third-party plugin that asks for `inference` uses the same providers and the same privacy controls: the provider you choose for each purpose, and the AI admission gate, which checks every request against the `purpose` its caller named and records it in the privacy log (see [Inference](/pages/capability-inference/#risk-and-privacy-rules)). No separate setting exists for file names, paths or terminal text: a plugin sees them only through its own read grants, and sends them only if it puts them in a request. What the Inference plugin keeps and for how long is in [Inference providers](/pages/core-plugin-inference/#what-is-kept-and-for-how-long).

### Errors and edge cases

| Situation | What happens |
|---|---|
| A plugin tries a host without a grant | The call returns `not-permitted`. The log gets `refused http:<host>` |
| You revoke a network grant during a request | The request completes as `failed(revoked)` |
| The audit log | Always on. Install, update, grant, consent, trust and block events are recorded. Plugin data and settings values are not. See [Audit log](/pages/reference-audit-log/) |
| You share a project folder | Plugin settings and project storage in `.ilium` go with it. Permissions do not |
| A plugin writes a secret to its log | Ilium cannot detect it. The log rotates at 4 MiB, keeps four rotated files, deletes rotated files older than 14 days at server start, and the logs of an uninstalled plugin are deleted 7 days after uninstall; `--purge` deletes them at once |

### Who is responsible for your data

Each party answers for the data it holds. Plugin data sits in your own folders on your machine. The other holders are:

- **The Ilium project.** It makes Ilium and the core plugins. It receives no plugin data from your machine ([USR-060]).
- **A registry operator.** It holds the access logs of the requests you make to its registry ([USR-956]).
- **A plugin's publisher and the host it sends to.** A plugin sends data only to hosts you granted ([USR-102]). What a plugin does with the data it sends is the responsibility of its publisher and of that host's operator.
- **The AI provider and the geocode provider.** They hold what is sent to them, under their own terms.
- **Your administrator.** On a managed machine, the administrator sets the policy ([Administration guide](/pages/guide-administration/)).

Whether any of these parties is a controller or a processor under a particular data protection law is owner to confirm. This page states only which party holds which data.

To ask for a copy or the erasure of your data, use `ilium plugin export-data` and `ilium plugin erase-data` for data on your machine ([USR-245], [USR-246]). For data that a registry, a provider or a host holds, ask that party. Ask the Ilium project about anything it holds.

- [USR-958] The Ilium project MUST answer a written request for access to or erasure of data it holds within 30 days (proposed, owner to confirm), and MUST name the party that holds any data it does not hold. Test: pending: test to be written.

## Personal data export and erasure requirements

- [USR-245] The host MUST let a user export all plugin data held about them with `ilium plugin export-data`, covering plugin settings, project storage references, grants, plugin logs, audit records and crash records, as readable files in one folder. Test: Run the export for a profile with each data kind and assert one folder with a file per kind.
- [USR-246] The host MUST let a user erase the data of one plugin or of all plugins with `ilium plugin erase-data`, after showing counts per data kind and asking for confirmation, and MUST report what was deleted and what was kept. Test: Run erase for one plugin and assert its settings, grants, logs and crash records are gone, others are untouched and the report lists both.
- [USR-247] The export and erase commands MUST work without network access and MUST NOT send any data to a plugin or a registry. Test: Run both with networking blocked and assert success and no outbound request.
- [USR-248] Erasure MUST NOT silently drop audit records that policy requires to keep; the host MUST name them as kept, with the retention period that keeps them, and MUST delete them when that period ends. Test: Set a retention policy, erase, and assert the audit records remain and are named; advance the clock and assert they are deleted.
- [USR-322] `ilium plugin data-report [<id>]` MUST list, per plugin, each data kind held (settings, data folder, storage buckets, cache, logs, grants, consent records), its byte size, file count and the newest modification time, and MUST finish for 50 plugins in 2 s (proposed, owner to confirm) without starting any plugin. Test: create every data kind for 50 plugins, run the report with no plugin started, and assert each kind is listed with its size, count and newest time within 2 s.
- [USR-323] `export-data` MUST write one folder with `manifest.json` (format version `1`, Ilium version, UTC time, per-file SHA-256), one subfolder per plugin, JSON for structured data and original bytes for files, and MUST refuse to write into a non-empty folder with `export.target-not-empty`. Test: export into an empty folder and check each file against its SHA-256 in the manifest; export into a folder that holds one file and assert `export.target-not-empty`.
- [USR-324] An export MUST NOT modify any source file, MUST read each plugin's data under a snapshot taken while the plugin is paused or by file copy with a post-copy size and hash check, and a failed export MUST remove the partial folder it created. Test: make the export fail midway, then assert the target folder is gone and the hashes of the source files are unchanged.
- [USR-325] After `erase-data` the host MUST recount each data kind, print `erased <kind>: <n> files, <bytes> bytes, remaining 0`, exit 0 only when every non-retained kind is empty, and otherwise exit 3 (proposed, owner to confirm) with `erase.incomplete` listing paths it could not delete. Test: make one file undeletable, run `erase-data`, and assert exit 3 and that the path is listed under `erase.incomplete`.
- [USR-326] Erasing a plugin MUST remove its entries from crash records, support-bundle staging files, pre-migration backups, cache, consent records, and `installed.json` extras that hold its settings values, and MUST name any location it intentionally keeps. Test: fill each location for one plugin, erase it, and assert no entry of that plugin remains, or that each kept location is named in the report.
- [USR-327] `erase-data` for all plugins MUST require the user to type the word `erase` (or pass `--yes-erase-all` non-interactively), and for one plugin MUST show the plugin name and the total size in the prompt; the default answer MUST be No. Test: press Enter at the prompt and assert nothing is deleted; type `erase` and assert the data is deleted.
- [USR-328] `export-data` and `erase-data` MUST work for a plugin that is failed, blocked, incompatible, dormant or uninstalled with kept data, without loading its code. Test: put one plugin in each of those states, run export and erase for each, and assert that no plugin code runs.
- [USR-348] The user privacy page MUST list in one table every kind of data Ilium keeps about plugins with its location, default retention (log 14 days, crash records 30 days, audit per `audit.keep_days`, pre-migration backup newest only) and the command that deletes it, and a test MUST check each row against the code constants. Test: a test reads each retention constant in the code and compares it with the matching row of the table.


### Restoring plugin data

- [USR-262] The host MUST provide `ilium plugin import-data <folder>` that restores plugin settings, storage and grants from a folder written by `ilium plugin export-data`, for plugins already installed at the same or a newer data version; it MUST show counts per data kind and ask for confirmation before writing, MUST re-check every restored grant against machine policy, MUST NOT use the network, and MUST NOT overwrite existing data unless `--replace` is given.

### See also

- [Plugins for users](/pages/user-plugins-overview/)
- [Permissions explained](/pages/user-permissions-explained/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Security](/pages/security/)
- [Plugin FAQ](/pages/user-faq/)

API version checked: `ilium:plugin@0.1.0`.

## Production additions: T7-users-privacy-a11y

- [USR-329] A manifest MAY declare a `[privacy]` table (proposed, owner to confirm) with `collects` (list of data kinds), `retention_days` and `contact`; if the plugin requests `http:` or `telemetry`-class grants and omits it, the review dialog MUST show 'This plugin does not describe what it collects' in the warning role, and a declared value MUST be shown beside the grant rows. Test: install a plugin that requests an `http:` grant without the table and assert the warning text; add the table and assert its values appear beside the grant rows.
- [USR-330] Ilium MUST NOT give a plugin a stable machine, user or installation identifier; the per-plugin identifier available to a plugin MUST be random, scoped to that plugin, and MUST change on `--purge` or erase. Test: read the identifier from two plugins and from two installs, assert the values differ, then purge a plugin and assert its identifier has changed.
- [USR-331] No host function MUST let a plugin read another plugin's settings, data folder, grants or consent records; a request for one MUST return `not-permitted` and be written to the audit log. Test: from plugin A, request the settings of plugin B and assert `not-permitted` and one audit record.
