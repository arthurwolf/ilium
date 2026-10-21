# Plugin administration

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
| [PKG-465] | An administrator MAY add a key with `ilium plugin trust add <file> --tier T2 --system`. A key added this way has `added_by: admin` and the user cannot remove it. |
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
| [PKG-468] | The timestamp document expires after one day. The operator MUST re-sign it at least daily, or clients report `registry.stale`. |
| [PKG-469] | To rotate the root key, publish a rotation document signed by the old and the new key. See [Registry reference](/pages/reference-registry/#rootjson). |

## Mirror and air-gap

A mirror copies a registry so that machines without internet can install. Run `ilium plugin mirror sync <dir> --registry <url> --allow <id>` on a connected machine, copy the folder to the isolated network and serve it. For a single machine, use `export-bundle` and `install --from-bundle`. Both copy signed bytes without change, so the signatures still verify.

| Id | Rule |
|---|---|
| [PKG-473] | A mirror MUST NOT be signed by the mirror operator. Clients check the signatures of the original registry. |
| [PKG-474] | With `packages.allowed_sources` set to the mirror, a client that cannot reach the mirror reports `registry.unreachable` and never falls back to another registry. |

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
| Policy file is invalid | The host refuses to start plugins and shows `policy.invalid`. See [Plugin policy](/pages/reference-plugin-policy/#errors-and-edge-cases). |
| Two policies conflict | The stricter value wins. |
| User adds a registry not in `packages.allowed_sources` | `policy.source-denied`, exit 7. |
| Mirror is missing a dependency | `dependency.unsatisfied` names the id. Add it with `--with-deps`. |

## See also

- [Plugin policy](/pages/reference-plugin-policy/)
- [Registry reference](/pages/reference-registry/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Plugin store](/pages/user-plugin-store/)
