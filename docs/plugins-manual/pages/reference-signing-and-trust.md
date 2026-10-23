---
layout: page
title: Signing and trust reference
permalink: /pages/reference-signing-and-trust/
---

## Signing and trust reference

This page specifies how a package is signed, how the host decides whom to trust, and what happens when a key changes or is revoked. It replaces the short description that older pages gave. A developer can implement the verifier from it. A test author can use the known-answer vector at the end.

A **publisher** is the person or organisation that releases a plugin. A **publisher id** is the machine-readable name of that publisher. A **key** is an Ed25519 key pair. The private half signs. The public half verifies. A **fingerprint**, also called the key id, is a short name for a public key. The **trust store** is the list of keys that a user or an administrator accepts. A **tier** is the trust level that the host assigns to a package at install time: T1 (bundled), T2 (signed by a trusted key) or T3 (unsigned).

Requirement ids such as `PKG-200` can be cited by tests. See [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

### You will need

- The package layout and the `signature` entry position: [Package format reference](/pages/reference-package-format/).
- The command syntax: [Plugin command line](/pages/reference-plugin-cli/).
- The user-facing dialog: [Finding and installing plugins](/pages/user-plugin-store/).

## Decision summary

| Question | Answer |
|---|---|
| Algorithm | Ed25519 (RFC 8032), pure mode, not prehashed |
| What is signed | A fixed message built from a domain prefix, the publisher id and the SHA-256 of the exact bytes of `manifest.json` |
| Canonical form | None. The exact stored bytes are signed. No canonicalisation step exists |
| Who is trusted | A key, never a free-text name. Each key belongs to exactly one publisher id |
| Trust on first use | Allowed only with an explicit user action that shows the full fingerprint |
| Key change | An update signed by another key is refused unless a valid rotation statement or earlier trust exists |
| T1 root | The release build. Bundled components are checked against a hash list built into the binary |

## The signed message

| Id | Rule |
|---|---|
| [PKG-200] | A signature MUST be an Ed25519 signature (RFC 8032, "pure" Ed25519) over the message `M` below. Ed25519ph and Ed25519ctx MUST NOT be used. |
| [PKG-201] | `M` is the concatenation of: the 25 ASCII bytes `ilium-plugin-signature-v1`, one `0x00` byte, the UTF-8 bytes of the publisher id, one `0x00` byte, and the 32 raw bytes of the SHA-256 of `manifest.json`. |
| [PKG-202] | The SHA-256 is computed over the exact bytes of the `manifest.json` entry as stored in the archive. The host MUST NOT parse, normalise or re-serialise the manifest before hashing. |
| [PKG-203] | A verifier MUST reject a signature when the public key is not a valid Ed25519 point or when the signature is not canonical (S not smaller than the group order). RFC 8032 section 5.1.7 rules apply. |

The domain prefix means that a signature made for a package cannot be replayed in another protocol that also uses Ed25519. The publisher id inside the message means that a key cannot be reused to claim a different publisher.

The manifest holds the SHA-256 of every other entry (the inventory). So the signature covers every listed file, transitively. The `signature` entry itself is not listed, and nothing else is exempt: the package format allows no unlisted entry.

Signing is not a hash of the whole archive. The archive bytes change when the `signature` entry is added, so the hash of the archive cannot be signed. That is why the manifest bytes are signed.

## The signature entry

The entry is named `signature` and holds a UTF-8 JSON document.

```json
{
  "format": "ilium-sig/1",
  "signatures": [
    {
      "algorithm": "ed25519",
      "key_id": "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c",
      "publisher_id": "example",
      "signature": "jv5zcDEiiDljfN-iy3nZLtCgScmbTMJoOv0M3X9r0D9Tm4dvdqE43gpEEAW_cqVod6iYQAjeIR8E0-vFwEplBw",
      "signed_at": "2026-10-12 10:00:00"
    }
  ]
}
```

| Id | Rule |
|---|---|
| [PKG-204] | The document MUST have exactly the keys `format` and `signatures`. `format` MUST be `ilium-sig/1`. |
| [PKG-205] | `signatures` MUST hold 1 to 4 objects. Each has exactly the keys `algorithm` (`ed25519`), `key_id`, `publisher_id`, `signature` and `signed_at`. |
| [PKG-206] | `key_id` is 64 characters of lowercase hexadecimal. `signature` is the 64 signature bytes in base64url without padding (86 characters). `signed_at` is `YYYY-MM-DD HH:mm:ss` in UTC and is informational: the host never uses it for a decision. |
| [PKG-207] | The first object is the publisher signature. A second object MAY be a registry countersignature (see [Registry reference](/pages/reference-registry/)). The host verifies every object and requires at least one valid object from a trusted key. |
| [PKG-208] | The entry is at most 16 KiB. A document that does not match these rules is refused with `package.signature-format`. A signature that does not verify is refused with `manifest.signature-invalid`. A `key_id` that does not match any key that verifies the signature is `manifest.signature-invalid`. |
| [PKG-209] | The entry sorts like any other entry (bytewise by path). Adding it keeps the archive valid and deterministic ([PKG-148]). |
| [PKG-650] | The signature document is serialised with the rules of [PKG-145]: UTF-8 without a byte order mark, 2-space indentation, LF line endings and one final LF. Keys appear in table order: `format`, `signatures`; in each object `algorithm`, `key_id`, `publisher_id`, `signature`, `signed_at`. Two runs of `sign` with the same key, manifest and `signed_at` MUST give byte-identical packages. |
| [PKG-651] | `signed_at` is taken by `sign` from `--signed-at <time>` (the form `YYYY-MM-DD HH:mm:ss`, in UTC), else from the environment variable `SOURCE_DATE_EPOCH` (seconds since 1970-01-01 UTC), else from the clock. The test vector below was made with `--signed-at "2026-10-12 10:00:00"`. |

## Keys and key files

`ilium plugin keygen --publisher-id <id> --publisher "<name>" --out <dir>` writes two files and refuses to overwrite an existing file (`trust.key-exists`).

**publisher.key** is the private key.

| Id | Rule |
|---|---|
| [PKG-210] | `publisher.key` MUST be a PKCS#8 (RFC 5958) private key in PEM, with the label `PRIVATE KEY`, or `ENCRYPTED PRIVATE KEY` when protected by a passphrase. |
| [PKG-211] | An encrypted key uses PBES2 with PBKDF2-HMAC-SHA256 (at least 600,000 iterations) and AES-256-CBC. `keygen` encrypts when `--passphrase-file` or the environment variable `ILIUM_KEY_PASSPHRASE` is given or when the user types a passphrase at the prompt. An empty passphrase is refused. |
| [PKG-212] | `keygen` MUST create the file with mode 0600 and the folder with mode 0700. `sign` MUST refuse a key file that is readable by group or others (`sign.key-permissions`), on systems that have such modes. |
| [PKG-213] | `sign --key -` reads the PEM from standard input. The environment variable `ILIUM_PUBLISHER_KEY` MAY hold the PEM text for CI use. The host never writes a private key to a log. |

**publisher.pub** is the public key. It is safe to share.

```json
{
  "format": "ilium-pub/1",
  "publisher_id": "example",
  "publisher": "Example Labs",
  "algorithm": "ed25519",
  "public_key": "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg",
  "key_id": "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c",
  "created": "2026-10-12 10:00:00"
}
```

| Id | Rule |
|---|---|
| [PKG-214] | `public_key` is the 32 raw bytes in base64url without padding. `key_id` MUST equal the lowercase hexadecimal SHA-256 of those 32 bytes. A file with another `key_id` is refused with `trust.key-malformed`. |
| [PKG-215] | `publisher_id` follows the grammar below. `publisher` is a display name of 1 to 64 characters with no control and no bidirectional-override characters. |
| [SEC-722] | Signing keys MUST be Ed25519 with a validity of at most 2 years (proposed, owner to confirm); the registry MUST refuse a publish signed with an expired key, and the CLI MUST warn from 60 days (proposed, owner to confirm) before expiry with a command to rotate. |

**Fingerprint display.** The stored form is the 64-character hex string. The display form groups it in 16 groups of 4 characters separated by single spaces, and the dialog splits it into two lines of 8 groups. [PKG-216] A dialog or prompt that asks the user to trust a key MUST show all 64 characters. It MUST NOT show a truncated fingerprint as the only identification. A short form (the first 8 characters) MAY appear next to the full form in lists.

### Signing with a hardware token or an external signer

A file key on disk can be copied. A publisher who must keep the private key in a hardware token, a smart card or a cloud key management service (KMS) uses an **external signer**: a program that holds the key and signs on request. The private key never reaches the `ilium` process. [SEC-174] advises protecting the publisher key. [SEC-728] requires hardware custody for the Ilium release key. This section lets any publisher follow the same practice.

`ilium plugin sign <package> --signer <command> --public-key <publisher.pub>` selects the external signer. The environment variable `ILIUM_SIGNER` holds the same command for CI use. `--signer` wins over `ILIUM_SIGNER`.

| Id | Rule |
|---|---|
| Rule | The signer command is read from `--signer` or from `ILIUM_SIGNER`. Credentials for a KMS or a token PIN are never accepted as `ilium` flags; the signer program reads them from its own environment or prompt. As for the registry token ([PKG-442], [PKG-446]), a PIN, token or secret variable never appears in a log, record or error message. |
| Rule | `--signer` excludes `--key` and `ILIUM_PUBLISHER_KEY`. Giving a signer together with either is refused with `sign.key-conflict`. With neither a key nor a signer, `sign` refuses with `sign.no-key`. |

The contract between `ilium` and the signer is fixed:

1. `ilium` builds the message `M` of [PKG-201] exactly as for a file key.
2. It starts the signer command without a shell. The command text is split into arguments with POSIX shell quoting rules, and the program is found through `PATH`. The signer runs with the environment of `sign` minus `ILIUM_PUBLISHER_KEY` and `ILIUM_KEY_PASSPHRASE`.
3. It writes the exact bytes of `M` to the signer's standard input and closes it. `M` is not hashed first: the signer receives the message of pure Ed25519 ([PKG-200]), not a digest.
4. It reads the standard output until the signer exits. Exactly 64 raw bytes (not hex, not base64) are the signature. Standard error is shown to the user as diagnostic text and is never parsed.

A PKCS#11 module is used through a signer program that wraps it (for example one that calls `C_Sign` with the `CKM_EDDSA` mechanism). The host does not load a PKCS#11 library itself, so no third-party native code runs inside `ilium`.

| Id | Rule |
|---|---|
| Rule | The public key file is mandatory with `--signer` (`sign.public-key-required` when it is missing). It MUST pass [PKG-214]. The `key_id` and the `publisher_id` of the signature object come from this file, never from the signer. A publisher whose version should reach review level B ([PKG-680] lists the level B inputs) SHOULD keep the key in hardware, because review level B is the level that users rely on most. The registry shows no difference between a file key and a hardware key, so this is a publisher practice and not a registry check. |
| Rule | Before it writes the package, `sign` verifies the returned 64 bytes against `M` with the public key from `--public-key`, using the strict rules of [PKG-203]. A signer that returns a signature for another key, another message or a non-canonical value is caught here. |
| Rule | The signer is refused with `sign.signer-failed` when it cannot be started, exits with a non-zero status, writes other than 64 bytes, does not finish within 120 seconds (the process is then killed), or returns a signature that does not verify. The message names the reason in plain words and the signer's exit status. No package is written and no partial file remains. |

The signing time follows [PKG-651]. An external signer does not set `signed_at`; `sign` does. Two runs with the same signer, `--signed-at` and manifest give byte-identical packages, because Ed25519 signatures are deterministic (a signer that adds randomness breaks reproducibility but not validity).

The user sees one line while the signer runs, `Waiting for the signer ...`, so that a touch prompt on a token is not mistaken for a hang. In a non-interactive run the 120 second limit applies in the same way.

## Publisher identity

The `publisher` field in the manifest is a display name. Anyone can write any display name. The identity that is checked is the publisher id and the key.

| Id | Rule |
|---|---|
| [PKG-217] | A publisher id matches `[a-z][a-z0-9]*(-[a-z0-9]+)*(\.[a-z][a-z0-9]*(-[a-z0-9]+)*){0,3}`, is 1 to 64 characters long and has at most 4 segments. |
| [PKG-218] | A signed package's plugin `id` MUST start with `<publisher_id>.` where `publisher_id` is the one in the signature. Otherwise the install is refused with `manifest.id-publisher`. Example: publisher id `com.acme` signs `com.acme.notes`. |
| [PKG-219] | The manifest `publisher` text MUST equal (after Unicode NFC normalisation, case-sensitive) the display name stored with the key in the trust store. A mismatch is refused with `manifest.publisher-mismatch`. |
| [PKG-220] | The prefix `ilium.` is reserved for the release. No key can be trusted for the publisher id `ilium`, `core`, `local` or `test`. Adding such a key is refused with `trust.publisher-reserved`. |

Registry names add homoglyph checks. See [Registry reference](/pages/reference-registry/#namespaces).

## The trust store

| Id | Rule |
|---|---|
| [PKG-221] | The user store is the file `trust.json` in the Ilium user state folder (`$XDG_STATE_HOME/ilium/trust.json`; `~/.local/state/ilium/trust.json` on Linux when `XDG_STATE_HOME` is unset). It is created with mode 0600. Writes are atomic: write a temporary file in the same folder, fsync, rename. |
| [PKG-222] | The system store is `/etc/ilium/trust.json` on Linux (the platform equivalent through the same crate elsewhere). It is read-only for the host. An administrator owns it. |
| [PKG-223] | The effective trust set is the union of both stores minus every revoked key. A key revoked in either store is revoked. A user cannot remove or override a system entry. |
| [PKG-224] | Each store entry binds one `key_id` to exactly one `publisher_id`. A second entry with the same `key_id` and another publisher id is refused with `trust.publisher-conflict`. One publisher id MAY have several keys (needed for rotation). |

Store format:

```json
{
  "format": "ilium-trust/1",
  "keys": [
    {
      "key_id": "56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c",
      "publisher_id": "example",
      "publisher": "Example Labs",
      "public_key": "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg",
      "added": "2026-10-12 10:05:00",
      "added_by": "user",
      "status": "trusted",
      "revoked": null
    }
  ]
}
```

`added_by` is `user`, `admin` or `registry`. `status` is `trusted` or `revoked`. `revoked` is null or an object `{ "reason", "at" }`. An unknown `format` value refuses the whole file with `trust.store-invalid`, and the host then treats the store as empty and shows a warning. It never guesses.

### Trust commands

| Command | Effect |
|---|---|
| `ilium plugin trust add <publisher.pub> [--yes]` | Shows the full fingerprint and asks. `--yes` skips the question. The file must pass [PKG-214] |
| `ilium plugin trust add --fingerprint <key_id> <package>` | Trusts the key that signed the package, only if its `key_id` equals the one given. Use this in scripts |
| `ilium plugin trust list [--json]` | Lists the effective set with `key_id`, publisher id, status and source (user or system) |
| `ilium plugin trust remove <key_id> [--force]` | Removes a user entry. See below |
| `ilium plugin trust revoke <key_id> --reason <reason>` | Writes the key as revoked in the user store; `reason` is `compromised`, `superseded` or `retired` [PKG-243] |
| `ilium plugin trust export <key_id>` | Writes the `publisher.pub` form to standard output |

| Id | Rule |
|---|---|
| [PKG-225] | `ilium plugin install --trust <key_id>` trusts that exact key for this install and for later ones. It is the non-interactive way to trust a key while installing; outside an install, `trust add --fingerprint <key_id> <package>` and `trust add <publisher.pub> --yes` are the non-interactive ways. A package signed by another key is refused with `trust.key-unknown`. |
| [PKG-226] | `trust remove` of a key that signed installed plugins MUST be refused with `trust.in-use` and list the plugins, unless `--force` is given. With `--force` the plugins change to the package state `disabled-untrusted` ([plugin lifecycle states](/pages/reference-plugin-lifecycle-states/#package-states)): they stay installed, are stopped, keep their grants and settings, and show a badge. They leave that state when the key is trusted again, or with `ilium plugin enable <id> --accept-risk`. |
| [PKG-227] | `trust remove` MUST NOT delete any plugin data. `trust remove` of a system entry is refused with `trust.system-entry`. |
| [SEC-721] | The host MUST detect modification of the trust store and the grant file between reads by comparing a keyed MAC (key kept in the user's OS secret store, falling back to a 0600 file) and, on mismatch, MUST refuse T2 and T3 plugins with `trust.store-tampered`, preserve the file as `.tampered-<timestamp>` and tell the user. |

## Tier assignment

| Id | Rule |
|---|---|
| [PKG-228] | A package with a valid signature from a trusted, non-revoked key is tier T2. |
| [PKG-229] | A package with no `signature` entry is tier T3. It installs only with `--allow-unsigned`, or through `ilium plugin dev`. Without either it is refused with `install.unsigned`. |
| [PKG-230] | A package with a valid signature from a key that is not in the trust store prompts the user with the key details. If the user accepts, the key is added with `added_by: user` and the install continues as T2. If the user declines, the install stops with `trust.declined`. In a non-interactive run without `--trust`, it stops with `trust.key-unknown`. |
| [PKG-231] | A package with an invalid signature is refused with `manifest.signature-invalid`, whatever the user could choose. There is no flag that installs it. |
| [PKG-232] | A package signed by a revoked key is refused with `trust.key-revoked`. |
| [PKG-233] | T1 is not a trust-store tier. A T1 component is a component embedded in the Ilium release. At build time the release records the SHA-256 of each embedded component in a table inside the binary. At load time the host compares the embedded bytes with the table. A mismatch disables the plugin with `trust.bundled-tampered`. |
| [PKG-234] | A package from any source whose `id` starts with `ilium.` is refused with `manifest.id-reserved`, even when it carries a valid signature. Core plugins change only with a new Ilium release. |
| [PKG-235] | The declared tier never comes from the manifest. There is no manifest key for it. |

<figure>
<pre class="term"><span class="t-warn">Unknown signing key</span>
Publisher  Example Labs (example)
Package    example.notes 1.2.0

Fingerprint
5647 5aa7 5463 474c 0285 df5d bf2b cab7
3da6 5135 8839 e9b7 7481 b2ea b107 708c

Ask the publisher for this fingerprint
over another channel before you trust it.

[ Trust key ]  [ Cancel ]
</pre>
<figcaption>Rendered at 48 columns. Illustrative. The trust prompt for an unknown key. All 64 characters are shown.</figcaption>
</figure>

The dialog splits the 16 groups of 4 into two lines of 8 groups. Command output prints the same groups on one line. Both are the same value.

## Publisher key change and rotation

A package update is signed by the same key in nearly every case. The next rules cover the exceptions.

| Id | Rule |
|---|---|
| [PKG-236] | When a plugin is installed and an update is signed by a key with another `key_id` than the installed version's key, the host checks the trust store. If the new key is already trusted for the same publisher id, the update continues. |
| [PKG-237] | Otherwise the update MUST be refused with `package.publisher-key-changed`, and the message names both fingerprints. No dialog can override this inside `install`. The user changes the situation first, by importing a rotation statement or by running `trust add --additional`. This decision keeps a stolen distribution channel from replacing a key through a prompt that users click through. |
| [PKG-238] | A **rotation statement** is the document `rotation.json`: `{ "format": "ilium-rot/1", "publisher_id", "old_key_id", "new_key": { "public_key", "key_id" }, "signature", "created" }`. `signature` is made by the old key over the message `ilium-key-rotation-v1`, `0x00`, the publisher id, `0x00`, the 32 raw bytes of the new `key_id`. |
| [PKG-239] | `ilium plugin trust add --rotation <rotation.json>` verifies the statement against the old key in the store. If valid, it adds the new key with `added_by: user` for the same publisher id, and keeps the old key as `trusted` until the user or the publisher revokes it. |
| [PKG-240] | `ilium plugin trust add --additional <publisher.pub>` adds a second key for a publisher id that already has a key. It shows the old and the new fingerprints and requires typing the word `replace` in an interactive session. |
| [PKG-241] | A rotation statement MUST be signed by a key that is currently trusted for the same publisher id. Otherwise `trust.rotation-invalid`. |

## Revocation and distrust

| Id | Rule |
|---|---|
| [PKG-242] | A **revocation statement** is the document `{ "format": "ilium-rev/1", "publisher_id", "key_id", "reason", "effective", "signature" }`. `reason` is `compromised`, `superseded` or `retired`. `effective` is `YYYY-MM-DD HH:mm:ss` in UTC. `signature` is made by the revoked key itself, or by the new key named in a rotation statement, over `ilium-key-revocation-v1`, `0x00`, the publisher id, `0x00`, the 32 raw bytes of `key_id`, `0x00`, the reason text. |
| [PKG-243] | `ilium plugin trust revoke <key_id> --reason <reason>` writes the key as revoked in the user store. A registry (when configured) distributes revocations in its metadata. See [Registry reference](/pages/reference-registry/#revocation). The administrator also revokes keys in the system store. |
| [PKG-244] | New installs of a package signed by a revoked key are refused with `trust.key-revoked`, for every reason. |
| [PKG-245] | Installed plugins signed by a key revoked as `compromised` are stopped at once, enter the package state `disabled-blocked` and show the reason. They leave that state only with `ilium plugin enable <id> --accept-risk`, or with an update signed by another trusted key of the same publisher id. |
| [PKG-246] | Installed plugins signed by a key revoked as `superseded` or `retired` keep running at T2 with the badge `key retired`. Updates must come from a trusted key of the same publisher id. |
| [PKG-247] | The host checks the revocation lists whenever the trust stores or the registry metadata change, and at every start. A revocation with an `effective` time in the future applies when that time passes. |

### A compromised key: the publisher runbook

1. Stop signing with the key. Take the signing machine or CI secret out of use.
2. Generate a new key with a new `key_id` and the same publisher id (`keygen`).
3. Publish a revocation statement of the old key with reason `compromised`, signed by the new key if the old key is not safe to use, together with a rotation statement if the old key is still under your control. Send both to the registry and place them on your site.
4. Release a new version of each plugin signed by the new key.
5. Tell your users which versions are affected. Every version signed with the old key after the compromise time is suspect.

If the private key is lost and no copy exists, there is no recovery. Nobody can sign a rotation statement. The publisher must start a new publisher id (and so new plugin ids), and users must trust the new key. This is the cost of having no central account.

## Compared with other schemes

| Scheme | Binding | Why not used here |
|---|---|---|
| Free-text publisher name | None | Anyone can write any name |
| X.509 certificate authorities | A third party vouches for a name | Needs paid infrastructure and a policy for the CA |
| Sigstore keyless signing | An identity provider vouches | Needs network access at install time |
| This design | Key, publisher id and user or admin decision | Works offline. The weakest point is the first trust decision |

## Codes and dialog wording

| Code | Message template |
|---|---|
| `manifest.signature-invalid` | `The signature of {package} does not match its manifest.` |
| `package.signature-format` | `The signature entry of {package} is not a valid signature document: {detail}.` |
| `trust.key-unknown` | `{package} is signed by key {key_id}, which is not trusted. Use --trust {key_id} to trust it.` |
| `trust.declined` | `Key {key_id} was not trusted. Nothing was installed.` |
| `install.unsigned` | `{package} is not signed. Use --allow-unsigned to install it as a dev (T3) package.` |
| `trust.key-revoked` | `Key {key_id} was revoked ({reason}). Nothing was installed.` |
| `package.publisher-key-changed` | `{id} was signed by {old_key_id}. This update is signed by {new_key_id}. Import a rotation statement first.` |
| `manifest.publisher-mismatch` | `The package says publisher "{manifest_publisher}". The key belongs to "{key_publisher}".` |
| `manifest.id-publisher` | `The id {id} must start with {publisher_id} because the package is signed for that publisher.` |
| `trust.in-use` | `Key {key_id} signed {count} installed plugins: {ids}. Use --force to continue.` |
| `trust.bundled-tampered` | `The bundled plugin {id} does not match this Ilium release.` |

## Test vector

A verifier MUST reproduce the following values. Ed25519 signatures are deterministic (RFC 8032), so any conforming library gives the same 64 bytes for the same seed and message. The values were computed with a reference implementation and checked independently with OpenSSL 3.5.5: the seed is wrapped as a PKCS#8 key (the 16 bytes `302e020100300506032b657004220420` followed by the seed), `openssl pkey -pubout` gives the public key, and `openssl pkeyutl -sign -rawin -inkey <key> -in <M file>` gives the signature.

| Item | Value |
|---|---|
| Private seed (32 bytes) | `000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f` |
| Public key (32 bytes) | `03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8` |
| `key_id` (SHA-256 of the public key) | `56475aa75463474c0285df5dbf2bcab73da651358839e9b77481b2eab107708c` |
| Publisher id | `example` |
| Manifest | the 402 bytes of the [package format vector](/pages/reference-package-format/#test-vector-a-minimal-package) |
| SHA-256 of the manifest | `350b5e47c5781e72a9bb7b3d6630335fca0390402d0f625a48857a234f99c99d` |
| Message `M` (hex) | `696c69756d2d706c7567696e2d7369676e61747572652d7631006578616d706c6500350b5e47c5781e72a9bb7b3d6630335fca0390402d0f625a48857a234f99c99d` |
| Signature (64 bytes, hex) | `8efe737031228839637cdfa2cb79d92ed0a049c99b4cc2683afd0cdd7f6bd03f539b876f76a138de0a441005bf72a56877a8984008de211f04d3ebc5c04a6507` |
| Signature (base64url) | `jv5zcDEiiDljfN-iy3nZLtCgScmbTMJoOv0M3X9r0D9Tm4dvdqE43gpEEAW_cqVod6iYQAjeIR8E0-vFwEplBw` |

The signed package (1,114 bytes, SHA-256 `d2238076eea48c58ceb4416dbb50438c6ffee23028051d7e9a0cb1f9ae7fc408`) was made with `ilium plugin sign --signed-at "2026-10-12 10:00:00"` and the serialisation of PKG-650. It holds the entries `manifest.json`, `server.wasm` and `signature`, with the signature document shown in the section [The signature entry](#the-signature-entry).

### Negative vectors

Each row changes the vector and expects a refusal.

| Id | Change | Expected |
|---|---|---|
| [PKG-248] | One byte of `manifest.json` changed (and the CRC repaired) | `manifest.signature-invalid` |
| [PKG-249] | One byte of `server.wasm` changed, manifest unchanged | `manifest.file-hash` (stage 5, before the signature) |
| [PKG-250] | One byte of the signature value changed | `manifest.signature-invalid` |
| [PKG-251] | The prefix `ilium-plugin-signature-v2` used to sign | `manifest.signature-invalid` |
| [PKG-252] | The same key signs with publisher id `other` but the document says `example` | `manifest.signature-invalid` |
| [PKG-253] | The document lists `publisher_id: "other"` and the plugin id is `example.minimal` | `manifest.id-publisher` |
| [PKG-254] | `format` is `ilium-sig/2` | `package.signature-format` |
| [PKG-255] | A key not in the trust store, non-interactive, no `--trust` | `trust.key-unknown` |
| [PKG-256] | The key is in the store with display name "Other Labs" | `manifest.publisher-mismatch` |
| [PKG-257] | The key is revoked as `compromised` | `trust.key-revoked` |
| [PKG-258] | An installed `example.minimal` signed by key A, an update signed by key B, no rotation | `package.publisher-key-changed` |

## Signature validity windows and clock skew

This section is target behaviour for the public plugin system. A validity window is the time between a signature's `not-before` and `not-after`. The host reads its own clock to check it, and that clock can be wrong.

- [PKG-712] The host MUST allow a clock skew of 10 minutes at both ends of a validity window. Outside it, the host MUST refuse the signature with `trust.signature-expired` or `trust.signature-not-yet-valid`, and MUST NOT take the time from the package or from an unsigned source.
- [PKG-713] When the host clock differs by more than 24 hours from the signed time of the registry metadata, the host MUST report `trust.clock-skew` with the measured difference and MUST refuse new installs and updates, but MUST keep already installed plugins running. The host MUST NOT disable a plugin only because its validity window ended after install.

### Trusted signing time for validity windows and key expiry

Two statements elsewhere in the manual seemed to disagree: [PKG-206] says the host never uses `signed_at` for a decision, and [EXT-071] says the host verifies against the signing time recorded in the signature. This section settles it.

**[PKG-206] is normative.** The value `signed_at` is written by the signer and covered by nothing: [PKG-201] signs the publisher id and the manifest hash only, not the time. A holder of a stolen key can write any `signed_at`, so the host never trusts it. [EXT-071] is to be read as "the trusted signing time defined below, not the current clock". The wording of [EXT-071] in [Requirements: lifecycle operations](/pages/reference-requirements-lifecycle-ops/) and in [Trust and supply chain](/pages/reference-trust-and-supply-chain/#clock-skew-and-time-based-trust) must be amended to refer to this section.

| Id | Rule |
|---|---|
| [PKG-207] | The only trusted time source is the registry countersignature. Its time is the `published_at` that the registry returns on publish ([PKG-809]) and writes into the countersignature object. The host calls this the **trusted signing time**. |
| [SEC-722] | Key expiry is judged at the trusted signing time. A key is acceptable when the trusted signing time is not later than its expiry. A signature with a trusted signing time after the key's expiry is refused with `trust.signature-expired`. |
| [PKG-712] | A validity window is judged against the trusted signing time with the 10 minute skew of this page. A time before the window is `trust.signature-not-yet-valid`, a time after it is `trust.signature-expired`. |

What the host uses in each case:

| Case | Time used | Result |
|---|---|---|
| Package from a registry, countersignature present and valid | `published_at` of the countersignature | Key expiry and windows are judged at that time |
| Package from a registry, no countersignature | None | Refused with `manifest.signature-invalid`. A registry install requires the countersignature of [PKG-207] |
| Package from a file or URL, countersignature present and valid | `published_at` of the countersignature | As for a registry package |
| Package from a file or URL, no countersignature | The host clock at install, stored as `installed_at` in `installed.json` ([Install, update and uninstall](/pages/reference-install-update-uninstall/#the-registry-file)) | Key expiry and windows are judged at that install time. A key that is already expired at install time is refused with `trust.signature-expired`. `signed_at` is shown to the user as the publisher's claim and is never compared |
| The key is revoked as `compromised` | Not time based | `trust.key-revoked` at install, whatever any time says ([PKG-244]). A claimed `signed_at` before the revocation never rescues the signature |

Rules after install:

- A key that expires after a file install does not stop the installed plugin and does not change its tier. The decision was made at install time. An update must itself pass the table above with its own trusted time.
- A host clock that differs from the registry metadata time by more than 24 hours stops new installs and updates with `trust.clock-skew` ([PKG-713]). For a file install this check is made against the newest registry metadata the host holds; with none, the host clock is accepted and the install is marked in `installed.json` by `source: file`.
- Because a file install trusts the local clock, a user who sets the clock back before an install can install a package signed by an expired key. This is a local choice by the owner of the machine, not an attack by the publisher. A package with a countersignature is not affected, because its time comes from the registry.

Nothing in the first case uses `signed_at`. A tool may print it, but a test MUST NOT make any pass or fail depend on it.

## Edge cases

| Situation | Result |
|---|---|
| Two valid signatures, one trusted and one not | The trusted one decides. The package is T2 |
| The same key listed twice in `signatures` | Refused with `package.signature-format` |
| The trust store file is unreadable | The effective set from that store is empty. Signed packages become untrusted. A warning is shown |
| The clock is wrong | No decision depends on `signed_at`. The clock matters for a future `effective` time of a revocation, for the install time of a file install without countersignature, and for `trust.clock-skew` (see the trusted signing time section) |
| The publisher edits the manifest after signing | The signature fails. Pack again, then sign again |

## See also

- [Package format reference](/pages/reference-package-format/)
- [Install, update and uninstall](/pages/reference-install-and-update/)
- [Registry reference](/pages/reference-registry/)
- [Administrator guide](/pages/guide-plugin-administration/)
- [Publishing guide](/pages/guide-publishing/)
- [Security model](/pages/security/)


### Cryptographic agility

- [SEC-459] The host MUST accept signature and digest algorithms only from a closed, versioned allow-list compiled into the binary, MUST treat an unknown algorithm in a key, signature or digest entry as a verification failure rather than a skip, MUST refuse a retired algorithm for any metadata or package issued after the registry marks it retired, and MUST announce a new algorithm in signed registry metadata at least one release before it is required, accepting dual signatures during the overlap and writing an audit event whenever a deprecated algorithm is still used.
