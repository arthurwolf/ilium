---
layout: page
title: Registry key compromise
permalink: /pages/reference-registry-key-compromise/
---

## Registry key compromise

This page says what happens when a key that signs registry metadata is lost or exposed, who acts, and how a client gets a new root key. The registry has two kinds of keys. The **root keys** sign `root.json`, which names every other role and its keys. The **online role keys** are the `timestamp`, `snapshot` and `targets` keys. They sign the documents that clients fetch often. The model follows [Registry reference](/pages/reference-registry/).

A compromised key is one that an attacker may have used or may be able to use. A **root rotation** replaces keys through a new `root.json` that the old root threshold signs. A **re-pin** is a user action that replaces the root key stored in the client configuration. A re-pin is needed when the rotation chain cannot be trusted.

### You will need

- The registry metadata and rotation rules: [Registry reference](/pages/reference-registry/).
- The publisher key revocation flow, which is separate: [Signing and trust](/pages/reference-signing-and-trust/).
- The registry commands: [Plugin distribution command line](/pages/reference-plugin-distribution-cli/).

## Definitions

| Term | Meaning |
|---|---|
| Root key | A key listed in the `roles.root` entry of `root.json`. A threshold of them signs `root.json` ([PKG-364](/pages/reference-registry/)). |
| Online role key | A key of the `timestamp`, `snapshot` or `targets` role. The registry operator uses these often, so they are more exposed than root keys. |
| Threshold | The number of distinct keys of a role that must sign a document. `root.json` sets it per role. |
| Rotation chain | The sequence of `root.json` versions in which each version is signed by the threshold of the previous root keys. |
| Re-pin | `registry remove` followed by `registry add --root-key`, which replaces the root key stored in the client configuration. |
| Last good metadata | The newest metadata that a client verified completely. A client keeps it when a newer document is refused. |

## Inputs and outputs

- Input from the operator: the new key ids, the new `root.json` and the re-signed documents. Output to clients: documents that verify against the current `root.json`.
- Input from the user: the registry URL and the new root `key_id` from the announcement of [PKG-692]. Output: a registry entry that pins that key.
- Output of a client on failure: one of the codes in the Errors table. It never partly applies a refused document.

## Limits

- `timestamp.json` expires within 1 day of its issue, `snapshot.json` within 7 days and `root.json` within 1 year ([PKG-367](/pages/reference-registry/)).
- Maximum sizes of the documents involved: `root.json` 64 KiB, `timestamp.json` 16 KiB, `snapshot.json` 4 MiB ([PKG-369](/pages/reference-registry/)). A larger response is aborted with `registry.size`.
- A new `root.json` has a `version` equal to the previous version plus 1 ([PKG-364](/pages/reference-registry/)).

## Who acts

| Actor | Action in a compromise |
|---|---|
| Registry operator | Rotates the affected keys, re-signs the metadata and publishes the new root key id |
| Client user or administrator | Re-pins the registry when the root key changed in a way the client cannot follow |
| Client | Refuses metadata that does not verify and reports the code |

## Rules

- [PKG-690] A registry operator whose threshold of root keys is compromised MUST NOT rely on the rotation chain of [PKG-364](/pages/reference-registry/) to recover. A new root key set that the attacker could also sign is not a recovery. Each client MUST re-pin the registry under [PKG-693](/pages/reference-registry-key-compromise/).
- [PKG-691] When an online role key (`timestamp`, `snapshot` or `targets`) is compromised, the registry operator MUST publish a new `root.json` with a higher `version`, signed by the threshold of the current root keys, whose `roles` entry for that role names the new key ids. The operator MUST then re-sign `timestamp.json`, `snapshot.json` and every affected document with the new keys. A client MUST refuse a document whose signature uses a key id that the current `root.json` no longer lists, with `registry.signature-invalid`.
- [PKG-692] The registry operator MUST announce a root key change outside the registry files, on a channel that names the new root `key_id`, before it retires the old root key. A client MUST NOT learn a new root key from the mirror or from the registry response that carries the change.
- [PKG-693] A user MUST re-pin a registry when a root key was replaced and the client stops with `registry.changed`. The user runs `ilium plugin registry remove <url>` and then `ilium plugin registry add <url> --root-key <new_key_id>`, using the `key_id` from the announcement of [PKG-692]. A client MUST NOT accept a changed root key without that explicit re-pin. `registry add` without `--root-key` fails with `registry.root-key-missing`.

