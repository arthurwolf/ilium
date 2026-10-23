---
layout: page
title: Plugin administration guide
permalink: /pages/guide-administration/
---

## Plugin administration guide

This guide is for the person who looks after Ilium on a shared or managed machine. It shows how to allow, limit or forbid plugins, how to approve publishers, how to install plugins for people without prompts, how to keep an audit trail, and what to do when a plugin goes wrong. Everything here uses the admin policy file. The exact keys are in the [Admin policy reference](/pages/reference-admin-policy/); [Plugin policy](/pages/reference-plugin-policy/) lists what users see.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/).

<div class="note">
<p>Decision status. The platform is not implemented. The rules about what a policy may do (stricter only) are fixed. The platform owner confirms the key names before release.</p>
</div>

### Contents

- What you control and what you do not
- Choose a stance
- Write the policy
- Approve a publisher
- Install for users without prompts
- Keep the audit trail
- Respond to a bad plugin
- Roll out a change
- Machines that are not Linux
- Check that the policy works
- Errors and edge cases
- Requirements

### What you control and what you do not

You control which plugins may be installed, which tiers, which publishers, which grants may be given and where the always-on audit log is also sent. You do not control what a user decides inside those limits. Within the policy, each user still chooses their own permissions.

| You can | You cannot |
|---|---|
| Forbid all third-party plugins | Stop core or bundled plugins from running (they ship with Ilium) |
| Allow only signed plugins from named keys | Give a user a permission that the plugin did not request |
| Forbid chosen grants for every plugin | Read a user's plugin data through Ilium |
| Deny or allow plugin ids | Make a plugin run when the user turned it off |
| Read the always-on audit log of plugin events | Replace the kernel recovery page or hide it |

A policy can only make Ilium stricter. It cannot allow what the security model refuses, such as a third-party plugin on Windows or macOS.

- [USR-070] A policy MUST only restrict. No key of the policy MAY enable a plugin, grant or tier that Ilium refuses without a policy. Test: set `packages.allowed_tiers = ["t3"]` on Windows and assert that T3 is still refused.

### Choose a stance

| Stance | When it fits | Policy |
|---|---|---|
| Open | Personal or developer machines | no file |
| Signed only | A team that builds its own plugins | `packages.allowed_tiers = ["t1","t2"]` and `publishers.allow = [<your key>]` |
| Approved list | Regulated machines | `plugins.allow_ids = [...]` plus signed only |
| No third-party | Kiosks, shared lab machines | `packages.allow_third_party = false` |
| Open with limits | Mixed use | `grants.deny = ["http:*", "process.run:*"]` |

### Write the policy

The file is at the path for your system:

| System | Path |
|---|---|
| Linux | `/etc/ilium/policy.toml` |
| macOS | `/Library/Application Support/Ilium/policy.toml` |
| Windows | `%ProgramData%\Ilium\policy.toml` |

On Linux, create it as root, with the mode `0644`, owned by root. Ilium ignores a policy file that a normal user can write, shows a persistent notice, and records `policy-violation` in the audit log.

```sh
sudo install -d -m 0755 /etc/ilium
sudo install -m 0644 -o root -g root policy.toml /etc/ilium/policy.toml
```

A first policy for a team with its own signing key:

```toml
version = 1

[packages]
allowed_tiers = ["t1", "t2"]

[publishers]
allow = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]

[grants]
deny = ["process.run:*", "sessions.cross"]

[audit]
keep_days = 180
```

Ilium reads the file when the server starts. A change takes effect at the next start, and a running plugin is not stopped by an edit. At that start a plugin the policy forbids is disabled and shown as "Blocked by policy". Its data stays on disk. A grant the policy denies is removed from the plugin, and the plugin keeps running without it.

### Approve a publisher

A fingerprint is the identity of a signing key. To approve a publisher:

1. Get the publisher's public key file (`publisher.pub`) from them by a route you trust.
2. Print its fingerprint: `ilium plugin trust add publisher.pub` prints it when run as a user, or read it from the package with `ilium plugin inspect notes.iliumpkg`.
3. Confirm the fingerprint with the publisher by a second route.
4. Put the fingerprint, in the 64-character form, into `publishers.allow`.

Users still see a trust prompt for the key on their first install, unless they already trust it. The policy decides whether the install is allowed at all; trust is each user's own answer. Removing a fingerprint from `publishers.allow` blocks the key's plugins at the next start.

To check a package before you hand it out:

```sh
ilium plugin inspect notes.iliumpkg --verify
```

This works offline. It recomputes every hash and compares it with the package's file list. A difference exits with 1 and the code `inspect.hash-mismatch`.

### Install for users without prompts

For scripts, images and setup tools there is no terminal, so the review dialog cannot show. Use:

```sh
ilium plugin install notes.iliumpkg --yes
ilium plugin install notes.iliumpkg --yes --grant terminal.history
```

