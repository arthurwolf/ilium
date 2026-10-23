---
layout: page
title: Enterprise deployment
permalink: /pages/guide-enterprise-deployment/
---

## Enterprise deployment

This guide is for people who run Ilium for others: a team lead, a system administrator, a security engineer. It shows how to decide which plugins are allowed, how to ship a plugin to everyone, how to run on shared machines and in CI, and how to check that the policy does what you think. The rules themselves are in [Admin policy](/pages/reference-admin-policy/). This page is the how-to.

### You will need

- Administrator rights on the machine, to write a root-owned file.
- Ilium installed for each user. Each user keeps their own settings, stores and sessions.
- Linux, if any third-party (T2 or T3) plugin should run. See [Sandbox](/pages/reference-sandbox/).
- A text editor and the CLI: `ilium plugin policy show`, `ilium plugin audit`, `ilium plugin inspect`.

## Choose a stance

Pick one of three stances before you write any policy.

| Stance | Who it fits | Policy |
|---|---|---|
| Built-in only | Regulated sites | `allowed_tiers = ["t1"]` |
| Signed from known publishers | Most companies | `allowed_tiers = ["t1","t2"]`, `allow_unsigned = false`, `publishers.allow = [...]` |
| Open with guard rails | Developer teams | Defaults, plus `grants.max_risk`, `deny_combinations` and a `deny` list |

## Worked examples

### Lock to one publisher

Create `/etc/ilium/policy.toml`, owned by root, mode 0644:

```toml
version = 1
[packages]
allowed_tiers  = ["t1", "t2"]
allow_unsigned = false
allow_dev_mode = false
[publishers]
allow = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
trusted_keys = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
```

The key in `trusted_keys` needs no user prompt. Everything else fails at install with `policy.publisher-denied`.

### Block some high-risk grants

```toml
version = 1
[grants]
max_risk = "high"
deny = ["process.run"]
```

`deny` locks off every grant that matches its pattern: `process.run` matches the bare grant and each of its arguments. The other high-risk grants stay available, subject to review. To block every high-risk grant, set `max_risk = "normal"`; that setting has no exception, and a `preapproved` entry cannot override it ([SEC-198]). Use `preapproved` for normal-risk grants you reviewed.

### An offline site

```toml
version = 1
[network]
offline = true
[packages]
allowed_sources = ["/srv/ilium-plugins/"]
```

No plugin egress, and installs only from `/srv/ilium-plugins/`. The default inference provider, the remote Kilo Gateway, stops working; set `[inference] allowed_providers = ["local"]` and configure a local provider.

### Preinstall a team package for everyone

1. Put the signed package in a root-owned folder, for example `/srv/ilium-plugins/example.team-rules-1.0.0.iliumpkg`.
2. Add the publisher key to `trusted_keys`.
3. Add the id to `force_enabled` and list its grants under `grants.preapproved`.
4. Each user runs `ilium plugin install /srv/ilium-plugins/example.team-rules-1.0.0.iliumpkg` once, or the login script does. Because the grants are preapproved, no dialog opens.

Ilium does not install `force_enabled` plugins for the user. The policy states what must be on; the administrator makes sure it is installed.

## Multi-user machines

**[SEC-194]** Each user has their own stores: grants, consents, trust, project trust, secrets, audit log and plugin install directory, all below the user's own directories and with owner-only modes. No file is shared between users and a user's decisions never apply to another user.

**[SEC-195]** The system package directory, if you use one, is read-only to users. Installs from it copy into the user's install directory.

**[SEC-196]** Sockets and runtime directories are per user (`$XDG_RUNTIME_DIR/ilium/`, mode 0700). Sessions of one user are not visible to another, and `sessions.cross` lists only the same user's sessions.

**[SEC-197]** Do not run Ilium as root. The server refuses T2 and T3 plugins when its effective uid is 0 ([SEC-056]).

## CI and headless use

Where no client is attached, no one can approve a dialog ([SEC-116]). Use `preapproved` for the exact plugins your pipeline needs, and set `allow_unsigned = false`. `ilium plugin install --yes` skips the review dialog for normal grants only. A high-risk grant needs `--grant <grant>` and a new key needs `--trust <key_id>`. `install.review-required` appears only when no client is attached and `--yes` is not given. Pipelines can verify packages without installing:

