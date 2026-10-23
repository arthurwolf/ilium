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
| `quarantine.json` | The signed emergency-disable feed |
| `log/head.json`, `log/entries/<n>.json`, `log/proofs/<sha256>.json`, `log/consistency/<m>-<n>.json` | The transparency log (optional; see its section) |

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
| [PKG-974] | The `signed` object of every metadata document MUST carry `spec`, the registry protocol version it follows, with the same value that `root.json` carries (proposed, owner to confirm: the examples of this page are updated to show it). The integer `version` stays a rollback counter under [PKG-366] and is not a format version. |
| [PKG-975] | A client MUST check `spec` before it reads any other field of a document. A `spec` it does not support is refused with `registry.format-unsupported`. The client keeps its cached metadata as [PKG-417] requires and reports the document's `spec` and the values it supports. |
| [PKG-976] | After a successor `spec` is first published, the registry MUST keep serving the previous `spec` for at least 12 months (proposed, owner to confirm). During that window the previous `spec` keeps the signature rules that applied to it when the successor was published. |

## Transport

| Id | Rule |
|---|---|
| [PKG-371] | A client MUST use HTTPS. A plain `http:` base URL is refused with `registry.insecure-url`, except for `localhost` in a development configuration (`[plugins.registry] allow_insecure_localhost = true`). |
| [PKG-372] | The client MUST send `If-None-Match` with the stored `ETag` of a document and accept `304 Not Modified`. It MUST follow at most 3 redirects and only to HTTPS. |
| [PKG-373] | Timeouts: 10 s to connect, 30 s to the first byte of a metadata document, 120 s total for a metadata document, 600 s total for a package download. Retries: up to 3 attempts for connection errors and for 5xx responses, with waits of 1 s, 2 s and 4 s. A 4xx response is not retried. |
| [PKG-374] | The client reads `HTTPS_PROXY` and `NO_PROXY` and the `[plugins.registry] proxy` setting. The setting wins. |
| [PKG-375] | A download is written to a temporary file under the cache folder and renamed only after the hash and length match. A resumed download MUST use `Range` and MUST verify the final hash. |
| [PKG-376] | The only request data the client sends is the HTTP request itself: the path, the standard headers and `User-Agent: ilium/<version>`. It sends no identifier, no list of installed plugins and no install count. A registry operator who wants statistics reads their own server logs. |
| [PKG-377] | The configured registries are listed in `[plugins.registry] urls` in order (default: none). A registry marked `default` is used when a request names no registry; when two registries return the same id, the order in `urls` decides ([PKG-422]). A client with no URLs makes no network request. |
| [PKG-972] | A client MUST treat an HTTP `429` response as retryable. This is an exception to [PKG-373] for that status code. It waits for the `Retry-After` value, given in seconds or as an HTTP date, and MUST NOT send the request again sooner. A `Retry-After` longer than 300 s is not waited for: the client fails with `registry.rate-limited` (proposed, owner to confirm: 300 s). Without a `Retry-After` header the client uses the waits of PKG-373 and counts the response against the same 3 attempts. |
| [PKG-973] | The registry MUST answer a request over its per-client limit on metadata and download requests with `429`, the code `registry.rate-limited` and a `Retry-After` header, as [PKG-704] requires. The limit is 600 metadata and download requests per hour per client address (proposed, owner to confirm). A mirror sync ([PKG-410]) that receives `registry.rate-limited` MUST stop, keep the files it has already verified and report the time it may resume. Cached and file installs are not limited. |

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

### Publish tokens

The token of [PKG-392] is a secret that authorises publishes. Its issue, expiry, rotation and revocation follow the rules below. Leak response for a publisher key is in [Registry key compromise](/pages/reference-registry-key-compromise/).

