---
layout: page
title: Security testing
permalink: /pages/reference-security-testing/
---

## Security testing

This page lists what must be fuzzed, what a hostile test plugin must fail to do, and how each security requirement maps to a test. It is the checklist a test author uses to turn the normative statements in the security pages into test cases. The general test approach is in [Testing](/pages/testing/).

A **fuzz target** is a function that takes arbitrary bytes and must never crash, hang or break an invariant. A **corpus seed** is an example input that starts the fuzzer near interesting cases. A **fixture** is a fixed file or program that a test uses.

### You will need

- The requirement ids from [Security and policy requirements](/pages/reference-requirements-security-policy/).
- A Linux machine that meets the [sandbox prerequisites](/pages/reference-sandbox/).
- A fuzzing tool for Rust (for example `cargo-fuzz`) and the project's test harness.

## Fuzz targets

**[SEC-340]** Each target below MUST exist in the repository, with corpus seeds and the stated invariants. The invariant for every target is: no panic, no out-of-bounds access, no unbounded memory or time (a limit of 1 s and 512 MiB per input), and a defined error code for every rejected input.

| Target | Seeds | Extra invariant |
|---|---|---|
| `.iliumpkg` reader | Valid packages; zip-slip names; duplicate names; ZIP64; header mismatch; overlapping entries; symlink entries; decompression bombs | Nothing is written outside the staging folder; errors use the `package.*` codes of [Trust and supply chain](/pages/reference-trust-and-supply-chain/) |
| Manifest parser and validator | Valid manifests; duplicate keys; depth 1,000; 10 MiB strings; unknown keys; bad grant strings | Duplicate keys refused; unknown keys give `manifest.unknown-key` |
| Grant string and host-pattern matcher | Valid and invalid grants | Property test against a reference implementation: the matcher and the reference agree on every input |
| Path canonicaliser and scope resolver | Traversal corpus (`..`, encoded dots, backslashes, long names), symlink loops | A result is always below the scope root, or an error |
| Settings schema compiler and value validator | Valid schemas; huge `oneOf`; recursive refs; bad `pattern` | Limits of the schema reference hold |
| `payload_schema` and `entry_schema` validators | As above, plus `x-ilium-data` annotations | Risk raised to `high` where [SEC-281] applies |
| Every WIT host function's lifting of guest values | Lengths up to `u32::MAX`, invalid UTF-8, bad enum discriminants, stale resource handles | Defined error, never a trap of the host |
| Dispatcher protocol messages from the helper | Truncated, oversized, malformed, wrong-identity messages | Helper ended and counted as a failure ([SEC-052]) |
| Codecs: inflate, image decoders, audio, video headers | Real files, truncated files, huge dimensions | Pixel cap and byte cap hold |
| HTTP API request parser | Smuggling cases; huge headers; wrong Host and Origin | 4xx, never a route call without the token |
| Transcript parsers behind `agent.transcript` | Real transcripts per agent, truncated, hostile JSON | Redaction still applies |
| Detector screen inputs | Real screens, random bytes, escape-sequence storms | Classification returns within the time limit |
| Regex compilation of plugin patterns | Catastrophic patterns (`(a+)+$`), huge classes, nested groups | Compile refused ([SEC-046]) or match is linear in the input |
| Audit record parser (`--verify`) | Tampered chains | The first broken `seq` is reported |

**[SEC-341]** Each fuzz target runs for at least 10 minutes in CI on every change that touches its code, and for at least 4 hours per target before each release. Native code runs under AddressSanitizer and UndefinedBehaviorSanitizer in at least one CI job.

**[SEC-342]** A fuzz crash, hang or invariant failure is a release blocker. The input is added to the corpus as a regression test before the fix merges.

## Sandbox escape probe suite

**[SEC-345]** A hostile test plugin (guest) and a hostile child program (started with `process.run`) MUST each attempt the actions below, and each attempt MUST fail with the defined result. The suite runs on every supported Linux configuration of CI and before each release.