```text
ilium plugin inspect ./dist/example.team-rules-1.0.0.iliumpkg
```

## Verify the policy

1. Run `ilium plugin policy show`. Check that every file you wrote has `"used": true` and the owner is root. A file with `"used": false` is ignored ([SEC-182]).
2. Try a forbidden install. It should fail with the `policy.*` code you expect.
3. Run `ilium plugin audit --event policy-loaded` and `--event policy-violation`.
4. Look at the Plugins screen. A forced or locked item shows a lock mark.

### Locked items as text

The lock mark in step 4 is a symbol only. The text below gives the same state to a screen reader and to a script. The rules are proposed and owned by the policy owner, so they carry no requirement ID; confirm them before release.

- A plugin in `force_enabled` or `force_disabled` shows the row text `Locked by your administrator` beside its enable control. The lock mark may stay, but never alone.
- A key in `settings.locked` shows the text `Locked by your administrator` and its policy value on its settings row.
- `ilium plugin list --json` adds a field `"lock"` to each plugin: `"force-enabled"`, `"force-disabled"` or `null`. The text mode of `ilium plugin list` prints the same word in its state column, as `locked: force-enabled`.
- A script that tries to change a forced plugin gets `not-permitted` with reason `policy` ([SEC-189]), so a fleet check can test the policy by calling `ilium plugin enable <id>` or `ilium plugin disable <id>` and reading the exit code and reason.
- With `[ui] screen_reader = true` ([USR-305]), a locked control is read with its label, its role word and the words `locked by your administrator`, for example `Enable plugin, unchecked, locked by your administrator`.

To verify the text: run `ilium plugin list --json` on a machine where a plugin is forced, and check that its `"lock"` field matches the policy file. Then open the Plugins screen with the screen reader setting on and check that the row is read with the words above.

## Audit export and emergency disable

Target behaviour (specified here before the code is written).

- [SEC-448] With `[audit] sink = "syslog"` or `"journald"` ([SEC-228]) every forwarded message MUST carry the record's `seq` and its chain hash, so a collector can detect a missing or reordered record without reading the local file.
- [SEC-449] `ilium plugin audit --export <file>` MUST write the records plus a final `anchor` line holding the last `seq` and chain hash, and `ilium plugin audit --verify <file>` MUST verify an exported file offline against that anchor. An operator MAY store anchors in a system the plugin host cannot write to.
- [SEC-450] When the local audit file cannot be written and the sink is a remote one, the server MUST apply the disk-full rule of [SEC-225] and MUST NOT drop records silently or buffer more than 10,000 records in memory.
- [SEC-457] An operator MUST be able to disable all third-party plugins at once, apart from safe mode (see [Security response](/pages/reference-security-response/)), with the policy key `[plugins] disable_third_party = true` or `ilium plugin disable --all-third-party --reason <text>`. Bundled T1 plugins keep running, and the change MUST apply to running servers within one policy reload.
- [SEC-458] Each emergency disable and each re-enable MUST write one audit record with the actor, the reason and the plugin count. Re-enabling MUST be a separate explicit step (`ilium plugin enable --all-third-party`) that re-runs the grant review ([Grant review and consent](/pages/reference-grant-review-and-consent/)) for any plugin whose grants changed meanwhile; it MUST NOT happen on its own.

## Edge cases

| Situation | Result |
|---|---|
| A user edits `/etc/ilium/policy.toml` as root, adds a typo | `policy.invalid`; third parties stop loading; the notice shows the line |
| A user copies the system policy to their own folder | The user file can only make it stricter |
| An old plugin is installed before the policy exists | Disabled at next start if it violates the policy |
| A laptop leaves the office network | `offline = true` still applies; use the proxy key for site networks |

## Reproducible plugin set requirements

