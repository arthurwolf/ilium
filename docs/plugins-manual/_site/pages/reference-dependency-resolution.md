# Versions and dependencies reference

## Versions and dependencies reference

This page defines how plugin versions are written and compared, how a plugin states what it needs, how the host chooses versions, and how update channels work. It settles three older statements that disagreed: the "same major, minor at least as high" rule, the cycle rule in the lifecycle page, and the two forms of a service version.

A **version** is a number such as `1.4.2` that names one release of a plugin. A **range** is a rule that accepts some versions, such as `^1.4`. A **dependency** is something a plugin needs: another plugin, a service, an extension point or a minimum Ilium release. **Resolution** is the choice of one version for every plugin so that all ranges hold. A **channel** is a named stream of releases, such as `stable` or `beta`.

Requirement ids such as `PKG-310` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The manifest keys: [Manifest reference](/pages/reference-manifest/).
- How the contract version differs from the plugin version: [Versioning](/pages/versioning/).
- How activation waits for services: [Lifecycle](/pages/lifecycle/).

## Plugin versions

A plugin version is a Semantic Versioning 2.0.0 string.

| Id | Rule |
|---|---|
| [PKG-310] | A version matches `MAJOR.MINOR.PATCH` with an optional `-prerelease`, as SemVer 2.0.0 defines. MAJOR, MINOR and PATCH are integers from 0 to 2,147,483,647 without leading zeros. |
| [PKG-311] | A prerelease is dot-separated identifiers of `[0-9A-Za-z-]`. Numeric identifiers have no leading zeros. The whole version is at most 64 characters. |
| [PKG-312] | A leading `v` is not part of a version. `v1.2.3` is refused with `manifest.version-invalid`. |
| [PKG-313] | Build metadata (`+build.5`) is allowed in a source manifest. It is ignored in every comparison and two versions that differ only in build metadata are equal. A registry refuses a version that has build metadata (`registry.version-invalid`), so each registry version has one meaning. |
| [PKG-314] | Precedence follows SemVer 2.0.0 section 11. A prerelease is lower than the release with the same numbers: `1.0.0-beta.2` is lower than `1.0.0`. Numeric identifiers compare as numbers. Alphanumeric identifiers compare in ASCII order. A numeric identifier is lower than an alphanumeric one. A shorter list is lower when all earlier identifiers are equal. |

Precedence examples, from lowest to highest: `0.9.0`, `1.0.0-alpha`, `1.0.0-alpha.1`, `1.0.0-alpha.beta`, `1.0.0-beta`, `1.0.0-beta.2`, `1.0.0-beta.11`, `1.0.0-rc.1`, `1.0.0`, `1.0.1`, `1.1.0`.

The plugin version is not the contract version. The contract is `ilium:plugin@0.1.0`, which the `api` key states. A plugin may be at version `7.3.0` and still use contract `0.1.0`.

## Ranges

| Id | Rule |
|---|---|
| [PKG-315] | A range is one or more comparators separated by single spaces. A version satisfies the range when it satisfies all comparators (logical AND). The operator `\|\|` is not supported and is refused with `manifest.requires-invalid`. |
| [PKG-316] | A comparator is one of: an exact version `=1.2.3` or `1.2.3`, `>1.2.3`, `>=1.2.3`, `<1.2.3`, `<=1.2.3`, a caret range `^1.2.3`, a tilde range `~1.2.3`, or the star `*`. |
| [PKG-317] | A bare version with no operator (`1.2.3`, `1.2`, `1`) means a caret range. This is the Cargo rule. It makes `@0.1.0` and `@^0.1.0` the same. |
| [PKG-318] | A caret range `^M.m.p` accepts versions that do not change the left-most non-zero part: `^1.2.3` is `>=1.2.3 <2.0.0`. `^0.2.3` is `>=0.2.3 <0.3.0`. `^0.0.3` is `>=0.0.3 <0.0.4`. `^0.0` is `>=0.0.0 <0.1.0`. `^0` is `>=0.0.0 <1.0.0`. |
| [PKG-319] | A tilde range `~M.m.p` accepts patch changes: `~1.2.3` is `>=1.2.3 <1.3.0`. `~1.2` is `>=1.2.0 <1.3.0`. `~1` is `>=1.0.0 <2.0.0`. |
| [PKG-320] | `*` accepts every release version. |
| [PKG-321] | A prerelease version satisfies a range only if some comparator of the range has the same MAJOR.MINOR.PATCH and itself carries a prerelease (the npm rule), or when the resolver runs on the `beta` channel for that plugin (see Channels). `1.3.0-beta.1` does not satisfy `^1.2.0` on the `stable` channel. |
| [PKG-322] | A range is at most 128 characters. An invalid range is refused with `manifest.requires-invalid`. |