| Attempt | Expected result | Requirement |
|---|---|---|
| Open a socket (TCP, UDP, UNIX, raw) | `EPERM` or no route | [SEC-022] |
| Read `/proc/<server pid>/environ`, `/proc/1/environ` | Path does not exist or `EACCES` | [SEC-026] |
| `ptrace` the server or a sibling helper | `EPERM` | [SEC-024] |
| `ioctl(TIOCSTI)` or `TIOCLINUX` on any fd | `EPERM`; no inherited PTY fd exists | [SEC-028] |
| Reach `$XDG_RUNTIME_DIR`, the Ilium socket, the API token | Not mounted | [SEC-023], [SEC-098] |
| Read `~/.ssh`, `~/.aws`, `~/.gnupg` without a grant | Not mounted | [SEC-023] |
| Write outside the granted scope, through `..`, a symlink swap race, a hard link, a bind mount | `not-permitted` | [SEC-058], [SEC-059] |
| Execute a binary written to `data` | `EACCES` | [SEC-023], [SEC-034] |
| Fork-bomb | `EAGAIN` at `pids.max`; helper still responsive | [SEC-029] |
| Allocate beyond `memory.max` | Helper killed; `trap.memory` | [SEC-029] |
| Call each denied system call of [SEC-067] | `EPERM` | [SEC-024] |
| Read another plugin's data folder | Not mounted | [SEC-020], [SEC-060] |
| Send a message with another plugin's id | Dropped; failure counted | [SEC-050] |
| Set `LD_PRELOAD` or `PATH` in `process.run` env | `invalid-argument` | [SEC-033] |
| Run `sh -c` through `panes.create` with no `panes.create-command` grant | `refused(not-permitted)` | [SEC-040] |

## Security test checklist

Every row below is a test the project MUST have. Kinds: unit, integration, e2e (end to end with the real client), fuzz, manual.

| SEC id | Kind | Fixture | Expected result |
|---|---|---|---|
| [SEC-141] | unit | A signed package; flip each byte of the manifest and signature | `manifest.signature-invalid` for every flip |
| [SEC-142] | integration | Install `a` signed by key 1, then update signed by key 2 | `package.publisher-key-changed` |
| [SEC-143] | integration | Update with a valid rotation statement | Accepted; dialog says `Signing key rotated` |
| [SEC-145] | integration | Install a lower version | Refused without the flag; audited with the flag |
| [SEC-146], [SEC-149] | integration | A blocklist entry for an installed package | Disabled at next load; `blocklist-hit` |
| [SEC-150] | unit | Archives with each defect of the table | The listed code |
| [SEC-153] | integration | Edit a file in the install folder | `manifest.file-hash`, plugin disabled |
| [SEC-103] | e2e | Hold Enter while the dialog opens | Dialog stays open |
| [SEC-110] | e2e | Decline an origin three times | Fourth call returns `not-permitted` with no prompt |
| [SEC-113] | integration | Add an H grant to `grants.json` by hand | Dropped at start; `store-external-edit` |
| [SEC-116] | e2e | `ilium plugin install --yes` from a pane with no client | `install.review-required` |
| [SEC-120] | integration | `consents --withdraw` | Next use asks again |
| [SEC-182] | integration | Policy file writable by the user | Ignored; notice; audit |
| [SEC-181] | unit | System and user policy pairs | Merge table of the policy page |
| [SEC-187] | integration | Malformed policy | `policy.invalid`; third parties not loaded |
| [SEC-189] | integration | `plugins.manage` enables a forced-off plugin | `not-permitted` reason `policy` |
| [SEC-221], [SEC-222] | integration | One scripted scenario per event | Exactly one record per event, chain verifies |
| [SEC-226] | unit | Edit a record in the middle | `--verify` names that `seq` |
| [SEC-072], [SEC-073] | integration | A local resolver that answers a public name with `127.0.0.1`, then `93.184.216.34` | Refused; no second resolution |
| [SEC-075], [SEC-076] | integration | Redirect to another origin | Authorization header removed; https to http refused |
| [SEC-077] | integration | Server with an invalid certificate | `network` reason `tls` |
| [SEC-097] | integration | Requests with a wrong Host, a foreign Origin, no token | 403, 403, 401 |
| [SEC-084] | integration | Offline switch | `not-permitted` reason `offline` |
| [SEC-243] | unit | A secret in a log line, in base64, in a URL | `<redacted>` in each |
| [SEC-242] | unit | `secret` with `scope: "project"` | `manifest.secret-scope` |
| [SEC-287] | integration | A veto handler that sleeps | Plugin-initiated send refused with `extension.vetoed` (reason `guard-failed`); user prompt for a paste |
| HST-472 | integration | A handler that starts before 100 ms and burns its deadline | Handlers not started at 100 ms are skipped as pass and logged `extension.chain-cap` |
| [SEC-284] | integration | A transform that returns an invalid payload | Next handler sees the previous valid payload |
| [SEC-289] | integration | Hooks nested three deep | `extension.depth-exceeded` at depth 3 |
| [SEC-290] | integration | Three consecutive breaches | Extension stopped; notice; audit |
| [SEC-300], [SEC-301] | integration | Replace a core plugin as T3 and as T2 | `manifest.replaces-tier`; T2 gets the confirmation |
| [SEC-311] | e2e | Open a repository with hostile `.ilium/` values | Ignored until trusted |
| [SEC-328] | e2e | `ilium --safe-mode` | Third-party plugins off, badge `SAFE` |
| [SEC-380] | unit | A corpus of spoofing strings in every plugin text field | Controls, bidi and fake borders stripped |
| [SEC-346] | manual | Review dialogs at 48 columns | Readable, aligned, screen-reader text present |