| Id | Rule |
|---|---|
| [PKG-968] | A registry token MUST be issued to one publisher id and MUST authorise publishes only under the namespace of that id ([PKG-388]). A token expires 90 days after it is issued (proposed, owner to confirm). A publish with an expired token is refused with `registry.auth`. |
| [PKG-969] | A publisher MUST be able to issue a new token and revoke an old one without a gap in publishing. The new token works as soon as it is issued, and the old one keeps working until it is revoked or expires. A CI token MUST be rotated at least every 90 days (proposed, owner to confirm). |
| [PKG-970] | A revocation MUST take effect within 1 hour of the publisher's request or of the operator's report of a leak (proposed, owner to confirm). From that moment a publish with the revoked token is refused with `registry.auth`. The revocation is appended to the transparency log ([PKG-802]), and the token value MUST NOT appear in that log or in any registry response. |
| [PKG-971] | Every publish made with a token between the leak and its revocation MUST be listed in the transparency log and sent to every maintainer of the publisher id ([SEC-723]). The registry MUST NOT remove or yank such a publish by itself. The publisher or the operator decides under [PKG-396] and [PKG-403]. |

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
| [PKG-986] | Registry and mirror requests MUST verify TLS against the system trust roots plus, when the policy sets `[network] ca_bundle`, the bundle it names, as [SEC-077] requires. No `[plugins.registry]` setting may disable verification or add a trust root. |
| [PKG-987] | A mirror with a certificate from an internal CA works when the administrator sets `[network] ca_bundle` ([Administrator policy](/pages/reference-admin-policy/)). A certificate failure MUST NOT be handled by skipping verification or by falling back to `http:` ([PKG-371]). The client reports it as `network` with reason `tls`, as [SEC-077] requires. |
| [PKG-988] | A change of a mirror's TLS certificate under the same CA needs no client change. A change of CA needs the administrator to update `ca_bundle`. TLS never decides metadata trust: every document is still verified with the root keys of [PKG-411], so a certificate change on a mirror cannot replace them. |

## Air-gapped installs

A machine with no network can install from a **bundle**: a directory with the same layout as a registry, or a `.tar` file of it.

| Id | Rule |
|---|---|
| [PKG-412] | `ilium plugin export-bundle <dir> <id>[@range]... [--with-deps]` writes a bundle with the signed metadata of the registry, filtered to the listed plugins, and their blobs. The bundle keeps the original signatures. |
| [PKG-413] | `ilium plugin install --from-bundle <dir> <id>` installs from the bundle. The client verifies the metadata with the root keys pinned in its configuration, enforces expiry (unless `--offline`) and rollback rules against its stored versions, and runs the normal install stages. |
| [PKG-414] | A bundle's `snapshot.json` expires like any other. For a long-lived air-gapped setup the operator re-exports a bundle regularly or sets `offline = true`. |
| [PKG-977] | A bundle given as a `.tar` file MUST have its outer listing checked before any entry is extracted. The client refuses the bundle with `registry.bundle-invalid` when an entry has an absolute path or a `..` component, is a symbolic link, a hard link, a device, a FIFO or a socket, or lies outside the layout of [PKG-412]. The package entry rules of [PKG-107] and [PKG-172] apply to each package inside the bundle. |
| [PKG-978] | A bundle `.tar` MUST NOT expand beyond 2 GiB or contain more than 10,000 entries (proposed, owner to confirm). A bundle over either limit is refused with `registry.size` before any file is written. |
| [PKG-979] | The client MUST extract a `.tar` bundle into a new temporary folder under the cache folder, verify its metadata with the pinned root keys ([PKG-413]) and move only verified packages into the install path. Nothing is written under the data directory before the install stages of [PKG-413] begin. A refused bundle leaves no extracted file behind. |

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
| `registry.rate-limited` | A request is over the registry's limit, or the `Retry-After` wait is longer than the client accepts ([PKG-972], [PKG-973]) |
| `registry.format-unsupported` | A document or bundle uses a `spec` the client does not support ([PKG-975]) |
| `registry.bundle-invalid` | A `.tar` bundle has an entry that its outer listing rules refuse ([PKG-977]) |

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

## Namespace ownership and name squatting

This section is target behaviour for the public plugin system. A namespace is the part of a plugin id before the first dot. This section sets the rules that stop a publisher from taking a name that belongs to another party or that looks like it.

