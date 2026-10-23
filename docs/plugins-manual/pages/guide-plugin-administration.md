---
layout: page
title: Plugin administration
permalink: /pages/guide-plugin-administration/
---

## Plugin administration

This guide is for the person who runs Ilium for a team. It shows how to decide which plugins people may install, which registries they may use, and how to run a private registry or an offline mirror. A plugin is a signed package that adds features to Ilium. The administrator sets the rules in one policy file. Users cannot loosen them.

### You will need

- The policy file format: [Plugin policy](/pages/reference-plugin-policy/).
- The trust model: [Signing and trust](/pages/reference-signing-and-trust/).
- The registry protocol: [Registry reference](/pages/reference-registry/).

Requirement ids such as `PKG-460` are collected in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

## Choose a stance

| Goal | Policy | Id |
|---|---|---|
| Allow only plugins from your company | `publishers.allow` with your key fingerprint, and `packages.allowed_sources` with your registry | [PKG-460] |
| Allow only bundled plugins | `packages.allow_third_party = false` | [PKG-461] |
| Block one plugin | `plugins.deny_ids = ["com.example.bad"]` | [PKG-462] |
| Block a risky grant everywhere | `grants.deny = ["process.run:*"]` | [PKG-463] |
| Stop beta builds | `packages.allowed_channels = ["stable"]` | [PKG-464] |

A policy that fails an install returns a `policy.*` code and exit 7 (see [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes)). The user sees the message "Blocked by your administrator".

## Pre-trust your key

