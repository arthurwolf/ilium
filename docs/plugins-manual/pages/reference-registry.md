---
layout: page
title: Registry reference
permalink: /pages/reference-registry/
---

## Registry reference

A **registry** is a service that lists plugins and serves their packages. It is an optional source. A file install works without any registry, and a user who sets no registry never contacts one. This page specifies the registry protocol, the index format, namespaces, publishing, policy, mirrors and offline use. It is decision D27, which supersedes D3 for distribution only. The package format, the signature and the contract `ilium:plugin@0.1.0` do not change. See [Design decisions](/pages/design-decisions/).

A **mirror** is a copy of a registry that clients use instead of the original. A **bundle** is a directory or file that holds an index and packages for an offline install. A **yank** withdraws a version from new installs but leaves it working where it is installed. A **namespace** is the set of plugin ids that one publisher owns. A **client** is the Ilium host that talks to a registry.

Requirement ids such as `PKG-361` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The signature rules: [Signing and trust](/pages/reference-signing-and-trust/).
- Version and channel rules: [Versions and dependencies](/pages/reference-dependency-resolution/).
- The user view: [Finding and installing plugins](/pages/user-plugin-store/).

## Roles and design

There are four roles: the **registry operator**, who runs the service; the **publisher**, who uploads packages; the **client**, which reads the index and downloads packages; and the **mirror operator**.

The registry is a set of static files: signed JSON documents and package blobs. Any web server or file share can host them, so mirrors and air-gapped copies are plain file trees. Only the upload side needs a service.

The metadata follows the model of The Update Framework (TUF). Four properties matter: signed metadata, a freshness window, rollback protection and key thresholds. A client that trusts the registry root keys learns whether the index is current and untampered, even when a mirror served it.

| Id | Rule |
|---|---|
| [PKG-361] | A registry MUST be usable as a static file tree over HTTPS for reading. Reading MUST NOT need a server-side program. |
| [PKG-362] | A client MUST verify every metadata document against the registry root keys before it uses any value from it. |
| [PKG-363] | A client MUST verify the SHA-256 and the length of a downloaded package against the index before it parses the package, and then run all install stages of [Package format reference](/pages/reference-package-format/). The registry never replaces those checks. |

## File layout

All paths are relative to the registry base URL, for example `https://plugins.example.org/ilium/v1/`.

| Path | Content |
|---|---|
| `root.json` | The root keys, role keys and thresholds |
| `timestamp.json` | The hash and version of `snapshot.json`; short expiry |
| `snapshot.json` | The hashes and versions of `index.json`, `revocations.json` and every `plugins/<id>.json` |
| `index.json` | A summary of every plugin for search and listing |
| `plugins/<id>.json` | All versions of one plugin |
| `revocations.json` | Revoked keys and blocked ids |
| `blobs/<sha256>.iliumpkg` | One package, named by its hash |
| `categories.json` | The closed list of categories (optional; a built-in list applies when absent) |

## Metadata documents

Every metadata document is a JSON object `{ "signed": { ... }, "signatures": [ { "key_id", "signature" } ] }`. The signature is Ed25519 over the bytes `ilium-registry-metadata-v1`, `0x00`, then the canonical form of the `signed` value: the JSON Canonicalization Scheme of RFC 8785 (JCS). The client parses the document with refusal of duplicate keys, serialises `signed` again with JCS and verifies over those bytes. Whitespace in the file and the position of `signed` before or after `signatures` do not matter.

**Vector.** The `signed` value `{ "version": 17, "type": "revocations", "expires": "2026-10-19 12:00:00", "keys": [ { "publisher_id": "example", "key_id": "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c", "reason": "compromised", "effective": "2026-10-12 10:00:00" } ], "blocked_ids": [] }` has these JCS bytes (253 bytes, keys in code point order, no whitespace), whatever the order in the file:

```text
{"blocked_ids":[],"expires":"2026-10-19 12:00:00","keys":[{"effective":"2026-10-12 10:00:00","key_id":"56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c","publisher_id":"example","reason":"compromised"}],"type":"revocations","version":17}
```

Their SHA-256 is `2ad56689abaf8da83199f2beca98b7342dab7a5fd2269e5a0616acb514b94130`.

