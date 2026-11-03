---
layout: page
title: Install flag matrix
permalink: /pages/install-flag-matrix/
---

## Install flag matrix

This page says what each flag of `ilium plugin install` does, which sources it applies to, and which combinations work. It covers the install flags only. The full command list, exit codes and JSON records are in [Distribution commands](/pages/reference-plugin-distribution-cli/), and the install transaction is in [Install and update](/pages/reference-install-and-update/).

Rules marked **proposed, owner to confirm** are not in the current specification. They are the behaviour this page recommends until the owner confirms or changes them. Tests may cite them only after confirmation.

### You will need

- The install command shape: `ilium plugin install <source> [flags]`.
- How a source is classified (URL, file path or registry id): [PKG-426](/pages/reference-plugin-distribution-cli/#install).
- The codes of the result record: [Plugin command line](/pages/reference-plugin-cli/#output-model).

### Sources and the flags that apply to them

A source is one of four kinds. Each kind accepts a different set of flags.

| Source | Example | Flags that apply |
|---|---|---|
| Local file | `ilium plugin install ./notes-1.3.0.iliumpkg` | All flags except `--registry`, `--from-bundle` and `--channel` |
| HTTPS URL | `ilium plugin install https://example.org/notes-1.3.0.iliumpkg` | As for a local file |
| Registry id | `ilium plugin install com.example.notes@^1.2` | All flags except `--from-bundle` |
| Bundle id | `ilium plugin install --from-bundle ./bundle com.example.notes` | All flags except `--registry` and `--channel` |

The table describes what the flag does. A flag that does not apply to a source is refused as described in the next section.

### Flag reference

Each flag has one effect. The effect is the same for every source it applies to.

<div class="note">
<p>The review is the screen where the user sees the permissions of the package. Flags that skip the review never skip the rules for high-risk grants or new keys. Those rules are the subject of the rows below.</p>
</div>

- **`--allow-unsigned`** accepts a package without a valid signature. It does not trust a key. A package that is not signed is shown as unsigned in the review.
- **`--allow-downgrade`** accepts an install whose version is lower than the active version. The downgrade still follows the data rule in [Install and update](/pages/reference-install-and-update/): a lower `data_version` is refused with `migration.data-newer` unless a backup restore is chosen on rollback.
- **`--trust <key_id>`** trusts the named key for this install and for later ones. It is the only non-interactive way to trust a key ([PKG-225](/pages/reference-signing-and-trust/)).
- **`--channel stable|beta|dev`** sets the channel that later updates use. `dev` is refused for an install; the dev channel exists only for `ilium plugin dev`.
- **`--yes`** skips the normal review. It does not accept a high-risk grant, a new `replaces` entry, or a new key.
- **`--grant <grant>`** accepts one named high-risk grant. It can be repeated. It accepts only the grant it names.
- **`--locked`** installs the versions and package hashes listed in `ilium-plugins.lock`.
- **`--dry-run`** runs the validation and prints the review as records. It writes nothing under the plugin root.
- **`--registry <url>`** names the registry to use for a registry id instead of the configured one.
- **`--from-bundle <dir>`** reads the package from an offline bundle. It makes no network request.
- **`--no-backup`** skips the pre-migration copy of the data during an update.
- **`--unskip`** removes the `skip` mark that a rollback set on a version ([PKG-292](/pages/reference-install-and-update/#rollback)).

### Rules for each flag

- **[INS-001]** `--yes` MUST NOT accept a grant with the risk level `high`, a new `replaces` entry, or a key that is not yet trusted. Only `--grant <grant>` accepts a high-risk grant and only `--trust <key_id>` trusts a key. Test: install a package with one high-risk grant and an unknown signing key using `--yes`, then assert that the install exits 1 with `install.review-required` and that `ilium plugin list` shows no new plugin.
- **[INS-002]** `--grant <grant>` MUST accept only the grant it names, and every other high-risk grant MUST still block the install. Test: install a package with grants `a` and `b`, both high risk, with `--grant a`, and assert that the install exits 1 and that the grant file is unchanged.
- **[INS-003]** `--grant` MUST be refused with `usage.invalid-value` and exit 2 when it names a grant that the package does not request. The refusal MUST happen before any file change (proposed, owner to confirm). Test: pass `--grant missing` for a package without that grant and assert exit 2 and an unchanged store.
- **[INS-004]** `--trust <key_id>` MUST be refused with `usage.invalid-value` and exit 2 when the value is not a key id of the form the trust list uses (proposed, owner to confirm). Test: pass `--trust abc` and assert exit 2.
- **[INS-005]** `--allow-unsigned` MUST NOT trust any key. A package installed with `--allow-unsigned` MUST show the state of an unsigned package in `ilium plugin list`, and a later update MUST still need its own signature rule (proposed, owner to confirm). Test: install an unsigned package with `--allow-unsigned`, then assert that `ilium plugin info <id>` reports no trusted signer.
- **[INS-006]** `--dry-run` MUST NOT write any file under the plugin root, the store, the registry cache or the grant file. It MUST exit with the code that the real install would give. Test: run `--dry-run` on a package that would succeed and on one that would fail with `package.hash-mismatch`, and compare the file tree before and after each run.
- **[INS-007]** `--locked` MUST fail with `dependency.lock-mismatch` and exit 1 when the package named by the lock file has a different hash, and MUST NOT contact a registry for resolution. Test: change one byte of the package in the registry mirror, run `--locked`, and assert exit 1 and no network request.
- **[INS-008]** `--locked` combined with `--channel` MUST be refused with `usage.conflicting-flags` and exit 2 before any work starts (proposed, owner to confirm). A lock file already fixes the version, so a channel has no effect.
- **[INS-009]** `--locked` combined with a local file or an HTTPS URL MUST be refused with `usage.conflicting-flags` and exit 2 before any work starts (proposed, owner to confirm). The lock file names plugins by id and version, so it cannot pin a file.
- **[INS-010]** `--from-bundle` combined with `--registry` MUST be refused with `usage.conflicting-flags` and exit 2 before any work starts (proposed, owner to confirm). A bundle is the only source for that install, so a second source is ambiguous.
- **[INS-011]** `--from-bundle` MUST read the package and its signed metadata from the bundle directory and MUST NOT open any network connection. Test: block the network, run `install --from-bundle ./bundle com.example.notes`, and assert exit 0 when the bundle is complete.
- **[INS-012]** `--channel` combined with a local file or an HTTPS URL MUST be refused with `usage.conflicting-flags` and exit 2 before any work starts (proposed, owner to confirm). A file has no channel, so the value would be ignored silently.
- **[INS-013]** `--reinstall` combined with `--locked` MUST reinstall the locked package and MUST still fail with `dependency.lock-mismatch` when its hash differs (proposed, owner to confirm). The flag rewrites the store only from the same package bytes.
- **[INS-014]** `--unskip` MUST clear the `skip` mark only for the version being installed. It MUST NOT clear the mark of another version. Test: roll back from 1.3.0 to 1.2.0 so that 1.3.0 is marked `skip`, then install 1.3.0 with `--unskip` and assert that the mark of 1.3.0 is cleared and that no other version changes.
- **[INS-015]** `--allow-downgrade` MUST NOT clear a `skip` mark, because a downgrade installs a version by name and the mark only controls which version update offers (proposed, owner to confirm).
- **[INS-016]** `--no-backup` MUST be accepted only by an update that runs a data migration. On a fresh install it MUST be ignored with a `warning` record and the command MUST still exit 0 (proposed, owner to confirm). Test: install a new plugin with `--no-backup` and assert exit 0 and one warning record.

### Combination table

The table lists the most common combinations. A combination that is not in the table is refused by the rule of [INS-017](#rules-for-each-flag).

| Combination | Result | Basis |
|---|---|---|
| `--yes` alone with a package with normal grants | Installs, no review | [PKG-429](/pages/reference-plugin-distribution-cli/#install) |
| `--yes` with a high-risk grant | Fails, exit 1 | [INS-001](#rules-for-each-flag) |
| `--yes --grant <g>` | Installs when `g` is the only confirmation needed | [PKG-282](/pages/reference-install-and-update/) |
| `--dry-run --yes` | Prints the plan, writes nothing | [PKG-427](/pages/reference-plugin-distribution-cli/#install) |
| `--locked --dry-run` | Prints the lock check, writes nothing | [PKG-428](/pages/reference-plugin-distribution-cli/#install) |
| `--locked --channel beta` | Refused, exit 2 | [INS-008](#rules-for-each-flag) |
| `--from-bundle --registry` | Refused, exit 2 | [INS-010](#rules-for-each-flag) |
| `--unskip --allow-downgrade` | Installs the named older version and clears its mark | [INS-014](#rules-for-each-flag), [INS-015](#rules-for-each-flag) |

- **[INS-017]** A combination of flags that this page does not accept MUST be refused with `usage.conflicting-flags` and exit 2 before any file or network access ([PKG-425](/pages/reference-plugin-distribution-cli/#install)). Test: pass each pair from the refusal rows of this page and assert exit 2 and an unchanged state.

### Worked example: a review that needs a grant

A package `com.example.notes` version 1.3.0 requests the high-risk grant `clipboard.read`. The user runs an unattended install.

1. `ilium plugin install com.example.notes --yes` finds the high-risk grant and stops, because `--yes` does not accept it ([INS-001](#rules-for-each-flag)).
2. The command exits 1 with `install.review-required` ([PKG-430](/pages/reference-plugin-distribution-cli/#install)). Nothing is written.
3. The administrator adds the grant to the command: `ilium plugin install com.example.notes --yes --grant clipboard.read`.
4. The grant is accepted by name ([INS-002](#rules-for-each-flag)). The install runs the normal transaction and exits 0.

### Edge cases

- A flag given twice: the last value wins for a single-value flag (`--channel`, `--registry`, `--from-bundle`). A repeated `--grant` adds each named grant. A repeated `--trust` trusts each key. A repeated single-value flag keeps the last value (proposed, owner to confirm). Test: pass `--channel beta --channel stable` and assert that the channel is `stable`.
- A flag name with a typo: the command fails with `usage.unknown-flag`, exit 2, before any work.
- Ctrl+C during a `--dry-run`: exit 130 and no file changed ([PKG-424](/pages/reference-plugin-distribution-cli/#exit-codes)).

### See also

- [Worked examples for install, update and rollback](/pages/install-worked-examples/)
- [Install and update](/pages/reference-install-and-update/)
- [Distribution commands](/pages/reference-plugin-distribution-cli/)
- [Signing and trust](/pages/reference-signing-and-trust/)