## Procedure for the registry operator

1. Stop publishing with the compromised key as soon as the compromise is known. Freeze new publishes until the new keys are live.
2. If only an online role key is compromised, sign a new `root.json` with the root threshold. Its `version` is the previous version plus 1 and its `roles` entry names the new key ids.
3. Re-sign `timestamp.json`, then `snapshot.json`, with the new keys. Keep the content of the other documents unchanged unless it changed.
4. If a root threshold is compromised, generate a new root key set offline and publish its `key_id` through the channel of [PKG-692]. Do not sign with the compromised keys.
5. Tell users which versions were published while the compromised key was usable. Published package signatures are checked by [Signing and trust](/pages/reference-signing-and-trust/), not by the registry key.

Decision needed: the time an operator may take between detecting a compromise and publishing the new `root.json`. Default: no fixed window; rotate as soon as the compromise is confirmed, because a compromised `timestamp` key can serve stale metadata only until `timestamp.json` expires, which [PKG-367](/pages/reference-registry/) limits to 1 day.

Decision needed: the out-of-band channel for the root key announcement of [PKG-692]. Default: the registry's own documentation site and the release notes of the first Ilium release that ships the new key id, published before the old root key is retired.

## Errors

| Situation | Code | Result |
|---|---|---|
| A document is signed by a key id that the current `root.json` no longer lists | `registry.signature-invalid` | Refused; the client keeps its last good metadata |
| The pinned root key no longer verifies `root.json` | `registry.changed` | Stops; the user re-pins under [PKG-693](/pages/reference-registry-key-compromise/) |
| A `root.json` with a lower `version` than the one seen | `registry.rollback` | Refused |
| `registry add` without `--root-key` | `registry.root-key-missing` | Refused; nothing is added |

## Worked example

A registry operator finds that the `timestamp` key was copied from a build server. The operator signs a new `root.json` with version 4 and a `timestamp` entry that names a new key id, then re-signs `timestamp.json` with that key. A client that already trusts the root keys verifies version 4 with the old root threshold and follows it. A client whose root keys were also replaced stops with `registry.changed`. Its user runs:

```text
ilium plugin registry remove https://plugins.example.org/ilium/v1/
ilium plugin registry add https://plugins.example.org/ilium/v1/ --root-key <new_key_id>
```

## Additional rules

- [PKG-696] A client MUST keep its last good metadata when it refuses a document under this page, and MUST NOT install, update or resolve a package from a document it refused.
- [PKG-697] After a root rotation, a client MUST verify the new `root.json` with the threshold of the previous root keys and with the threshold of its own `roles.root` keys, as [PKG-364](/pages/reference-registry/) requires, before it uses any key id from it.
- [PKG-698] The registry operator MUST NOT reuse a retired key id for a new key.
- [PKG-699] A client that stops with `registry.changed` MUST name the registry URL and MUST tell the user to re-pin under [PKG-693]. It MUST NOT re-pin by itself.

## What this page does not cover

- The document formats, the freshness window and rollback protection: [Registry reference](/pages/reference-registry/).
- Revoking a publisher key: [Signing and trust](/pages/reference-signing-and-trust/).
- The exact command options: [Plugin distribution command line](/pages/reference-plugin-distribution-cli/).
- Administrator policy for many clients: [Administrator guide](/pages/guide-plugin-administration/).

## See also

- [Registry reference](/pages/reference-registry/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Plugin distribution command line](/pages/reference-plugin-distribution-cli/)
- [Administrator guide](/pages/guide-plugin-administration/)
