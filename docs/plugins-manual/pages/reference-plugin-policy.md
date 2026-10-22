---
layout: page
title: Plugin policy
permalink: /pages/reference-plugin-policy/
---

## Plugin policy

An administrator can limit which plugins a machine may run. The limits are written in one file, the admin policy. This page is the user-facing companion to the [Admin policy reference](/pages/reference-admin-policy/), which is the only normative definition of the file: its paths, every key, the merge rules and the evaluation steps. This page does not repeat the key table. It maps the short names that older notes used to the real keys, and it lists what a user sees when the policy blocks something. The [Administration guide](/pages/guide-administration/) shows how to use the file in practice.

Contract version: `ilium:plugin@0.1.0`. Requirement ids have the form `[USR-nnn]`, collected in [User and administrator requirements](/pages/reference-requirements-user-admin/). The security rows of the policy are `SEC-180` to `SEC-199` in the [security requirements](/pages/reference-requirements-security-policy/).

<div class="note">
<p>A policy can only make Ilium stricter, never looser than the defaults in <a href="/pages/security/">Security</a>. The platform owner confirms the key names before release.</p>
</div>

### Contents

- Where the file lives
- Format and loading
- Keys
- How the keys combine
- What the user sees
- The audit log
- Errors and edge cases
- Requirements

### Where the file lives

The file is `policy.toml`. On Linux it is `/etc/ilium/policy.toml`, on macOS `/Library/Application Support/Ilium/policy.toml`, and on Windows `%ProgramData%\Ilium\policy.toml`. An optional user file `<user config dir>/policy.toml` can only make the result stricter ([SEC-181]). The system file is owned by an administrator; a file that group or others can write is ignored ([SEC-182]).

### Format and loading

The file is TOML with `version = 1`. The server reads it at start only ([SEC-188]). A change takes effect at the next start, and a running plugin is not stopped by an edit. A malformed or unreadable file makes T2 and T3 plugins refuse to load with `policy.invalid`, and T1 plugins run normally ([SEC-187]).

### Keys

