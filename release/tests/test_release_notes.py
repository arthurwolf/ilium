"""Release notes must enumerate the exact qualified publication assets."""
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import release_pipeline as pipeline


class ReleaseNotesTests(unittest.TestCase):
    def test_notes_list_every_candidate_asset_once_in_stable_order(self):
        assets = {'ilium-windows-x86_64.zip': 'a' * 64,
                  'SHA256SUMS': 'b' * 64,
                  'ilium-linux-x86_64.deb': 'c' * 64}
        notes = pipeline.release_notes(assets)
        self.assertEqual([line for line in notes.splitlines() if line.startswith('- ')],
                         ['- `SHA256SUMS`', '- `ilium-linux-x86_64.deb`',
                          '- `ilium-windows-x86_64.zip`'])

    def test_notes_reject_empty_or_unsafe_asset_names(self):
        for assets in ({}, {'bad`name.zip': 'a' * 64}, {'../archive.zip': 'a' * 64}):
            with self.subTest(assets=assets), self.assertRaises(ValueError):
                pipeline.release_notes(assets)

    def test_draft_posts_the_same_inventory_recorded_in_its_intent(self):
        assets = {'SHA256SUMS': 'a' * 64, 'ilium-linux-x86_64.deb': 'b' * 64}
        paths = [Path(name) for name in assets]
        metadata = {'tag': 'v0.1.1', 'commit': 'c' * 40}
        arguments = SimpleNamespace(candidate=Path('/candidate'), manifest=Path('/manifest'),
                                    workspace=Path('/workspace'), output=Path('/draft.json'))
        def github(path, *, method='GET', **kwargs):
            if path == 'immutable-releases':
                return {'enabled': True}
            if path == 'releases' and method == 'POST':
                self.assertEqual(kwargs['payload']['body'], pipeline.release_notes(assets))
                return {'id': 42, 'upload_url': 'https://uploads.github.com/upload{?name}'}
            return {}

        with (patch.object(pipeline, 'candidate_data', return_value=(metadata, [])),
              patch.object(pipeline.release_tool, 'read_json', return_value={}),
              patch.object(pipeline, 'validate_qualified_publication'),
              patch.object(pipeline, 'require_release_absent'),
              patch.object(pipeline, 'require_remote_tag'),
              patch.object(pipeline, 'publication_files', return_value=paths),
              patch.object(pipeline, 'asset_hashes', return_value=assets),
              patch.object(pipeline, 'github', side_effect=github),
              patch.object(pipeline, 'validate_release'),
              patch.object(pipeline, 'write_json') as write_json,
              patch.object(pipeline, 'gh_output'), patch.object(pipeline, 'emit')):
            pipeline.draft(arguments)
        self.assertEqual(write_json.call_args_list[0].args[1]['assets'], assets)


if __name__ == '__main__':
    unittest.main()
