"""Keep the root README's download links aligned with release outputs."""

from pathlib import Path
import hashlib
import re
import sys
import unittest


PROJECT_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = PROJECT_ROOT / "release" / "targets.toml"
sys.path.insert(0, str(PROJECT_ROOT / "release" / "scripts"))
import build_linux_packages
import build_macos_packages
import build_windows_installers
import release_pipeline
import release_tool


LATEST_DOWNLOAD = re.compile(
    r"https://github\.com/arthurwolf/ilium/releases/latest/download/([^\s)]+)"
)


class ReadmeReleaseAssetTests(unittest.TestCase):
    def test_root_readme_links_every_published_user_download(self):
        targets = release_tool.load_targets(MANIFEST)
        expected = {target["archive"] for target in targets}
        expected.add("SHA256SUMS")
        for architecture in ("x86_64", "aarch64"):
            expected.update(build_linux_packages.package_names(architecture))
            expected.update(build_macos_packages.package_names(architecture))
        expected.update(build_windows_installers.INSTALLER_NAMES)

        readme = (PROJECT_ROOT / "README.md").read_text(encoding="utf-8")
        actual = set(LATEST_DOWNLOAD.findall(readme))

        self.assertEqual(
            actual,
            expected,
            "README direct download links differ from the release asset inventory; "
            f"missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}",
        )

    def test_official_archives_match_rust_and_packaging_inventory(self):
        source = (PROJECT_ROOT / "ilium-animation-js/src/release.rs").read_text()
        table = source.split("pub const PACKAGES:", 1)[1].split("];", 1)[0]
        rows = re.findall(
            r'\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"([0-9a-f]{64})"\s*,?\s*\)',
            table,
        )
        inventory = {filename: digest for _identifier, filename, digest in rows}
        self.assertEqual(len(rows), len(inventory), "duplicate Rust release package")
        self.assertEqual(inventory, release_tool.APPROVED_PACKAGES)
        for filename, digest in inventory.items():
            with self.subTest(package=filename):
                archive = PROJECT_ROOT / "ilium-animation-js/assets/packages" / filename
                self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), digest)

    def test_both_installers_pin_current_official_archives(self):
        for installer in ("install.sh", "install.ps1"):
            lines = (PROJECT_ROOT / "release" / installer).read_text().splitlines()
            for filename, expected in release_tool.APPROVED_PACKAGES.items():
                with self.subTest(installer=installer, package=filename):
                    digests = [digest for line in lines if filename in line
                               for digest in re.findall(r"[0-9a-f]{64}", line)]
                    self.assertGreaterEqual(len(digests), 2,
                                            "install and existing-install checks need release pins")
                    self.assertEqual(set(digests), {expected})

    def test_quick_start_commands_match_public_installer_contract(self):
        readme = (PROJECT_ROOT / "README.md").read_text(encoding="utf-8")

        self.assertIn(release_pipeline.POSIX_COMMAND, readme)
        self.assertIn(release_pipeline.WINDOWS_COMMAND, readme)


if __name__ == "__main__":
    unittest.main()