- [PKG-702] The registry MUST bind each namespace to one verified publisher at its first publication. It MUST refuse a publication by any other publisher with `registry.namespace-unowned`, MUST reserve the prefixes `ilium.` and `core.` for the project, and MUST refuse a new id that is confusable with an existing id after Unicode normalisation, case folding and removal of separators, with `registry.name-confusable`.
- [PKG-703] A namespace with no published release for 24 months and no answer from its owner to two notices sent 30 days apart MUST be marked abandoned in the signed metadata. Only an abandoned namespace MAY be reclaimed, and a reclaimed id MUST NOT reuse the version numbers or the keys of the earlier owner, and MUST be announced in the transparency log.

## Security metadata refresh independent of auto_update

The setting `[plugins] auto_update` ([PKG-300]) controls only whether the host looks for and offers new package versions. Key revocations ([PKG-407], [PKG-247]), takedowns ([PKG-403]) and quarantines ([EXT-057]) reach a host only through metadata, so the host fetches that metadata on its own schedule.

| Item | Behaviour |
|---|---|
| Who refreshes | A host with at least one URL in `[plugins.registry] urls` ([PKG-377]). A host with no configured registry makes no request. |
| What is fetched | `timestamp.json`; when its snapshot version changed, `snapshot.json`; then only `revocations.json`, the quarantine feed (below) and the transparency log head, when `snapshot.json` lists a newer version of them. The order is that of [PKG-370]. |
| Cadence | Every 6 hours with a random delay of up to 15 minutes, and once at server start, whatever `auto_update` is set to (`off`, `notify` or `install`). The cadence is fixed and `auto_update_interval_hours` ([PKG-301]) does not change it. |
| What it never does | It never reads `index.json` or `plugins/<id>.json` for new versions, never shows an update notice, and never downloads a package. Update checks stay under `auto_update`. |
| Switching it off | Only `[plugins] security_refresh = off` (default `on`) stops it. The host then makes no request without a user command, and shows `Security refresh is off` in `ilium plugin status` and on the Plugins screen. |
| Policy | `security_refresh` is a policy item like `auto_update` ([PKG-305]). An administrator can set it and lock it ([Administrator policy](/pages/reference-admin-policy/)). A locked value shows `Set by your administrator` and a user edit is refused with `policy.locked`. |
| Failure | A failed refresh (network, `registry.signature-invalid`, `registry.rollback`) keeps the cached documents and keeps enforcing them. The next attempt follows the retry waits of [PKG-373], then the next 6-hour slot. |
| Stale data | After 7 days without a successful refresh the revocation data is stale ([EXT-072]). The host keeps enforcing it and shows `Revocation data is stale` with the date of the last success, in the Plugins screen header and in `ilium plugin status`. |
| `--offline` | With `--offline` or `[plugins.registry] offline = true` ([PKG-378]) no refresh runs. The stale rule still counts the days since the last success, so an offline host shows the warning after 7 days. |

## Transparency log documents and inclusion proofs

The log of [PKG-700] is part of the file tree. It is an append-only Merkle tree (RFC 9162 hashing, SHA-256) over the entries below.

| Path | Content |
|---|---|
| `log/head.json` | The signed head. Listed in `snapshot.json` like any metadata document |
| `log/entries/<n>.json` | Entry number `n` (from 0), unsigned; its leaf hash is `SHA-256(0x00 \|\| JCS(entry))` |
| `log/proofs/<sha256>.json` | The inclusion proof of the package whose digest is `<sha256>`; one file per package |
| `log/consistency/<m>-<n>.json` | The consistency proof between tree sizes `m` and `n`; produced on request by a static copy for the sizes the registry publishes (every size that was a head) |

**Entry.** A JSON object with the fields `index` (integer), `type` (`publish`, `yank`, `unyank`, `takedown`, `key-rotation`, `owner-change`, `token-revocation`, `recovery`), `plugin_id` (null for `token-revocation` and `recovery` of an account), `version` (null when not per version), `sha256` (package digest, null when none), `key_id`, `time` (UTC) and `previous` (the SHA-256 of the JCS form of entry `index - 1`; 64 zero hex digits for entry 0). A token value never appears ([PKG-970]).

