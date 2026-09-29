"""Portable contract checks; native behavior is covered by Install.Tests.ps1."""
import pathlib
import re
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class WindowsInstallerSourceTests(unittest.TestCase):
    def test_generated_table_matches_authoritative_manifest(self):
        source = (ROOT / 'release/install.ps1').read_text(encoding='utf-8-sig')
        targets = tomllib.loads((ROOT / 'release/targets.toml').read_text())['target']
        block = source.split('# BEGIN GENERATED WINDOWS TARGETS\n')[1].split('\n# END GENERATED WINDOWS TARGETS')[0]
        expected = "$checksum_archives = @(" + ', '.join("'" + row['archive'] + "'" for row in targets) + ")\n"
        expected += "$windows_targets = @{\n"
        for row in targets:
            if row['os'] == 'windows':
                expected += "    'AMD64' = @{ archive = '" + row['archive'] + "'; target = '" + row['rust_target'] + "'; prefix = '" + row['archive'].removesuffix('.zip') + "' }\n"
        expected += "}"
        self.assertEqual(block, expected)

    def test_installer_does_not_change_execution_policy_or_machine_path(self):
        source = (ROOT / 'release/install.ps1').read_text(encoding='utf-8-sig')
        self.assertNotRegex(source, r'(?i)Set-ExecutionPolicy|ExecutionPolicy\s+Bypass|Stop-Process|taskkill|Expand-Archive|Invoke-Expression')
        self.assertNotRegex(source, r"SetEnvironmentVariable\([^\n]+['\"]Machine['\"]")

    def test_native_suite_and_installer_exist(self):
        self.assertTrue((ROOT / 'release/install.ps1').is_file())
        source = (ROOT / 'release/tests/Install.Tests.ps1').read_text(encoding='utf-8-sig')
        for gate in ('checksum', 'traversal', 'reparse', 'rollback', 'concurrent', 'uninstall', 'Unicode', 'ARM64', 'TLS', 'locked'):
            self.assertRegex(source.lower(), re.escape(gate.lower()))


if __name__ == '__main__':
    unittest.main()