All times are `YYYY-MM-DD HH:mm:ss` in UTC.

### root.json

```json
{
  "signed": {
    "type": "root",
    "spec": "ilium-registry/1",
    "version": 3,
    "expires": "2027-10-12 00:00:00",
    "keys": { "<key_id>": { "algorithm": "ed25519", "public_key": "<base64url>" } },
    "roles": {
      "root": { "key_ids": ["<key_id>"], "threshold": 1 },
      "timestamp": { "key_ids": ["<key_id>"], "threshold": 1 },
      "snapshot": { "key_ids": ["<key_id>"], "threshold": 1 },
      "targets": { "key_ids": ["<key_id>"], "threshold": 1 }
    }
  },
  "signatures": []
}
```

### timestamp.json and snapshot.json

```json
{ "signed": { "type": "timestamp", "version": 4821, "expires": "2026-10-13 12:00:00",
    "snapshot": { "version": 4821, "sha256": "<hex>", "length": 18233 } }, "signatures": [] }
```

```json
{ "signed": { "type": "snapshot", "version": 4821, "expires": "2026-10-19 12:00:00",
    "files": { "index.json": { "version": 4821, "sha256": "<hex>", "length": 90211 },
               "revocations.json": { "version": 17, "sha256": "<hex>", "length": 412 },
               "plugins/com.example.notes.json": { "version": 9, "sha256": "<hex>", "length": 2104 } } },
  "signatures": [] }
```

### index.json

One entry per plugin, with only what search and listing need.

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Plugin id |
| `name` | string | Display name, at most 64 characters |
| `description` | string | At most 500 characters |
| `publisher_id`, `publisher` | string | Namespace owner and display name |
| `categories` | list | 1 to 3 values from the category list |
| `keywords` | list | At most 8 words of at most 24 characters |
| `latest_stable`, `latest_beta` | string or null | Highest non-yanked version of each channel |
| `license` | string | SPDX expression |
| `icon` | string | Tabler icon name |
| `updated` | string | Time of the newest publish |
| `deprecated` | object or null | `{ "message", "successor" }` |

### plugins/&lt;id&gt;.json

```json
{
  "signed": {
    "type": "plugin", "version": 9, "id": "com.example.notes", "publisher_id": "com.example",
    "versions": [
      {
        "version": "1.2.0", "channel": "stable", "api": "ilium:plugin@0.1.0",
        "url": "blobs/<sha256>.iliumpkg", "sha256": "<hex>", "length": 523044,
        "key_id": "<publisher key id>", "countersigned": true,
        "host": ">=0.1.0", "platforms": ["linux"],
        "categories": ["editors"], "license": "MIT", "published": "2026-10-12 10:00:00",
        "yanked": null, "deprecated": null
      }
    ]
  },
  "signatures": []
}
```

`yanked` is null or `{ "reason", "at" }`. `deprecated` is null or `{ "message", "successor" }`. `platforms` lists `linux`, `windows`, `macos`, matching the platform gate.

### revocations.json

The document has the wrapper of every metadata document and is signed with the `timestamp` role keys of `root.json`:

```json
{
  "signed": {
    "type": "revocations",
    "version": 17,
    "expires": "2026-10-19 12:00:00",
    "keys": [ { "publisher_id": "example", "key_id": "<64 hex>", "reason": "compromised", "effective": "2026-10-12 10:00:00" } ],
    "blocked_ids": [ { "id": "example.notes", "reason": "<text>", "at": "2026-10-12 10:00:00" } ]
  },
  "signatures": [ { "key_id": "<key id>", "signature": "<base64url>" } ]
}
```

A `revocations.json` without a valid signature of the threshold of those keys is refused with `registry.signature-invalid`.

### Rules for the documents