**Head.** `{ "signed": { "type": "log-head", "spec", "version", "expires", "tree_size", "root_hash" }, "signatures": [...] }`, signed with the `timestamp` role keys by the threshold of `root.json`, with the wrapper of every metadata document. `version` never goes down ([PKG-366]).

**Inclusion proof.** `{ "tree_size", "leaf_index", "leaf_hash", "path": ["<hex>", ...] }`. The client rebuilds the root from the leaf hash and the path and compares it with `root_hash` of the head it holds.

| Rule | Basis |
|---|---|
| Before each install or update the client fetches `log/head.json` and the proof of the package, verifies the head signature and the proof, and refuses on failure with `registry.signature-invalid` ([PKG-701]). | PKG-701 |
| The client stores the last head it accepted. For a new head with a larger `tree_size` it fetches the consistency proof and verifies it; a head that is not an extension, or a smaller `tree_size`, is refused with `registry.rollback` and the stored head stays. | PKG-701 |
| Size caps: `head.json` 16 KiB, one entry 8 KiB, one proof 4 KiB (at most 40 path hashes), one consistency proof 8 KiB. A larger response is aborted with `registry.size`. | PKG-369 |
| Expiry: a head expires within 7 days of issue and is renewed by re-signing without content change, as `snapshot.json` is ([PKG-367]). An expired head is `registry.metadata-expired`. | PKG-367 |
| A mirror copies `log/` byte for byte ([PKG-408]); a partial mirror ([PKG-409]) copies the head, the entries and proofs of its blobs, and the consistency proofs. A bundle ([PKG-412]) carries the head and the proof of each blob; the client checks them with the rules above, and `--offline` skips only the head expiry check. | PKG-408, PKG-412 |
| Absence: a registry whose `root.json` has no `log` key (`"log": { "required": true }` declares one) has no log. The client then skips the check, shows `This registry has no transparency log` in the plugin detail view, and does not treat that as an error. When the root declares a log and the head or a proof is missing, the install is refused with `registry.not-found`. | PKG-700 |
| The client keeps the log head per registry, so a registry that later drops its declared log is detected only by a new root that the client accepts under [PKG-364]. | PKG-364 |

## The emergency-disable feed document

The feed that [EXT-057] polls is the file `quarantine.json`. It is listed in `snapshot.json` with `version`, `sha256` and `length`, has the wrapper of every metadata document and is signed with the `timestamp` role keys ([PKG-653] applies by analogy). A feed without a valid signature of the threshold is refused with `registry.signature-invalid` and ignored ([EXT-057]).

```json
{
  "signed": {
    "type": "quarantine", "spec": "ilium-registry/1", "version": 6, "expires": "2026-10-19 12:00:00",
    "entries": [
      { "id": "com.example.notes", "versions": ">=1.2.0 <1.2.4", "reason": "Reads files outside its scope",
        "severity": "high", "issued": "2026-10-12 10:00:00", "withdrawn": false }
    ]
  },
  "signatures": []
}
```

| Field | Meaning |
|---|---|
| `id` | Plugin id |
| `versions` | A version range in the syntax of [Versions and dependencies](/pages/reference-dependency-resolution/) |
| `reason` | 1 to 500 characters of plain text |
| `severity` | `low`, `medium`, `high` or `critical` |
| `issued` | UTC time of the entry |
| `withdrawn` | `true` lifts the quarantine of that range ([EXT-059]) |

Limits: 1 MiB, at most 5,000 entries, expiry within 7 days (renewed by re-signing). A later entry for the same `id` and `versions` replaces the earlier one.

| | Quarantine (`quarantine.json`) | Takedown (`revocations.json` `blocked_ids`) |
|---|---|---|
| Scope | A version range | An id or version |
| Reversible | Yes, by a newer entry with `withdrawn: true` or by an upgrade to a version outside the range | No; the blob is removed ([PKG-403]) |
| Resulting state | `quarantined` | `disabled-blocked` |

