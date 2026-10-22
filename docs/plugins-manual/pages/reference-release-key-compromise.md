---
layout: page
title: Release key compromise
permalink: /pages/reference-release-key-compromise/
---

## Release key compromise

The **release key** is the Ed25519 key that the Ilium project uses to sign the release blocklist ([SEC-146](/pages/reference-trust-and-supply-chain/)). Its public half is compiled into every Ilium binary. A binary accepts a release blocklist only when the key that signed it is the one compiled into that binary. This page says what the project does when that key is lost or may be stolen, how installed binaries learn of a replacement key, and what a user must do.

The release key is separate from publisher keys. A publisher key signs one plugin's packages and is managed with `ilium plugin trust` ([SEC-144](/pages/reference-trust-and-supply-chain/)). A compromise of the release key does not change any publisher's trust, and it does not change the trust store.

### Terms

- **Release key**: the Ed25519 key pair whose private half signs the release blocklist. The public half is compiled into each binary.
- **Release blocklist**: the signed list of packages, plugin ids, publisher keys and runtime advisories shipped with a release ([SEC-146](/pages/reference-trust-and-supply-chain/)). It is not the policy blocklist ([SEC-147](/pages/reference-trust-and-supply-chain/)) and not a user blocklist.
- **Fingerprint**: the short, fixed-size name of a public key, as defined in [Trust and supply chain](/pages/reference-trust-and-supply-chain/).
- **Last good copy**: the most recent release blocklist that a binary verified with its compiled-in key. A binary keeps it and uses it when a newer file is refused or malformed.
- **Compiled-in key**: the release public key built into one binary. It cannot be changed without building a new binary.
- **Replacement release**: a new Ilium release binary that carries a new compiled-in release key.

### You will need

- The blocklist format and the release blocklist rules: [Trust and supply chain](/pages/reference-trust-and-supply-chain/).
- How the project publishes advisories: [Security response](/pages/reference-security-response/).
- The audit events: [Audit log](/pages/reference-audit-log/).

## Treating the key as compromised

A key is compromised when the project cannot rule out that someone else holds it. The project decides this from its own records. A lost key counts as compromised.

- [SEC-424] When the release key is lost or may be exposed, the Ilium project MUST treat it as compromised. It MUST stop signing release blocklists with that key, and it MUST NOT sign any other release artefact with it. A compromised key MUST NOT be used to endorse a replacement key.

- [SEC-431] A release key compromise MUST NOT change the trust store, any publisher pin, or any installed plugin's trust state. Plugins keep loading exactly as before.

## How installed binaries learn of a new key

A binary cannot be updated in place to trust a new key, because its key is compiled into it. A stolen key can sign anything, so a file that a key signs cannot be the only way to tell an old binary about its replacement. The replacement therefore reaches users in a new release binary.

- [SEC-425] A binary MUST verify the release blocklist only with the release key compiled into that binary. A release blocklist signed by any other key MUST be refused: the binary keeps its last good copy, writes an audit line, and does not apply the new entries.
- [SEC-426] A replacement release key MUST reach users only inside a new Ilium release that the user installs. An installed binary MUST NOT learn a new release key from a blocklist, a policy file, a plugin package or any other file.

- [SEC-432] No command, setting, environment variable or policy key MAY accept a release key, a release key fingerprint or a retirement statement. The only path to a new release key is a new binary ([SEC-426]).

Decision needed: whether the old key may sign one final retirement statement that names the replacement key and is delivered in a blocklist. Default: no. A stolen key can sign that statement as easily as any other file, so the replacement key arrives only in a new binary, as [SEC-426] requires. A user who never installs the new release keeps the last good blocklist and receives no further entries.

Decision needed: the audit event written when a release blocklist is refused. Default: `signature-failure`, the event the audit log already uses for failed signature checks ([Audit log](/pages/reference-audit-log/)), so no new event name is added.

## What the user does

A user acts on a compromise only through a new release. The user does not edit the blocklist, and no command accepts a key from the command line.

