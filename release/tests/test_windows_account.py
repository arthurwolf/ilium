"""Portable account-custody regressions; no Windows registry or process is used."""  # Bound the evidence claim.
from __future__ import annotations  # Keep fixture annotations portable.
from copy import deepcopy  # Preserve independently compared snapshots.
from pathlib import Path  # Locate the production module.
import sys  # Install import-time native fences.
from types import SimpleNamespace  # Supply explicit native adapter doubles.
import unittest  # Run on every release target.
from unittest import mock  # Inject only external API behavior.
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))  # Support ordinary release/tests discovery.
import windows_account as target  # Import without native initialization.
directory = r'C:\Users\fixture\AppData\Local\Programs\ilium'  # Use a stable Windows fixture path.
product_code = '{12345678-1234-1234-1234-123456789ABC}'  # Identify one synthetic MSI product.
def path_state(value: str | None, value_type: int = 2) -> dict:  # Preserve missing and empty values distinctly.
    return {'exists': value is not None, 'type': None if value is None else value_type, 'value': value}  # Match observable registry state.
class memory_key:  # Model handles without accessing the host registry.
    def __init__(self, identity: tuple) -> None:  # Keep the explicitly opened key identity.
        self.identity = identity  # Permit hive/path/view assertions.
    def __enter__(self):  # Match the registry handle context protocol.
        return self  # Retain the same fixture handle.
    def __exit__(self, *arguments) -> None:  # Close without native resources.
        return None  # Do not suppress production errors.
class memory_registry:  # Execute production read/write logic against memory.
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE = 1, 2  # Distinguish fixture hives.
    REG_SZ, REG_EXPAND_SZ, REG_DWORD = 1, 2, 4  # Preserve real registry type values.
    KEY_READ, KEY_QUERY_VALUE, KEY_SET_VALUE = 0x20019, 1, 2  # Preserve native access masks.
    KEY_WOW64_32KEY, KEY_WOW64_64KEY = 0x200, 0x100  # Keep view selection observable.
    def __init__(self) -> None:  # Initialize a private empty account.
        self.keys, self.writes, self.path_reads = {}, [], 0  # Track all mutable fixture state.
        self.race_at, self.race_value, self.failure, self.write_override = None, None, None, None  # Control independent failure boundaries.
        self.path_identity = (self.HKEY_CURRENT_USER, target.path_key, self.KEY_WOW64_64KEY)  # Identify only account PATH.
        self.keys[self.path_identity] = {}  # Keep the environment key separate from its value.
    def set_path(self, state: dict) -> None:  # Set fixture state outside production writes.
        self.keys[self.path_identity] = {'Path': (state['value'], state['type'])} if state['exists'] else {}  # Preserve original absence.
    def OpenKey(self, hive, path, reserved, access):  # Substitute the actual winreg API boundary.
        if self.failure == 'open':  # Inject an access failure, not absence.
            raise PermissionError('registry open denied')  # Production must propagate this failure.
        identity = (hive, path, access & 0x300)  # Retain explicit native-view selection.
        if identity not in self.keys:  # Missing keys remain distinct from denied keys.
            raise FileNotFoundError(path)  # Exercise the actual absence handling.
        return memory_key(identity)  # Return a scoped memory handle.
    def QueryValueEx(self, key, name):  # Exercise production typed-value reads.
        if self.failure == 'query':  # Simulate a denied value read.
            raise PermissionError('registry query denied')  # Never return a fabricated absent value.
        if key.identity == self.path_identity and name == 'Path':  # Count exact rollback read boundaries.
            self.path_reads += 1  # Keep first, reread and final-handle reads distinct.
            if self.path_reads == self.race_at:  # Inject a concurrent author at one boundary.
                self.set_path(self.race_value)  # Change the observable current state.
        if name not in self.keys[key.identity]:  # Preserve value absence independently.
            raise FileNotFoundError(name)  # Exercise missing-value handling.
        return self.keys[key.identity][name]  # Return raw data and original type.
    def SetValueEx(self, key, name, reserved, value_type, value) -> None:  # Record each attempted scoped write.
        self.writes.append(('set', key.identity, name, value_type, value))  # Detect accidental machine or broad writes.
        if self.failure == 'write':  # Inject rejected account mutation.
            raise PermissionError('registry write denied')  # Require cleanup failure to remain visible.
        self.keys[key.identity][name] = self.write_override or (value, value_type)  # Permit a mismatched post-write readback.
    def DeleteValue(self, key, name) -> None:  # Model only one exact value deletion.
        self.writes.append(('delete', key.identity, name))  # Verify absence restoration is narrowly scoped.
        del self.keys[key.identity][name]  # Leave the environment key intact.
