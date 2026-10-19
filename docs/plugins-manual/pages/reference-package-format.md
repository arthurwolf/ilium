---
layout: page
title: Package format reference
permalink: /pages/reference-package-format/
---

## Package format reference

This page is the byte-level specification of the `.iliumpkg` file for contract `ilium:plugin@0.1.0`. A host developer can write a reader from it. A tool author can write a packer. A test author can derive hostile-archive tests from it. Statements that tests can cite carry a requirement id such as `PKG-101`. The full list, with test methods, is in [Requirements: manifest and package](/pages/reference-requirements-manifest-package/).

The words MUST, MUST NOT, SHOULD and MAY are used in the sense of RFC 2119.

A **package** is one file that holds one plugin version. A **component** is a WebAssembly component, the compiled code for one side of the plugin. The **inventory** is the `files` list in the manifest: one line per other entry, with its length and SHA-256 hash. The **manifest** is the `manifest.json` entry, described in [Manifest reference](/pages/reference-manifest/).

### You will need

- The layout overview in [Packages and manifests](/pages/packages/).
- The signing rules in [Signing and trust](/pages/reference-signing-and-trust/), because the signature is one entry of the archive.
- The error codes are collected, with message templates, in [Manifest schema and error catalogue](/pages/reference-manifest-schema/).

## Design goals

The format has three goals. The host must be able to refuse a hostile file before it runs any plugin code. Two builds of the same sources must give the same bytes. A package must install the same way on every operating system.

To reach them the format is a small subset of ZIP. The subset removes compression, extra fields, comments, directory entries and everything else that two ZIP libraries read differently.

## The container

The container is a ZIP archive as described by the PKWARE APPNOTE 6.3.x, restricted as follows.

| Id | Rule |
|---|---|
| [PKG-100] | Every entry MUST use compression method 0 (stored). Any other method is refused with `package.zip-compressed`. |
| [PKG-101] | General-purpose flag bit 11 (UTF-8 names) MUST be set on every entry. Bit 0 (encryption) and bit 3 (data descriptor) MUST NOT be set. They are refused with `package.zip-encrypted` and `package.zip-descriptor`. All other flag bits MUST be zero. |
| [PKG-102] | The archive MUST NOT use ZIP64. A ZIP64 end record, a ZIP64 locator or a size field of `0xFFFFFFFF` is refused with `package.zip64`. |
| [PKG-103] | Local headers and central directory headers MUST NOT carry extra fields. The extra field length MUST be 0. Refused with `package.zip-extra`. |
| [PKG-104] | The end-of-central-directory record MUST have a comment length of 0. No per-entry comment is allowed. Refused with `package.zip-comment`. |
| [PKG-105] | The archive MUST NOT contain directory entries (names ending in `/`). Refused with `package.entry-directory`. |
| [PKG-106] | The DOS modification time of every entry MUST be 1980-01-01 00:00:00 (time field `0x0000`, date field `0x0021`). |
| [PKG-107] | The external attributes of every entry MUST be `0x81A40000` (Unix mode `0100644`, a regular file with mode 0644). A symbolic link, a device or any other mode is refused with `package.entry-symlink`. |
| [PKG-108] | The "version made by" field MUST be `0x0314` (Unix, specification 2.0) and "version needed to extract" MUST be 20. The disk number fields MUST be 0 and the internal attributes MUST be 0. |
| [PKG-109] | For every entry the local header and the central directory header MUST agree on the name, the CRC-32, the compressed size and the uncompressed size. For a stored entry the two sizes are equal. A mismatch is refused with `package.header-mismatch`. |
| [PKG-110] | The CRC-32 of every entry MUST be correct. A wrong CRC is refused with `package.crc-mismatch`. The host checks it while it reads the entry, before it parses the content. |
| [PKG-111] | The archive MUST consist of the local entries, then the central directory, then the end record, with no other bytes. Bytes before the first local header, between records or after the end record are refused with `package.zip-trailing`. |
| [PKG-112] | The local entries MUST be adjacent and in the same order as the central directory, and their byte ranges MUST NOT overlap. A central directory entry whose local header offset does not point to the next unread byte is refused with `package.header-mismatch`. |
| [PKG-113] | The count of entries in the end record MUST equal the count of central directory headers, and the central directory size and offset MUST match the real positions. A truncated or inconsistent central directory is refused with `package.zip-invalid`. |
| [PKG-114] | The reader MUST locate the end record by reading the last 22 bytes of the file. Because the comment length is 0, the end record always starts exactly 22 bytes before the end. A file in which that position does not hold the signature `PK\x05\x06` is refused with `package.zip-invalid`. |

