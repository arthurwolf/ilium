---
layout: page
title: Plugin files on disk
permalink: /pages/reference-plugin-files/
---

## Plugin files on disk

This page lists every file and folder that the plugin platform creates, where it lives on Linux, macOS and Windows, what format it has, who writes it and what removes it. It is the one table that the user pages, the administration guide and the troubleshooting page point to. A host implementer uses it to place files. A test author uses it to check what an install, a grant change or an uninstall left behind.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]` and are collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. Folder and file names below are the proposed layout. They are marked normative here so that tests can be written, and the platform owner may still rename a file. Names that other pages already use, such as <code>installed.json</code> and <code>plugins/staging/</code>, are kept.</p>
</div>

### Contents

- How paths are chosen
- The table of items
- The plugin log
- File permissions
- What each action leaves behind
- Back up and move plugins to another machine
- Errors and edge cases
- Requirements

### How paths are chosen

Ilium never writes a hard-coded `~`. It asks the operating system for three roots through the `directories` crate, with the project name `ilium` and no organisation: the configuration folder, the data folder and the cache folder. A fourth root, the state folder (`<state>`), holds the grant, consent, trust and audit files.

| Root | Linux | macOS | Windows |
|---|---|---|---|
| Configuration (`<config>`) | `~/.config/ilium` | `~/Library/Application Support/ilium` | `%APPDATA%\ilium\config` |
| Data (`<data>`) | `~/.local/share/ilium` | `~/Library/Application Support/ilium` | `%APPDATA%\ilium\data` |
| Cache (`<cache>`) | `~/.cache/ilium` | `~/Library/Caches/ilium` | `%LOCALAPPDATA%\ilium\cache` |
| State (`<state>`) | `$XDG_STATE_HOME/ilium` (default `~/.local/state/ilium`) | `~/Library/Application Support/Ilium/state` | `%LOCALAPPDATA%\Ilium\state` |

On Linux the roots follow `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` when they are set. The rest of this page writes `<config>`, `<data>`, `<cache>` and `<state>` and never repeats the system paths. A project folder (`<project>`) is the canonical launch folder of a session. Project-scope files live in `<project>/.ilium/`.

- [USR-200] The host MUST resolve `<config>`, `<data>`, `<cache>` and `<state>` (the per-OS paths of [SEC-112]) with the `directories` crate for the project name `ilium`, and MUST NOT write plugin files anywhere else. Test: start Ilium with `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` pointing at four empty folders, install a plugin, and assert that no plugin file appears outside those folders and the project's `.ilium` folder.

### The table of items

Paths start at `<config>`, `<data>`, `<cache>`, `<state>` or `<project>`. The plugin root is `<data>/plugins/`. The layout of the plugin root is owned by [Install, update and uninstall](/pages/reference-install-update-uninstall/); this table repeats it for users and adds the user-facing files.

| Item | Location | Format | Written by | Removed by |
|---|---|---|---|---|
| Registry (which plugins exist, versions, enabled flags) | `<data>/plugins/installed.json` | JSON | Install, update, enable, disable, uninstall | Uninstall edits it. Nothing deletes the file |
| Registry lock | `<data>/plugins/installed.lock` | Exclusive OS advisory lock on this file; the file also holds the holder's process id and boot id for messages | Every command that changes the registry | The lock is released when the holder exits |
| Installed packages | `<data>/plugins/store/<id>/<version>/` | `manifest.json`, `manifest.sha256`, components, files | Install, update | Uninstall. An update removes the old version after the swap |
| Unpacking area | `<data>/plugins/staging/<uuid>/` | Same as a package | Install, update | The rename into the store, a failed operation, or the next start (older than 1 hour or its process is gone) |
| Pre-migration copies | `<data>/plugins/backups/<id>/<version>-<utc timestamp>/` | Copy of data and buckets | Update with a data migration | The next committed update keeps only the newest; `uninstall --purge` deletes them |
| Plugin data | `<data>/plugins/data/<id>/` | Whatever the plugin writes | The plugin, through `fs.write:data` | `uninstall --purge`, or Delete data in the uninstall dialog |
| Purge note | `<data>/plugins/purged.json` | JSON list of plugin id and purge time | `uninstall --purge` | Ilium, once every known project has been opened and cleaned of that plugin's data; a project never reopened keeps its data |
| Global storage bucket | `<data>/plugins/storage/<id>/global.db` | Key-value database | The plugin, through `storage` | Same as plugin data |
| Project and session buckets | Inside `<project>/.ilium/` and the session snapshot `<project>/.ilium/sessions/<session>.json` | Database and JSON | The plugin and the host | Same as plugin data, for the open project and session. Other projects get a tombstone |
| Grants and consent answers | `<state>/grants.json` and `consents.json` (one file each for all plugins) | JSON | Review dialog, Capabilities tab, `ilium plugin grants`, first-use prompts | Every uninstall |
| Trusted publisher keys | `<state>/trust.json`, or the path in `ILIUM_TRUST_STORE` | JSON object `ilium-trust/1` with a `keys` list; see [Signing and trust](/pages/reference-signing-and-trust/) | `ilium plugin trust add` and the trust prompt | `ilium plugin trust remove` |
| Plugin settings, user scope | `<config>/config.toml`, table `[plugins."<id>"]` | TOML | The Settings screen | `uninstall --purge`, Forget settings |
| Plugin settings, project scope | `<project>/.ilium/config.yaml`, key `plugins.<id>` | YAML | The Settings screen | Same two actions, for the open project |
| Plugin limits | `<config>/config.toml`, table `[plugins.limits]` | TOML | The user | The user |
| Plugin cache | `<cache dir>/ilium/plugins/<id>/` | Whatever the plugin writes | The plugin, through `fs.write:cache` | The host may evict it at any time. Removed on every uninstall (USR-208) |
| Compile cache | `<cache dir>/ilium/plugins/compile/` | Compiled components | Install of a T2 or T3 plugin | The host, when space is low |
| Plugin log | `<state>/plugins/logs/<session-slug>/<id>.log` and rotated files | Text, five columns | The plugin and the host | Rotation. See below |
| User extension order and toggles | `<config>/config.toml`, table `[plugins.extensions]` | TOML | The Extensions tab | Every uninstall of the owner |
| Replacement record | `<config>/config.toml`, table `[plugins.replaces]` | TOML | The replace confirmation | Undoing the replacement |
| Administrator policy | See [Plugin policy](/pages/reference-plugin-policy/) | TOML | The administrator | The administrator |
| Audit log | `<state>/audit/audit.jsonl` (see [Audit log](/pages/reference-audit-log/)) | JSON lines | The host | Rotation by `audit.max_mib` and `audit.keep_days` |

The two last settings-adjacent tables, `[plugins.extensions]` and `[plugins.replaces]`, are proposed names that the owner of [Extension points](/pages/api-extension-points/) confirms.

- [USR-201] The host MUST keep each item at the location in the table, and MUST create the parent folders on first use. Test: on a clean profile, run install, grant, trust add, enable, disable and uninstall, then list the folders and match each file against the table.
- [USR-202] The host MUST write `installed.json`, the grant files and the trust store atomically: it writes a temporary sibling file, flushes it and renames it over the target. Test: kill the process during a grant change and assert that the file holds either the old or the new content, never a mix.
- [USR-203] Grants MUST be stored per user and per plugin, and MUST NOT be stored in a project folder. Test: grant `fs.read:project` while a project is open, open a second project and assert that the grant applies there and that `<project>/.ilium/` holds no grant.

### Contents of the user-facing files

The grant file has one entry per side. A grant value is `granted`, `declined` or `revoked`. A grant that the manifest never requested has no entry. Consent answers for the prompts that are not grants sit in the same file.

```json
{
  "server": { "tree.read": "granted", "http:api.example.com": "declined" },
  "client": { "ui.panels": "granted" },
  "consent": {
    "open_external": ["https://status.example.com"],
    "http_hosts": ["api.example.com"],
    "location_geocode": true,
    "first_start_not_now": 1
  }
}
```

`first_start_not_now` counts how often the user chose Not now at the first-start prompt of a bundled plugin. See [Install and manage plugins](/pages/user-install-and-manage/#first-start-of-a-bundled-plugin).

The fingerprint of a key is shown as 64 lowercase hexadecimal characters in 16 groups of 4 (see the CLI reference, [DEV-112]). The short form is the first 8 characters, for example `3f2a91c0`. It is shown only beside the full form or in `trust list` ([SEC-177]).

### The plugin log

Each plugin has one log, shared by all its sides. The host writes refusals, traps, restarts and process runs into it, and the plugin writes its own lines through the `log` import. See [log](/pages/api-log/).

| Property | Value |
|---|---|
| File | `<state>/plugins/logs/<session-slug>/<id>.log`, with rotated files `<id>.log.1` to `<id>.log.4` |
| Line format | Four columns: local timestamp `YYYY-MM-DD HH:mm:ss`, level padded to five characters, plugin id, message |
| Rotation | When the file passes 4 MiB it becomes `.1`, `.1` becomes `.2`, `.2` becomes `.3`, `.3` becomes `.4`, and the old `.4` is deleted |
| Retention | The active file plus at most 4 rotated files (5 files per plugin). Rotated files older than 14 days are deleted at server start |
| Rate limit | 200 lines per second for a standard plugin. Extra lines are dropped and counted in one `warn` summary line |
| Who can read it | The user who owns the folder. A plugin cannot read its own log |
| After uninstall | Kept for 7 days, then deleted. `--purge` deletes it at once |

- [USR-204] The host MUST rotate the log at 4 MiB, MUST keep at most four rotated files, and MUST delete rotated files older than 14 days at server start. Test: write 5 MiB of log lines through a test plugin and list the folder; then age `<id>.log.2` by 15 days, restart, and assert it is gone.
- [USR-205] Each log line MUST have the five columns timestamp, level, side, plugin id and message, with the timestamp in the form `YYYY-MM-DD HH:mm:ss`. Test: read the log and match every line against the pattern.

### File permissions

On Linux and macOS, every folder under the plugin roots has mode `0700` and every file has mode `0600`. On Windows the host sets an access list that allows only the current user. A package that ships files with other modes does not change this: the host extracts files with the modes above.

- [USR-206] The host MUST create plugin folders with owner-only access and plugin files with owner-only read and write access. Test: after an install, run `stat` on every created path and assert mode `0700` for folders and `0600` for files on Linux.
- [USR-207] The host MUST refuse to read a grant file or the trust store when another user can write to it, and MUST treat every third-party plugin as not allowed to run until the file is fixed. Test: set the mode of a grant file to `0666`, start Ilium, and assert that T2 and T3 plugins are refused at load with the code `grants.unsafe-permissions` while core plugins still run.

### What each action leaves behind

The default uninstall, `--purge` and `--keep-data` are specified in the table of [Install, update and uninstall](/pages/reference-install-update-uninstall/#uninstall). The rows that matter to a user:

| Action | Package | Grants | Settings | Data, storage | Cache | Extensions order | Log | Trusted keys |
|---|---|---|---|---|---|---|---|---|
| Disable | kept | kept | kept | kept | kept | kept | kept | kept |
| Uninstall, default | removed | removed | kept | kept, no question asked | removed | removed | kept 7 days | kept |
| Uninstall `--purge` | removed | removed | removed | removed | removed | removed | removed | kept |
| Uninstall `--keep-data` | removed | removed | kept | kept | removed | removed | kept 7 days | kept |
| Forget settings (recovery page) | kept | kept | removed | kept | kept | kept | kept | kept |
| `trust remove` | kept | kept | kept | kept | kept | kept | kept | key removed |

Trust entries are never removed by an uninstall, because one key may sign several plugins. After `trust remove`, plugins signed with that key stay installed and show `untrusted` in `ilium plugin list`.

- [USR-208] A default uninstall MUST remove the package, the grants, the cache and the Extensions order entries, and MUST keep settings. Test: install a plugin, write one setting, uninstall with no flag, and list what remains.
- [USR-209] `--purge` MUST also remove settings, data, storage buckets and the log, and `--keep-data` MUST keep data and storage without asking. The two flags exclude each other. Test: run each flag on a copy of the same profile and compare the remains with the table.

### Back up and move plugins to another machine

Copy these items, in this order, with Ilium stopped:

1. `<data>/plugins/store/` and `<data>/plugins/installed.json`, so the same versions install again.
2. The trust store `trust.json`, so signed packages are still trusted.
3. `<state>/grants.json` and `consents.json`, if you want the same answers. Skip it to be asked again on the new machine.
4. `<config>/config.toml` and the `.ilium` folders of your projects, for settings.
5. `<data>/plugins/data/` and `<data>/plugins/storage/`, for the plugin's own data.

Skip the cache, the logs, the staging folder and the backups. A machine that runs a different operating system cannot run T2 and T3 plugins unless it is Linux (see [Plugins overview](/pages/user-plugins-overview/)). Core and bundled plugins need no copy, because they ship with Ilium.

### Errors and edge cases

| Situation | What happens |
|---|---|
| `installed.json` is missing | The host treats the profile as empty and starts with core plugins only. Packages in `store/` stay on disk and are not listed. `ilium plugin reinstall <id>` repairs a store entry from the cached package |
| `installed.json` does not parse | The host starts with core plugins only, writes one error to the Ilium log, and opens the recovery page. It does not overwrite the file |
| A grant file does not parse | The file is renamed to `grants.json.corrupt-<YYYYMMDD-HHmmss>` and the server logs `grants.unreadable`. T2 and T3 plugins start as not installed with no grants; T1 plugins start with only normal grants ([HST-044]) |
| A package folder is missing but the registry lists it | The plugin is refused at load with the code `package.corrupt` |
| The disk is full during a write | The action stops, the old file stays, and the error is `install.io` or `io` |
| Two Ilium servers share one profile | Each write takes `installed.lock`. The second writer polls every 100 ms for up to 30 s, then fails the action with `install.locked` (exit code 4, [LIF-066]) |

### See also

- [Install and manage plugins](/pages/user-install-and-manage/)
- [Plugin privacy](/pages/user-privacy/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [Plugin policy](/pages/reference-plugin-policy/)
- [Administration guide](/pages/guide-administration/)
- [log](/pages/api-log/)
- [Lifecycle](/pages/lifecycle/)

API version checked: `ilium:plugin@0.1.0`.