This rule replaces the rule "same major, minor at least as high" that three pages stated. That rule is wrong for versions that start with `0`, because under SemVer every `0.x` minor may break the interface. The caret rule gives the right answer for both cases.

| Required range | Provider `0.1.0` | Provider `0.1.4` | Provider `0.2.0` | Provider `1.0.0` |
|---|---|---|---|---|
| `0.1.0` (caret) | yes | yes | no | no |
| `^0.2` | no | no | yes | no |
| `>=0.1.0 <1.0.0` | yes | yes | yes | no |
| `~0.1.2` | no | yes | no | no |
| `*` | yes | yes | yes | yes |

## The `requires` object

```json
"requires": {
  "host": ">=0.1.0",
  "services": ["ilium:inference/provider@0.1.0", "com.example.git/status@^1.2"],
  "plugins": [
    { "id": "com.example.git", "version": "^1.2", "optional": false },
    { "id": "com.example.theme-pack", "version": "*", "optional": true }
  ],
  "wait_ms": 2000
}
```

| Key | Type | Default | Constraint | Code |
|---|---|---|---|---|
| `host` | string | none | A range over Ilium release versions. The host version is a SemVer string such as `0.1.0` | `manifest.requires-invalid`, `dependency.host-version` |
| `services` | list of string | `[]` | At most 32. Each is `<interface>@<range>`. The interface name follows the grant argument rule for `services.use`. The `@<range>` part is optional and defaults to `*` | `manifest.requires-invalid` |
| `plugins` | list of object | `[]` | At most 32. Keys `id` (a plugin id), `version` (a range, default `*`) and `optional` (boolean, default `false`) | `manifest.requires-invalid`, `manifest.requires-self` |
| `wait_ms` | integer | 2000 | 0 to 5000 | `manifest.requires-invalid` |

| Id | Rule |
|---|---|
| [PKG-323] | A plugin MUST NOT list its own id in `requires.plugins`. It is refused with `manifest.requires-self`. |
| [PKG-324] | `requires.plugins` MUST NOT name an id twice. Duplicates are refused with `manifest.requires-invalid`. |
| [PKG-325] | `requires.host` is checked against the version of the running Ilium release. A release that does not satisfy it is refused with `dependency.host-version` at install and keeps the plugin `disabled` with a reason at load time. |
| [PKG-326] | `wait_ms` replaces the fixed 2 s activation wait of the lifecycle page. It is the time a plugin's activation waits for each required service provider. A value above 5000 is refused. The older fixed values 2 s default and 5 s maximum remain the default and the ceiling. |
| [PKG-327] | The service form in `requires.services` is `interface@range` with the range grammar of this page. The form `ilium:inference/provider@0.1.0` stays valid and means `^0.1.0`. The older forms `@0.1` and `@0.1.0` are therefore the same. |
| [PKG-328] | An extension point reference `<plugin-id>/<point>@<minimum>` in `contributes.extensions.point` uses the same grammar: the part after `@` is a range, and a bare version is a caret range. `requires.points` does not exist. The point reference is the dependency. |

## What blocks an install and what does not

| Dependency | At install | At activation |
|---|---|---|
| `requires.host` | Refuses if not satisfied | Plugin stays disabled with a reason |
| `requires.plugins` (not optional) | Resolves from installed plugins and configured sources, or refuses with `dependency.unresolved` | Plugin does not activate until the required plugin is active |
| `requires.plugins` (optional) | Never blocks | Plugin activates without it |
| `requires.services` | A missing provider never blocks. A cycle is refused with `manifest.service-cycle` | Activation waits up to `wait_ms` per provider in the instance state `waiting-dependency`, then the side returns to `inactive` and shows "waiting for" |
| Extension point owner | Never blocks | The contribution is dormant until the owner is active |

[PKG-329] A missing service provider and a missing extension point owner MUST NOT block an install. This keeps the rule of the versioned-service tutorial: a missing provider never makes install fail. Only `requires.host`, non-optional `requires.plugins` and a service cycle block.

## Resolution

The resolver chooses one version per plugin id for the whole installation. Two versions of one id never run side by side.