| Id | Rule |
|---|---|
| [PKG-364] | `root.json` MUST be signed by at least `threshold` of its own `roles.root` keys. A new root MUST also be signed by the threshold of the previous root's keys, and its `version` MUST be the previous version plus 1. |
| [PKG-365] | The client MUST stop and report `registry.metadata-expired` when a document's `expires` time is in the past, unless the client works in offline mode with a cached index (see below). |
| [PKG-366] | Version numbers are integers that never go down. A client MUST remember the highest `version` it has seen for each document and MUST refuse a lower one with `registry.rollback`. |
| [PKG-367] | `timestamp.json` MUST expire within 1 day of its issue. `snapshot.json` SHOULD expire within 7 days, `root.json` within 1 year. A registry operator renews the first two by re-signing without content change. |
| [PKG-368] | `index.json` and each `plugins/<id>.json` MUST match the hash, length and version in `snapshot.json`. A mismatch is `registry.hash-mismatch`. |
| [PKG-369] | The maximum sizes are: `root.json` 64 KiB, `timestamp.json` 16 KiB, `snapshot.json` 4 MiB, `index.json` 32 MiB, each `plugins/<id>.json` 1 MiB, `revocations.json` 4 MiB. A larger response is aborted with `registry.size`. |
| [PKG-370] | The sequence in which a client refreshes is: fetch `timestamp.json`; if its snapshot version is unchanged, stop; fetch `snapshot.json`; fetch each changed file listed in it. A client fetches `root.json` first only when the timestamp signature fails with the known keys. |
| [PKG-652] | The signature of a metadata document covers the JCS (RFC 8785) form of its `signed` value. A client MUST parse with refusal of duplicate keys, serialise `signed` again with JCS and verify over those bytes. The result for the vector of the metadata section MUST have the SHA-256 given there. |
| [PKG-653] | `revocations.json` MUST carry the `signed` and `signatures` wrapper and MUST be signed with the `timestamp` role keys. Without a valid signature of the threshold it is refused with `registry.signature-invalid`. |

## Transport

| Id | Rule |
|---|---|
| [PKG-371] | A client MUST use HTTPS. A plain `http:` base URL is refused with `registry.insecure-url`, except for `localhost` in a development configuration (`[plugins.registry] allow_insecure_localhost = true`). |
| [PKG-372] | The client MUST send `If-None-Match` with the stored `ETag` of a document and accept `304 Not Modified`. It MUST follow at most 3 redirects and only to HTTPS. |
| [PKG-373] | Timeouts: 10 s to connect, 30 s to the first byte of a metadata document, 120 s total for a metadata document, 600 s total for a package download. Retries: up to 3 attempts for connection errors and for 5xx responses, with waits of 1 s, 2 s and 4 s. A 4xx response is not retried. |
| [PKG-374] | The client reads `HTTPS_PROXY` and `NO_PROXY` and the `[plugins.registry] proxy` setting. The setting wins. |
| [PKG-375] | A download is written to a temporary file under the cache folder and renamed only after the hash and length match. A resumed download MUST use `Range` and MUST verify the final hash. |
| [PKG-376] | The only request data the client sends is the HTTP request itself: the path, the standard headers and `User-Agent: ilium/<version>`. It sends no identifier, no list of installed plugins and no install count. A registry operator who wants statistics reads their own server logs. |
| [PKG-377] | The configured registries are listed in `[plugins.registry] urls` in order (default: none). A registry marked `default` is searched first. A client with no URLs makes no network request. |

Offline behaviour: the client keeps the last verified set of metadata in the cache folder. When the network fails, it uses the cached metadata and shows its age. [PKG-378] A cached index MAY be used after its `expires` time only when the user passes `--offline` or sets `[plugins.registry] offline = true`; the client then shows a clear warning that the data is stale. Without that setting the expired metadata is refused with `registry.metadata-expired`.

## Search and categories

Search runs on `index.json`. A client that has downloaded it can search with no further network use.

| Id | Rule |
|---|---|
| [PKG-379] | A search query is a string of at most 200 characters. The client splits it on whitespace into terms, folds case and accents (Unicode NFKD, remove combining marks) and requires every term to match as a prefix of a word in `id`, `name`, `description`, `keywords` or `publisher`. |
| [PKG-380] | The relevance score of a match is the sum over terms of: 8 for a match in `id`, 6 in `name`, 4 in `keywords`, 2 in `publisher`, 1 in `description`. Ties break by `updated` (newer first), then by `id`. |
| [PKG-381] | Ranking uses no download counts and no user ratings in version 0.1. |
| [PKG-382] | Sort orders: `relevance` (default with a query), `name` (A to Z, default without a query), `updated` (newest first). |
| [PKG-383] | Pagination: the client shows pages of 20 results. `ilium plugin search --limit <1..100> --offset <n>` applies the same rule. |
| [PKG-384] | The `category` filter takes one value from the list below. A plugin lists 1 to 3 categories. A value outside the list is refused at publish with `registry.category-unknown`. |