The package state `quarantined` is added to [Plugin lifecycle states](/pages/reference-plugin-lifecycle-states/): running instances are stopped within 60 s of the poll, the package is not instantiated, its data is kept ([EXT-058]), the plugin is listed as `quarantined` with `reason` and `severity`, and `ilium plugin enable` is refused. Only a withdrawal or an upgrade lifts it ([EXT-059]). The feed is fetched in the refresh of "Security metadata refresh independent of auto_update".

## Release notes in the signed version entry

Each entry of `versions` in `plugins/<id>.json` has these extra fields:

| Field | Type | Rule |
|---|---|---|
| `notes` | string | Plain text, at most 2,000 characters, no control characters except line feed |
| `notes_url` | string or absent | An HTTPS URL to the full changelog |
| `security` | boolean | `true` marks a security fix |

`ilium plugin publish --notes <file>` reads `notes` from the file. Without the option the publisher takes the top section of the package's `CHANGELOG.md` (the file is a package file, not a manifest key, see [AUT-321]), cut at 2,000 characters. A publish with `notes` above 2,000 characters, with a control character, or with a `notes_url` that is not HTTPS is refused with `registry.policy` and the detail `notes-invalid`.

The client takes the text from the signed entry, so the update notice of [PKG-304] and the store detail view ([PKG-535]) show it before any download. An absent field shows `No release notes`. `security: true` adds `Security fix` to the notice. The link in the notice is `notes_url` when present. The text is rendered as plain text and never as markup.

## Notice to users running a yanked version

At each metadata refresh the client compares the installed `active_version` of every plugin with the yank records of `plugins/<id>.json` ([PKG-396], [PKG-801]).

| Rule | Behaviour |
|---|---|
| Notice | When the active version is yanked, the client shows a notice with the plugin name, the version, the yank `reason` and the newest non-yanked version. Actions: Update, Later, Keep this version. |
| Frequency | At most once per version per 7 days; Later hides it for that period as in [PKG-304]; Keep this version hides it until the version changes. |
| Listing | `ilium plugin list` and `ilium plugin status` show `yanked` on that version, with the reason in `--json`. |
| Auto-update | With `auto_update = install` ([PKG-300]) a yank whose reason starts with `security:` is applied at the next safe point ([PKG-358] pins are overridden), unless an administrator pins the version in policy ([SEC-723] covers who may yank). A yank for any other reason is only offered. |
| Locks | A lock file naming the yanked version still installs by sha256 with the warning of [PKG-803]. |
| Offline | The notice appears at the first refresh after the host is online. |

## Publisher account recovery and loss of the second factor

These duties belong to the registry operator, under "Publish tokens".

| Rule | Behaviour |
|---|---|
| Recovery codes | When a second factor is enrolled ([SEC-723]) the registry shows 10 single-use recovery codes once. Using one allows a new factor to be enrolled. |
| Route for `user.<name>` | Without a code, another maintainer of the publisher id confirms the recovery with their own second factor. |
| Route for a domain id | The publisher proves control with a fresh DNS TXT record ([PKG-386]). |
| Hold | A recovered account cannot publish for 7 days. Every maintainer gets a notice on request, at the end of the hold and when it ends. Any maintainer can cancel the hold. |
| Keys | A recovery never issues or changes a signing key. A key change stays under [PKG-238]. Tokens issued before the recovery are revoked ([PKG-970]). |
| Sole maintainer unreachable | The namespace is not changed. If no version is published for 730 days the dormancy rule of [PKG-655] starts. |
| Log | Each recovery, hold and cancel is a `recovery` entry in the transparency log ([PKG-819]). |

## Retention of published versions and operator backups