The keys are defined in the [schema of the Admin policy reference](/pages/reference-admin-policy/#schema). This table maps the names that older pages and notes used to the keys of that schema.

| Older name | Key in `policy.toml` |
|---|---|
| `install.allow_third_party` | `packages.allow_third_party` |
| `install.allowed_tiers` | `packages.allowed_tiers` |
| `install.allowed_publishers` | `publishers.allow` |
| `install.denied_ids` | `plugins.deny_ids` |
| `install.allowed_ids` | `plugins.allow_ids` |
| `grants.forbidden` | `grants.deny` |
| `grants.forced_declined` | removed; it meant the same as `grants.deny` |
| `registry.allowed_urls` | `packages.allowed_sources` |
| `registry.allow_url_installs` | `packages.allow_url_installs` |
| `update.allowed_channels` | `packages.allowed_channels` |
| `audit.enabled`, `audit.path`, `audit.retention` | removed; the audit log is always on, see below |

A plugin that the policy blocks is in the package state `disabled-by-policy` and has the disabled reason `policy.blocked` (see [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)). The state is not written to `installed.json`, so removing the key lets the plugin run again after the next start.

The two labels have different scopes: a grant row in the review dialog keeps "Blocked by your administrator" ([SEC-185]), and a plugin row in the plugin list keeps "Blocked by policy" ([LIF-193]). A grant that matches `grants.deny` never fails an install and never disables the plugin ([SEC-198]). The review dialog shows the row locked off with the text "Blocked by your administrator", and a call returns `not-permitted` with the reason `policy`.

- [USR-214] A grant that matches `grants.deny` MUST NOT be granted, MUST be shown as locked in the review dialog and in the Capabilities tab with the text "Blocked by your administrator", and MUST make calls return `not-permitted`. Test: deny `clipboard.read`, install a plugin that requests it, and assert the row is locked and a call returns `not-permitted` with the message starting `policy: ` and the log line `refused clipboard.read by <function> (policy)`.
- [USR-215] A plugin whose manifest requests a denied grant MUST still install and run, with that grant refused. Test: install such a plugin and assert state `enabled` and the refusal log line.
- [USR-216] `plugins.deny_ids` MUST win over `plugins.allow_ids`. Test: list one id in both and assert it cannot be installed, with the code `policy.blocked` and the message starting `policy.id-denied: `.

### How the keys combine

At install the host evaluates the policy at the policy step (step 7 of the 11 steps of SEC-100) of the install pipeline, after the signature and manifest checks and before any prompt. Inside that step it checks these keys in this order and stops at the first failure.

1. `packages.allow_third_party`
2. `plugins.deny_ids`
3. `plugins.allow_ids`
4. `packages.allowed_tiers`
5. `publishers.deny` and `publishers.allow`
6. `packages.allowed_sources`

Grants are not checked at install. A user can make the result stricter with their own declines, never looser. Core plugins ignore every `packages`, `plugins` and `publishers` key, because they ship with Ilium. They do respect `grants.deny` for third-party callers only.

- [USR-217] The host MUST check the policy keys in the order above and report the first failing key as the detail code. Test: write a policy that fails four keys at once and assert the detail code is the first in the order.
- [USR-218] The policy MUST NOT stop a core plugin or a bundled T1 plugin from running. Test: set `packages.allow_third_party = false` and `packages.allowed_tiers = []`; assert all 30 core plugins still start.

### What the user sees

The install command and the review dialog name the policy and the key that blocked the action. They do not say how to remove the block, because the user cannot.

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin install notes.iliumpkg
<span class="t-err">error</span> policy.blocked
Cannot install Notes 1.2.0.
policy.tier-denied: tier T3 is not allowed.
Nothing was installed.
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Install blocked by the policy.</figcaption>
</figure>

Every block is reported with the one install error code `policy.blocked` (the policy step of the install pipeline in [Install, update and uninstall](/pages/reference-install-update-uninstall/)). The message starts with a detail code from this table, followed by a colon and a space.

| Detail code | Meaning |
|---|---|
| `policy.third-party-denied` | `packages.allow_third_party` is `false` |
| `policy.tier-denied` | The tier is not in `packages.allowed_tiers` |
| `policy.publisher-denied` | The signing key is denied or not in `publishers.allow` |
| `policy.id-denied` | The id is in `plugins.deny_ids`, or not in `plugins.allow_ids` |
| `policy.source-denied` | The package source is not in `packages.allowed_sources`, or URL installs are off |
| `policy.grant-forbidden` | A grant matches `grants.deny`. This is never an install failure. It is the detail shown next to the locked row, and calls fail with `not-permitted` and the reason `policy` |

### The audit log

The audit log is always on. It is defined only in the [Audit log reference](/pages/reference-audit-log/) ([SEC-220] to [SEC-228]). Policy may set only `audit.sink`, `audit.max_mib` and `audit.keep_days`. The events `policy-loaded` and `policy-violation` record what the policy did.

- [USR-219] The host MUST write a `policy-loaded` audit record at start when a policy file exists, and a `policy-violation` record when a plugin or file violates it, and MUST NOT write plugin data into either. Test: start with a policy that denies one installed plugin and count the two records.
- [USR-220] The host MUST delete rotated audit files older than `audit.keep_days` at server start ([SEC-224]). Test: age a rotated file by 366 days and restart.

### Errors and edge cases

| Situation | What happens |
|---|---|
| No policy file | Defaults apply: everything the security model allows is allowed |
| `version` is not `1` | The file is malformed: T2 and T3 plugins refuse to load with `policy.invalid` |
| A key lists a plugin that is not installed | Accepted, no effect |
| An installed plugin is forbidden after the fact | At the next start it is disabled, shown as `Blocked by policy`, and kept on disk. Removing the key lets the user enable it again |
| The audit file cannot be written | No action is refused. The server queues up to 1,000 records, retries every 10 s and shows a notice ([SEC-225]) |
| A user sets their own `XDG_CONFIG_HOME` | No effect on the system file. A user file can only add restrictions |

- [USR-210] The host MUST read the policy file before it loads any third-party plugin, and MUST NOT load a third-party plugin when the file exists but cannot be read or parsed. Test: make the file unreadable and start Ilium; assert that core plugins run and that every T2 and T3 plugin refuses to load with `policy.invalid`.
- [USR-211] The host MUST ignore a system policy file that a non-administrator can write, MUST show a persistent notice, and MUST record the audit event `policy-violation` with the reason `policy-file-writable` ([SEC-182]). Test: set the mode to `0666`; assert the notice and the audit record.
- [USR-212] The host MUST treat a policy with an unknown key or a wrong type as malformed and MUST name the line of the first error in the notice. Test: write a policy with the key `allow_thirdparty`; assert that T2 and T3 plugins refuse to load, T1 plugins run, and the notice names line and key.
- [USR-213] The host MUST read the policy at server start only. Test: set `packages.allow_third_party = false` while a T2 plugin runs and assert that nothing changes; restart and assert the plugin is shown as `Blocked by policy` with the message `policy.third-party-denied`.

## Audit log tamper evidence requirements

- [USR-257] The host MUST chain each audit record to the previous one with a hash, and `ilium plugin audit verify` MUST report the first record where the chain is broken, or confirm the file is intact. Test: Edit one record and assert verify names it; truncate the file and assert verify reports the break.
- [USR-258] The host MUST cap the total storage of audit and crash records at a configurable ceiling (default 256 MiB), delete the oldest rotated files first when it is reached, and write a record that says how many were deleted. Test: Fill storage above the ceiling and assert oldest files go first and a deletion record exists.

### See also

- [Admin policy reference](/pages/reference-admin-policy/)
- [Administration guide](/pages/guide-administration/)
- [Audit log](/pages/reference-audit-log/)
- [Plugin files on disk](/pages/reference-plugin-files/)
- [Security](/pages/security/)
- [Permissions explained](/pages/user-permissions-explained/)

API version checked: `ilium:plugin@0.1.0`.