1. Read the advisory that the project publishes ([SEC-427](/pages/reference-security-response/)).
2. Install the new release from the project's download channel. Compare the release signature with the value in the advisory through a channel other than the download itself.
3. Stop using the old binary once the new release is installed. It keeps running the plugins it already has, but it receives no further release blocklist entries.

Decision needed: the place where the replacement release key's fingerprint is published, so users can compare it. Default: in the advisory and on the project's download page, both reached over HTTPS, since a fingerprint in the same channel as the binary is only as safe as that channel.

## Inputs and outputs

| Item | Direction | Detail |
|---|---|---|
| Release blocklist file | Input to the binary | Verified only against the compiled-in key ([SEC-425]) |
| Last good copy | Kept by the binary | Used when a newer file is refused or malformed |
| Audit line | Output of the binary | Written on every refusal; the event name is a decision above |
| Advisory | Output of the project | Content fixed by [SEC-427] |
| Replacement release | Output of the project | Carries the new compiled-in key ([SEC-426]) |

## Behaviour of an old binary after a compromise

An old binary does not stop working. It keeps its last good blocklist, so entries added before the compromise stay in force. It receives no later entries, so a package that the project blocks afterwards is not blocked on that binary. A refused file never replaces the last good copy, and a binary with no last good copy has no release blocklist entries.

## Limits

This page sets no numeric limits of its own. The release blocklist size and format limits, and the blocklist handling rules, are in [Trust and supply chain](/pages/reference-trust-and-supply-chain/). No deadline for publishing the advisory or the replacement release is set anywhere in the manual.

Decision needed: the deadline for publishing the advisory after the project declares the key compromised. Default: none stated; the project publishes the advisory as soon as the replacement release is available, and no number is promised until the project sets one.

## Advisories

- [SEC-427] After a release key compromise, the Ilium project MUST publish an advisory that names the compromised key by its fingerprint, states that binaries using it no longer receive blocklist updates, and tells users to install the replacement release. The advisory MUST state that plugin trust does not change, so users who trust publishers keep the same trust store.

## Example

The project declares release key `K1` compromised. It publishes release 2.0 with compiled-in key `K2` and an advisory that names the fingerprint of `K1`. A user on release 1.9 later receives a blocklist file signed with `K1` by an attacker. Release 1.9 verifies it against `K1`, which is still its compiled-in key, so the file passes the signature check. This is why the advisory tells users to install the replacement release: an old binary cannot tell a stolen-key file from a real one. A user on release 2.0 refuses the same file, because it is not signed with `K2` ([SEC-425]).

## Refusals and errors

| Condition | Result |
|---|---|
| A release blocklist signed by a key other than the compiled-in release key | Refused; last good copy kept; audit line written ([SEC-425]) |
| A release blocklist that is malformed | Ignored with an audit line; last good copy used (see [Trust and supply chain](/pages/reference-trust-and-supply-chain/)) |
| A new release key found in any file other than a new release binary | Not accepted ([SEC-426]) |

This page names no new error codes. A refused blocklist is reported only through the audit line and the unchanged last good copy. The error codes for blocked packages, such as `package.blocked`, are in [Trust and supply chain](/pages/reference-trust-and-supply-chain/).

## What this page does not cover

- The blocklist format, entry kinds and publisher key handling: [Trust and supply chain](/pages/reference-trust-and-supply-chain/).
- How the project receives reports and publishes advisories in general: [Security response](/pages/reference-security-response/).
- Audit event names and retention: [Audit log](/pages/reference-audit-log/).
- Checking a downloaded archive by hand: [Host release verification](/pages/reference-host-release-verification/).
- Publisher key compromise: [SEC-174](/pages/reference-trust-and-supply-chain/).

### See also

- [Trust and supply chain](/pages/reference-trust-and-supply-chain/)
- [Security response](/pages/reference-security-response/)
- [Audit log](/pages/reference-audit-log/)
- [Security and policy requirements](/pages/reference-requirements-security-policy/)

Contract: `ilium:plugin@0.1.0`.