| Version kind | Retention |
|---|---|
| `stable` | Kept as long as any installed copy may update from it. Removed only by takedown ([PKG-399], [PKG-403]). |
| `beta` and prerelease | Kept for at least 180 days after a later stable version is published. After that the operator may prune it with 30 days' notice on the plugin page. |
| Pruned version | Its blob is removed and `plugins/<id>.json` keeps a `pruned` tombstone `{ "version", "sha256", "time" }` ([PKG-821] keeps the same form for takedowns). A lock file naming it fails with `registry.not-found`; the message names the tombstone and the `pruned` time. |

Operator duties: back up the whole file tree and the signing keys (root keys offline, [PKG-690]); restore into an empty location at least every 90 days and check that `snapshot.json` hashes match. If a restore loses entries, the operator updates the status page within 1 hour ([AUT-952]) and re-publishes from the transparency log. Retirement of a registry URL follows [AUT-951]. For the procedure see [Administrator failure runbook](/pages/guide-admin-failure-runbook/).

## Paid and licensed plugins

| Rule | Behaviour |
|---|---|
| Protocol | The registry carries no payment, licence or entitlement data and never gates a download on payment. |
| Declaration | A plugin that checks a licence key or subscription says so in its `description`, declares the `http` grant to the exact licensing host, works in a reduced mode without the key, and sets `license` to an SPDX expression of the proprietary terms ([PKG-019]; `LicenseRef-` expressions are valid). |
| Refusal | A plugin that hides a paywall or disables itself without that declaration is refused under [PKG-402] with `registry.policy` and the detail `undeclared-licensing`. |
| Index | The version entry has a boolean `licensing`. When `true` the store detail view shows `Requires a licence key` next to the grants ([PKG-535]). An absent field means `false`. |
| Ratings | Ratings stay out of scope ([PKG-406]). |

## Publisher security advisories for a plugin version

A publisher marks their own versions as vulnerable with a signed `advisories` array in `plugins/<id>.json`. Each entry has the OSV fields of [SEC-700]: `id`, `aliases`, `affected` (ranges), `severity`, `fixed`, `published`, `summary`.

| Rule | Behaviour |
|---|---|
| Publishing | `ilium plugin advisory publish <id> --file <osv.json>` is signed by a trusted key of the owner. A signer who is not an owner is refused with `registry.not-owner` ([PKG-800]). The registry appends an entry to the transparency log. |
| Plugins screen | A plugin with an installed affected version shows `Security update available` with the severity, in `ilium plugin list` too. |
| Update | The client prefers the `fixed` version. |
| Install | A fresh install of an affected version is refused unless `--accept-risk` is passed. |
| Takedown and project advisories | An advisory does not stop the plugin; a takedown ([PKG-403]) does. Project advisories ([SEC-703]) may lead to a quarantine ([EXT-057]) when the publisher does not act. |

## Operator security hardening

| Area | Rule |
|---|---|
| Tokens | Stored only as salted argon2id hashes and shown once at issue ([PKG-968]). |
| Keys | Online role keys in an HSM or cloud KMS with a signing audit log; root keys offline ([PKG-690], [PKG-691]). |
| Audit | Every publish, yank, owner change and token event goes to an operator audit log, mirrored publicly by the transparency log ([PKG-970]). |
| Alerts | First publish from a new address, a publish outside the publisher's usual hours, and a burst of versions (above the quota of [PKG-704]) alert the operator. |
| Abuse | The intake of [PKG-705] acknowledges within 1 business day. |
| Logs | Access logs keep addresses at most as long as [USR-956]. |
| Recovery | Backup and disaster recovery: [Administrator failure runbook](/pages/guide-admin-failure-runbook/). |

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
- [Registry key compromise](/pages/reference-registry-key-compromise/)
- [Versions and dependencies](/pages/reference-dependency-resolution/)
- [Install, update and uninstall](/pages/reference-install-and-update/)
- [Plugin command line](/pages/reference-plugin-cli/)
- [Administrator guide](/pages/guide-plugin-administration/)
- [Publishing guide](/pages/guide-publishing/)
- [Release rollout](/pages/reference-release-rollout/)

## Production additions: T2-security

- **[SEC-723]** Publishing to a registry account MUST require a second factor (TOTP or WebAuthn), MUST support at least two maintainers per publisher id, and MUST send a notice to every maintainer on publish, key change, transfer and yank.

