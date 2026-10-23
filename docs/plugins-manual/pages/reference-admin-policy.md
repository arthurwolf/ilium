---
layout: page
title: Admin policy reference
permalink: /pages/reference-admin-policy/
---

## Admin policy reference

An administrator of a shared machine, a team or a company needs to decide which plugins people may run, without editing each person's settings. The **admin policy** is a file that the administrator owns. It limits what users and plugins can do. This page defines its locations, its schema, how it combines with user choices, and what users see when it blocks something.

The broker rule in the design is: a plugin may do something only if the tier ceiling, the install plan, the user's grants **and** the policy all allow it. The **install plan** is the ordered list of package id, version, tier, publisher fingerprint and requested grants that step 6 of the install pipeline evaluates. The broker evaluates the policy against this list and returns the first failing key's code, in the order of [USR-217](/pages/reference-plugin-policy/#how-the-keys-combine). This page defines the policy part. For deployment recipes see [Enterprise deployment](/pages/guide-enterprise-deployment/).

### You will need

- A system where you can write a root-owned file (administrator rights).
- The install pipeline in [Grant review and consent](/pages/reference-grant-review-and-consent/).
- The grant names in the [Capability table](/pages/reference-capabilities/).

## Locations and precedence

**[SEC-180]** The server reads these files at start, in this order, and merges them:

| Order | Platform | Path |
|---|---|---|
| 1 | Linux | `/etc/ilium/policy.toml` |
| 1 | macOS | `/Library/Application Support/Ilium/policy.toml` |
| 1 | Windows | `%ProgramData%\Ilium\policy.toml` |
| 2 | All | `<user config dir>/policy.toml` (optional user file) |

**[SEC-181]** Merge rules. The user file can only make the policy stricter. For a list of allowed values the effective list is the intersection; for a list of denied values it is the union; for a boolean `allow_*` the effective value is the logical AND; for a boolean `*_only` or `offline` the effective value is the logical OR. A user file can never lift a restriction of the system file. **Deny wins over allow.** Policy wins over user choices and over the manifest.

**[SEC-182]** The system file MUST be owned by root (or by the Administrators group on Windows) and not writable by group or others. If it is not, it is **ignored** and the audit log records `policy-violation` with reason `policy-file-writable`. Because a policy that is silently ignored would be a security loss, the server also shows a persistent notice `Admin policy ignored: file is writable by non-administrators`.

## Schema

All keys are optional. A missing section means no restriction from that section. Types: `string`, `[string]` (list), `bool`, `int`.

```toml
version = 1

[packages]
allow_third_party = true           # default: true; false refuses every T2 and T3 install
allowed_tiers   = ["t1", "t2"]        # default: ["t1","t2","t3"]
allow_unsigned  = false               # default: true (T3 allowed)
allow_dev_mode  = false               # default: true
allowed_sources = ["file:///opt/ilium-plugins/", "/srv/plugins/"]  # default: any
blocklist_file  = "/etc/ilium/blocklist.json"
precompiled_cache = true
allow_url_installs = false          # default: true; false refuses a bare URL or file path source
allowed_channels = ["stable"]       # default: ["stable","beta"]
block_vulnerable_runtime = false

[publishers]
trusted_keys = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
allow        = ["3f2a91c0…"]          # when set, only these keys may sign T2 packages
deny         = ["a1b20c9e…"]

[plugins]
allow_ids      = ["example.*", "ilium.*"]    # glob on ids; default: all
deny_ids       = ["example.experimental.*"]
force_enabled  = ["example.compliance"]      # cannot be disabled by the user
force_disabled = ["example.fun"]             # cannot be enabled by the user

[grants]
deny              = ["http:*.pastebin.example", "process.run:sh", "terminal.history"]
max_risk          = "high"            # "normal" refuses every high-risk grant
deny_combinations = true              # refuse plugins that read data and send it out
[grants.preapproved]
"example.compliance" = ["fs.read:project", "process.run:git"]

[extensions]
allow_replacement = false
deny_points       = ["ilium.core.terminal/before-send-input"]

[network]
allow_hosts = ["*.example.com"]
deny_hosts  = ["*.pastebin.example"]
ca_bundle   = "/etc/ssl/corp-ca.pem"
proxy       = "http://proxy.example.com:3128"
offline     = false

[processes]
deny_env = ["EXAMPLE_INTERNAL_TOKEN"]

[settings]
locked = { "terminal.scrollback" = 20000, "plugins.example.compliance.endpoint" = "https://c.example.com" }

[inference]
allowed_providers = ["local", "corp-gateway"]

[projects]
allow_project_values = false          # plugin values in .ilium/ are always ignored

[audit]
sink      = "file"                    # "file", "syslog" or "journald"
max_mib   = 64
keep_days = 365
```

| Key | Type | Default | Effect |
|---|---|---|---|
| `packages.allow_third_party` | bool | true | `false` refuses to install, enable or run T2 and T3 plugins with the detail code `policy.third-party-denied`. Core and bundled plugins are not affected |
| `packages.allow_url_installs` | bool | true | `false` refuses `install` from a bare URL or file path with `policy.source-denied`; registry ids and bundles still work |
| `packages.allowed_channels` | list of `stable`, `beta` | both | Channels a user may select |
| `packages.allowed_tiers` | list | all | Install of another tier fails with `policy.tier-denied`; an installed one is disabled |
| `packages.allow_unsigned` | bool | true | `false` refuses `--allow-unsigned` and `ilium plugin dev` for T3 |
| `packages.allow_dev_mode` | bool | true | `false` refuses `ilium plugin dev` |
| `packages.allowed_sources` | list | any | Install files must lie below a listed path, file URL or `https://` registry prefix (scheme, host and path prefix are compared; a trailing slash does not matter); else `policy.source-denied`. This is the "managed registry" of 0.1.0: a managed folder, not a server |
| `packages.blocklist_file` | path | none | Extra blocklist ([SEC-147]) |
| `publishers.trusted_keys` | list | none | Keys that count as trusted without a user prompt |
| `publishers.allow` / `deny` | list | none | Fingerprints allowed or refused as signers; refusal code `policy.publisher-denied` |
| `plugins.allow_ids` / `deny_ids` | list | none | Glob on ids; refusal code `policy.id-denied` |
| `plugins.force_enabled` / `force_disabled` | list | none | See [SEC-189] |
| `grants.deny` | list | none | Grant patterns ([SEC-193]). A matching grant is locked off and refused at run time with reason `policy`; the install still succeeds ([SEC-198]) |
| `grants.max_risk` | string | high | `normal` locks off every high-risk grant like a denied grant ([SEC-198]) |
| `grants.deny_combinations` | bool | false | Refuses the toxic combination ([SEC-004]) |
| `grants.preapproved` | table | none | Grants given without a dialog ([SEC-117]) |
| `extensions.allow_replacement` | bool | true | `false` refuses `replaces` with `policy.replacement` |
| `extensions.deny_points` | list | none | Contributions to the listed points are dropped and the grant row is locked off ([SEC-382]) |
| `network.*` | | | See [Network egress](/pages/reference-network-egress/) |
| `processes.deny_env` | list | none | Added to the environment denylist ([SEC-033]) |
| `settings.locked` | table | none | Setting key to value; the key is read-only for the user and plugins |
| `inference.allowed_providers` | list | all | Other providers are refused with reason `policy` |
| `projects.allow_project_values` | bool | true | `false` ignores all plugin values in `.ilium/` |
| `audit.sink`, `max_mib`, `keep_days` | | file, 64, 365 | See [Audit log](/pages/reference-audit-log/) |
| `packages.blocked_versions` | list of `id@version` | none | Exact versions refused with `policy.version-denied` (proposed, owner to confirm; see [SEC-951]) |
| `packages.pinned_versions` | table, id to version | none | The only version of that id that may be installed or run (proposed, owner to confirm; see [SEC-952]) |

## Evaluation

**[SEC-184]** The install pipeline applies the policy at the steps marked in [Grant review and consent](/pages/reference-grant-review-and-consent/): `policy.tier-denied`, `policy.id-denied`, `policy.publisher-denied` and `policy.source-denied` at SEC-100 step 6; `policy.replacement` at SEC-100 step 9. A grant that matches `grants.deny` is not an install failure ([SEC-198]). At run time, a host call that the policy forbids returns `not-permitted` with reason `policy`. The user and the plugin see reason `policy`, never the policy text.

**[SEC-185]** In the review dialog a policy-denied row is locked off and reads `Blocked by your administrator`:

<figure>
<pre class="term"><span class="t-hi">╭─ Install example.pane-tools 2.0.0 ───────────╮</span>
│ [ ] <span class="t-warn">HIGH</span> Run rg ………………………………… process.run:rg │
│ <span class="t-dim">    Blocked by your administrator</span>            │
│ [x] Show toasts ……………………………………… notify.toast │
│                                              │
│ <span class="t-hi"> Esc </span> Cancel                                 │
╰──────────────────────────────────────────────╯
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A policy-denied row is locked off and cannot be checked.</figcaption>
</figure>

**[SEC-186]** An installed plugin that violates a new policy rule of the kinds that disable a plugin (`plugins.deny_ids`, `packages.allowed_tiers`, `packages.allow_third_party` and the publisher rules, see [SEC-198]) is disabled at the next start, with a notice `Disabled by your administrator`, and the audit log records `policy-violation`. Its data is kept.

**[SEC-187]** A malformed policy file (TOML error, unknown key, wrong type, `version` not 1) fails **closed** for third parties: T2 and T3 plugins refuse to load with `policy.invalid`, and T1 plugins run normally. The server shows `Admin policy is invalid: <line and message>` and keeps running. A policy file that is missing is not an error (no policy). An unreadable one (permission error) counts as malformed.

**[SEC-188]** The policy is read at server start only. A change takes effect at the next start. `ilium plugin policy reload` is not part of version 0.1.0.

**[SEC-189]** Forced states. `plugins.manage` and the Plugins screen MUST NOT enable a plugin in `force_disabled` or disable one in `force_enabled`. The call returns `not-permitted` with reason `policy`, and the screen shows a lock mark. A `force_enabled` plugin that is not installed is reported in `policy show` as `missing` and in the audit log; the server does not install it.

**[SEC-190]** Locked settings. A key in `settings.locked` is shown with a lock mark and its policy value. A write by the user, by `ilium config set`, or by a plugin with `settings.write-all` is refused with `settings.locked`. A plugin reads the policy value.

**[SEC-191]** `ilium plugin policy show` prints the effective policy as JSONL: one `policy-source` line per file read (path, owner, hash, used or ignored), one `policy` line with the merged values, and one `summary` line.

**[SEC-192]** `[projects] allow_project_values = false` makes every plugin value in a project folder ignored, regardless of project trust ([SEC-310]).

**[SEC-193]** Grant patterns. An entry of `grants.deny` has one of these forms and no other: `<name>` matches the bare grant and every argument form of that name; `<name>:<arg>` matches exactly that argument; `<name>:*` matches any argument; `<prefix>.*` matches every grant name that starts with `<prefix>.`, such as `clipboard.*`; and for http, `http:*.<domain>` matches any host grant whose host ends in `.<domain>`, including a `*.<sub>.<domain>` grant, so `http:*.pastebin.example` matches `http:a.pastebin.example`. `*` is valid only in these positions. A policy with any other pattern is rejected with `policy.invalid` ([SEC-187]). Test: deny `clipboard.*` and assert `clipboard.read` is locked; deny `http:*.pastebin.example` and assert `http:a.pastebin.example` is locked and `http:pastebin.example` is not; deny `net.*` and assert `policy.invalid`.

**[SEC-198]** Denied grants. A grant that matches `grants.deny` MUST NOT fail the install and MUST NOT disable the plugin. The review dialog shows the row locked off with `Blocked by your administrator`, and a call returns `not-permitted` with the message starting `policy: `. When the server reads a policy that denies a grant a plugin already holds, it removes the grant through the normal revocation path (authorization epoch bump, audit event `grant-revoked`, and `deactivate(revoked)` when the plugin was running). Only `plugins.deny_ids`, `packages.allowed_tiers`, `packages.allow_third_party` and the publisher rules disable a plugin. Test: deny `clipboard.read`, install a plugin that requests it, and assert state `enabled`, a locked row and a refused call; restart with the grant newly denied and assert the grant is gone from the plugin's granted list.

**[SEC-382]** Denied extension points. A contribution grant to a point in `extensions.deny_points` is treated as a denied grant ([SEC-198]): the install succeeds, the grant row is locked off, the contribution is not registered, and the owner never calls it. A plugin that owns a denied point (`extensions.provide`) still runs, and contributions from other plugins to it are dropped. The audit log records `policy-violation` with the point id. Test: deny a point, install a plugin that contributes to it, and assert the install succeeds, the row is locked, the owner's callback is never invoked and the audit record exists.

**[SEC-199]** Multi-user machines: see [Enterprise deployment](/pages/guide-enterprise-deployment/). Each user has their own stores and runtime directory. The system policy applies to all users. No store is shared between users.

## Ceilings, versions, retention and notices

### Resource ceilings

**[SEC-950]** The policy has no key that sets a ceiling for a session resource: `session_memory_mib`, `session_helpers`, `session_processes`, `disk_mib`, `storage_bucket_mib` and the `ilium-execution` CPU budget. A user may only lower them under `[plugins.limits]`, and a higher user value is clamped with a logged warning ([HST-406], [Errors and limits](/pages/reference-errors-and-limits/)). An administrator who needs a ceiling per resource must ask the owner for a key such as `limits.session_memory_mib_max` (proposed, owner to confirm); until that key exists, this page does not promise any administrator cap. Test: set `[plugins.limits] session_memory_mib` above the default and assert the clamp and the warning, with no policy key that changes the result.

### Version rules

**[SEC-951]** (proposed, owner to confirm) An entry of `packages.blocked_versions` has the form `<id>@<version>` with an exact version. A package whose id and version match an entry MUST be refused at install and update with `policy.version-denied`. An installed copy of a blocked version MUST be disabled at the next server start, with the notice `Disabled by your administrator` ([SEC-186]), and its data MUST be kept. The other versions of the same id are not affected. Test: block `example.pane-tools@2.0.0`, assert that 2.0.1 installs, that 2.0.0 is refused, and that an installed 2.0.0 is disabled at the next start.

**[SEC-952]** (proposed, owner to confirm) An entry of `packages.pinned_versions` names one version for one id. Install and update of that id MUST install only the pinned version, and a different installed version MUST NOT run; the refusal code is `policy.version-pinned`. A pin never removes data. Test: pin `example.pane-tools` to 1.4.2, attempt an update to 2.0.1 and assert the refusal, then remove the pin and assert the update succeeds.

### Audit retention and the storage ceiling

**[SEC-953]** (proposed, owner to confirm) Precedence between `audit.keep_days` and the storage ceiling of [USR-258]. `audit.keep_days` is the minimum retention for audit files: the host MUST NOT delete a rotated audit file younger than `audit.keep_days`, even when the ceiling is reached. When the ceiling is reached, the host deletes crash records first, then the oldest rotated audit files that are older than `audit.keep_days`. If only younger audit files remain above the ceiling, the host keeps them, writes `policy-violation` with reason `audit-storage-full`, and shows a persistent notice. Test: age three rotated files to 10, 100 and 400 days, fill the ceiling, and assert that only the 400-day file is deleted.

**[SEC-954]** (proposed, owner to confirm) Every deletion of an audit or crash file by the storage ceiling or by `audit.keep_days` MUST write one `audit-retention-deleted` record first, with the file name, the age in days, the bytes freed and the rule that applied (`keep-days` or `storage-ceiling`). The record is written to the current audit file, so the deletion stays visible after the file is gone. Test: trigger each rule and read back the record.

### Schema versions

**[SEC-101]** (proposed, owner to confirm) Schema version `1` is the only version in 0.1.0 ([SEC-187]). A later schema version is introduced in a release note, and a server MUST keep reading the previous version for two minor releases after it is introduced (proposed, owner to confirm). A server that reads a `version` it does not support treats the file as malformed under [SEC-187]. Because a key that a newer server understands is an unknown key to an older server, the staged path is: write the file as `version = 1` until every machine runs a server that reads the new key, then change the version. The file is not ignored key by key, so [SEC-187] stays the only rule for unknown keys. Test: a file with `version = 2` on a server that supports 1 is refused with `policy.invalid` and third parties do not load.

### Notice text

**[SEC-115]** (proposed, owner to confirm) The reason codes are the stable contract for every notice on this page: `policy.invalid`, `policy.tier-denied`, `policy.id-denied`, `policy.publisher-denied`, `policy.source-denied`, `policy.replacement`, `policy.version-denied` and `policy.version-pinned`. The notice and refusal texts are host English text in 0.1.0, with no translation shipped and no locale switch. A localised text MUST map from the code, and support staff MUST match a user's text to the code in the `audit` record. Open item: [SEC-186] gives `Disabled by your administrator`, while the Plugins row label in [Settings, plugin rows](/pages/core-plugin-settings/) is `Blocked by policy`. The owner MUST choose one text per surface before release. Test: for each code, trigger the refusal and assert that the audit record carries the code and not the text.

## Examples

Lock the machine to one publisher and block all high-risk grants:

```toml
version = 1
[packages]
allowed_tiers  = ["t1", "t2"]
allow_unsigned = false
[publishers]
allow = ["3f2a91c07b6d1e44a1b20c9e55d3f7a8b6c102e94d7798af13c56e0bd2f48a19"]
[grants]
max_risk = "normal"
```

A package signed by another key fails with `policy.publisher-denied`. A package asking for `terminal.history` still installs. The review dialog shows the `terminal.history` row locked off, and a call returns `not-permitted` with the reason `policy`.

An offline site:

```toml
version = 1
[network]
offline = true
[packages]
allowed_sources = ["/srv/ilium-plugins/"]
```

## Edge cases

| Situation | Result |
|---|---|
| Both files set `offline`; user file sets `false` | Effective `true` (OR) |
| System allow list `["a"]` and user allow list `["a","b"]` | Effective `["a"]` |
| `force_enabled` plugin is blocked by `deny_ids` | Deny wins; reported in `policy show` as `conflict` |
| Policy denies a grant a bundled T1 plugin needs | The grant stays off; the plugin runs without it |
| `preapproved` grant not listed in the manifest | Ignored |
| Policy file replaced while the server runs | Not read until restart; audit line at next start |

### See also

- [Security model](/pages/security/)
- [Enterprise deployment](/pages/guide-enterprise-deployment/)
- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Audit log](/pages/reference-audit-log/)
- [Capability: plugins.manage](/pages/capability-plugins-manage/)

Contract: `ilium:plugin@0.1.0`.