A reader MUST NOT fall back to a more lenient parse when one of these rules fails. A package either meets the subset or is refused.

### Why stored and not compressed

Deflate output depends on the compressor version, so two correct tools give different bytes. A stored archive does not. WebAssembly components compress poorly after a good optimiser run, and the size cap in this page is large enough for that. A transport may compress the file (HTTPS does so already). The format does not.

### Entry layout on disk

The following diagram shows the byte order of a package with two entries.

<figure>
<pre class="term">offset  record
0       local header "manifest.json" + data
...     local header "server.wasm"   + data
...     central header "manifest.json"
...     central header "server.wasm"
end-22  end record (22 bytes, comment length 0)
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Records of a two-entry package in file order.</figcaption>
</figure>

Header field layouts follow APPNOTE sections 4.3.7 (local), 4.3.12 (central) and 4.3.16 (end record). All integers are little-endian. The 30-byte local header and the 46-byte central header are written with these values:

| Field | Local header | Central header |
|---|---|---|
| Signature | `50 4B 03 04` | `50 4B 01 02` |
| Version made by | not present | `14 03` |
| Version needed | `14 00` | `14 00` |
| Flags | `00 08` | `00 08` |
| Method | `00 00` | `00 00` |
| Time, date | `00 00`, `21 00` | `00 00`, `21 00` |
| CRC-32, sizes | real values | real values |
| Name length, extra length | name bytes, `00 00` | name bytes, `00 00` |
| Comment length, disk, internal | not present | `00 00`, `00 00`, `00 00` |
| External attributes | not present | `00 00 A4 81` |
| Local header offset | not present | real offset |

## Paths

A path is the name of an entry. It is relative to the package root.