| Category | Definition |
|---|---|
| `agents` | Detects, controls or reports on AI agents and their sessions |
| `appearance` | Themes of the interface other than ambient scenes, icons, status and tree chrome |
| `scenes` | Ambient background scenes |
| `editors` | Editing, viewing and language support |
| `git` | Source control and worktrees |
| `inference` | Model providers and cost tracking |
| `notifications` | Sounds, alerts and reports |
| `search` | Finding text, panes or files |
| `terminal` | Terminal behaviour, links and filters |
| `tools` | Utilities that fit no other category |
| `voice` | Speech input and output |

The same list is used by `ilium plugin search --category` and by the store view. A client may fall back to a built-in copy when `categories.json` is absent.

## Namespaces

A **namespace** is the set of plugin ids that start with a publisher id and a dot. The publisher id `com.example` owns every id that starts with `com.example.`.

| Id | Rule |
|---|---|
| [PKG-385] | A registry publisher id is one of: a reverse-DNS name of a domain that the publisher controls (`com.example`), or a registry account name written `user.<name>`, where `<name>` is 3 to 32 characters and matches the segment grammar of PKG-217 (`[a-z][a-z0-9]*(-[a-z0-9]+)*`). |
| [PKG-386] | A domain publisher id is claimed by proving control of the domain: the publisher adds a DNS TXT record `_ilium-publisher.<domain>` with the value `ilium-publisher=<publisher_id>;key=<key_id>`. The registry checks the record at claim time and again every 30 days. A domain whose record disappears loses new-publish rights after a 14 day grace period, but its published versions stay. |
| [PKG-387] | The reserved prefixes are `ilium.`, `core.`, `local.`, `test.` and `example.`. A publish with such an id is refused with `registry.id-reserved`. The prefix `user.` is reserved for account ids and cannot be claimed as a domain. |
| [PKG-388] | A plugin id MUST start with the publisher id of its signature plus a dot (rule [PKG-218]). A publish whose id belongs to another namespace is refused with `registry.namespace-unowned`. |
| [PKG-389] | **Confusable check.** At claim and publish the registry computes a skeleton of the id in this order: 1, NFKC normalisation, then lowercase; 2, each non-ASCII character is mapped with `confusables.txt` of Unicode 16.0.0; 3, the characters `-`, `_` and `.` are removed; 4, in one pass from left to right, `rn` becomes `m`, `vv` becomes `w`, `0` becomes `o`, `1` and `l` become `i`, and `5` becomes `s`. A new id or publisher id whose skeleton equals the skeleton of an existing different one is refused with `registry.confusable`, and a human reviews the exception. Vectors: `r-n.tool` and `m.tool` give `mtooi`; `vv.tool` and `w.tool` give `wtooi`; `Ilium.Tool` and `ilium_tool` give `iiiumtooi`; `examp1e.n0tes` and `example.notes` give `exampienotes`; `corn` gives `com`. |
| [PKG-390] | **Transfer.** The owner of a namespace can transfer it to a new publisher id by publishing a transfer statement signed by a trusted key of the current owner, naming the new publisher id and key. The registry records the transfer in the plugin metadata (`transferred_from`). Installed clients accept the new key because the registry countersignature chain covers it (see revocation) or because the user adds it with a rotation statement. |
| [PKG-391] | **Disputes.** A claim dispute is decided by the registry operator under the published policy. An id is never reassigned to a different publisher when any of its versions was downloaded in the last 365 days, except by a takedown and a clearly marked new publisher. |
| [PKG-655] | **Dormancy.** A namespace is **dormant** when no version has been published under it for 730 days. The registry MUST compute dormancy from publish dates in the metadata, never from a clock the publisher controls. A namespace with a published, unrevoked `maintained: true` statement is not dormant until that statement is older than 730 days. |
| [PKG-656] | **Reclaim.** The registry MUST NOT reassign a dormant namespace to another publisher unless all of these hold: the namespace is dormant as defined in PKG-655 (no publish for 730 days and no download of any version from the registry for 365 days, read from the registry's own server logs; [PKG-376] keeps clients anonymous, so this counts requests, not users); the registry sent a notice to the contact address in the last signed metadata and published it on the namespace page; 90 days passed after that notice with no reply or publish from the owner; and no version of the namespace was downloaded in those 365 days. Otherwise the namespace stays with its owner. A reclaim MUST be recorded in the plugin metadata as `reclaimed_from`, and the new owner MUST publish under a new key. Refusal code: `registry.namespace-not-dormant`. |
| [PKG-657] | **User notice.** When a plugin's namespace becomes dormant, the client MUST show the notice `dormant namespace` in the plugin detail page and in `ilium plugin list` output, with the date of the last publish. The notice MUST NOT block use, update or removal. A client MUST warn the user before an update that follows a reclaim, naming the old and the new publisher id. |

## Publishing

Publishing is the only part of the protocol that needs a server. The upload API is specified here so that one client works with any registry that follows it.

| Id | Rule |
|---|---|
| [PKG-392] | `ilium plugin publish <file> --registry <url> [--channel stable\|beta]` sends the package with `PUT <base>/publish/<id>/<version>` over HTTPS. The body is the package. The `Authorization` header holds a registry token from `ILIUM_REGISTRY_TOKEN` or the credential file of the registry. Tokens are never written to a log. |
| [PKG-393] | The server runs install stages 1 to 8 on the upload, including the signature check (stage 7) and key trust (stage 8), and a policy check. It refuses the publish with the same codes as the client would, plus the registry codes. |
| [PKG-394] | A version number is **immutable**. A publish of an existing `id@version` is refused with `registry.version-exists`, even when the content is identical. A fixed package needs a new version. |
| [PKG-395] | On success the server adds the version to `plugins/<id>.json`, stores `blobs/<sha256>.iliumpkg`, adds a registry countersignature to a copy of the signature document (a second entry of `signatures`, see [PKG-207]), recomputes `snapshot.json` and `timestamp.json`, and answers `201` with a JSON body `{ "id", "version", "sha256", "url" }`. |
| [PKG-396] | **Yank.** `ilium plugin yank <id>@<version> --reason <text>` marks a version as yanked. Clients do not choose it for a new install or update. Installed copies keep working. `unyank` reverses it. |
| [PKG-397] | **Deprecate.** `ilium plugin deprecate <id> --message <text> [--successor <id>]` marks the whole plugin. Clients show the message and offer the successor. Installs still work. |
| [PKG-398] | **Unlist.** A publisher can `unlist` a plugin: it disappears from `index.json` and from search, but `plugins/<id>.json` and the blobs remain so that installed copies update safely and lock files still resolve. |
| [PKG-399] | A publisher MUST NOT delete a published blob. Removal happens only through takedown (below). |
| [PKG-400] | Upload limits: 300 MiB per package (the package format cap), 20 publishes per hour per publisher id. A larger upload is refused with `registry.size`. |

## Policy

| Id | Rule |
|---|---|
| [PKG-401] | **Review levels.** Level A: automated checks only (all package checks, a scan for known-bad hashes, the confusable check). Level B: a human review is also required when the package requests any high-risk grant (`process.run`, `fs.write` outside the plugin scope, `http` with `*`, `agent.control`, `commands.invoke:*`) or `replaces`, or when it is the first publish of a publisher id. A registry MUST state which level applies to which version on the plugin page. |
| [PKG-680] | **What level B requires.** A version is at level B only when all of these hold: (a) a review record exists for that version with the reviewer id, the review time and the SHA-256 of the package; (b) the package contains `sbom.json` of at most 4 MiB (PKG-502); (c) the manifest `license` is a valid SPDX expression (PKG-019); (d) the package contains a `LICENSE` or `NOTICE` file. A version that lacks any of (a) to (d) is at level A, whatever else the operator claims. |
| [PKG-681] | **Publish refusal.** When a registry requires level B for the package (PKG-401), a publish without (b), (c) or (d) of PKG-680 is refused before review with `registry.policy` and the detail `sbom-missing`, `license-invalid` or `license-notice-missing`. A publish that meets (b) to (d) is queued for review and is shown at level A until (a) holds. |
| [PKG-682] | **Level A stays open.** At level A the SBOM and the licence notice are optional. A version published without them is accepted and the plugin page states `No SBOM` or `No licence notice`. It never carries the level B badge. |
| [PKG-683] | **Badge is derived, per version.** The level B badge is computed from the review record of that exact version and package hash. A new version, a re-upload with another hash or a withdrawn review record returns the version to level A. A level is never inherited from an earlier version. |
| [PKG-684] | **Level in the index.** Every version entry of `index.json` carries `review_level` (`"A"` or `"B"`), `sbom` (boolean) and `license_notice` (boolean). A client MUST show the level B badge only when `review_level` is `"B"` and `sbom` and `license_notice` are both `true`; with a missing field it shows level A. |
| [PKG-402] | **Content policy.** A registry refuses plugins that: contain malware or hidden data collection, impersonate another publisher or product, hide behaviour that the manifest does not declare, or break the law of the registry's jurisdiction. The operator publishes this policy at `policy.json` or on its site. |
| [PKG-403] | **Takedown.** An operator removes a version or a plugin by adding it to `revocations.json` (`blocked_ids`) and removing the blob. The reason and time are recorded. At the next metadata refresh the client moves the installed copies that match to the package state `disabled-blocked` ([plugin lifecycle states](/pages/reference-plugin-lifecycle-states/)), with the reason `registry.takedown` and the registry's reason text, and shows both. `ilium plugin enable <id> --accept-risk` enables the plugin again. |
| [PKG-404] | **Appeal.** A publisher can appeal a takedown once, in writing to the contact in `policy.json`. The operator answers within 14 days. A reinstated plugin gets a new version number. |
| [PKG-405] | **Emergency revocation** of a publisher key follows [PKG-242] to [PKG-247]. The registry adds the key to `revocations.json`. |
| [PKG-406] | **Ratings and reviews.** User ratings and reviews are **out of scope for 0.1**. A registry compliant with this page MUST NOT show a rating, and a client MUST NOT send one. A later version of this page may add them. The policy for any future rating: only users of a version may rate it, a rating needs moderation, and paid ratings are forbidden. |

## Revocation

Revocations travel in `revocations.json`. [PKG-407] A client MUST fetch it on every metadata refresh, and MUST apply `keys` to its trust store (status `revoked`, `added_by: registry` is not used for revocations) and `blocked_ids` to its installed plugins. The effects are those of [PKG-244] to [PKG-247] for keys and [PKG-403] for ids.

## Mirrors

| Id | Rule |
|---|---|
| [PKG-408] | A **full mirror** serves a byte-identical copy of the file tree. Because every document is signed and the blobs are named by hash, the signature chain survives mirroring unchanged. |
| [PKG-409] | A **partial mirror** serves a subset: a list of plugin ids with their dependencies. It MUST include the metadata files, `index.json` filtered to the subset and a `snapshot.json` that lists only the mirrored files. A partial mirror re-signs `index.json` and `snapshot.json` with its own keys and has its own `root.json`. Clients treat it as a separate registry with its own root, set in `[plugins.registry]`. |
| [PKG-410] | `ilium plugin mirror sync <dir> --registry <url> [--allow <id>...] [--channel stable\|beta]` copies the metadata and the blobs to `<dir>` and verifies every hash. For a full copy, omit `--allow`. The command is idempotent: it downloads only changed files. |
| [PKG-411] | A client configured with a mirror URL uses it exactly like a registry. The root keys are the ones in the client's configuration (`[plugins.registry] root`), never fetched from the mirror alone, except at the first setup where `ilium plugin registry add <url> --root-key <key_id>` pins them. |

## Air-gapped installs

A machine with no network can install from a **bundle**: a directory with the same layout as a registry, or a `.tar` file of it.

| Id | Rule |
|---|---|
| [PKG-412] | `ilium plugin export-bundle <dir> <id>[@range]... [--with-deps]` writes a bundle with the signed metadata of the registry, filtered to the listed plugins, and their blobs. The bundle keeps the original signatures. |
| [PKG-413] | `ilium plugin install --from-bundle <dir> <id>` installs from the bundle. The client verifies the metadata with the root keys pinned in its configuration, enforces expiry (unless `--offline`) and rollback rules against its stored versions, and runs the normal install stages. |
| [PKG-414] | A bundle's `snapshot.json` expires like any other. For a long-lived air-gapped setup the operator re-exports a bundle regularly or sets `offline = true`. |

## Client error codes

| Code | Meaning |
|---|---|
| `registry.unreachable` | The network failed after retries and no cache applies |
| `registry.insecure-url` | A non-HTTPS URL was configured |
| `registry.metadata-expired` | A metadata document is past its `expires` time |
| `registry.rollback` | A document has a lower version than one seen before |
| `registry.signature-invalid` | A metadata signature or threshold failed |
| `registry.hash-mismatch` | A document or package differs from the hash in the metadata |
| `registry.size` | A document, package or upload exceeds its cap |
| `registry.not-found` | The plugin id or the version is not in the index |
| `registry.yanked` | The version is yanked and was requested by exact version without `--allow-yanked` |
| `registry.version-invalid` | A version has build metadata or is malformed |
| `registry.version-exists` | A publish of an existing version |
| `registry.id-reserved` | A publish with a reserved prefix |
| `registry.namespace-unowned` | The id is outside the publisher's namespace |
| `registry.confusable` | The id looks like an existing one |
| `registry.category-unknown` | A category is not in the list |
| `registry.auth` | The token is missing or refused |
| `registry.policy` | The package fails the registry policy |

Message templates are in [Manifest schema and error catalogue](/pages/reference-manifest-schema/).

## Client behaviour on each failure

| Id | Failure | Required behaviour |
|---|---|---|
| [PKG-415] | Network unreachable | Use the cache if valid. Otherwise report `registry.unreachable`, exit 8. A file install is unaffected |
| [PKG-416] | Metadata expired | Refuse to install from it. Offer `--offline` |
| [PKG-417] | Rollback | Refuse, keep the old cache, report the two versions |
| [PKG-418] | Hash mismatch on a package | Delete the partial download, refuse, never retry the same URL more than the transport retries |
| [PKG-419] | Signature threshold not met | Refuse. Do not use the document at all |
| [PKG-420] | A version the user asked for is yanked | Refuse unless `--allow-yanked`. Dependency resolution skips yanked versions silently |
| [PKG-421] | Revoked key in a freshly fetched package | Refuse with `trust.key-revoked` |
| [PKG-422] | A different registry returns the same id | The first registry in the configured order wins. `--registry <url>` selects one. A plugin installed from one registry updates only from it |

## Worked example

An administrator sets up a mirror for a team:

```text
ilium plugin registry add https://plugins.example.org/ilium/v1/ --root-key 9c0b...e41d
ilium plugin mirror sync /srv/ilium-mirror --registry https://plugins.example.org/ilium/v1/ \
    --allow com.example.notes --allow com.example.git
```

They serve `/srv/ilium-mirror` over HTTPS at `https://mirror.team.example/ilium/`. On each machine:

```text
ilium plugin registry add https://mirror.team.example/ilium/ --root-key 9c0b...e41d
ilium plugin install com.example.notes
```

The client checks the metadata with the pinned root key, so a compromised mirror cannot serve a changed package. At worst it can serve old metadata, and the freshness and rollback rules limit that.

## Edge cases

| Situation | Result |
|---|---|
| The index lists a version whose blob is missing | `registry.not-found` for that version. Other versions work |
| The clock of the client is far in the future | Metadata looks expired. The message shows both times. Fix the clock or use `--offline` |
| Two versions with the same number but different hashes appear in two mirrors | `registry.hash-mismatch`. The version number is immutable |
| The registry root key changes | A new `root.json` signed by the old threshold is followed automatically. Otherwise the client stops and the user re-pins |

## What the user sees

The user sees the registry only through search results, plugin detail pages and update notices. Errors appear as a short line with the code and a hint. See [Finding and installing plugins](/pages/user-plugin-store/).

## See also

- [Signing and trust](/pages/reference-signing-and-trust/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
- [Install, update and uninstall](/pages/reference-install-and-update/)
- [Plugin command line](/pages/reference-plugin-cli/)
- [Administrator guide](/pages/guide-plugin-administration/)
- [Publishing guide](/pages/guide-publishing/)
