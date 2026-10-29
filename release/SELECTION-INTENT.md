# Offline target-selection intent

`release/scripts/release_selection.py` introduces a proposed, independent target-intent contract. The source matrix and aggregate now carry its canonical selection identity, while final candidate validation continues to require the full five-target release. Pages, publication and recovery do not accept partial scope. This tool alone cannot enable a partial release.

The tool can validate a consciously selected target set before native qualification evidence exists. Every accepted record has `schema: 1`, `kind: "target-selection-intent"`, `state: "planned"` and `publication_allowed: false`. These are new planning fields, not claimed fields of an existing producer receipt. A valid intent cannot substitute for native or installation evidence.

## Exact contract

The record contains exactly `schema`, `kind`, `state`, `publication_allowed`, `tag`, `commit`, `target_manifest_sha256`, `mode`, `selected_targets`, `omitted_targets` and `omitted_assets`. Unknown or missing fields fail validation.

`tag` and `commit` must equal the caller's independently supplied expected values. Tags follow the existing workspace-version grammar; commits must be lowercase 40-hex strings. This is equality against asserted expectations. The tool does not query Git, check tracked-source cleanliness, inspect Cargo version, query a remote tag or authenticate an author. Those checks remain separate acceptance obligations.

`target_manifest_sha256` identifies the exact raw bytes of the full `release/targets.toml`, including comments and whitespace. The bounded descriptor reader passes the same captured bytes to TOML parsing and hashing. The extracted `release_tool.validate_targets_document` owns the existing complete target policy; `release_tool.load_targets` calls the same validator and retains its full five-target requirement. A shortened or altered policy catalog fails independently of selection.

With no `--intent` option, the tool constructs a full plan containing all five approved targets and no omissions. An explicitly supplied missing, invalid or unreadable file is an error; it never falls back to default scope. Explicit full mode also requires all five targets.

Explicit partial mode requires at least one approved Linux target and at least one omitted target or individual asset. A Linux x86_64-only selection is planning data for the initial milestone; the later accepted cross-platform partial release still requires qualified Linux and Windows assets. A selection may include all five targets while omitting an individual package. This checks planned platform presence only and does not establish that any platform has a qualifying artifact. `selected_targets` uses exact policy-approved Rust target names, with duplicate, unknown and differently cased names rejected. `omitted_targets` must map every unselected target, and no selected or unknown target, to a nonblank reason of at most 2,000 printable characters with no surrounding whitespace, line breaks or control characters. `omitted_assets` maps exact optional filenames belonging to selected targets to reasons under the same rules. Shared files and selected targets' archives are mandatory. The planner does not infer dependent package or evidence omissions: each must be accounted for explicitly, and later qualification must check the dependency graph.

The canonical intent representation sorts target names and object keys, uses ASCII JSON escaping and compact separators, and ends with one LF. The external `intent_sha256` hashes those canonical intent bytes. It is not a field of the intent itself and does not hash the raw input JSON or the complete CLI output envelope. Formatting-equivalent input JSON can therefore have the same canonical identity; changing a reason changes that identity. The explicit JSON reader rejects duplicate keys and parses the bounded bytes read from the checked file descriptor. It refuses symlink replacement when opening where the platform supports that operation, and rechecks the path against the opened file. These checks do not promise atomic custody against a concurrent writer modifying the same file while it is being read.

## Standalone use and tests

From the repository root, this read-only command constructs the full default intent using an explicitly supplied commit:

```sh
python release/scripts/release_selection.py --manifest release/targets.toml --tag v0.1.1 --commit "$(git rev-parse HEAD)"
```

Add `--intent` followed by a reviewed intent file to validate an explicit plan. The command emits one JSONL result containing `intent`, `intent_sha256`, and `planned_assets`, a filename-to-owner map derived from the existing packaging name functions. The `intent` object, serialized canonically, is the input document for a later validation. The asset map is outside the intent digest and does not claim files exist or passed checks. The command writes no files, creates no tags and contacts no service. A successful exit means only that planning data satisfies this contract.

The portable test command is:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest release.tests.test_release_selection release.tests.test_targets release.tests.test_release_cli
```

Fixtures use synthetic source identities and contain no native qualification evidence. The existing target and CLI suites cover preservation of full-policy behavior through the extracted parser helper.

## Exact unfinished scope

The current full plan derives 44 expected public filenames: five archives, six shared release files, three Windows package files, twelve Linux package files and eighteen macOS package/evidence files. The map assigns each target-owned name to its exact Rust target and the six common names to `shared`. Individual omissions affect only this plan; publication consumers are not implemented here.

Actual inclusion remains unknown. Every proposed published asset still needs its exact bytes, source binding and applicable existing semantic, installation and clean-install qualification. The native candidate, audit and installation scripts and their dependencies exist in this checkout; their contracts must be inspected before changing those producers or validators. This tool invents no flags or receipt fields for them.

Source matrix construction carries a validated selection, and candidate inventories retain a full selection. Partial scope still requires integration with per-format qualification, installers, Pages, qualified publication, GHCR and recovery. The separate read-only Linux preview workflow runs qualification only and does not change those final-release publication gates. The actual retained previous-channel baseline and installer bytes are also required to evaluate compatibility. The modified installers can interpret an explicit partial availability inventory only when it includes Linux and Windows archives; they do not accept the initial Linux-only plan. A partial intent alone never waives the checksum gate. Current candidate source identity, native qualification and remote release/channel evidence remain required before any acceptance claim or write.
