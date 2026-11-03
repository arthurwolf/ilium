---
layout: page
title: Publisher workflow and version states
permalink: /pages/publish-publisher-workflow/
---

## Publisher workflow and version states

This page is the step-by-step guide for a publisher who sends a plugin version to a registry. It covers the order of the steps, the states a version passes through on the registry, the errors a publish can return, and what a publisher does when an upload fails in the middle. The command reference is in [Distribution commands](/pages/reference-plugin-distribution-cli/#publish-yank-deprecate). The metadata layout and the upload API are in [Registry reference](/pages/reference-registry/#publishing). Yank, deprecate and takedown are covered in [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/).

### You will need

- A publisher id that you own, and a namespace that matches it ([PKG-388](/pages/reference-registry/#namespaces)). The namespace rules are in [Namespaces](/pages/reference-registry/#namespaces).
- A maintainer account on the registry with a second factor enrolled ([SEC-723](/pages/reference-trust-and-supply-chain/)).
- A signing key for the package, created as in [Publishing guide](/pages/guide-publishing/#create-a-signing-key).
- A publish token, issued to your publisher id ([PKG-968](/pages/reference-registry/#publish-tokens)). Keep it in an environment variable or a token file. Never on a command line.

### The steps

Run these steps in this order. Each step has one command, and the step before must pass.

1. Choose the version number. It must be a new `MAJOR.MINOR.PATCH` value that the registry does not hold for this id. Published versions are immutable ([PKG-394](/pages/reference-registry/#publishing)).
2. Pack the plugin and sign it. The packing and signing commands are in [Pack, sign, check](/pages/guide-publishing/#pack-sign-check).
3. Verify the package file locally. This runs install stages 1 to 7 and changes no file:

```
ilium plugin verify dist/notes.iliumpkg
```

4. Publish with the token in the environment:

```
export ILIUM_REGISTRY_TOKEN="$(cat ~/.config/ilium/publish.token)"
ilium plugin publish dist/notes.iliumpkg --registry https://registry.example.org --channel stable
```

5. Read the result record. A success prints the id, the version, the SHA-256 hash and the public URL of the version. Keep the hash. You need it for the convergence check in [PUB-006](#failed-uploads).

Step 4 runs `verify` again before it uploads ([PKG-441](/pages/reference-plugin-distribution-cli/#publish-yank-deprecate)), so a package that fails step 3 never reaches the network.

### Version states on the registry

A version moves through the states in the table. The registry keeps the state in its signed metadata. A client sees the state through `index.json` and the version entry in `plugins/<id>.json`.

| State | Entered when | Visible to installers | Leaves the state by |
|---|---|---|---|
| `verifying` | The upload is received and the server runs stages 1 to 8 and the policy check. | No | Passing, or the rejection below. |
| `rejected` | A stage or policy check fails. | No | Not applicable. The publisher uploads a fixed package under a new version. |
| `queued-review` | The version needs a level B review ([PKG-401](/pages/reference-registry/#policy)) and the review has not finished. | Yes, at level A only ([PKG-681](/pages/reference-registry/#policy)). | The review record is added (level B), or the review refuses the version (`rejected`). |
| `live` | The checks pass and no review is pending. | Yes. | `yanked`, or `takedown` by the operator. |
| `yanked` | `ilium plugin yank` ([PKG-396](/pages/reference-registry/#publishing)). | Yes, but not chosen for new installs or updates. | `live` again through `unyank`. |
| `takedown` | The operator adds the version to `revocations.json` and removes its blob ([PKG-403](/pages/reference-registry/#policy)). | Installed copies move to `disabled-blocked`. | Reinstated only as a new version after an appeal ([PKG-404](/pages/reference-registry/#policy)). |

**[PUB-001]** The registry MUST keep a version in `verifying` until stages 1 to 8 and the policy check finish, and MUST NOT show the version in `index.json` before it reaches `live` or `queued-review`. Test: upload a package whose signature check is slow, and read `index.json` during the wait; the version is absent.
**[PUB-002]** A version that reaches `rejected` MUST NOT be stored as a published version, and its version number MUST remain free for a new upload. Proposed, owner to confirm: the rejection record names the stage and the code, and the publisher can read it with `ilium plugin info <id>@<version>`, which exits 5 for a version that never became live.
**[PUB-003]** A `live` version MUST move to `yanked` only through a signed change to the metadata, and the transition MUST keep the blob, the version entry and its hash ([PKG-443](/pages/reference-plugin-distribution-cli/#publish-yank-deprecate)). Test: yank a version, then compare the blob hash in `snapshot.json` before and after; the hash is unchanged.

### Errors a publish can return

The publish command uses the exit codes of [Distribution commands](/pages/reference-plugin-distribution-cli/#exit-codes). The table below lists the codes a publisher meets most often and the action each one needs.

| Code | Exit | Cause | What the publisher does |
|---|---|---|---|
| `publish.unsigned` | 1 | The package has no signature from a key the registry accepts. | Sign the package with the publisher key and pack again. |
| `registry.version-exists` | 1 | The version is already on the registry. | Check the hash with `ilium plugin info`. See [PUB-006](#failed-uploads). Otherwise choose a new version. |
| `registry.namespace-unowned` | 1 | The id is outside the namespace of the publisher token. | Use an id under your publisher id, or ask the operator to correct the namespace. |
| `registry.auth` | 1 | The token is missing, expired or revoked ([PKG-968](/pages/reference-registry/#publish-tokens), [PKG-970](/pages/reference-registry/#publish-tokens)). | Issue a new token and set `ILIUM_REGISTRY_TOKEN` again. |
| `registry.policy` | 7 | The package fails the registry policy ([PKG-681](/pages/reference-registry/#policy)). The detail names `sbom-missing`, `license-invalid` or `license-notice-missing`. | Add the missing file or fix the licence, then pack again. Proposed, owner to confirm: exit 7 for `registry.policy`, because it is a policy refusal, as [PKG-424](/pages/reference-plugin-distribution-cli/#exit-codes) gives for `policy.*`. |
| `registry.size` | 1 | The package is over 300 MiB ([PKG-400](/pages/reference-registry/#publishing)). | Remove files that the package does not need. |
| `registry.rate-limited` | 1 | The publisher is over 20 publishes in one hour ([PKG-400](/pages/reference-registry/#publishing)). | Wait for the `Retry-After` time and publish again. |
| `registry.unreachable` | 8 | The network failed and no response came back. | Go to [PUB-006](#failed-uploads) before you publish again. |

**[PUB-004]** `publish` MUST NOT print the token, and MUST NOT print the `Authorization` header in any error message. Test: publish with a wrong token and grep the output and the log for the token value; no match is found.
**[PUB-005]** `publish` MUST exit with the code in the table above for each listed code, and the `exit_code` of its `result` record MUST equal the process exit code ([PKG-423](/pages/reference-plugin-distribution-cli/#exit-codes)). Test: trigger each listed code against a test registry and compare the two values.

### Failed uploads

A publish can fail after the server has stored the version but before the client receives the `201` answer. A network drop, a closed laptop or a timeout causes this. The next publish of the same file then returns `registry.version-exists`, even though the upload worked. A publisher must not treat that as an error without checking.

**[PUB-006]** After a `registry.unreachable` or a timeout on `publish`, the publisher MUST run `ilium plugin info <id>@<version>` before publishing again. If the registry returns the version with the same SHA-256 hash as the local package, the publish is complete and the publisher records the hash. If the version is absent, the publisher MAY publish again with the same file. Proposed, owner to confirm: the CLI does this check itself and reports the outcome as `publish.already-complete` with exit 0 when the hashes match, and `registry.version-exists` with exit 1 when they differ.
**[PUB-007]** A publish with the same file whose hash differs from the hash of the live version of that `id@version` MUST exit 1 with `registry.version-exists`, and MUST NOT overwrite the live version ([PKG-394](/pages/reference-registry/#publishing)). The fix is a new version number.

### Worked example

A publisher, Ana, releases version 0.3.1 of `com.example.notes`. The first upload times out. She checks the registry and finds the version live with the same hash.

<pre class="term"><span class="t-prompt">$</span> ilium plugin publish dist/notes.iliumpkg --registry https://registry.example.org
<span class="t-err">error</span> registry.unreachable: no response within 60 s
<span class="t-prompt">$</span> ilium plugin info com.example.notes@0.3.1 --json
<span class="t-ok">plugin</span>  com.example.notes 0.3.1 channel=stable state=live
<span class="t-dim">sha256</span>  9c1e...4f20
<span class="t-prompt">$</span> sha256sum dist/notes.iliumpkg
9c1e...4f20  dist/notes.iliumpkg
<span class="t-ok">ok</span>      publish complete, hashes match (proposed check of PUB-006)
</pre>

Ana records the hash in her release notes and moves on. Had the hashes differed, she would bump the version to 0.3.2, because the live 0.3.1 cannot be replaced ([PUB-007](#failed-uploads)).

### Edge cases

- **Two publishers race for one version.** The first complete upload wins. The second receives `registry.version-exists` ([PKG-394](/pages/reference-registry/#publishing)).
- **A token expires during a release.** The upload fails with `registry.auth` before any version is stored, so no state changes and the publisher can retry after the new token is set.
- **A package is correct but the policy changes.** A version that was `live` stays `live`. The policy applies to new uploads only, unless the operator takes it down under [PKG-403](/pages/reference-registry/#policy).
- **A publisher uploads to a mirror URL.** Mirrors do not accept publishes. A mirror is read-only ([Mirror runbook](/pages/publish-mirror-runbook/)). Publish to the registry, and the mirror picks the version up at its next sync.

### See also

- [Yank, deprecate and takedown](/pages/publish-yank-deprecate-takedown/): how a published version is withdrawn without deleting it.
- [Publisher key compromise](/pages/reference-registry-key-compromise/): what to do when the publish token or the signing key leaks.
- [Registry operator guide](/pages/publish-registry-operator-guide/): the operator's side of the same steps.
