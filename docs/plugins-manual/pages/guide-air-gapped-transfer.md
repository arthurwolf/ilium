---
layout: page
title: Air-gapped transfer bundles
permalink: /pages/guide-air-gapped-transfer/
---

## You will need

This page specifies how plugins reach an isolated site without a network connection. An operator builds a bundle on a connected machine, carries it across on removable media, and imports it on the isolated side, where every hash, signature and metadata version is checked before anything is copied. It is for operators who run Ilium on machines with no network access and for the people who prepare the bundles on the connected side.

You need the `ilium` command on both machines. On the connected side you need the installed plugins that you want to carry across. On the isolated side you need the registry root key pinned under [PKG-985], because import refuses any bundle whose metadata is signed by a root key that is not pinned. You also need a removable medium to carry the archive, and the package source directory named in `packages.allowed_sources` must exist on the isolated side.

## Build a bundle on the connected side

There is one offline bundle format. A bundle is the directory that `ilium plugin export-bundle <dir> <id>... [--with-deps]` writes ([PKG-412](/pages/reference-registry/#air-gapped-installs)). `ilium plugin bundle create --out <file> <id>... [--with-deps]` writes the same directory as one deterministic `.tar` archive ([PKG-098](/pages/reference-registry/#air-gapped-installs)), so that it can be carried as a single file. Both commands verify the same signatures and produce the same members.

Run `ilium plugin bundle create --out <file>` with the path where the archive should be written and the ids to carry. With no id, it takes every installed plugin. The command writes one archive that holds the package files, their signature files, the signed registry metadata and a `bundle.json` file. The `bundle.json` file lists the SHA-256 of every other member under its `members` list, so the isolated side can check each member on its own. A bundle never carries registry root keys.

For example, a bundle built for two installed plugins contains two package files, their two signature files, the metadata files and one `bundle.json`. The `members` list in that `bundle.json` holds one SHA-256 for each of the other members.

## Verify and import on the isolated side

Copy the archive to the isolated site and run `ilium plugin bundle import <file>`. Import verifies every member hash and every signature before it copies anything. It then unpacks the archive as a bundle directory, named after the bundle SHA-256, inside the package source named in `packages.allowed_sources`, and prints that directory. A bundle directory that was carried across without an archive needs no import: `install --from-bundle <dir>` runs the same checks on it. If a hash does not match, import copies nothing, exits with code 1 with `registry.hash-mismatch` and names the failing member in its output. A signature that does not verify fails in the same way with `registry.signature-invalid`. The allowed package source directory is left unchanged in that case.

Import also checks the root key that signed the metadata. A bundle whose metadata is signed by a root key not pinned under [PKG-985] is refused with `bundle.root-not-pinned` and exit code 1. A bundle whose metadata version is older than the stored metadata version is refused with `registry.rollback` and exit code 1. The exit codes are those of the one table in [Plugin command line](/pages/reference-plugin-cli/#exit-codes) ([DEV-007]); code 3 is kept for `check`, `lint` and `test` findings.

After a successful import, Ilium writes one `bundle-import` audit record that holds the bundle SHA-256, the member count and the result. Check it with `ilium plugin audit --event bundle-import --json`, which prints one line containing `sha256`, `member_count` and `result: "ok"`. Installs from the imported bundle run with `ilium plugin install --from-bundle <dir> <id>`, where `<dir>` is the directory that import printed. That install writes the normal `install` audit record, whether the directory came from `bundle import` or straight from `export-bundle`.

## Metadata freshness and revocations offline

The signed metadata carries an `expires` time. When that time has passed, installs from the bundle are refused with `registry.metadata-expired` and exit code 8. Plugins that are already installed keep running, and `ilium plugin list` still shows them as `enabled`. The site loses only the ability to install from bundles.

Revocations reach the site inside the bundle. A bundle that carries a revocation list newer than the stored one applies that list at import. Ilium writes one `trust-key-revoked` audit record for each newly revoked key. Each record holds the key id and the `reason` `trust.key-revoked`. A bundle that revokes two new keys therefore adds exactly two `trust-key-revoked` lines to the audit log.

## Update cadence

The requirements do not fix an interval between bundles. The `expires` time of the current bundle sets the deadline: the site needs a newer bundle before that time, or installs from bundles stop. Each new bundle must also carry a metadata version that is not older than the stored version, because an older version is refused as a rollback. Mirrors and the bundle refresh that follows a yank are covered by the publisher recall playbook, not by this page.

## Failure codes and edge cases

- A hash mismatch, such as one flipped byte in a package inside a bundle, fails import with `registry.hash-mismatch` and exit code 1. A signature mismatch fails with `registry.signature-invalid` and exit code 1. The output names the member, nothing is copied and the allowed package source directory is unchanged.
- A bundle signed by a root key that is not pinned under [PKG-985] fails import with `bundle.root-not-pinned` and exit code 1.
- A bundle with a metadata version older than the stored version fails import with `registry.rollback` and exit code 1.
- A bundle whose metadata `expires` time has passed fails install with `registry.metadata-expired` and exit code 8. Existing plugins stay `enabled`.
- Only newly revoked keys produce a `trust-key-revoked` audit record. A key that was already revoked by the stored list produces no new record.
- Each successful import writes one `bundle-import` audit record, including an import that revoked no keys.

## Requirements

- **[PDV-001]** `ilium plugin bundle create --out <file> <id>... [--with-deps]` MUST write a single deterministic `.tar` archive of the bundle directory that `export-bundle` writes for the same ids ([PKG-412], [PKG-098]), holding the package files, their signatures and the signed registry metadata, plus a `bundle.json` listing the SHA-256 of every member.
- **[PDV-002]** `ilium plugin bundle import <file>` MUST verify every member hash and every signature before copying anything into the package source named in `packages.allowed_sources`, and on any mismatch MUST copy nothing and exit with code 1 and the code `registry.hash-mismatch` or `registry.signature-invalid`. On success it MUST unpack the archive as a bundle directory inside that package source, which `install --from-bundle <dir> <id>` reads ([PKG-413]).
- **[PDV-003]** A bundle MUST NOT carry registry root keys, and import MUST refuse with `bundle.root-not-pinned` and exit code 1 a bundle whose metadata is signed by a root key not pinned under [PKG-985].
- **[PDV-004]** When the signed metadata `expires` time has passed, installs from the bundle MUST be refused with `registry.metadata-expired` and exit code 8, and already installed plugins MUST keep running.
- **[PDV-005]** A bundle carrying a revocation list newer than the stored one MUST apply it at import and write one `trust-key-revoked` audit record per newly revoked key, with the `reason` `trust.key-revoked`.
- **[PDV-006]** Import MUST write one `bundle-import` audit record with the bundle SHA-256, the member count and the result.
- **[PDV-007]** Importing a bundle whose metadata version is older than the stored metadata version MUST be refused with `registry.rollback` and exit code 1.
- **[PRM-004]** The system must treat the archive of `bundle create` and the directory of `export-bundle` as one bundle format: `bundle import <file>` followed by `install --from-bundle <dir> <id>` and a direct `install --from-bundle <dir> <id>` on the exported directory must verify the same signatures and hashes, must fail with the same `registry.*` and `bundle.*` codes and exit codes, and must write the same `install` audit record. Test: export one set of plugins both ways, install each on an offline fixture, and assert equal verification steps, equal audit `install` records and, for a bundle with one flipped byte, the same code `registry.hash-mismatch` with exit code 1 on both paths.

## Production additions (round 3)

- **[PRY-031]** Importing an air-gapped bundle MUST verify every entry hash before changing the local registry, MUST refuse a bundle larger than 4 GiB with `bundle.too-large`, and after an interrupted import (process killed at any point) the registry MUST equal its state from before the import started.