`--yes` accepts every normal permission the package requests. A HIGH permission is given only if you name it with `--grant`. This runs as the user, writes to the user's folders, and is limited by the policy. Without `--yes` and without a terminal, the install fails with `install.review-required`.

There is no machine-wide plugin folder. Each user installs into their own profile. To give a plugin to many users, run the command in each user's login script or setup step, or place the package file where users can read it and let them install it.

- [USR-071] A non-interactive install without `--yes` MUST fail with `install.review-required`, and `--yes` MUST NOT grant a HIGH permission that is not named. Test: install a package with one HIGH row with `--yes` and assert that the row is declined.

### Keep the audit trail

The audit log is always on; there is nothing to enable. The host appends one JSON line for every install, update, uninstall, enable, disable, grant change, consent change, trust change, policy event and sandbox kill. The events are listed in the [Audit log](/pages/reference-audit-log/) reference. A line holds the UTC time `ts`, a sequence number, the actor, the plugin id, the event and the result. It holds no plugin data and no setting values. Policy may set only `audit.sink`, `audit.max_mib` and `audit.keep_days`.

```sh
jq -r 'select(.event=="grant-given") | [.ts,.plugin_id,.grant] | @tsv' "$XDG_STATE_HOME/ilium/audit/audit.jsonl"
```

The audit file is per user, in the user's state folder, because Ilium runs as that user. For a central copy set `[audit] sink = "syslog"` or `"journald"`. If the file cannot be written, no action is refused: the server queues up to 1,000 records, retries every 10 s and shows a notice.

#### Audit size and retention defaults

| Setting | Default | Range you can set | What it controls |
|---|---|---|---|
| `audit.max_mib` | 64 MiB | policy key | Size at which `audit.jsonl` is rotated |
| `audit.keep_days` | 365 days | policy key | Age after which a rotated file is deleted |
| Audit and crash storage ceiling | 256 MiB | not a policy key | Total size of audit and crash records together |

Use these defaults to size the disk. Each user's log grows until it reaches `audit.max_mib`, then it is renamed to `audit-<first seq>.jsonl` and a new file starts. The hash chain continues into the new file, so `ilium plugin audit --verify` covers the rotated files as well. Rotated files are deleted at server start once they are older than `audit.keep_days`. A policy of `keep_days = 180` therefore deletes a rotated file once it is older than 180 days, not 365.

What happens at each limit:

- **Size reached (`audit.max_mib`).** The file rotates. No record is refused or dropped.
- **Total storage reaches 256 MiB.** The host deletes the oldest rotated files first, even if they are still inside `audit.keep_days`, and writes one record that states how many files it removed. The page does not name a policy key that changes this ceiling, so plan disk space for the 256 MiB limit as it stands.
- **Disk full, so a record cannot be written.** The server keeps running and holds up to 1,000 records in memory, retries every 10 s, and shows a notice. When the queue is full, the oldest `grant-refused` and `egress-host-first-use` records are dropped first. The `audit-dropped` record that counts the loss is written when space returns.

To check the rotation and size on a machine, run `ls -lh "$XDG_STATE_HOME/ilium/audit/"`. Several `audit-<seq>.jsonl` files near 64 MiB each is normal. If the folder approaches 256 MiB, either allow more disk per user or lower `audit.keep_days` so old files are removed sooner. For a central copy of every record, set `audit.sink` as described above.

### Respond to a bad plugin

1. **Stop it now.** Add its id to `plugins.deny_ids`. At the next server start every copy is disabled and shows "Blocked by policy".
2. **Block its key.** If a whole publisher is at fault, remove its fingerprint from `publishers.allow`.
3. **Find who has it.** Search the audit log for the plugin id.
4. **Clean up.** Ask each user to run `ilium plugin uninstall <id> --purge`. The policy does not delete anything on its own, and this guide has no command that purges a plugin's data on a machine you manage (see [USR-111]). Whether the purge also removes crash records is an open decision, see [Crash records and pre-migration backups](/pages/user-privacy/#crash-records-and-pre-migration-backups). When a plugin must be fully erased, also ask each user to run `ilium plugin erase-data` for it [USR-954].
5. **Remove the key.** Users can run `ilium plugin trust remove <key_id> --force`. Without `--force` the command refuses with `trust.in-use` while an installed plugin is signed with the key. Plugins signed with it then show `untrusted`.

Users see "Blocked by policy" and the detail code, for example `policy.id-denied`. See [Troubleshooting plugins](/pages/user-troubleshooting/).

### Roll out a change

1. Copy the current policy file to a previous-copy file beside it, for example `/etc/ilium/policy.toml.previous`, owned by root with mode `0644`. This copy is the revert point.
2. Write the new policy to a temporary file next to the real one.
3. Check it for syntax. An unknown key or a wrong type makes the file malformed: at the next start T2 and T3 plugins refuse to load with `policy.invalid`, and the notice names the line. A file that passes this check can still forbid a grant or plugin you need, so step 6 checks the effect.
4. Rename it over the old one. The rename is atomic.
5. Restart the sessions on the first ring only. Each server reads the policy at start only.
6. On that machine, run `ilium plugin policy show`. Compare the merged `policy` line with the values you intended, and check that the `policy-source` line marks the file `used` [SEC-191]. Look for `policy-violation` and `policy.invalid` in the audit log.