class memory_msi:  # Supply bounded native-query fixtures.
    def __init__(self) -> None:  # Begin with no related product.
        self.codes, self.state, self.version, self.enum_failure, self.version_failure = [], 5, '1.2.3', None, 0  # Separate identity from query failures.
    def MsiEnumRelatedProductsW(self, upgrade, reserved, index, buffer):  # Model the documented enumeration boundary.
        if index == self.enum_failure:  # Inject failure after any admitted prefix.
            return 1610  # A partial inventory must fail closed.
        if index >= len(self.codes):  # Complete an actual finite enumeration.
            return 259  # ERROR_NO_MORE_ITEMS ends the inventory.
        buffer.value = self.codes[index]  # Return one exact product identity.
        return 0  # Admit this enumeration item only.
    def MsiQueryProductStateW(self, code):  # Retain native installation-state distinctions.
        return self.state  # Permit DEFAULT, ADVERTISED, ABSENT and UNKNOWN cases.
    def MsiGetProductInfoW(self, code, attribute, buffer, length):  # Model a registered version query.
        buffer.value, length._obj.value = self.version, len(self.version)  # Write only fixture-owned buffers.
        return self.version_failure  # Allow explicit native query rejection.
@mock.patch.dict(sys.modules, {'winreg': SimpleNamespace()})  # Prevent any accidental real-registry import.
@mock.patch.object(target.ctypes, 'WinDLL', new=mock.Mock(side_effect=AssertionError('native API forbidden')), create=True)  # Forbid native initialization on Windows too.
class windows_account_tests(unittest.TestCase):  # Run real account policy behind fake external adapters.
    def setUp(self) -> None:  # Recreate independent state for every test.
        self.account = target.windows_account.__new__(target.windows_account)  # Bypass every native constructor call.
        self.registry, self.msi = memory_registry(), memory_msi()  # Own all external state in memory.
        self.account.winreg, self.account.msi = self.registry, self.msi  # Bind only safe adapter doubles.
        self.account.kernel = SimpleNamespace(ExpandEnvironmentStringsW=self.expand)  # Keep path expansion deterministic.
    def expand(self, value: str, buffer, capacity: int) -> int:  # Substitute native string expansion.
        buffer.value = value.replace('%LOCALAPPDATA%', r'C:\Users\fixture\AppData\Local')  # Resolve only the authored fixture variable.
        return len(buffer.value) + 1  # Include the native terminating character.
    def test_inno_receipt_reads_exact_types_and_refuses_partial_or_wrong_view(self):
        identity = (self.registry.HKEY_CURRENT_USER, target.inno_key, self.registry.KEY_WOW64_64KEY)
        expected = {'IliumPathReceipt': 1, 'IliumPathExisted': 1, 'IliumPathOwned': 1,
                    'IliumPathBefore': ' Authored;;', 'IliumPathAfter': ' Authored;;' + directory,
                    'IliumPathEntry': directory}
        values = {name: (value, 4 if type(value) is int else 1) for name, value in expected.items()}
        self.registry.keys[identity] = deepcopy(values)
        self.assertEqual(self.account.inno_path_receipt(), expected)
        self.assertEqual(self.registry.writes, [])
        for name, replacement in [('IliumPathReceipt', ('1', 1)), ('IliumPathOwned', (True, 4)),
                                  ('IliumPathBefore', ('expanded', 2)), ('IliumPathEntry', None)]:
            with self.subTest(name=name):
                self.registry.keys[identity] = deepcopy(values)
                if replacement is None:
                    del self.registry.keys[identity][name]
                else:
                    self.registry.keys[identity][name] = replacement
                with self.assertRaises((ValueError, FileNotFoundError)):
                    self.account.inno_path_receipt()
        del self.registry.keys[identity]
        self.registry.keys[(identity[0], identity[1], 0x200)] = values
        with self.assertRaises(FileNotFoundError):
            self.account.inno_path_receipt()
    def put_key(self, hive: int, path: str, view: int, values: dict) -> None:  # Create typed fixture registrations.
        self.registry.keys[(hive, path, view)] = {name: (value, 1) for name, value in values.items()}  # Use plain registry strings.
    def install_msi_registration(self) -> dict:  # Construct the supplied MSI's observable registration.
        self.msi.codes = [product_code]  # Publish one synthetic related product.
        self.put_key(1, r'Software\Ilium', 0x100, {'InstallDirectory': directory + '\\'})  # Preserve the native trailing separator.
        return self.account.snapshot()  # Run actual production snapshot assembly.
    def install_exe_registration(self) -> dict:  # Construct Inno's shared current-user registration.
        values = {'DisplayName': 'Ilium', 'DisplayVersion': '1.2.3', 'Publisher': 'Arthur Wolf', 'InstallLocation': directory + '\\', 'UninstallString': '"' + directory + '\\unins000.exe"', 'QuietUninstallString': '"' + directory + '\\unins000.exe" /SILENT'}  # Bind the exact installed identity.
        for view in (0x100, 0x200):  # Model identical shared HKCU registry views.
            self.put_key(1, target.inno_key, view, values)  # Keep independent fixture copies for disagreement faults.
        return self.account.snapshot()  # Exercise native-view aggregation through the real adapter.
    def test_restore_exact_original_presence_type_and_every_authored_character(self) -> None:  # Force byte/type preservation across common edge cases.
        originals = [path_state(None), path_state('', 1), path_state('', 2), path_state(' C:\\A ;;C:\\A;%LOCALAPPDATA%\\keep; ', 1), path_state('C:\\A;;C:\\A;', 2)]  # Include absence, empties, duplicates and trailing delimiters.
        for original in originals:  # Apply the same contract independently.
            with self.subTest(original=original):  # Retain the precise failing account prestate.
                installed = path_state('known-installed-transition', 2)  # Represent a separately acknowledged native write.
                self.registry.set_path(installed)  # Begin from that exact current state.
                self.registry.writes.clear()  # Isolate this restoration's write evidence.
                self.account.restore_path(deepcopy(original), [deepcopy(installed)])  # Execute the production rollback method.
                self.assertEqual(self.account.read_path(), original)  # Compare complete unexpanded typed state.
                expected = [('set', self.registry.path_identity, 'Path', original['type'], original['value'])] if original['exists'] else [('delete', self.registry.path_identity, 'Path')]  # Specify the only authorized mutation.
                self.assertEqual(self.registry.writes, expected)  # Reject broad, duplicate or machine writes.
                self.assertIn(self.registry.path_identity, self.registry.keys)  # Never delete the Environment key.
    def test_already_original_never_writes(self) -> None:  # Repeated cleanup must be idempotent.
        for original in (path_state(None), path_state('', 1), path_state('A;;A;', 2)):  # Cover distinct original states.
            self.registry.set_path(original)  # Restore fixture state without production mutation.
            self.account.restore_path(original, [])  # An original state needs no acknowledgement.
            self.assertEqual(self.registry.writes, [])  # Assert the absence of any registry write.
    def test_foreign_current_state_is_preserved(self) -> None:  # Reject partial or normalized matches as rollback authority.
        original, acknowledged = path_state('A;;', 1), path_state('A;;owned', 2)  # Separate original bytes from acknowledged bytes.
        for foreign in (path_state('A;;owned;authored', 2), path_state('A;;owned', 1), path_state('A;owned', 2), path_state(None)):  # Vary append, type, delimiter and presence.
            with self.subTest(foreign=foreign):  # Identify the violated custody dimension.
                self.registry.set_path(foreign)  # Apply an unacknowledged external state.
                with self.assertRaisesRegex(ValueError, 'outside acknowledged'):  # Require explicit refusal.
                    self.account.restore_path(original, [acknowledged])  # No speculative transition may authorize this write.
                self.assertEqual(self.account.read_path(), foreign)  # Preserve the foreign author exactly.
                self.assertEqual(self.registry.writes, [])  # Refuse all mutation attempts.
    def test_reread_and_final_handle_races_refuse_writes(self) -> None:  # Force both pre-write comparison boundaries.
        for boundary in (2, 3):  # Race the second read and the final opened handle.
            with self.subTest(boundary=boundary):  # Keep each boundary independently observable.
                original, installed, foreign = path_state('before', 1), path_state('installed', 2), path_state('concurrent;;', 2)  # Use distinct custody states.
                self.registry.set_path(installed)  # Start with a valid acknowledgement.
                self.registry.path_reads, self.registry.race_at, self.registry.race_value = 0, boundary, foreign  # Inject at an exact read boundary.
                with self.assertRaisesRegex(ValueError, 'PATH changed'):  # The production recheck must detect the author.
                    self.account.restore_path(original, [installed])  # Attempt only the acknowledged rollback.
                self.assertEqual(self.account.read_path(), foreign)  # Keep the concurrent state intact.
                self.assertEqual(self.registry.writes, [])  # Assert no stale comparison was used for a write.
    def test_failed_write_and_wrong_readback_remain_failures(self) -> None:  # Require authoritative cleanup readbacks.
        original, installed = path_state('before', 1), path_state('installed', 2)  # Define the legitimate restoration.
        self.registry.set_path(installed)  # Begin with an acknowledged current value.
        self.registry.failure = 'write'  # Deny the actual external write.
        with self.assertRaisesRegex(PermissionError, 'write denied'):  # Do not turn rejected cleanup into success.
            self.account.restore_path(original, [installed])  # Exercise the real setter boundary.
        self.registry.failure, self.registry.write_override = None, ('unexpected-after-write', 2)  # Simulate a different persisted result.
        with self.assertRaisesRegex(ValueError, 'readback differs'):  # Successful setter return is insufficient.
            self.account.restore_path(original, [installed])  # Require the typed final comparison.
    def test_registry_access_failures_never_become_absence(self) -> None:  # Keep unavailable evidence separate from missing state.
        for failure in ('open', 'query'):  # Cover both key and value operations.
            self.registry.failure = failure  # Reject the chosen external read.
            with self.assertRaises(PermissionError):  # No missing-key fallback may swallow denial.
                self.account.read_path()  # Execute actual production read logic.
        self.registry.failure = None  # Restore a readable absent fixture.
        self.assertEqual(self.account.read_path(), path_state(None))  # Actual absence remains supported.
        self.registry.keys[self.registry.path_identity]['Path'] = (7, 4)  # Supply an invalid DWORD PATH.
        with self.assertRaisesRegex(ValueError, 'string type'):  # Never coerce invalid registry data.
            self.account.read_path()  # Fail before any mutation.
    def test_path_aliases_match_without_changing_raw_text(self) -> None:  # Reject equivalent authored installation tokens.
        for raw in ('  "' + directory.upper() + '\\"  ;C:\\keep', directory.replace('\\', '/') + '/', '%LOCALAPPDATA%\\Programs\\ilium;', ';;' + directory + ';;'):  # Cover quotes, case, slash, expansion and empties.
            self.assertTrue(self.account.path_has(raw, directory))  # Match the intended directory identity.
        self.assertFalse(self.account.path_has(directory + '-other;' + directory + '\\child', directory))  # Reject prefix and child false positives.
        self.assertFalse(self.account.path_has(';;  ;', directory))  # Empty tokens cannot claim the installation.
        self.account.kernel.ExpandEnvironmentStringsW = mock.Mock(return_value=0)  # Deny native expansion authority.
        with self.assertRaisesRegex(ValueError, 'expansion failed'):  # Do not accept unknown token identity.
            self.account.path_has(directory, directory)  # Exercise expansion failure explicitly.
    def test_msi_registration_requires_exact_version_single_installed_product(self) -> None:  # Validate current-user product custody.
        good = self.install_msi_registration()  # Read the complete synthetic native registration.
        self.assertEqual(self.account.validate_installed('msi', '1.2.3', Path(directory), good), {'kind': 'MSI', 'product_code': product_code})  # Preserve the exact cleanup identity.
        for version, state, count in (('11.2.30', 5, 1), ('1.2.3', 1, 1), ('1.2.3', 2, 1), ('1.2.3', -1, 1), ('1.2.3', 5, 2), ('1.2.3', 5, 0)):  # Reject substring, advertised, other-user and ambiguous products.
            bad = deepcopy(good)  # Preserve the valid control sample.
            bad['registrations']['msi_products'] = [{'product_code': product_code, 'state': state, 'version': version}] * count  # Inject one independent registration defect.
            with self.subTest(version=version, state=state, count=count), self.assertRaises(ValueError):  # Require every invalid combination to fail.
                self.account.validate_installed('msi', '1.2.3', Path(directory), bad)  # Run the same installed registration gate.
        with self.assertRaisesRegex(ValueError, 'existing MSI'):  # Even valid native installation is foreign to a fresh gate.
            self.account.require_clean(good)  # Protect pre-existing product ownership.
    def test_msi_query_failures_and_partial_enumeration_are_not_empty_accounts(self) -> None:  # Fail closed on incomplete native evidence.
        self.install_msi_registration()  # Start with a valid related product.
        self.msi.enum_failure = 1  # Fail after returning one product.
        with self.assertRaisesRegex(ValueError, 'enumeration failed'):  # Never return a successful partial list.
            self.account.registrations()  # Run the production enumeration loop.
        self.msi.enum_failure, self.msi.version_failure = None, 1608  # Make only the version read fail.
        with self.assertRaisesRegex(ValueError, 'VersionString query failed'):  # Missing registered version blocks custody.
            self.account.registrations()  # Preserve unavailable evidence as failure.
    def test_inno_views_and_uninstaller_identity_are_bound(self) -> None:  # Accept shared HKCU aliases but reject other owners.
        good = self.install_exe_registration()  # Build identical 32/64 current-user observations.
        self.assertEqual(self.account.validate_installed('exe', '1.2.3', Path(directory), good), {'kind': 'EXE', 'uninstaller': directory + '\\unins000.exe'})  # Bind the exact clean-install uninstaller.
        for fault in ('machine', 'alias', 'substring', 'arguments', 'outside', 'different_uninstaller'):  # Exercise independent ownership failures.
            bad = deepcopy(good)  # Keep each mutation isolated.
            if fault == 'machine':  # A machine registration exceeds per-user custody.
                bad['registrations']['inno']['HKLM32'] = deepcopy(bad['registrations']['inno']['HKCU64'])  # Introduce a foreign installation.
            elif fault == 'alias':  # Shared view values must agree.
                bad['registrations']['inno']['HKCU32']['values']['DisplayVersion'] = path_state('other', 1)  # Break only the alias.
            else:  # Keep view aliases consistent while corrupting the semantic identity.
                field, value = ('DisplayVersion', '11.2.30') if fault == 'substring' else ('UninstallString', {'arguments': '"' + directory + '\\unins000.exe" /foreign', 'outside': '"C:\\foreign\\unins000.exe"', 'different_uninstaller': '"' + directory + '\\unins001.exe"'}.get(fault))  # Supply a precise adversarial value.
                for view in ('HKCU32', 'HKCU64'):  # Avoid an alias mismatch hiding the intended assertion.
                    bad['registrations']['inno'][view]['values'][field] = path_state(value, 1)  # Mutate the relevant raw registration field.
            with self.subTest(fault=fault), self.assertRaises(ValueError):  # Reject every invalid native identity.
                self.account.validate_installed('exe', '1.2.3', Path(directory), bad)  # Never select an arbitrary uninstall command.
        with self.assertRaisesRegex(ValueError, 'existing Inno'):  # A clean gate cannot claim this pre-existing installation.
            self.account.require_clean(good)  # Preserve both registry views.
if __name__ == '__main__':  # Permit standalone portable execution.
    unittest.main()  # Report unittest results without native acceptance claims.
