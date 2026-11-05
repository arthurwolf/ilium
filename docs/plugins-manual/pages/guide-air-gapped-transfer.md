---
layout: page
title: Air-gapped transfer bundles
permalink: /pages/guide-air-gapped-transfer/
---

## You will need

This page specifies how plugins reach an isolated site without a network connection. An operator builds a bundle on a connected machine, carries it across on removable media, and imports it on the isolated side, where every hash, signature and metadata version is checked before anything is copied. It is for operators who run Ilium on machines with no network access and for the people who prepare the bundles on the connected side.

You need the `ilium` command on both machines. On the connected side you need the installed plugins that you want to carry across. On the isolated side you need the registry root key pinned under [PKG-985], because import refuses any bundle whose metadata is signed by a root key that is not pinned. You also need a removable medium to carry the archive, and the package source directory named in `packages.allowed_sources` must exist on the isolated side.

## Build a bundle on the connected side

Run `ilium plugin bundle create --out <file>` with the path where the archive should be written. The command writes one archive that holds the package files, their signature files, the signed registry metadata and a `bundle.json` file. The `bundle.json` file lists the SHA-256 of every other member under its `members` list, so the isolated side can check each member on its own. A bundle never carries registry root keys.

For example, a bundle built for two installed plugins contains two package files, their two signature files, the metadata files and one `bundle.json`. The `members` list in that `bundle.json` holds one SHA-256 for each of the other members.

## Verify and import on the isolated side

Copy the archive to the isolated site and run `ilium plugin bundle import <file>`. Import verifies every member hash and every signature before it copies anything into the package source named in `packages.allowed_sources`. If any hash or signature does not match, import copies nothing, exits with code 3 and names the failing member in its output. The allowed package source directory is left unchanged in that case.

Import also checks the root key that signed the metadata. A bundle whose metadata is signed by a root key not pinned under [PKG-985] is refused with `bundle.root-not-pinned` and exit code 3. A bundle whose metadata version is older than the stored metadata version is refused with the reason `rollback` and exit code 3.

After a successful import, Ilium writes one `bundle-import` audit record that holds the bundle SHA-256, the member count and the result. Check it with `ilium plugin audit --event bundle-import --json`, which prints one line containing `sha256`, `member_count` and `result: "ok"`. Installs from the imported bundle run with `ilium plugin install --from-bundle`.

## Metadata freshness and revocations offline

The signed metadata carries an `expires` time. When that time has passed, installs from the bundle are refused with the reason `metadata-expired` and a non-zero exit code. Plugins that are already installed keep running, and `ilium plugin list` still shows them as `enabled`. The site loses only the ability to install from bundles.

Revocations reach the site inside the bundle. A bundle that carries a revocation list newer than the stored one applies that list at import. Ilium writes one `trust.key-revoked` audit record for each newly revoked key, and each record holds the key id. A bundle that revokes two new keys therefore adds exactly two `trust.key-revoked` lines to the audit log.

## Update cadence

The requirements do not fix an interval between bundles. The `expires` time of the current bundle sets the deadline: the site needs a newer bundle before that time, or installs from bundles stop. Each new bundle must also carry a metadata version that is not older than the stored version, because an older version is refused as a rollback. Mirrors and the bundle refresh that follows a yank are covered by the publisher recall playbook, not by this page.

## Failure codes and edge cases

- A hash or signature mismatch, such as one flipped byte in a package inside a bundle, fails import with exit code 3. The output names the member, nothing is copied and the allowed package source directory is unchanged.
- A bundle signed by a root key that is not pinned under [PKG-985] fails import with `bundle.root-not-pinned` and exit code 3.
- A bundle with a metadata version older than the stored version fails import with `rollback` and exit code 3.
- A bundle whose metadata `expires` time has passed fails install with `metadata-expired` and a non-zero exit code. Existing plugins stay `enabled`.
- Only newly revoked keys produce a `trust.key-revoked` audit record. A key that was already revoked by the stored list produces no new record.
- Each successful import writes one `bundle-import` audit record, including an import that revoked no keys.

## Requirements

- **[PDV-001]** `ilium plugin bundle create --out <file>` MUST write a single archive holding the package files, their signatures and the signed registry metadata, plus a `bundle.json` listing the SHA-256 of every member.
- **[PDV-002]** `ilium plugin bundle import <file>` MUST verify every member hash and every signature before copying anything into the package source named in `packages.allowed_sources`, and on any mismatch MUST copy nothing and exit with code 3.
- **[PDV-003]** A bundle MUST NOT carry registry root keys, and import MUST refuse with `bundle.root-not-pinned` a bundle whose metadata is signed by a root key not pinned under [PKG-985].
- **[PDV-004]** When the signed metadata `expires` time has passed, installs from the bundle MUST be refused with reason `metadata-expired`, and already installed plugins MUST keep running.
- **[PDV-005]** A bundle carrying a revocation list newer than the stored one MUST apply it at import and write one `trust.key-revoked` audit record per newly revoked key.
- **[PDV-006]** Import MUST write one `bundle-import` audit record with the bundle SHA-256, the member count and the result.
- **[PDV-007]** Importing a bundle whose metadata version is older than the stored metadata version MUST be refused with reason `rollback` and exit code 3.
