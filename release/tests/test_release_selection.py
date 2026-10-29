"""Portable target-intent tests use synthetic identities and never represent native qualification.""" # Keep planning checks separate from release acceptance.
from __future__ import annotations # Preserve the production module's Python baseline.
#
import copy # Mutate independent malformed intent cases without changing valid fixtures.
import hashlib # Verify the external identity against exact canonical bytes.
import json # Build duplicate-key and command-output fixtures explicitly.
from pathlib import Path # Locate only supplied source and temporary input files.
import subprocess # Exercise the real standalone JSONL command boundary.
import sys # Use the interpreter running this suite for CLI assertions.
import tempfile # Isolate every test's intent and manifest inputs.
import unittest # Preserve the repository's portable test framework.
from unittest import mock # Exercise concurrent-read rejection without pretending to execute native tools.
#
root = Path(__file__).resolve().parents[2] # Locate the source tree containing the proposed module.
sys.path.insert(0, str(root / 'release/scripts')) # Match existing release-test import conventions.
import release_selection as selection # Test the real proposed parser and serializer.
import release_tool # Confirm the unchanged full target policy remains authoritative.
import release_pipeline
#
class selection_intent_tests(unittest.TestCase): # Cover admission, rejection, default policy and canonical identity.
    def setUp(self) -> None: # Use the unittest lifecycle API while isolating all mutable fixtures.
        self.temporary = tempfile.TemporaryDirectory() # Allocate scratch inputs that are not release artifacts.
        self.addCleanup(self.temporary.cleanup) # Remove only this test's temporary directory.
        self.directory = Path(self.temporary.name) # Retain a typed local root for fixture inputs.
        self.manifest = self.directory / 'targets.toml' # Keep tamper tests away from the supplied manifest.
        self.manifest.write_bytes((root / 'release/targets.toml').read_bytes()) # Reuse the actual complete five-target catalog.
        self.tag = 'v0.1.1' # Use a fixture tag without claiming a remote tag exists.
        self.commit = 'a' * 40 # Use an explicitly synthetic source identity.
        self.full = selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit) # Construct the real absent-option full default.
        self.targets = release_tool.load_targets(self.manifest) # Observe the unchanged authoritative target rows independently.
        self.linux = next(row['rust_target'] for row in self.targets if row['os'] == 'linux') # Select a fixture Linux identifier without assuming a built artifact exists.
        self.windows = next(row['rust_target'] for row in self.targets if row['os'] == 'windows') # Select the sole approved Windows identifier for structural tests.
    def partial(self) -> dict[str, object]: # Construct an explicit planned subset with no qualification claim.
        value = copy.deepcopy(self.full) # Keep each malformed test independent of the default record.
        value['mode'] = 'partial' # Require an explicit request for partial intent.
        value['selected_targets'] = [self.linux, self.windows] # Satisfy only the planned platform-presence floor.
        value['omitted_targets'] = {row['rust_target']: 'Synthetic test omission; no qualification evidence supplied.' for row in self.targets if row['rust_target'] not in (self.linux, self.windows)} # Account for every unselected policy target.
        return value # Return planned scope rather than a passed release receipt.
    def validate(self, value: object) -> dict[str, object]: # Share one explicit identity and manifest expectation across fixtures.
        return selection.validate_intent(value, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit) # Keep the production validation path active in every case.
    def test_default_full_and_explicit_full_preserve_all_five(self) -> None: # Reject any implementation that silently changes the default policy.
        self.assertEqual(self.full['mode'], 'full') # Absence of a selection option means full mode.
        self.assertEqual(set(self.full['selected_targets']), {row['rust_target'] for row in self.targets}) # Preserve every approved target.
        self.assertEqual(self.full['omitted_targets'], {}) # Full mode cannot carry omissions.
        self.assertEqual(self.validate(self.full), self.full) # Explicit and default full intents have the same validated representation.
        self.assertFalse(self.full['publication_allowed']) # Successful planning never authorizes publication.
    def test_linux_x86_only_selection_is_unqualified_planning(self) -> None:
        value = self.partial()
        value['selected_targets'] = [self.linux]
        value['omitted_targets'] = {row['rust_target']: 'Not ready for the initial Linux release.'
                                    for row in self.targets if row['rust_target'] != self.linux}
        result = self.validate(value)
        self.assertEqual(result['selected_targets'], [self.linux])
        self.assertFalse(result['publication_allowed'])

    def test_linux_first_policy_plans_every_x86_package_without_authorizing_publication(self) -> None:
        policy = (root / 'release/linux-x86-first-policy.json').read_bytes()
        intent = selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag,
                                          expected_commit=self.commit, policy_content=policy)
        self.assertEqual(intent['selected_targets'], ['x86_64-unknown-linux-gnu'])
        self.assertFalse(intent['publication_allowed'])
        self.assertEqual(intent['omitted_assets'], {})
        assets = selection.planned_assets(intent, manifest=self.manifest,
                                          expected_tag=self.tag, expected_commit=self.commit)
        self.assertTrue({f'ilium-linux-x86_64.{suffix}' for suffix in
                         ('tar.gz', 'deb', 'rpm', 'AppImage', 'snap', 'flatpak')} <= set(assets))
        self.assertTrue(all(owner in ('shared', 'x86_64-unknown-linux-gnu')
                            for owner in assets.values()))

    def test_partial_is_only_explicit_unqualified_intent(self) -> None: # Exercise a legitimate planned subset without faking native evidence.
        result = self.validate(self.partial()) # Use the complete actual target policy for validation.
        self.assertEqual((result['kind'], result['state'], result['publication_allowed']), ('target-selection-intent', 'planned', False)) # Keep the authority boundary visible in every valid partial result.
        self.assertEqual(set(result['selected_targets']), {self.linux, self.windows}) # Retain exactly the consciously selected target identifiers.

    def test_tracked_policy_is_bound_to_actual_release_identity(self) -> None:
        policy = self.partial()
        for name in ('tag', 'commit', 'target_manifest_sha256'):
            del policy[name]
        policy['kind'] = 'target-selection-policy'
        policy_path = self.directory / 'policy.json'
        policy_path.write_text(json.dumps(policy), encoding='utf-8')
        first = selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag,
                                         expected_commit=self.commit, policy_path=policy_path)
        second = selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag,
                                          expected_commit='b' * 40, policy_path=policy_path)
        self.assertEqual(first, self.validate(self.partial()))
        self.assertEqual(second['commit'], 'b' * 40)
        self.assertNotEqual(selection.canonical_bytes(first), selection.canonical_bytes(second))
        self.assertFalse(second['publication_allowed'])

    def test_tracked_policy_rejects_claimed_identity_and_qualification(self) -> None:
        policy = self.partial()
        for name in ('tag', 'commit', 'target_manifest_sha256'):
            del policy[name]
        policy['kind'] = 'target-selection-policy'
        policy_path = self.directory / 'policy.json'
        for name, value in [('commit', self.commit), ('publication_allowed', True),
                            ('kind', 'target-selection-intent'), ('selected_targets', [])]:
            with self.subTest(field=name):
                bad = copy.deepcopy(policy)
                bad[name] = value
                policy_path.write_text(json.dumps(bad), encoding='utf-8')
                with self.assertRaises(release_tool.ReleaseError):
                    selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag,
                                             expected_commit=self.commit, policy_path=policy_path)

    def test_tracked_policy_and_explicit_intent_cannot_be_combined(self) -> None:
        with self.assertRaises(release_tool.ReleaseError):
            selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag,
                                     expected_commit=self.commit, intent_path=self.directory / 'intent',
                                     policy_path=self.directory / 'policy')

    def test_policy_bytes_must_match_the_commit_and_be_inside_checkout(self) -> None:
        repo = self.directory / 'checkout'
        repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(repo)], check=True)
        policy = repo / 'selection.json'
        policy.write_bytes(b'{"reviewed":true}\n')
        subprocess.run(['git', '-C', str(repo), 'add', 'selection.json'], check=True)
        subprocess.run(['git', '-C', str(repo), '-c', 'user.name=Test', '-c',
                        'user.email=test@example.invalid', 'commit', '-qm', 'policy'], check=True)
        commit = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip()
        selection.require_committed_policy(repo, policy, commit)
        policy.write_bytes(b'{"reviewed":false}\n')
        with self.assertRaises(release_tool.ReleaseError):
            selection.require_committed_policy(repo, policy, commit)
        untracked = repo / 'untracked.json'
        untracked.write_bytes(b'{}\n')
        with self.assertRaises(release_tool.ReleaseError):
            selection.require_committed_policy(repo, untracked, commit)
        with self.assertRaises(release_tool.ReleaseError):
            selection.require_committed_policy(repo, self.manifest, commit)

    def test_source_rejects_policy_not_in_the_checkout_commit(self) -> None:
        repo = self.directory / 'release-checkout'
        repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(repo)], check=True)
        workspace = repo / 'Cargo.toml'
        workspace.write_bytes((root / 'Cargo.toml').read_bytes())
        manifest = repo / 'targets.toml'
        manifest.write_bytes(self.manifest.read_bytes())
        policy = self.partial()
        for name in ('tag', 'commit', 'target_manifest_sha256'):
            del policy[name]
        policy['kind'] = 'target-selection-policy'
        tracked = repo / 'policy.json'
        tracked.write_text(json.dumps(policy), encoding='utf-8')
        subprocess.run(['git', '-C', str(repo), 'add', 'Cargo.toml', 'targets.toml', 'policy.json'], check=True)
        subprocess.run(['git', '-C', str(repo), '-c', 'user.name=Test', '-c',
                        'user.email=test@example.invalid', 'commit', '-qm', 'release policy'], check=True)
        tag = 'v' + release_pipeline.workspace_version(workspace)
        with mock.patch.dict('os.environ', {'GITHUB_SHA': ''}):
            matrix, _, _ = release_pipeline.source_selection(manifest, workspace, tag, policy_path=tracked)
            self.assertEqual(len(matrix['include']), 2)
            untracked = repo / 'untracked.json'
            untracked.write_bytes(tracked.read_bytes())
            with self.assertRaises(release_tool.ReleaseError):
                release_pipeline.source_selection(manifest, workspace, tag, policy_path=untracked)

    def test_partial_can_document_individual_missing_package_assets(self) -> None:
        value = self.partial()
        available = selection.planned_assets(value, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        package = next(name for name, owner in available.items() if owner == self.linux and name.endswith('.deb'))
        value['omitted_assets'] = {package: 'Package smoke check did not pass.'}
        result = self.validate(value)
        self.assertEqual(result['omitted_assets'], value['omitted_assets'])
        planned = selection.planned_assets(value, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        self.assertNotIn(package, planned)
        self.assertFalse(result['publication_allowed'])

    def test_partial_with_every_target_can_still_omit_a_package(self) -> None:
        value = copy.deepcopy(self.full)
        value['mode'] = 'partial'
        available = selection.planned_assets(self.full, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        package = next(name for name, owner in available.items() if owner == self.linux and name.endswith('.deb'))
        value['omitted_assets'] = {package: 'Synthetic missing package.'}
        self.assertEqual(self.validate(value)['omitted_assets'], value['omitted_assets'])

    def test_asset_omissions_reject_unknown_shared_archive_and_missing_reason(self) -> None:
        available = selection.planned_assets(self.partial(), manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        archive = next(row['archive'] for row in self.targets if row['rust_target'] == self.linux)
        package = next(name for name, owner in available.items() if owner == self.linux and name.endswith('.deb'))
        for name, reason in [('unknown-package', 'missing'), ('SHA256SUMS', 'missing'),
                             (archive, 'missing'), (package, ''), (package, 'line\nbreak')]:
            with self.subTest(name=name, reason=reason):
                value = self.partial()
                value['omitted_assets'] = {name: reason}
                with self.assertRaises(release_tool.ReleaseError):
                    self.validate(value)

    def test_planned_asset_ownership_preserves_full_and_partial_inventories(self) -> None:
        full = selection.planned_assets(self.full, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        self.assertEqual(len(full), 44)
        self.assertEqual(full['ilium-windows-x86_64.zip'], self.windows)
        self.assertEqual(full['SHA256SUMS'], 'shared')
        partial = selection.planned_assets(self.partial(), manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        self.assertEqual(set(partial), {name for name, owner in full.items() if owner in {'shared', self.linux, self.windows}} | {'AVAILABILITY'})
        self.assertEqual(partial['AVAILABILITY'], 'shared')
        self.assertEqual(len(partial), 18)
        self.assertFalse(any(name.startswith('ilium-macos-') for name in partial))
        invalid = self.partial()
        invalid['selected_targets'].append('unknown-target')
        with self.assertRaises(release_tool.ReleaseError):
            selection.planned_assets(invalid, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)

    def test_planned_assets_reject_catalog_changed_after_validation(self) -> None:
        targets, digest = selection.load_catalog(self.manifest)
        with mock.patch.object(selection, 'load_catalog', side_effect=[(targets, digest), (targets, '0' * 64)]):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'manifest bytes differ'):
                selection.planned_assets(self.full, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)

    def test_availability_binds_exact_selection_and_checksum_inventory(self) -> None:
        intent = self.validate(self.partial())
        selected = {row['archive'] for row in self.targets if row['rust_target'] in intent['selected_targets']}
        sums = ''.join('a' * 64 + '  ' + name + '\n' for name in sorted(selected)).encode('ascii')
        availability = selection.availability_bytes(intent, sums, manifest=self.manifest,
                                                    expected_tag=self.tag, expected_commit=self.commit)
        lines = availability.decode('ascii').splitlines()
        self.assertEqual(lines[:5], ['ilium-availability-v1', 'tag ' + self.tag, 'commit ' + self.commit,
            'selection-sha256 ' + hashlib.sha256(selection.canonical_bytes(intent)).hexdigest(),
            'checksums-sha256 ' + hashlib.sha256(sums).hexdigest()])
        self.assertEqual(set(lines[5:]), {('available ' if row['archive'] in selected else 'omitted ') + row['archive']
                                        for row in self.targets})
        for bad in (b'', sums.rstrip(b'\n'), sums + sums.splitlines(keepends=True)[0],
                    sums.replace(b'  ', b' ', 1), sums + b'a' * 64 + b'  unknown.zip\n',
                    sums.splitlines(keepends=True)[0]):
            with self.subTest(sums=bad):
                with self.assertRaises(release_tool.ReleaseError):
                    selection.availability_bytes(intent, bad, manifest=self.manifest,
                                                 expected_tag=self.tag, expected_commit=self.commit)

    def test_full_plan_matches_existing_publication_reader(self) -> None:
        archives = {row['archive']: 'synthetic' for row in self.targets}
        linux = {name: 'synthetic' for row in self.targets if row['os'] == 'linux'
                 for name in (*selection.build_linux_packages.package_names(row['arch']), selection.build_linux_packages.receipt_name(row['arch']))}
        macos = {name: 'synthetic' for row in self.targets if row['os'] == 'macos'
                 for name in (*selection.build_macos_packages.package_names(row['arch']), selection.build_macos_packages.receipt_name(row['arch']), selection.build_macos_packages.smoke_receipt_name(row['arch']), *selection.smoke_macos_packages.retained_evidence_names(row['arch']))}
        windows = {name: 'synthetic' for name in (*selection.build_windows_installers.INSTALLER_NAMES, selection.build_windows_installers.RECEIPT_NAME)}
        (self.directory / 'candidate.json').write_text(json.dumps({'archives': archives, 'windows_installers': windows, 'linux_packages': linux, 'macos_packages': macos}))
        actual = {path.name for path in release_pipeline.publication_files(self.directory)}
        planned = selection.planned_assets(self.full, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit)
        self.assertEqual(actual, set(planned))
    def test_unknown_duplicate_and_case_alias_targets_fail(self) -> None: # Ensure normalization cannot hide an invalid target name.
        for names in ([self.linux, self.windows, 'unknown-target'], [self.linux, self.windows, self.windows], [self.linux, self.windows.upper()]): # Cover unsupported, repeated and differently cased identities.
            with self.subTest(names=names): # Report the rejected identity shape independently.
                value = self.partial() # Start from a valid explicit partial fixture.
                value['selected_targets'] = names # Change only the target-name boundary.
                with self.assertRaises(release_tool.ReleaseError): # Require a policy failure instead of a shortened selection.
                    self.validate(value) # Execute the production validator.
    def test_omissions_are_exact_complement(self) -> None: # Cover undocumented, overlapping, unknown and aliased omission owners.
        original = self.partial() # Retain the valid omission universe for each mutation.
        reasons = original['omitted_targets'] # Use known fixture keys without inventing policy targets.
        first = next(iter(reasons)) # Select one legitimately omitted target for deletion and alias tests.
        cases = [{name: reason for name, reason in reasons.items() if name != first}, {**reasons, self.linux: 'Overlap'}, {**reasons, 'unknown-target': 'Unknown'}, {first.upper() if name == first else name: reason for name, reason in reasons.items()}] # Preserve distinct failure causes before validation.
        for omissions in cases: # Check all complement failures with the same selected targets.
            with self.subTest(omissions=omissions): # Keep each failure attributable to its malformed omission map.
                value = copy.deepcopy(original) # Avoid modifying another case's omission dictionary.
                value['omitted_targets'] = omissions # Install exactly one malformed complement.
                with self.assertRaises(release_tool.ReleaseError): # Require rejection of every undocumented or unknown name.
                    self.validate(value) # Exercise the production complement check.
    def test_omission_reasons_are_explicit_bounded_text(self) -> None: # Refuse empty or unsafe explanations while retaining real Unicode prose.
        for reason in ('', ' ', ' padded', 'trailing ', 'line\nbreak', '\x00', 'line\x85break', 'line\u2028break', 'line\u2029break', 'control\x80text', 'x' * 2_001, None, False): # Cover blank, padded, Unicode-control-bearing, oversized and nontext reasons.
            with self.subTest(reason=repr(reason)): # Preserve useful diagnostics even for control-character fixtures.
                value = self.partial() # Create a fresh valid subset for the changed reason.
                value['omitted_targets'][next(iter(value['omitted_targets']))] = reason # Change one required explanation only.
                with self.assertRaises(release_tool.ReleaseError): # Require a structured policy failure.
                    self.validate(value) # Exercise the actual reason validator.
        value = self.partial() # Check that bounded meaningful non-ASCII reasons remain supported.
        value['omitted_targets'][next(iter(value['omitted_targets']))] = 'Qualification différée; preuve absente.' # Preserve the exact user's reason rather than silently normalizing it.
        self.assertEqual(self.validate(value)['omitted_targets'], value['omitted_targets']) # Keep accepted explanations byte-deterministic after JSON escaping.
    def test_full_omissions_and_partial_full_or_empty_fail(self) -> None: # Preserve mode semantics independently of target-name validity.
        full_with_omissions = self.partial() # Start with a structurally complete partial fixture.
        full_with_omissions['mode'] = 'full' # Attempt to disguise omitted targets as a full release.
        partial_full = copy.deepcopy(self.full) # Start with all five targets and no omissions.
        partial_full['mode'] = 'partial' # Refuse a misleading partial label on the full target set.
        empty = self.partial() # Build an explicitly empty subset with complete omission accounting.
        empty['selected_targets'] = [] # Empty scope must never satisfy partial intent.
        empty['omitted_targets'] = {row['rust_target']: 'Fixture omission' for row in self.targets} # Ensure rejection is not caused merely by an incomplete complement.
        for value in (full_with_omissions, partial_full, empty): # Exercise each independent mode boundary.
            with self.assertRaises(release_tool.ReleaseError): # Require every mislabeled or empty scope to fail.
                self.validate(value) # Use the real mode checks.
    def test_partial_without_linux_fails(self) -> None: # Planning cannot omit Linux from the authorized release scope.
        for selected in ([self.windows], [row['rust_target'] for row in self.targets if row['os'] != 'linux']):
            with self.subTest(selected=selected):
                value = self.partial() # Preserve valid schema and identity fields.
                value['selected_targets'] = selected # Remove one required platform structurally.
                value['omitted_targets'] = {row['rust_target']: 'Fixture omission' for row in self.targets if row['rust_target'] not in selected} # Make omission accounting complete so the floor is the discriminating gate.
                with self.assertRaisesRegex(release_tool.ReleaseError, 'Linux target'):
                    self.validate(value) # Exercise the production OS ownership map.
    def test_unknown_fields_types_and_qualification_claims_fail(self) -> None: # Keep the new intent schema strict and semantically isolated.
        cases = ({'schema': True}, {'schema': 2}, {'mode': 'automatic'}, {'selected_targets': 'all'}, {'selected_targets': [False]}, {'omitted_targets': []}, {'state': 'passed'}, {'publication_allowed': True}, {'publication_allowed': 0}, {'kind': 'qualification'}, {'native_receipts': {}}) # Include invented producer fields and boolean/integer aliases.
        for changes in cases: # Apply one malformed field or unsupported extension at a time.
            with self.subTest(changes=changes): # Report the failed schema boundary separately.
                with self.assertRaises(release_tool.ReleaseError): # Require fail-closed handling of every malformed contract.
                    self.validate({**self.full, **changes}) # Retain all unrelated full-mode fields.
        missing = dict(self.full) # Verify that required fields cannot be omitted.
        del missing['mode'] # Absence of the mode inside an explicit intent is not a default request.
        with self.assertRaises(release_tool.ReleaseError): # Require the complete explicit schema.
            self.validate(missing) # Exercise exact field equality.
    def test_source_and_raw_manifest_tampering_fail(self) -> None: # Bind plans to the independently supplied expected identity and exact catalog bytes.
        for changes in ({'tag': 'v0.1.2'}, {'commit': 'b' * 40}, {'target_manifest_sha256': '0' * 64}): # Cover version, repaired commit and catalog digest substitution.
            with self.subTest(changes=changes): # Keep the identity mismatch diagnosis attributable.
                with self.assertRaises(release_tool.ReleaseError): # Refuse a plan for another source snapshot.
                    self.validate({**self.full, **changes}) # Execute actual identity comparisons.
        self.manifest.write_bytes(self.manifest.read_bytes() + b'\n# harmless semantic change, different raw identity\n') # Prove that even a semantically unchanged edited catalog changes provenance.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'manifest bytes differ'): # Require raw-byte binding rather than target-set equality alone.
            self.validate(self.full) # Attempt to reuse the previously frozen intent.
    def test_expected_identity_and_pure_catalog_entry_are_strict(self) -> None: # Cover the new independent API boundaries as well as file-based callers.
        for tag, commit in (('invalid', self.commit), (self.tag, 'a' * 39), (self.tag, 'A' * 40), (self.tag, None)): # Reject incomplete, unsafe and nonstring caller identities.
            with self.subTest(tag=tag, commit=commit): # Keep expected-input failures distinguishable.
                with self.assertRaises(release_tool.ReleaseError): # Require rejection before an intent can appear well formed.
                    selection.validate_intent(self.full, manifest=self.manifest, expected_tag=tag, expected_commit=commit) # Exercise the public pure validation boundary.
        for document in (None, [], {'target': []}): # Cover malformed direct calls to the extracted full-policy helper.
            with self.assertRaises(release_tool.ReleaseError): # Keep direct document validation fail closed.
                release_tool.validate_targets_document(document) # Use the same helper as both manifest loaders.
    def test_manifest_reader_rejects_symlink_replacement_to_same_inode(self) -> None:
        original_check = selection.regular_input
        for nofollow in {getattr(selection.os, 'O_NOFOLLOW', 0), 0}:
            with self.subTest(nofollow=nofollow):
                path = self.directory / f'input-{nofollow}.toml'
                retained = self.directory / f'retained-{nofollow}.toml'
                path.write_bytes(self.manifest.read_bytes())

                def replace_after_check(checked: Path):
                    attributes = original_check(checked)
                    checked.rename(retained)
                    try:
                        checked.symlink_to(retained.name)
                    except OSError as error:
                        self.skipTest(f'symlink creation unavailable: {error}')
                    return attributes

                with mock.patch.object(selection.os, 'O_NOFOLLOW', nofollow, create=True), mock.patch.object(selection, 'regular_input', side_effect=replace_after_check):
                    with self.assertRaises((OSError, release_tool.ReleaseError)):
                        selection.input_bytes(path)

    def test_manifest_reader_rejects_replacement_and_oversize(self) -> None: # Ensure the bounded descriptor reader rejects changed custody and excessive content.
        with mock.patch.object(selection.os.path, 'samestat', return_value=False): # Simulate replacement between lstat and open while retaining a real file descriptor.
            with self.assertRaisesRegex(release_tool.ReleaseError, 'changed before opening'): # Reject a different opened file even when both are regular.
                selection.input_bytes(self.manifest) # Exercise the real descriptor-reader boundary.
        self.manifest.write_bytes(b' ' * 1_048_577) # Create an actually oversized local input rather than mocking the size result.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'size bound'): # Require rejection before parsing an oversized document.
            selection.load_catalog(self.manifest) # Exercise the real bounded manifest reader.
    def test_explicit_intent_uses_the_checked_file_snapshot(self) -> None:
        intent = self.directory / 'intent.json'
        intent.write_bytes(selection.canonical_bytes(self.validate(self.partial())))
        with mock.patch.object(selection.os.path, 'samestat', return_value=False), mock.patch.object(selection, 'load_catalog', side_effect=AssertionError('intent must be checked first')):
            with self.assertRaisesRegex(release_tool.ReleaseError, 'changed before opening'):
                selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit, intent_path=intent)

    def test_shortened_catalog_fails_and_parsing_uses_one_snapshot(self) -> None: # Ensure intent admission cannot relax the source catalog or hash a different read.
        original = self.manifest.read_bytes() # Retain the complete valid catalog bytes for both cases.
        self.manifest.write_bytes(original[:original.rfind(b'[[target]]')]) # Remove exactly one complete target record from the fixture catalog.
        with self.assertRaisesRegex(release_tool.ReleaseError, 'five approved targets'): # Require the existing full policy to reject the shortened catalog.
            self.validate(self.full) # Exercise the supplied load_targets implementation.
        self.manifest.write_bytes(original + b'\n# a newer path snapshot\n') # Simulate a path changed after a descriptor reader captured the original bytes.
        with mock.patch.object(selection, 'input_bytes', return_value=original), mock.patch.object(release_tool, 'load_targets', side_effect=AssertionError('must not reread manifest')): # Replace only the byte acquisition and forbid the old separate path parser.
            rows, digest = selection.load_catalog(self.manifest) # Keep real TOML parsing and full policy validation active on the captured snapshot.
        self.assertEqual(rows, self.targets) # Require policy rows from the captured bytes.
        self.assertEqual(digest, hashlib.sha256(original).hexdigest()) # Require the digest to identify those same bytes, not the newer path contents.
    def test_canonical_identity_is_order_independent_and_external(self) -> None: # Avoid self-hashing cycles and arbitrary array-order changes.
        first = self.partial() # Retain a valid planned subset.
        second = copy.deepcopy(first) # Change only semantically irrelevant selected-target order.
        second['selected_targets'].reverse() # Exercise canonical target ordering.
        content = selection.canonical_bytes(self.validate(first)) # Serialize the validated intent with the production function.
        self.assertEqual(content, selection.canonical_bytes(self.validate(second))) # Require equal intent bytes for equivalent selections.
        self.assertTrue(content.endswith(b'\n') and not content.endswith(b'\n\n') and b'\r' not in content) # Verify LF with exactly one final newline.
        self.assertNotIn('intent_sha256', json.loads(content)) # The external identity cannot be a field of the bytes it identifies.
        self.assertEqual(self.full['target_manifest_sha256'], hashlib.sha256(self.manifest.read_bytes()).hexdigest()) # Independently check the raw catalog binding.
    def test_duplicate_json_keys_missing_path_and_nonregular_inputs_fail(self) -> None: # Cover the real file boundary without defaulting away explicit bad input.
        intent = self.directory / 'intent.json' # Reuse one local path for malformed explicit inputs.
        valid = json.dumps(self.partial(), separators=(',', ':')) # Produce a realistic explicit partial record.
        first = next(iter(self.partial()['omitted_targets'])) # Select a legitimate omission key for nested duplicate injection.
        for content in (valid.replace('"mode":"partial"', '"mode":"partial","mode":"partial"'), valid.replace('"' + first + '":', '"' + first + '":"duplicate","' + first + '":')): # Cover top-level and nested duplicate keys.
            intent.write_text(content, encoding='utf-8') # Retain the exact duplicate-bearing JSON fixture.
            with self.assertRaisesRegex(release_tool.ReleaseError, 'duplicate JSON key'): # Require the supplied strict JSON reader to reject both cases.
                selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit, intent_path=intent) # Keep the real explicit-file loading path active.
        intent.unlink() # Make an explicitly requested file absent rather than omitting the option.
        with self.assertRaises(OSError): # An explicit missing file must fail instead of becoming full mode.
            selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit, intent_path=intent) # Exercise the absent-file custody boundary.
        with self.assertRaises(release_tool.ReleaseError): # A directory cannot masquerade as an intent document.
            selection.resolve_intent(manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit, intent_path=self.directory) # Exercise regular-file validation.
    def test_cli_emits_only_intent_and_rejects_explicit_missing_input(self) -> None: # Exercise the actual standalone command and externally verifiable digest.
        command = [sys.executable, str(root / 'release/scripts/release_selection.py'), '--manifest', str(self.manifest), '--tag', self.tag, '--commit', self.commit] # Supply only local synthetic identities.
        result = subprocess.run(command, capture_output=True, text=True, check=False) # Run the real CLI without network, package tools or native artifacts.
        self.assertEqual(result.returncode, 0, result.stderr) # Successful validation must have a zero process status.
        record = json.loads(result.stdout) # Require exactly one JSON value on stdout.
        self.assertEqual(record['intent'], self.full) # Preserve the same absent-option full default at the command boundary.
        self.assertEqual(record['planned_assets'], selection.planned_assets(self.full, manifest=self.manifest, expected_tag=self.tag, expected_commit=self.commit))
        self.assertEqual(record['intent_sha256'], hashlib.sha256(selection.canonical_bytes(record['intent'])).hexdigest()) # Verify the digest without trusting the CLI's declaration.
        partial_path = self.directory / 'partial.json' # Retain a reviewed fixture intent for the explicit-file success path.
        partial_path.write_bytes(selection.canonical_bytes(self.validate(self.partial()))) # Use the documented canonical intent bytes, excluding the CLI envelope.
        explicit = subprocess.run(command + ['--intent', str(partial_path)], capture_output=True, text=True, check=False) # Round-trip the real partial intent through the actual parser.
        self.assertEqual(explicit.returncode, 0, explicit.stderr) # Require success for a complete explicit planned subset.
        self.assertEqual(json.loads(explicit.stdout)['intent'], self.validate(self.partial())) # Preserve exact selected targets, reasons and lack of publication authority.
        failed = subprocess.run(command + ['--intent', str(self.directory / 'absent.json')], capture_output=True, text=True, check=False) # Attempt an explicitly named missing selection file.
        self.assertEqual(failed.returncode, 1) # Require a hard failure instead of an automatic fallback.
        self.assertEqual(json.loads(failed.stdout)['type'], 'error') # Preserve structured failure output.
#
if __name__ == '__main__': # Support direct execution as well as unittest discovery.
    unittest.main() # Run only the portable intent-contract tests.