| Id | Rule |
|---|---|
| [PKG-330] | The inputs are: the plugin being installed, the installed plugins with their current versions, the pins (see below), the available sources, and the user's channel settings. |
| [PKG-331] | For each required plugin id, the resolver collects every range from every installed or incoming plugin that requires it. The chosen version is the highest version that satisfies all ranges, is permitted by the channel and is not yanked. |
| [PKG-332] | A version that is already installed is preferred over a higher one when it satisfies all ranges and the command is not an update. The resolver does not upgrade a dependency during an install of another plugin unless the range forces it. |
| [PKG-333] | When no version satisfies all ranges, the resolver refuses with `dependency.conflict`. The message lists each constraint with its source plugin, and the versions that were available. |
| [PKG-334] | When a required id has no installed copy and no source has it, the resolver refuses with `dependency.unresolved` and lists every missing id with its range. |
| [PKG-335] | The resolver builds a graph from `requires.plugins` edges that are not optional. A cycle is refused with `dependency.cycle`, naming the cycle in order. Cycles through optional edges never block install. |
| [PKG-654] | A cycle in the graph whose edges are the non-optional `requires.services` entries, resolved against the installed set plus the package, is refused at stage 10 with `manifest.service-cycle`, listing the service names in cycle order starting with the smallest. Activation order is therefore always a directed acyclic graph. |
| [PKG-336] | The tie-break between equal candidates is the order: highest version, then the source order in the configuration, then the lowest `sha256` of the package. The result is deterministic. |
| [PKG-337] | `ilium plugin install --dry-run` prints the resolution plan without changing anything. |

The cycle rule covers hard plugin requirements and non-optional service requirements, as PKG-335 and PKG-654 say. A cycle through an optional edge is not an error.

### Resolution test table

| Id | Installed | Install | Result |
|---|---|---|---|
| [PKG-338] | none | A needs B `^1.0`, source has B 1.0.0, 1.2.0, 2.0.0 | B 1.2.0 installed, then A |
| [PKG-339] | B 1.0.0 | A needs B `^1.0` | B 1.0.0 kept. No dependency change |
| [PKG-340] | B 1.0.0 | A needs B `^1.1`, source has B 1.2.0 | B updated to 1.2.0, then A. The user sees both in the review |
| [PKG-341] | C needs B `^1.0` | A needs B `^2.0` | `dependency.conflict` naming C and A |
| [PKG-342] | none | A needs B, no source has B | `dependency.unresolved` |
| [PKG-343] | none | A needs B, B needs A (hard) | `dependency.cycle` |
| [PKG-344] | none | A needs B optional, no B | A installed. B is not required |
| [PKG-345] | none | A needs service S, no provider | A installed. Instance state `waiting-dependency` for S |
| [PKG-346] | none | A needs host `>=9.0.0`, running release 0.1.0 | `dependency.host-version` |
| [PKG-347] | none | A needs B `^1.0`, only B 1.3.0-beta.1 exists, channel stable | `dependency.unresolved` |

## Effects on update and uninstall

| Id | Rule |
|---|---|
| [PKG-348] | Updating plugin B is refused with `dependency.conflict` when the new version would leave an installed dependant outside its range. The message offers `ilium plugin update --all` or `update B A` to update both together when a satisfying version of A exists. |
| [PKG-349] | `ilium plugin uninstall B` is refused with `dependency.in-use` when an installed, enabled plugin requires B (not optional). `--force` removes B anyway. The dependants are then disabled with the reason `requires B` and are re-enabled when B is installed again. |

## Activation order

| Id | Rule |
|---|---|
| [PKG-350] | The start order is Kahn's algorithm over the edges of the non-optional `requires.plugins` entries and of the `requires.services` entries to their providers. At each step the host takes the ready node with the smallest key: the tier number (T1 first), then the plugin id in byte order. Up to 4 activations run at once, started in that order (LIF-045 to LIF-047). |
| [PKG-351] | Shutdown runs the exact reverse of the start order, not the reverse of the order in which activations finished. Stops run one at a time (LIF-050). |
| [PKG-352] | A plugin with `replaces: ["id"]` satisfies a `requires.plugins` entry for the replaced id when the replacement is enabled and its version satisfies the range. The replacement is a different plugin, so its own version is compared with the range. A consumer that needs the replaced plugin's version series MUST name a range that the replacement also satisfies, or declare the dependency optional. |

The last rule is deliberately narrow. A replacement states which surface it provides, but it does not claim the version history of the plugin it replaces.

## Channels

A channel is a stream of releases. There are three.