- [USR-241] The host MUST export the installed plugin set to a lockfile with `ilium plugin lock`, recording for each plugin its id, exact version, content hash, signing key id and granted permissions. Test: Run `ilium plugin lock` with two plugins installed and assert the file lists id, version, hash, key id and grants for both.
- [USR-242] The host MUST replay a lockfile with `ilium plugin sync --locked` so that the installed set equals the lockfile exactly, installing missing entries at the pinned version and removing extras, and MUST fail with `lock.mismatch` without changing anything if a hash or key id does not match. Test: Replay a lockfile on a clean profile and assert the same set; corrupt one hash and assert `lock.mismatch` and an unchanged profile.
- [USR-243] The lockfile MUST be deterministic text with sorted entries and no timestamps or machine paths, so that two machines with the same set produce byte-identical files. Test: Lock the same set on two profiles and assert identical bytes.
- [USR-244] A replayed grant MUST still pass the machine policy: a grant the policy forbids MUST be left off and reported as `Blocked by your administrator`, and the sync MUST NOT raise any grant above what the lockfile records. Test: Replay a lockfile containing a grant the policy forbids and assert it is off and reported; assert no extra grant appears.

## Shared hosts and per-user installs

These requirements cover a machine where several users run Ilium.

- [EXT-066] Plugins MUST be installed either per user, under that user's data directory, or system-wide, in a directory only an administrator can write. A per-user install MUST NOT be visible to or writable by another user.
- [EXT-067] When the same plugin id is installed both system-wide and per user, the system-wide copy MUST win unless the policy sets `allow_user_override = true`, and `ilium plugin list --json` MUST show the winning scope and the shadowed one.
- [EXT-068] Plugin data, storage and grants MUST be kept per user even for a system-wide plugin. A plugin instance of one user MUST NOT read the data, sockets or process list of another user's session.

### See also

