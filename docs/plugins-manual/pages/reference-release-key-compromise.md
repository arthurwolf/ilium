---
layout: page
title: Release key compromise
permalink: /pages/reference-release-key-compromise/
---

## Release key compromise

The **release key** is the Ed25519 key that the Ilium project uses to sign the release blocklist ([SEC-146](/pages/reference-trust-and-supply-chain/)). Its public half is compiled into every Ilium binary. A binary accepts a release blocklist only when the key that signed it is the one compiled into that binary. This page says what the project does when that key is lost or may be stolen, how installed binaries learn of a replacement key, and what a user must do.

The release key is separate from publisher keys. A publisher key signs one plugin's packages and is managed with `ilium plugin trust` ([SEC-144](/pages/reference-trust-and-supply-chain/)). A compromise of the release key does not change any publisher's trust, and it does not change the trust store.

### You will need

- The blocklist format and the release blocklist rules: [Trust and supply chain](/pages/reference-trust-and-supply-chain/).
- How the project publishes advisories: [Security response](/pages/reference-security-response/).
- The audit events: [Audit log](/pages/reference-audit-log/).

## Treating the key as compromised

A key is compromised when the project cannot rule out that someone else holds it. The project decides this from its own records. A lost key counts as compromised.

- [SEC-424] When the release key is lost or may be exposed, the Ilium project MUST treat it as compromised. It MUST stop signing release blocklists with that key, and it MUST NOT sign any other release artefact with it. A compromised key MUST NOT be used to endorse a replacement key.

## How installed binaries learn of a new key

A binary cannot be updated in place to trust a new key, because its key is compiled into it. A stolen key can sign anything, so a file that a key signs cannot be the only way to tell an old binary about its replacement. The replacement therefore reaches users in a new release binary.

- [SEC-425] A binary MUST verify the release blocklist only with the release key compiled into that binary. A release blocklist signed by any other key MUST be refused: the binary keeps its last good copy, writes an audit line, and does not apply the new entries.
- [SEC-426] A replacement release key MUST reach users only inside a new Ilium release that the user installs. An installed binary MUST NOT learn a new release key from a blocklist, a policy file, a plugin package or any other file.

Decision needed: whether the old key may sign one final retirement statement that names the replacement key and is delivered in a blocklist. Default: no. A stolen key can sign that statement as easily as any other file, so the replacement key arrives only in a new binary, as [SEC-426] requires. A user who never installs the new release keeps the last good blocklist and receives no further entries.

Decision needed: the audit event written when a release blocklist is refused. Default: `signature-failure`, the event the audit log already uses for failed signature checks ([Audit log](/pages/reference-audit-log/)), so no new event name is added.

## What the user does

A user acts on a compromise only through a new release. The user does not edit the blocklist, and no command accepts a key from the command line.

1. Read the advisory that the project publishes ([SEC-427](/pages/reference-security-response/)).
2. Install the new release from the project's download channel. Compare the release signature with the value in the advisory through a channel other than the download itself.
3. Stop using the old binary once the new release is installed. It keeps running the plugins it already has, but it receives no further release blocklist entries.

Decision needed: the place where the replacement release key's fingerprint is published, so users can compare it. Default: in the advisory and on the project's download page, both reached over HTTPS, since a fingerprint in the same channel as the binary is only as safe as that channel.

## Advisories

- [SEC-427] After a release key compromise, the Ilium project MUST publish an advisory that names the compromised key by its fingerprint, states that binaries using it no longer receive blocklist updates, and tells users to install the replacement release. The advisory MUST state that plugin trust does not change, so users who trust publishers keep the same trust store.

## Refusals and errors

| Condition | Result |
|---|---|
| A release blocklist signed by a key other than the compiled-in release key | Refused; last good copy kept; audit line written ([SEC-425]) |
| A release blocklist that is malformed | Ignored with an audit line; last good copy used (see [Trust and supply chain](/pages/reference-trust-and-supply-chain/)) |
| A new release key found in any file other than a new release binary | Not accepted ([SEC-426]) |

### See also

- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Security response](/pages/reference-security-response/)
- [Audit log](/pages/reference-audit-log/)
- [Security and policy requirements](/pages/reference-requirements-security-policy/)

Contract: `ilium:plugin@0.1.0`.
