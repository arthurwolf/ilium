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