## Production additions: T4-package-registry

- **[PKG-800]** A yank or unyank request MUST be signed by a trusted key of the plugin's current publisher; any other signer MUST be refused with `registry.not-owner` and the metadata MUST stay unchanged.
- **[PKG-801]** A yank MUST record `yanked_at` (UTC, second precision), `yanked_by` (key fingerprint) and `reason` (1 to 500 (proposed, owner to confirm) Unicode scalar values) in plugins/<id>.json. `ilium plugin yank` without `--reason` MUST fail with `usage.missing-argument`. The reason MUST be shown by `info` and by the install refusal of PKG-420.
- **[PKG-802]** unyank MUST clear the yanked fields of that version, MUST append an entry to the transparency log, and MUST fail with `registry.not-yanked` when the version is not yanked. A version removed by takedown MUST NOT be unyankable (`registry.taken-down`).
- **[PKG-809]** A successful publish MUST answer 201 with JSON `{id, version, sha256, length, url, channel, published_at}` and a registry timestamp, and the CLI MUST print the same fields as a `result` record. The release MUST appear in the signed index within 60 s (proposed, owner to confirm).
- **[PKG-810]** The registry MUST refuse a publish body over 300 MiB with 413 `registry.size` before reading it fully, and MUST abort an upload that makes no progress for 60 s with `registry.upload-timeout`.
- **[PKG-811]** The registry MUST run publish checks in this order and report the first failure: size, package format stages, manifest schema, signature, namespace ownership, confusable check, version-exists, dependency existence. A failed publish MUST leave no blob and no metadata change.
- **[PKG-812]** A publish whose `requires.plugins` names an id that does not exist in the same registry or in a configured upstream MUST be refused with `registry.dependency-unknown`, unless the entry is optional.
- **[PKG-813]** When one id is served by two configured registries, the client MUST install from the registry that first served an installed or locked copy of that id and MUST refuse a different source with `registry.source-changed` unless `--allow-source-change` is passed.
- **[PKG-814]** `[plugins.registry]` MUST support a per-namespace route (prefix to registry url); an id matching a route MUST be requested only from that registry and a miss MUST be `plugin.not-found`, never a fall-through.
- **[PKG-816]** A mirror that serves a `snapshot.json` or `timestamp.json` whose version number is lower than the highest the client has stored MUST be refused with `registry.rollback` and the stored version MUST NOT change.
- **[PKG-817]** `mirror` with `--allow` MUST fail with `mirror.closure-incomplete` and write nothing when the closure of the allowed ids (non-optional requires, all versions in range) contains an id the source does not serve.
- **[PKG-818]** The registry MUST serve metadata with `Content-Type: application/json` and blobs with `application/octet-stream`; a client MUST ignore the content type (as PKG-261 does for packages) and MUST accept `Content-Encoding: gzip` for metadata only, applying the size limits to the decoded bytes.
- **[PKG-819]** `ilium plugin owner add|remove <id> <publisher-id>` MUST require a signature by a key of an existing owner, MUST refuse removing the last owner with `registry.last-owner`, and MUST be recorded in the transparency log.
- **[PKG-821]** After a takedown of `id@version`, the registry MUST refuse any new publish of that same `id@version` with `registry.version-exists`, and the tombstone (id, version, sha256, reason code, time) MUST remain in plugins/<id>.json.
- **[PKG-822]** A publish on channel `stable` of a version lower than the highest stable version already published MUST be accepted only for a different MAJOR line (proposed, owner to confirm), otherwise refused with `registry.version-order`.
- **[PKG-823]** `yank` MUST print the number of plugins in the registry whose latest release requires a range that only the yanked version satisfies, and MUST require `--confirm-dependents` when that number is above 0.
- **[PKG-824]** A search request with `--limit` outside 1..100 or `--offset` above 10,000 (proposed, owner to confirm) MUST fail with `usage.invalid-value` before any network request, and a result page MUST contain at most the requested limit.