| Channel | Meaning | Where it appears |
|---|---|---|
| `stable` | Releases meant for everyone. Version numbers have no prerelease part | Registry, files |
| `beta` | Releases for testers. Version numbers may carry a prerelease part | Registry, files |
| `dev` | Local builds from `ilium plugin dev` | Local only, never in a registry |

| Id | Rule |
|---|---|
| [PKG-353] | Each registry version belongs to exactly one channel. A version with a prerelease part belongs to `beta`. A version without one belongs to `stable` or `beta`, and the publisher chooses at publish time. |
| [PKG-354] | A user sets the default channel in `[plugins] channel = "stable"` (default `stable`) and can set a channel for one plugin with `ilium plugin channel <id> <stable\|beta>`. A plugin on `beta` is offered both `beta` and `stable` versions. A plugin on `stable` is offered only `stable` versions. |
| [PKG-355] | A switch from `beta` to `stable` never installs a lower version by itself. The plugin stays at its version until a `stable` version with a higher precedence appears. `ilium plugin install <id>@<version> --allow-downgrade` is the explicit way back. |
| [PKG-356] | A file install ignores channels. The version is whatever the file holds. The record keeps `channel: dev` for `ilium plugin dev` and `channel: file` for other files. |
| [PKG-357] | Administrators can limit the allowed channels with the policy key `packages.allowed_channels`. See [Administrator guide](/pages/guide-plugin-administration/). |

## Pins and lock files

| Id | Rule |
|---|---|
| [PKG-358] | `ilium plugin pin <id>@<version>` records an exact version. Update commands skip pinned plugins, and the resolver treats a pin as the only allowed version of that id. `ilium plugin unpin <id>` removes it. |
| [PKG-359] | The lock file `plugins.lock` is JSON: `{ "format": "ilium-lock/1", "plugins": [ { "id", "version", "sha256", "source" } ] }`, sorted by id. `ilium plugin lock` writes it from the installed set. `ilium plugin install --locked <file>` installs exactly those versions. A locked version that no source offers fails with `dependency.unresolved`. A resolution that differs from the lock fails with `dependency.lock-mismatch`. |
| [PKG-360] | A lock file entry whose `sha256` differs from the SHA-256 of the downloaded package is refused with `registry.hash-mismatch`. |

Use a lock file when the same plugin set must run on many machines. See [Administrator guide](/pages/guide-plugin-administration/).

## Worked example

Plugin `com.example.reviews` needs the plugin `com.example.git` at `^1.2` and an inference service. The user has `com.example.git 1.1.0` and no other plugin.

1. The resolver reads the requirement `com.example.git ^1.2`.
2. `1.1.0` does not satisfy it. The sources list `1.2.0`, `1.3.1` and `2.0.0`.
3. It chooses `1.3.1`, the highest version below `2.0.0`.
4. The review dialog shows two packages: `com.example.git 1.3.1` (update) and `com.example.reviews` (new).
5. After approval the host installs `git`, then `reviews`. At start `git` activates first.
6. The inference provider is missing, so `reviews` activates after the 2 s wait in state `waiting-dependency` and shows "waiting for ilium:inference/provider".

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin install reviews.pkg --dry-run
<span class="t-dim">plan</span> update  com.example.git 1.1.0 > 1.3.1
<span class="t-dim">plan</span> install com.example.reviews 0.4.0
<span class="t-warn">note</span> waits for ilium:inference/provider
<span class="t-hi">dry-run</span> nothing was changed
</pre>
<figcaption>Rendered at 48 columns. Illustrative. A resolution plan.</figcaption>
</figure>

## Edge cases

| Situation | Result |
|---|---|
| A range with a space inside a comparator (`>= 1.0`) | Refused with `manifest.requires-invalid` |
| Same version, different hash | Refused with `package.version-reused` |
| A yanked version is already installed | It stays installed and works. It is never chosen for a new install |
| Required plugin disabled by the user | The dependant stays enabled but in state `waiting-dependency` until the required plugin is on |
| `requires.host` is a prerelease range | Supported with the prerelease rule of [PKG-321] |

## What the user sees

The review dialog lists plugins that will be installed or updated because of a dependency, under **Also installs**. A plugin that waits shows `waiting for <name>` in Settings > Plugins. See [Finding and installing plugins](/pages/user-plugin-store/).

## See also

- [Manifest reference](/pages/reference-manifest/)
- [Install, update and uninstall](/pages/reference-install-and-update/)
- [Registry reference](/pages/reference-registry/)
- [Versioning](/pages/versioning/)
- [Lifecycle](/pages/lifecycle/)
- [Services](/pages/capability-services/)