- [Admin policy](/pages/reference-admin-policy/)
- [Audit log](/pages/reference-audit-log/)
- [Network egress](/pages/reference-network-egress/)
- [Plugin CLI](/pages/reference-plugin-cli/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T7-users-privacy-a11y

- [USR-341] `ilium plugin list --json --inventory` MUST output id, version, tier, content hash, key id, state and granted permissions for every plugin, one object per line, sorted by id, without paths under the user's home, so that a fleet tool can compare machines.
- [USR-343] A policy MAY set `notice.contact` (at most 200 characters, proposed, owner to confirm; no control characters) and the host MUST append it to every message that says `Blocked by your administrator`.
- [USR-344] Applying a policy MUST NOT give an administrator access to a user's plugin data, settings values or consent records; `ilium plugin audit` MUST show plugin ids, events and reasons only, and the manual MUST list this limit in the administration guide.

## Production additions: T10-operations

- [HST-686] The policy MUST support `max_user_plugin_disk_mib` and `max_user_active_plugins` (proposed, owner to confirm) as caps per user, enforced by each user's own session server, and the host MUST refuse an install that would exceed the disk cap with `policy.user-quota` showing used and allowed MiB. The system-wide aggregate ceilings (HST-500..511) apply per machine across users.
- [HST-687] A system-wide install MUST be stored read-only for users (owner root/administrator, no write for others), MUST be verified by `manifest.sha256` on each server start for each user, and MUST be loaded by many users concurrently without a per-user copy; compiled-component cache for it MUST be per user.
- [HST-688] When two users open the same project folder, each user's session MUST keep its own grants, and project buckets MUST be stored per user (keyed by user id) or the host MUST refuse the second user with `project.owned-by-other-user`; a plugin MUST NOT be able to read the other user's project bucket. The `.ilium` folder permissions of 0700 MUST NOT be widened by the host.
- [HST-689] The host MUST provide `ilium plugin admin purge-user <user>` (proposed, owner to confirm) for an administrator that deletes the plugin data, storage, grants, logs and cache of that user under the user's roots, requires `--yes`, writes a system audit entry and refuses a user with a running session server (`purge.user-active`).

The two per-user caps have no default value in this manual. An unset key means no cap, because no policy key sets a cap for a session resource yet ([SEC-950]). The owner must pick each number before release. The behaviour below is proposed and owner to confirm; it has no requirement ID.

- **Active-plugin cap, enabling.** `max_user_active_plugins` counts the plugins of one user that are enabled and running, bundled T1 plugins included. Enabling a plugin that would take the count over the cap returns `not-permitted` with reason `policy.user-quota`, and the plugin stays off. The plugins already running are not affected. The reason code is new; add it to the stable reason codes of [Admin policy](/pages/reference-admin-policy/) ([SEC-115]) when it is confirmed.
- **Active-plugin cap, start.** When the policy is loaded at start and the enabled plugins are more than the cap, the host starts them in the order of [LIF-046] until the cap is reached. The rest stay enabled, keep their grants, settings and data, and do not run. The notice names `policy.user-quota`. They start at a later start, once the count is under the cap.
- **Lowering a cap.** A lowered cap takes effect at the next start, like any policy edit ([Administration guide](/pages/guide-administration/)). A running plugin is not stopped and no data is deleted.
- **Disk cap.** The cap is checked when a plugin is installed or upgraded. An install or upgrade that would exceed `max_user_plugin_disk_mib` is refused with `policy.user-quota`, as HST-686 states. A plugin's own writes stay limited by its `disk_mib` ([Files](/pages/api-fs/)), so the user cap does not make a running plugin's writes fail.

## Plugin logs and metrics on managed machines

Ilium does not send plugin logs, trap or crash lines, or metrics to a fleet collector. The audit log is the only record that can leave a machine by itself ([SEC-228]). The rules below set the route for everything else. They are new and owner to confirm.

- [HST-990] The host MUST NOT send plugin operational logs (`<state>/plugins/logs/`), host trap or crash lines, or `ilium plugin metrics` output to any network address, socket or remote service. The only remote route is the audit sink of [SEC-228], which carries audit records and nothing else.
- [HST-991] `ilium plugin metrics` MUST write its Prometheus text to standard output only and MUST NOT open a listening socket. A fleet collector obtains the text by running the command from a job the administrator controls (for example a scheduled job that writes stdout to a file that the collector reads).
- [HST-992] An administrator who copies plugin logs to another machine MUST keep the copy under owner-only modes (0600 for files, 0700 for folders), MUST NOT copy the trust store, grants, consents or plugin data folders, and MUST document the copy in the site's change record. A copy holds whatever the log files hold, so review it before sharing it outside the administrator's team.
- [HST-993] The admin policy MUST NOT define a key that forwards plugin logs or metrics in this version. A future forwarding key needs its own requirement with a record format and a rate limit (proposed, owner to confirm).

Crash loops and trap counts are visible only on the machine that produced them. To compare plugin state across machines, run `ilium plugin list --json --inventory` ([USR-341]); that output carries no crash or trap counts. For a single machine, `ilium plugin doctor --bundle` ([DEV-941]) packs the doctor records and the last log lines into one file.

## Distribute trusted publisher keys

The system trust store is `/etc/ilium/trust.json` ([PKG-222]). The effective trust set is the union of the user store and the system store, minus revoked keys ([PKG-223]). This section says how the administrator produces that file, pushes it, checks it and revokes a key with it. The rules are new and owner to confirm.

- [PKG-980] The administrator MUST produce `/etc/ilium/trust.json` from one source file under version control, in the format of [Signing and trust](/pages/reference-signing-and-trust/#the-trust-store), and MUST record the SHA-256 of the produced file in the change record for each push.
- [PKG-981] A push MUST write the file as root with mode 0644, through a temporary file in `/etc/ilium/`, fsync and rename. A machine MUST NOT have the file edited in place. A file that another user can write is refused at load with `grants.unsafe-permissions`, as [USR-207] describes.
- [PKG-982] After each push, the administrator MUST run `ilium plugin trust list` on each machine and `ilium plugin doctor` ([USR-337]). A trust store line that is not `ok`, or a key list that differs from the source file, MUST count as a failed push. Use the configuration-management tool that already owns `/etc` on the machine, or a root-run copy job. Do not use a per-user `ILIUM_TRUST_STORE` for fleet keys.
- [PKG-983] An unreadable system store makes its set empty, so signed packages from those keys become untrusted ([PKG-223]). A machine whose push fails MUST be treated as not trusting the new keys until the push succeeds.
- [PKG-984] To revoke a compromised key across the fleet, the administrator MUST push a revocation statement ([PKG-242]) in the same change as the new file, then run the post-push checks of [PKG-982]. A machine not yet reached keeps trusting the key until the new file arrives.
- [PKG-985] On an air-gapped machine, the registry root key MUST be pinned with `ilium plugin registry add <url> --root-key <key_id>` ([PKG-411]) only after the `key_id` has been compared with the publisher's published fingerprint through a second channel, such as a signed release note. The pinned `key_id` MUST be written to the change record. Root keys are never taken from a mirror alone.


## Distributing policy, trust and lockfile with a configuration-management tool

A fleet is configured by a management tool, not by hand. Three files are managed: the system `policy.toml` ([SEC-180]), the system trust store `trust.json` ([PKG-465], [PKG-980]) and the plugin lockfile ([USR-241]). The recipes below are examples of the pattern for each platform; the pattern is the requirement, the tool is your choice. The rules without an ID are new and owner to confirm.

### What each file needs

| File | Linux | macOS | Windows | Pushed by |
|---|---|---|---|---|
| `policy.toml` | `/etc/ilium/`, root:root, 0644 | `/Library/Application Support/Ilium/`, root:wheel, 0644 | `%ProgramData%\Ilium\`, write for Administrators and SYSTEM only | The management tool, as root |
| `trust.json` | `/etc/ilium/`, root:root, 0644 | same folder as the policy, root:wheel, 0644 | same folder as the policy, same ACL | `ilium plugin trust add <file> --tier T2 --system` as root, or a whole-file push ([PKG-981]) |
| plugin lockfile | any path the user can read, for example `/etc/ilium/ilium-plugins.lock`, 0644 | same idea | same idea | Written by the tool; applied by each user, see below |

A policy or trust file that another user can write is refused at load ([USR-207]), so the mode is not optional. Write through a temporary file in the same folder, fsync and rename, so a session starting during the push never reads half a file. The policy is read at start only ([SEC-188]): a push does not change a running session, and the effect begins at the next session start.

The lockfile is the one file that is applied per user, because the plugin install directory belongs to each user ([SEC-194]). The management tool puts the file in place and runs `ilium plugin sync --locked <lockfile>` as the user, for example from a login script. The sync installs the missing entries at the pinned version, removes extras and fails with `lock.mismatch`, changing nothing, if a hash or key id differs ([USR-242]). A grant that the machine policy forbids stays off and is reported as `Blocked by your administrator`; the sync never raises a grant above what the lockfile records ([USR-244]). Produce the file once, on a reference machine, with `ilium plugin lock`; it is deterministic text, so the same set always gives the same bytes and a changed hash in version control means a changed set ([USR-243]).

### Linux, with a configuration-management tool

Declare the three files as managed resources with the owner and mode above, and make the tool's own atomic-replace option the push method. In Ansible, for example:

```yaml
- name: Ilium policy
  ansible.builtin.copy:
    src: files/policy.toml
    dest: /etc/ilium/policy.toml
    owner: root
    group: root
    mode: "0644"
  register: ilium_policy
- name: Ilium system trust store
  ansible.builtin.copy:
    src: files/trust.json
    dest: /etc/ilium/trust.json
    owner: root
    group: root
    mode: "0644"
- name: Check the effective policy
  ansible.builtin.command: ilium plugin policy show
  register: ilium_policy_show
  changed_when: false
  failed_when: "'used' not in ilium_policy_show.stdout"
```

The `copy` module writes through a temporary file and renames it. Puppet, Chef and Salt resources with the same owner and mode are equivalent. Add a trust entry without owning the whole file with `ilium plugin trust add key.pub --tier T2 --system`, run as root; the entry has `added_by: admin` and the user cannot remove it ([PKG-465]).

### macOS, with a profile or MDM script

macOS has no plugin policy key in a configuration profile. Use the MDM's script or package payload to run a root script that does what the Linux recipe does:

```sh
#!/bin/sh
set -eu
dir="/Library/Application Support/Ilium"
mkdir -p "$dir"
for f in policy.toml trust.json; do
  install -m 0644 -o root -g wheel "/private/var/tmp/ilium-staging/$f" "$dir/.$f.new"
  mv "$dir/.$f.new" "$dir/$f"
done
```

`mv` inside one folder is the rename. The MDM re-runs the script on its own schedule, which also repairs a file someone changed. Remove the staging files after the run.

### Windows, with a management script

Run a script as SYSTEM from the management agent (Intune, a scheduled task, a group-policy startup script):

```powershell
$dir = Join-Path $env:ProgramData 'Ilium'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
foreach ($f in 'policy.toml','trust.json') {
  Copy-Item "C:\ProgramData\IliumStaging\$f" "$dir\$f.new" -Force
  Move-Item "$dir\$f.new" "$dir\$f" -Force
}
icacls $dir /inheritance:r /grant:r "SYSTEM:(OI)(CI)F" "Administrators:(OI)(CI)F" "Users:(OI)(CI)RX" | Out-Null
```

The ACL gives ordinary users read and execute only. A file that a non-administrator can write is treated as in the Linux case.

### The check after each write

A management run is not finished when the file is written. It runs these checks on the machine and reports a failure to the tool:

1. `ilium plugin policy show` ([SEC-191]). The `policy-source` line for the system file must say `used` with the SHA-256 recorded in the change record, and the `policy` line must equal the expected values. A file marked `ignored`, or `policy.invalid` in the audit log, is a failed push ([SEC-187]).
2. `ilium plugin trust list` and `ilium plugin doctor` ([PKG-982]). The key list must equal the source file.
3. `ilium plugin list --json --inventory` ([USR-341]). Compare the output, which is sorted by id and has no home paths, with the expected set derived from the lockfile. Any difference in version, hash, key id or state is a failed run.

### Staging through the rings

Treat the three files as one change and roll them out together through the rings of [USR-109]: a first ring of at most five machines, each later ring at most twice the size, and the next ring only after one working day with no `policy-violation` or `policy.invalid` event. Run the checks above on the first ring before the second starts ([USR-108]). Keep the previous copies of the three files until every ring has passed, so that a revert is a rename back and a session restart ([USR-107]). In the tool, a ring is a host group; the tool's own staged-rollout feature moves the files, and the checks gate the move.

### Never push these

- User grants (`grants.json`), consent records (`consents.json`) and project trust. They are decisions of one user on one machine ([SEC-194]); a pushed copy would grant permissions nobody gave, and the policy is the only administrator control over them.
- Secrets, keyring entries and registry tokens.
- Anything under a user's `<data>`, `<state>` or `<cache>` folders, the audit log, the per-plugin random identifier, and the runtime directory.
- A per-user `ILIUM_TRUST_STORE` for fleet keys ([PKG-982]).

## Ilium over SSH and moving a profile between machines

### Ilium on a remote machine over SSH

When you run Ilium in an SSH session, the server, the plugins and every file are on the remote machine. Your local machine only displays the terminal. In practice:

- **Policy, trust and sandbox.** The policy, the system trust store and the sandbox are those of the remote host ([SEC-180], [SEC-199]). A policy on your laptop has no effect on a session on the server, and the Linux sandbox requirement applies to the remote machine.
- **Stores.** The grants, consents, secrets, audit log and install directory are those of your account on the remote machine ([SEC-194]).
- **Clipboard.** There is no desktop clipboard on the remote host. When the terminal supports OSC 52, a copy is written through that sequence and reaches your local clipboard; with neither route, `write-text` returns `unsupported` ([Clipboard](/pages/api-clipboard/)). Some terminals and multiplexers block OSC 52 by default; allow it in the terminal.
- **Desktop notifications.** Without a session bus on the remote host, desktop notifications are unavailable. The client footer still shows notices, and a plugin that asks for a desktop notice receives the unavailable result.
- **HTTP API.** The HTTP API listens on the loopback address of the remote machine only ([HTTP API](/pages/api-http-api/)). To reach it from your laptop, forward a port through SSH (`ssh -L`); never bind it to a public address.
- **Sockets and runtime directory.** The session sockets and the runtime directory stay on the remote machine and are not meant to be forwarded or copied.
- **Locks.** If your home directory is on a network filesystem, the host detects it and starts with the lock scheme of [HST-665]; run one session server per account and machine.
- **Closing the connection.** Losing the SSH connection ends the client, not the session server; the next login attaches to the same session.

### What you can copy to another machine

| Item | Copy it? | Why |
|---|---|---|
| Settings (`config.toml` without secrets) | Yes | Plain preferences; check paths in them |
| Keymaps | Yes | Plain text |
| Plugin lockfile | Yes | Deterministic text with no machine paths ([USR-243]) |
| Trust entries | Yes, as entries added with `ilium plugin trust add` | Each machine decides what it trusts; a system policy still applies |
| Secrets and keyring entries | No | They belong to one machine's keyring; set them again on the new machine |
| Sockets and runtime directory | No | They are tied to one running server |
| `<state>` folders (logs, crash records, snapshots, installation key) | No | They describe one machine and one install |
| Consent records and `grants.json` | No | A decision is made again on each machine ([SEC-194]) |
| Per-plugin random identifier | No | It must be random per install and changes on purge or erase ([USR-330]); a copy would link two machines |

### Reproducing a plugin set on a new machine

1. On the old machine run `ilium plugin lock` and keep the file.
2. Copy the lockfile, your settings and your keymaps to the new machine.
3. Add the trust entries you need, if the policy allows it.
4. Run `ilium plugin sync --locked <lockfile>`. It installs the pinned versions and fails with `lock.mismatch`, without changes, if a hash or key id differs ([USR-242]).
5. Grant the permissions again. A replayed grant that the new machine's policy forbids stays off and is reported as `Blocked by your administrator` ([USR-244]).
6. Run `ilium plugin list --json --inventory` on both machines and compare ([USR-341]).

## Removing Ilium from a managed machine

Plugin uninstall and data erasure are covered elsewhere ([Plugin privacy](/pages/user-privacy/)); this section covers the host itself. The order matters, because the per-plugin commands need the host to be present. The exact folder names are the ones in [Plugin files](/pages/reference-plugin-files/).

1. **Stop the sessions cleanly.** Ask each user to end their sessions, or end each with a clean stop, not a kill signal ([LIF-296]). A kill skips `deactivate` and loses restore bytes. `purge-user` refuses a user with a running session server (`purge.user-active`, [HST-689]).
2. **Erase the plugin data, before the binary is removed.** For each user run `ilium plugin admin purge-user <user> --yes` ([HST-689]) as the administrator, or have each user run `ilium plugin erase-data` for every plugin ([USR-246], [USR-327]). Check the output: it prints `erased <kind>: <n> files, <bytes> bytes, remaining 0` and exits 3 with `erase.incomplete` if a path could not be deleted ([USR-325]). Both commands print a `not cleaned` line for files a plugin wrote into a project or home folder, which no Ilium command removes ([Plugin privacy](/pages/user-privacy/)).
3. **Remove the system files.** Delete `policy.toml` and `trust.json` from the system folder of the platform (see the table above), then the binaries and the notices file whose path `ilium --version --verbose` prints. Do this with the same tool that installed them.
4. **Remove what stays in each user's profile.** Nothing in steps 1 to 3 removes everything a user has:

| What stays | Where | Remove with |
|---|---|---|
| Settings and keymaps | the user's config folder | delete the `ilium` folder there |
| Audit log | the user's state folder | delete it after your retention period; export it first if you must keep it |
| Crash records | the user's state folder; kept 30 days by default ([SEC-955]) | `ilium plugin erase-data`, or delete the folder |
| Snapshots and session state | `<project>/.ilium/` of each project | delete the folder |
| Cache | the user's cache folder | delete the `ilium` folder there |
| Keyring secrets | the user's keyring | removed by `purge-user` and `erase-data`; check with the keyring tool |

   Deleting a user's folders is the administrator's act on that user's own data; it falls under the retention table of the privacy page ([USR-348], [USR-955]).
5. **Confirm the machine is clean.** `ilium` is no longer on the path. A search of the system folders and each user's config, data, state and cache folders for `ilium` finds nothing, and the fleet tool's installed-software inventory no longer lists the package. Where the software inventory cannot see user folders, run the folder search as a management job and record its result.

If a machine is wiped or retired as an image, run step 2 for every user first ([USR-955]); deleting the disk later removes the data, but only the commands above produce the audit entry and the report.