Roll out in rings. The first ring is one machine, or at most five machines (proposed, owner to confirm). Move to the next ring only after the first ring has run the new policy for one working day (proposed, owner to confirm), with no `policy-violation` or `policy.invalid` event in its audit log and no plugin reported as "Blocked by policy" that should run. Each later ring may be at most twice the size of the one before (proposed, owner to confirm). [USR-109]

While a malformed policy is in place, a machine that restarts refuses T2 and T3 plugins and keeps core and T1 plugins running. A bad policy therefore reaches each machine at its next restart, not when the file is written. Stop the rollout at the first sign of a problem.

To revert, copy the previous-copy file back over the policy file with the same rename, then restart the sessions on the affected machines. Keep the previous copy until every ring has passed [USR-107].

Test the policy on one machine first. A policy that forbids `ilium.core.*` has no effect, because the policy never stops a core plugin.

### Machines that are not Linux

On Windows and macOS, third-party plugins are refused at install, with or without a policy. The policy file is still read there. `grants.deny` has little effect, because only bundled plugins run, and the bundled plugins ignore `install` keys. Use the same file to keep your fleet uniform, and expect the refusal text "Cannot install <name> <version>."

### Check that the policy works

After writing a policy, check it with a throwaway user:

1. `ilium plugin doctor` warns when the platform allows T2 and T3 and a policy disables T3.
2. Try to install a package that the policy should block and read the error. Look for `policy.blocked` and the detail code.
3. Install an allowed package and open its Capabilities tab. Check that denied grants are locked and off.
4. Look at the audit log for the `policy-loaded` event and any `policy-violation` events.

### Errors and edge cases

| Situation | What happens |
|---|---|
| The policy file is unreadable | T2 and T3 plugins refuse to load with `policy.invalid`. Core and T1 plugins run |
| The policy file is writable by a user | The file is ignored, a persistent notice appears and the audit log records `policy-violation` |
| A bad edit (wrong type, unknown key) | The file is malformed: T2 and T3 plugins refuse to load with `policy.invalid`, and the notice names the line |
| The policy denies a grant a running plugin holds | At the next start the grant is removed. Calls return `not-permitted` with the reason `policy` |
| A user has their own `XDG_CONFIG_HOME` | No effect on the system policy path |
| Two policies (system and per user) | The user file at `<user config dir>/policy.toml` can only add restrictions |

### Requirements

- [USR-072] An administrator MUST be able to block a running plugin by id, and the host MUST disable it at the next server start. Test: add the id while the plugin runs, restart the server and assert the state `Blocked by policy`.
- [USR-073] The documentation MUST state that the policy only restricts, where the file lives per system, and how to respond to a bad plugin. Test: search this page for the three statements.
- [USR-107] An administrator MUST keep the previous policy file until the change has passed every rollout ring, and MUST be able to restore it by renaming it back and restarting the sessions. Test: pending: test to be written.
- [USR-108] Before a policy change leaves its first ring, an administrator MUST run `ilium plugin policy show` on that ring and MUST see the intended effective values, because a syntactically valid policy can still forbid a grant or plugin that is needed [SEC-191]. Test: pending: test to be written.
- [USR-109] A policy change MUST roll out in rings. The first ring MUST be at most five machines (proposed, owner to confirm), and each later ring MUST be at most twice the size of the one before (proposed, owner to confirm). The next ring MUST NOT start until the previous ring has run the policy for one working day (proposed, owner to confirm) with no `policy-violation` or `policy.invalid` event and no wrongly blocked plugin. Test: pending: test to be written.
- [USR-111] Version 0.1.0 has no policy key and no command that makes a user purge a plugin's data. An administrator MUST treat the removal of a plugin for a security or contract reason as incomplete until each user has run `ilium plugin uninstall <id> --purge`, and MUST NOT report it as done on the strength of the policy file alone. Test: pending: test to be written.
- [USR-954] When an administrator removes a plugin for a security or contract reason, the administrator MUST ask each user to run `ilium plugin erase-data` for that plugin, because the purge may leave crash records in place ([USR-326], [USR-246]). Test: pending: test to be written.

### See also

- [Plugin policy](/pages/reference-plugin-policy/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Security](/pages/security/)
- [Install and manage plugins](/pages/user-install-and-manage/)
- [Plugin privacy](/pages/user-privacy/)
- [Troubleshooting plugins](/pages/user-troubleshooting/)
- [CLI reference](/pages/reference-plugin-cli/)
- [Administrator failure runbook](/pages/guide-admin-failure-runbook/)

API version checked: `ilium:plugin@0.1.0`.