Put the key of your company in the machine trust store so that users do not see a trust prompt. The trust store format and the `trust add` command are in [Signing and trust](/pages/reference-signing-and-trust/#the-trust-store).

| Id | Rule |
|---|---|
| [PKG-465] | An administrator MAY add a key to the system store `/etc/ilium/trust.json` with an entry whose `added_by` is `admin`, in the store format of [Signing and trust](/pages/reference-signing-and-trust/#the-trust-store). The host never writes the system store ([PKG-222]), and the user cannot remove the entry. |
| [PKG-466] | A key in the trust store never overrides a revocation. A revoked key stays revoked. |

## Run a private registry

A registry is a folder of signed files on a web server. You need no database. Follow [Registry reference](/pages/reference-registry/#file-layout) for the layout. Steps:

1. Create the root key offline and keep it on a device that is not on the network.
2. Publish `root.json`, `timestamp.json`, `snapshot.json` and `index.json` to an HTTPS folder.
3. Give every client the root key id with `ilium plugin registry add <url> --root-key <key_id>`, or write it in the user configuration under `[plugins.registry]`.
4. Set `packages.allowed_sources` in the policy to the same URL.

| Id | Rule |
|---|---|
| [PKG-467] | A registry MUST be served over HTTPS. The client refuses plain `http:` except `localhost` in development (PKG-371). |
| [PKG-468] | The timestamp document expires after one day. The operator MUST re-sign it at least daily, or clients report `registry.metadata-expired`. |
| [PKG-469] | To rotate the root key, publish a rotation document signed by the old and the new key. See [Registry reference](/pages/reference-registry/#rootjson). |

## Mirror and air-gap

A mirror copies a registry so that machines without internet can install. Run `ilium plugin mirror sync <dir> --registry <url>` on a connected machine (a full copy; add `--allow <id>` for a partial one), copy the folder to the isolated network and serve it. For a single machine, use `export-bundle` and `install --from-bundle`. The bundle and a full copy copy signed bytes without change, so the signatures still verify. A partial copy re-signs its index and snapshot with its own keys ([PKG-409]); clients verify it against its own root ([PKG-473]).

| Id | Rule |
|---|---|
| [PKG-473] | A full mirror MUST NOT be signed by the mirror operator. Clients check the signatures of the original registry. The one exception is a partial mirror ([PKG-409]), which signs with its own keys and its own `root.json`; clients verify that mirror against its own root, set in `[plugins.registry]`. |
| [PKG-474] | With `packages.allowed_sources` set to the mirror, a client that cannot reach the mirror reports `registry.unreachable` and never falls back to another registry. |

### Keep an isolated fleet fresh

An isolated network receives no metadata unless someone copies it in, and the metadata expires ([PKG-367], [PKG-468]). A mirror or bundle that is not re-synced therefore goes stale by itself. The rules below set the cadence, the expiry check and a fleet-wide offline mode.

| Id | Rule |
|---|---|
| [PKG-950] | On an isolated network, the administrator MUST re-run `ilium plugin mirror sync` (or re-export the bundle with `export-bundle`) on a connected machine at least once a day (proposed, owner to confirm). A full copy keeps the source `timestamp.json` byte for byte ([PKG-408]), and that document expires after one day. Test: sync a mirror, move the client clock 25 hours forward, and assert an install fails with `registry.metadata-expired`. |
| [PKG-951] | An administrator MAY set `[plugins.registry] offline = true` in the system policy file (proposed: this key is not yet in the [policy keys](/pages/reference-admin-policy/) table). The setting is combined with the user file by logical OR, as the `offline` flag is ([SEC-181]), so a user cannot clear it. With it set, a cached index past its `expires` time is used with the stale-data warning of [PKG-378], and the install does not fail with `registry.metadata-expired`. Test: set the key, expire the cached index, and assert the install succeeds with the warning. |
| [PKG-952] | After every sync, the administrator MUST read the `signed.expires` field of `timestamp.json` in each mirror folder or unpacked bundle, and MUST raise an alert when less than 12 hours remain before it passes (proposed, owner to confirm). Test: create a mirror whose timestamp expires in 6 hours and assert that the check reports it. |

## Policy keys for registries and sources

The keys `packages.allowed_sources` [PKG-470], `packages.allow_url_installs` [PKG-471] and `packages.allowed_channels` [PKG-472] are defined in [Plugin policy](/pages/reference-plugin-policy/#keys). Rules:

| Id | Rule |
|---|---|
| [PKG-475] | `packages.allowed_sources` compares the scheme, host and path prefix of the configured URL. A trailing slash does not matter. |
| [PKG-476] | A package installed before the policy changed is not removed. It follows the existing `disabled-by-policy` rule of the policy page. |

## Audit

The audit log is always on. Each install, update, removal, trust change and policy refusal writes one record. See [Audit log](/pages/reference-audit-log/).

## Automatic updates

Automatic updates are off. A user can set `[plugins] auto_update` to `notify` or `install` in their configuration. The administrator controls the effect through `packages.allowed_channels` and `plugins.deny_ids`, because an automatic update passes through the same policy check as a manual one. Details: [Install and update](/pages/reference-install-and-update/).

| Id | Rule |
|---|---|
| [PKG-477] | An automatic update MUST run all install checks, and MUST NOT install when new grants or a new signing key would need a prompt. It then notifies only. |

## Edge cases

| Case | Result |
|---|---|
| Policy file is invalid | Third-party plugins (T2 and T3) refuse to load with `policy.invalid`; bundled T1 plugins run normally. See [Plugin policy](/pages/reference-plugin-policy/#errors-and-edge-cases). |
| Two policies conflict | The stricter value wins. |
| User adds a registry not in `packages.allowed_sources` | `policy.source-denied`, exit 7. |
| Mirror is missing a dependency | `dependency.unsatisfied` names the id. Add the id to the mirror's `--allow` list and run `ilium plugin mirror sync` again; a partial mirror includes its dependencies ([PKG-409]). For an air-gapped bundle, export with `ilium plugin export-bundle <dir> <id> --with-deps` ([PKG-412]). |

## Shared machine grant scope requirements

- [USR-252] On a machine with several OS users, the host MUST keep grants, trusted keys and plugin data per OS user, so that a grant or trust decision by one user never applies to another. Test: Grant a permission as user A and assert user B sees it as not granted.
- [USR-253] Machine policy MUST apply to every OS user, and a per-user trust or grant MUST NOT widen it; when a user's trust conflicts with policy, the policy wins and the user sees `Blocked by your administrator`. Test: Trust a key as a user, deny it in machine policy and assert the plugin is blocked for that user.

## See also

- [Plugin policy](/pages/reference-plugin-policy/)
- [Registry reference](/pages/reference-registry/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Plugin store](/pages/user-plugin-store/)