| Id | Rule |
|---|---|
| [PKG-115] | A path MUST use `/` as its separator and be made of components of the characters `A-Z`, `a-z`, `0-9`, `.`, `_` and `-` only. Any other byte, including backslash, NUL, a space, a control character or a non-ASCII character, is refused with `package.path-invalid`. |
| [PKG-116] | A path MUST NOT start with `/`, MUST NOT contain an empty component, and MUST NOT contain a component equal to `.` or `..`. |
| [PKG-117] | No component may start with `.` or end with `.`. This keeps hidden files and Windows trailing-dot names out of the package. |
| [PKG-118] | A component MUST NOT be a name that Windows reserves (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`), with or without an extension, in any letter case. |
| [PKG-119] | A path is at most 255 bytes long and has at most 16 components. |
| [PKG-120] | No two paths may be equal, and no two paths may be equal after ASCII case folding. A duplicate is refused with `package.path-duplicate`. A case-only collision is refused with `package.path-case-collision`, so that the package unpacks the same way on a case-insensitive file system. |
| [PKG-121] | A path that is a prefix of another path followed by `/` (a file that is also a folder) is refused with `package.path-invalid`. |
| [PKG-122] | Entries MUST be ordered by the bytewise comparison of their UTF-8 paths, which for this character set is ASCII order. `Z` sorts before `a`, and `/` (0x2F) sorts before digits. Out-of-order entries are refused with `package.entry-order`. |

The character set is deliberately ASCII. It removes Unicode normalisation (NFC and NFD) and confusable-name problems from the file system level. Display names and descriptions in the manifest may use any Unicode text.

## Reserved names and allowed folders

| Path | Meaning |
|---|---|
| `manifest.json` | The manifest. Required. |
| `server.wasm`, `client.wasm`, `scene.wasm` | The component of that side. Required when the side is declared. |
| `signature` | The signature document. Required for tier T2. See [Signing and trust](/pages/reference-signing-and-trust/). |
| `assets/**` | Read-only data, visible through the `package` file scope. |
| `help/*.md` | Help topics. |
| `sounds/*` | Sound files. |
| `screenshots/*` | PNG images for the store. Not loaded by the plugin. |
| `src/**` | Sources and source maps, for review only. Never loaded. |
| `README.md`, `CHANGELOG.md`, `LICENSE`, `NOTICE` | Documents shown by Settings > Plugins and the store. |
| `sbom.json` | The software bill of materials. See [Publishing guide](/pages/guide-publishing/). |
| `build-info.json` | The build record. See [Publishing guide](/pages/guide-publishing/). |

| Id | Rule |
|---|---|
| [PKG-123] | Any top-level entry that is not in the table, and any entry under a top-level folder that the table does not list, is refused with `package.entry-unexpected`. |
| [PKG-124] | `help/` MUST hold only files whose names end in `.md`. `sounds/` MUST hold only `.wav` and `.ogg` files. `screenshots/` MUST hold only `.png` files. These folders are flat: a nested folder is refused with `package.entry-unexpected`. |
| [PKG-125] | A component MUST be named after its side. A file named `server.wasm` that is declared as the `client` entry is refused with `manifest.entry-invalid`. |

## Size caps

| Id | Cap | Value | Code |
|---|---|---|---|
| [PKG-126] | Entries in the archive | 4,096 | `package.entry-limit` |
| [PKG-127] | `manifest.json` | 1 MiB (1,048,576 bytes) | `package.manifest-size` |
| [PKG-128] | One component, at the archive check (the component check later refuses a component over 32 MiB with `component.too-large`) | 64 MiB | `package.size-limit` |
| [PKG-129] | One non-component entry | 64 MiB | `package.size-limit` |
| [PKG-130] | The package file | 300 MiB | `package.size-limit` |
| [PKG-131] | `signature` | 16 KiB | `package.size-limit` |
| [PKG-132] | Asset bytes (`assets/**`, `help/*.md`, `sounds/*`, fonts) | 64 MiB, or 256 MiB for T1 (decision D24) | `manifest.asset-size` |

The asset total in [PKG-132] counts the uncompressed bytes of `assets/**`, `help/*.md`, `sounds/*` and any font file. It does not count components, `src/**`, `screenshots/*`, the documents of the table above, `signature` or the manifest. Those count only toward the 300 MiB package cap. The reader checks every cap from the central directory, before it reads any entry data.

## The inventory

The manifest key `files` lists every entry except `manifest.json` and `signature`.

| Id | Rule |
|---|---|
| [PKG-133] | Each `files` item has exactly the keys `path`, `length` and `sha256`. `path` is an archive path. `length` is the size in bytes of the uncompressed data and is an integer from 0 to 67,108,864. `sha256` is 64 characters of lowercase hexadecimal. |
| [PKG-134] | `files` MUST list the paths in the same order as the archive (the bytewise order of [PKG-122]), and MUST NOT contain a path twice. A packed manifest whose list is unsorted is refused with `manifest.files-order`. |
| [PKG-135] | Every archive entry except `manifest.json` and `signature` MUST be in `files` (`manifest.file-unlisted`). Every `files` item MUST name an archive entry (`manifest.file-missing`). `files` MUST NOT list `manifest.json` or `signature` (`manifest.file-self`). |
| [PKG-136] | The length of each entry MUST equal `length` (`manifest.file-length`), and its SHA-256 MUST equal `sha256` (`manifest.file-hash`). |
| [PKG-137] | A packed manifest MUST contain `files`. A source manifest MAY omit it, and `ilium plugin pack` then writes it. The installer refuses a manifest without `files` with `manifest.key-missing` (pointer `/files`). |

Because the signature covers the exact bytes of `manifest.json` and the manifest holds the SHA-256 of every other entry, the signature covers every file of the package, transitively. Changing one byte of any listed file changes its hash, which changes the manifest, which breaks the signature.

## The api-version section

Every component carries a WebAssembly custom section that states which contract version it was built for.

| Id | Rule |
|---|---|
| [PKG-138] | A component MUST contain exactly one custom section named `ilium:api-version`. None gives `package.api-section-missing`. A second one gives `package.api-section-repeated`. |
| [PKG-139] | The payload (after the section name) is exactly 6 bytes: three unsigned 16-bit integers in big-endian order, for major, minor and patch. For `ilium:plugin@0.1.0` the bytes are `00 00 00 01 00 00`. Another length gives `package.api-section-invalid`. |
| [PKG-140] | The three integers MUST equal the version in the manifest `api` key, or the install is refused with `manifest.api-version`. |
| [PKG-141] | A file named `*.wasm` that is a WebAssembly core module, and not a component, is refused with `package.component-invalid`. The check reads the 8-byte preamble: the magic `00 61 73 6D`, then the layer field. Layer 0 is a core module. Layer 1 is a component. |
| [PKG-142] | The component MUST validate as a component against the world named in the manifest. A component that fails validation is refused with `package.component-invalid`. A component that exports something that the manifest does not list, or the reverse, is refused with `manifest.exports-mismatch`. |

The minimal component of the test vector below is the 8-byte preamble `00 61 73 6D 0D 00 01 00` (component, layer 1) followed by one custom section. It is a valid empty component for the purposes of the container tests. It is not a working plugin.

## The `ilium plugin pack` algorithm

Packing is a pure function of the project directory. The same inputs give the same bytes.

| Id | Rule |
|---|---|
| [PKG-143] | **Input set.** `pack` includes: the source `manifest.json`; the built components `server.wasm`, `client.wasm` and `scene.wasm` for each declared side, taken from the build output folder; every file under `assets/`, `help/`, `sounds/`, `screenshots/` and `src/`; and `README.md`, `CHANGELOG.md`, `LICENSE`, `NOTICE`, `sbom.json` and `build-info.json` when present. Nothing else is included. |
| [PKG-144] | **Ignore rules.** Inside the included folders `pack` skips files whose name starts with `.`, `*~`, `*.swp`, `*.tmp`, and the folders `.git`, `target`, `node_modules` and `dist`. A skipped file is reported as a `warning` line. A file that remains and breaks a path rule of this page stops `pack` with the code of that rule. |
| [PKG-145] | **Manifest serialisation.** `pack` parses the source manifest, removes any `files` key, builds the inventory, and writes the packed manifest as UTF-8 without a byte order mark, with 2-space indentation, `\n` line endings and one final `\n`. Top-level keys are written in the order of the table in [Manifest reference](/pages/reference-manifest/#top-level-keys), and `files` last. Keys of nested objects keep the order of their schema table; unknown nested maps (settings values, extension entries) sort by key in byte order. Arrays hold one item per line, except that an empty array prints `[]` and an empty object prints `{}` on the key line. Strings escape only `"`, `\` and U+0000 to U+001F; the control characters use `\uXXXX` with lowercase hexadecimal, except `\n`, `\t`, `\r`, `\b` and `\f`. Non-ASCII text is written as UTF-8. Numbers are integers in decimal without a sign `+` and without an exponent; a non-integer number is refused. |
| [PKG-146] | **Archive order and fields.** Entries are written in the order of [PKG-122] with the header values of [PKG-100] to [PKG-108]. |
| [PKG-147] | **Determinism.** Two runs of `pack` over identical inputs MUST give byte-identical files. The test is to compare the SHA-256 of two outputs. |
| [PKG-148] | **Signing.** `ilium plugin sign` adds the `signature` entry by writing a new archive: it reads the package, adds the entry in its sorted position (it sorts between `server.wasm` and `sounds/`, as byte order requires) and rewrites the central directory. Unsigned entries keep their bytes. The result is again a valid, deterministic package. |

A source manifest and its packed form are different files. The signature is made over the packed manifest, so a package cannot be signed before it is packed.

## The validation order

The installer validates in a fixed order. The full pipeline has eleven stages. A stage collects all of its errors, and the install stops after the first stage that found any. This rule reconciles two older statements: a single stage can print several errors, and no later stage runs after a failing one.

| Stage | Name | What it checks | Codes (families) | Collects errors |
|---|---|---|---|---|
| 1 | archive | The container, paths, caps ([PKG-100] to [PKG-132]) | `package.*` | yes |
| 2 | manifest-parse | UTF-8, JSON syntax, duplicate keys | `manifest.json-*` | no (first error) |
| 3 | schema | Keys, types, ranges, grammars of [Manifest reference](/pages/reference-manifest/) | `manifest.*` | yes |
| 4 | semantic | Cross references, uniqueness, side and export rules | `manifest.*` | yes |
| 5 | inventory | `files` against the entries; lengths and hashes ([PKG-133] to [PKG-137]) | `manifest.file-*` | yes |
| 6 | components | Preamble, api-version section, exports ([PKG-138] to [PKG-142]) | `package.*`, `manifest.api-version`, `manifest.exports-mismatch` | yes |
| 7 | signature | Format and Ed25519 verification | `package.signature-format`, `manifest.signature-invalid` | no (first error) |
| 8 | trust | Key trust, publisher binding, tier assignment | `trust.*`, `manifest.publisher-mismatch`, `package.publisher-key-changed` | no |
| 9 | policy | Platform gate, tier grant rules, administrator policy, version rules | `package.platform-unsupported`, `manifest.grant-*`, `manifest.limits-tier`, `policy.*`, `package.downgrade`, `package.version-reused` | yes |
| 10 | dependencies | `requires.plugins`, host version, cycles | `dependency.*`, `manifest.service-cycle` | yes |
| 11 | review | The user reviews grants | none | not applicable |

| Id | Rule |
|---|---|
| [PKG-149] | The installer MUST run the stages in the order of the table. A stage MUST NOT run when an earlier stage reported an error. |
| [PKG-150] | Within a stage that collects errors, the installer MUST report every error it finds, up to 100 per stage. The 101st is replaced by one `warning` line that says how many more exist. |
| [PKG-151] | No plugin code MUST run before stage 11 has finished. Components are not instantiated, and no import is called. |
| [PKG-152] | Tier-dependent rules (T1-only grants, `limits_tier`) run in stage 9, after the tier is known from stages 7 and 8. A source of tier that the user cannot influence MUST NOT be read from the manifest. |
| [PKG-153] | `ilium plugin check` runs stages 2 to 4 on a source manifest (it needs no archive) and, with `--package <file>`, all stages up to 6. It never contacts the network and never writes state. |

The three older orders in `reference-manifest`, `packages` and `security` are replaced by this table. Those pages now link here.

## Error codes of this page

The codes below belong to the `package.*` family. Message templates are in [Manifest schema and error catalogue](/pages/reference-manifest-schema/).

| Code | Cause |
|---|---|
| `package.zip-invalid` | The file is not a ZIP archive, or the end record or central directory is inconsistent or truncated |
| `package.zip64` | ZIP64 structures or sentinels were found |
| `package.zip-compressed` | An entry uses a method other than stored |
| `package.zip-encrypted` | An entry is encrypted |
| `package.zip-descriptor` | An entry uses a data descriptor |
| `package.zip-extra` | An extra field is present |
| `package.zip-comment` | An archive or entry comment is present |
| `package.zip-trailing` | Bytes exist outside the record structure |
| `package.header-mismatch` | A local header disagrees with its central header, or entries overlap |
| `package.crc-mismatch` | A CRC-32 is wrong |
| `package.path-invalid`, `package.path-duplicate`, `package.path-case-collision` | A path breaks the path rules |
| `package.entry-directory`, `package.entry-symlink`, `package.entry-order`, `package.entry-unexpected` | An entry type, order or location is not allowed |
| `package.entry-limit`, `package.size-limit`, `package.manifest-size` | A cap was exceeded |
| `package.manifest-missing` | There is no `manifest.json` entry |
| `package.component-invalid` | A `.wasm` entry is not a valid component |
| `package.api-section-missing`, `package.api-section-repeated`, `package.api-section-invalid` | The `ilium:api-version` section is absent, repeated or malformed |
| `package.api-unsupported` | The `api` is outside the host window. See [Versioning](/pages/versioning/) |
| `package.signature-format` | The `signature` entry is not a valid signature document |
| `package.old-format` | The file is an `.iliumanim` package. See [Porting from .iliumanim](/pages/porting-iliumanim/) |

The install error catalogue in [Manifest schema](/pages/reference-manifest-schema/#error-catalogue) is the only list of codes. It also has a table of retired spellings.

## Offline verification: `ilium plugin verify`

`ilium plugin verify <file>` runs stages 1 to 7 on a package without installing it. The `tier` it reports comes from a read-only look-up of the signing key in the trust store, not from stage 8. It needs no network and changes no state. It prints JSONL lines.

| Id | Rule |
|---|---|
| [PKG-154] | `verify` prints one `progress` line per stage with `stage` (1 to 7) and `name`, then one `error` line per error, then one `result` line. |
| [PKG-155] | The `result` line has `ok` (boolean), `id`, `version`, `sha256` (of the package file), `manifest_sha256`, `tier` (`T2`, `T3` or `untrusted`), `signer_key_id` (or null) and `publisher_id` (or null). |
| [PKG-156] | The exit code is 0 when stages 1 to 7 pass, and 1 when any of them fails. For `verify`, a key that is not in the trust store prints `trust.key-unknown` as a `warning` line, reports `tier: untrusted` and keeps the exit code 0. A revoked key prints `trust.key-revoked` as an `error` line and exits 1. During `install`, `trust.key-unknown` without an answer to the trust prompt is an `error` line and exits 1 (PKG-230). |

<figure>
<pre class="term"><span class="t-prompt">$</span> ilium plugin verify example.minimal.iliumpkg
<span class="t-dim">stage</span> 1 archive          <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 2 manifest-parse   <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 3 schema           <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 4 semantic         <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 5 inventory        <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 6 components       <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 7 signature        <span class="t-ok">ok</span>
<span class="t-dim">stage</span> 8 trust            <span class="t-warn">untrusted</span>
<span class="t-hi">result</span> example.minimal 0.1.0 untrusted
</pre>
<figcaption>Rendered at 48 columns. Illustrative. Verification of a signed package whose key is not trusted yet.</figcaption>
</figure>

## Test vector: a minimal package

This vector is real. Every value was computed with a reference script. A reader that implements the page MUST accept it, and a packer MUST reproduce it.

The component is 34 bytes (hex):

```text
0061736d0d000100001811696c69756d3a6170692d76657273696f6e000000010000
```

Its SHA-256 is `028fb8ab3e154570597f3cc09ad59782ab44d9a6873f1834490da625ef3c6cd8`. The api-version payload is `000000010000`.

The packed manifest is 402 bytes with this text (the final newline included):

```json
{
  "api": "ilium:plugin@0.1.0",
  "id": "example.minimal",
  "name": "Minimal",
  "version": "0.1.0",
  "publisher": "Example Labs",
  "server": {
    "entry": "server.wasm",
    "world": "ilium:server/base@0.1.0",
    "exports": []
  },
  "files": [
    {
      "path": "server.wasm",
      "length": 34,
      "sha256": "028fb8ab3e154570597f3cc09ad59782ab44d9a6873f1834490da625ef3c6cd8"
    }
  ]
}
```

The SHA-256 of these manifest bytes is `350b5e47c5781e72a9bb7b3d6630335fca0390402d0f625a48857a234f99c99d`.

The unsigned package is 658 bytes, with SHA-256 `58aa1172e083701a96593f7531f21c0f3668e2f66c3166899f9b90ee3fe5616a`. In hexadecimal, 32 bytes per line:

```text
504b03041400000800000000210072c5884392010000920100000d0000006d61
6e69666573742e6a736f6e7b0a202022617069223a2022696c69756d3a706c75
67696e40302e312e30222c0a2020226964223a20226578616d706c652e6d696e
696d616c222c0a2020226e616d65223a20224d696e696d616c222c0a20202276
657273696f6e223a2022302e312e30222c0a2020227075626c6973686572223a
20224578616d706c65204c616273222c0a202022736572766572223a207b0a20
20202022656e747279223a20227365727665722e7761736d222c0a2020202022
776f726c64223a2022696c69756d3a7365727665722f6261736540302e312e30
222c0a20202020226578706f727473223a205b5d0a20207d2c0a20202266696c
6573223a205b0a202020207b0a2020202020202270617468223a202273657276
65722e7761736d222c0a202020202020226c656e677468223a2033342c0a2020
2020202022736861323536223a20223032386662386162336531353435373035
3937663363633039616435393738326162343464396136383733663138333434
393064613632356566336336636438220a202020207d0a20205d0a7d0a504b03
041400000800000000210017cea50d22000000220000000b0000007365727665
722e7761736d0061736d0d000100001811696c69756d3a6170692d7665727369
6f6e000000010000504b010214031400000800000000210072c5884392010000
920100000d0000000000000000000000a481000000006d616e69666573742e6a
736f6e504b010214031400000800000000210017cea50d22000000220000000b
0000000000000000000000a481bd0100007365727665722e7761736d504b0506
000000000200020074000000080200000000
```

The signed form of the same package adds the `signature` entry of [Signing and trust](/pages/reference-signing-and-trust/#test-vector). It is 1,114 bytes with SHA-256 `d2238076eea48c58ceb4416dbb50438c6ffee23028051d7e9a0cb1f9ae7fc408`.

## Hostile archive matrix

Each row is one test. The package is the vector above with the change given. The last column lists every code the failing stage prints, in print order ([PKG-578]), not only the first.

Rules that apply to every row:

- A mutation that adds an entry writes it at its sorted position with a correct CRC-32 and correct headers. The entry is not listed in `files` unless the row says so. Only the rows that change a component rebuild the package.
- For one entry, the checks of stage 1 run in this order: entry type (directory, symlink), path rules, duplicate and case collision, entry order, location, caps. An entry that fails a check takes no later check of the stage. For a colliding pair, the code is printed for the second entry in central directory order.
- Stage 6 checks one component in this order: preamble, api-version section, exports. The first failing check ends the checks of that component.

| Id | Mutation | Expected codes, in print order |
|---|---|---|
| [PKG-157] | An entry named `../evil.wasm` (zip slip) | `package.path-invalid` |
| [PKG-158] | An entry named `/etc/passwd` | `package.path-invalid` |
| [PKG-159] | An entry named `assets\x.png` (backslash) | `package.path-invalid` |
| [PKG-160] | An entry name that holds a NUL byte | `package.path-invalid` |
| [PKG-161] | Two entries `Server.wasm` and `server.wasm` | `package.entry-unexpected` (for `Server.wasm`), `package.path-case-collision` (for `server.wasm`) |
| [PKG-162] | The entry `server.wasm` written twice | `package.path-duplicate` |
| [PKG-163] | A central header whose local offset points into another entry (overlap) | `package.header-mismatch` |
| [PKG-164] | One data byte changed, CRC unchanged | `package.crc-mismatch` |
| [PKG-165] | The central directory cut in half | `package.zip-invalid` |
| [PKG-166] | `manifest.json` with 0 bytes | `manifest.json-invalid` (stage 2) |
| [PKG-167] | An archive with no `manifest.json` | `package.manifest-missing` |
| [PKG-168] | The method field of one entry set to 8 (deflate) in both headers, data unchanged | `package.zip-compressed` |
| [PKG-169] | Bit 0 set on one entry | `package.zip-encrypted` |
| [PKG-170] | An archive comment of 4 bytes | `package.zip-comment` |
| [PKG-171] | 16 bytes appended after the end record | `package.zip-trailing` |
| [PKG-172] | A symbolic link entry `assets/link` (mode `0120777`) | `package.entry-symlink` |
| [PKG-173] | A directory entry `assets/` | `package.entry-directory` |
| [PKG-174] | An unlisted file `extra.txt` at the top level | `package.entry-unexpected` |
| [PKG-175] | The entries `manifest.json` and `server.wasm` swapped, headers rewritten | `package.entry-order` (for `manifest.json`) |
| [PKG-176] | An archive with 4,097 entries: the vector plus 4,095 files `assets/f0001.bin` to `assets/f4095.bin` | `package.entry-limit` |
| [PKG-177] | `server.wasm` replaced by a core module (layer 0). Rebuild with `pack` after the change, so `files` lists the new length and SHA-256 and the CRC-32 is correct | `package.component-invalid` |
| [PKG-178] | The api-version section duplicated. Rebuild with `pack` after the change, so `files` lists the new length and SHA-256 and the CRC-32 is correct | `package.api-section-repeated` |
| [PKG-179] | The api-version payload 5 bytes long. Rebuild with `pack` after the change, so `files` lists the new length and SHA-256 and the CRC-32 is correct | `package.api-section-invalid` |
| [PKG-180] | The api-version payload `00 00 00 02 00 00`. Rebuild with `pack` after the change, so `files` lists the new length and SHA-256 and the CRC-32 is correct | `manifest.api-version` |
| [PKG-181] | One byte of `server.wasm` changed, with a matching CRC-32 in both headers and `files` left as it was | `manifest.file-hash` |
| [PKG-182] | A signed package with one manifest byte changed: the letter `M` of `"name": "Minimal"` becomes `N`, and the correct CRC-32 is written to both headers, so that stages 1 to 6 pass and only stage 7 fails | `manifest.signature-invalid` |

These rows are the minimum matrix. A host MAY add more rows and MUST NOT remove any.

## Edge cases

| Situation | Result |
|---|---|
| A 0-byte file in `assets/` | Allowed. `length` is 0 and `sha256` is the hash of the empty string |
| A package with only a `scene` side | Allowed. Only `manifest.json` and `scene.wasm` are required |
| A `.iliumanim` file | Refused with `package.old-format` by the signature of its first bytes. See [Porting from .iliumanim](/pages/porting-iliumanim/) |
| Two packs of the same sources on two operating systems | Identical bytes, because paths, order and times are fixed |
| A package renamed to another file name | Valid. The host reads the manifest, never the file name |
| A path that is valid on Linux but reserved on Windows | Refused on every system ([PKG-118]) |

## What the user sees

The user sees a package only through the install dialog and the CLI. A refused package prints the failing stage, the code and the path of the offending entry. No plugin code has run.

## See also

- [Manifest reference](/pages/reference-manifest/)
- [Manifest schema and error catalogue](/pages/reference-manifest-schema/)
- [Signing and trust](/pages/reference-signing-and-trust/)
- [Install, update and uninstall](/pages/reference-install-and-update/)
- [Plugin command line](/pages/reference-plugin-cli/)
- [Packages and manifests](/pages/packages/)