**[SEC-346]** The review dialog, trust dialog and consent prompts are checked by hand on each release in a terminal of 48 columns and in one of 120 columns, in light and dark themes.

## Side-channel tests and accepted risks

**[SEC-347]** Tests exist for: clock resolution (a guest reading `clock` twice 100 times sees steps of 1 ms or more, and jitter [SEC-370]); no `screen-changed` event while the pane is in no-echo mode ([SEC-371]); identical errors for a missing and an ungranted plugin ([SEC-373]); no absolute paths in error strings ([SEC-374]). Accepted risks (cache timing between processes, coarse output rhythm, a busy-loop timer) are not tested; they are listed in the [Threat model](/pages/reference-threat-model/).

## Reusing the fixtures

**[SEC-348]** The hostile-input fixtures (archives, manifests, path corpus, spoofing strings, catastrophic patterns) live in `tests/security-fixtures/` of the repository and are released with the SDK. Plugin authors MAY use them to test their own plugins against the same inputs; see [Testing](/pages/testing/).

## Edge cases

| Situation | Result |
|---|---|
| CI machine without user namespaces | The escape suite is skipped with a visible failure; the release cannot be tagged |
| A fuzz target takes more than 1 s on one input | The input is a failure |
| Sanitizer reports a leak in a test-only helper | Fixed or suppressed with a written reason |

### See also

- [Testing](/pages/testing/)
- [Sandbox reference](/pages/reference-sandbox/)
- [Threat model](/pages/reference-threat-model/)
- [Security and policy requirements](/pages/reference-requirements-security-policy/)

Contract: `ilium:plugin@0.1.0`.

## Production additions: T2-security

**[SEC-704]** Every fixed High or Critical vulnerability MUST add a regression fixture to the hostile-plugin suite in the same release, and the release checklist MUST fail when the advisory id has no fixture whose name contains it.

**[SEC-705]** The project MUST fuzz the manifest parser, the package archive reader, the signature/metadata parsers and the dispatcher channel decoder in continuous fuzzing with a corpus kept in the repository, and a crash found MUST block the next release until fixed or triaged as not exploitable in writing.

**[SEC-733]** The hostile-plugin suite MUST contain at least one fixture for each of: fork bomb, memory bomb, fd exhaustion, infinite loop, symlink swap during open (TOCTOU), path traversal, environment leak, network probe, ptrace of sibling and abuse of a revoked grant, and each fixture MUST be run on every release candidate with a pass/fail line in JSONL.
