"""Keep the root README's download links aligned with release outputs."""

from pathlib import Path
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

    def test_quick_start_commands_match_public_installer_contract(self):
        readme = (PROJECT_ROOT / "README.md").read_text(encoding="utf-8")

        self.assertIn(release_pipeline.POSIX_COMMAND, readme)
        self.assertIn(release_pipeline.WINDOWS_COMMAND, readme)


if __name__ == "__main__":
    unittest.main()
