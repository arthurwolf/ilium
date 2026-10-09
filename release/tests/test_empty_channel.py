"""Empty-channel checks never mistake an available installer for an absent one."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import release_pipeline as pipeline


class EmptyChannelTests(unittest.TestCase):
    def test_unconfigured_host_checks_all_routes_after_gateway_errors(self):
        with patch.object(pipeline, 'missing_response', side_effect=pipeline.HTTPFailure(522)) as missing:
            pipeline.verify_absent_installers('https://ilium-setup.pages.dev', allow_unconfigured=True)
        self.assertEqual(missing.call_count, 3)

    def test_available_installer_after_gateway_error_still_fails(self):
        with patch.object(pipeline, 'missing_response', side_effect=[
            pipeline.HTTPFailure(522), pipeline.release_tool.ReleaseError('expected absent endpoint is publicly available')
        ]) as missing:
            with self.assertRaisesRegex(ValueError, 'publicly available'):
                pipeline.verify_absent_installers('https://ilium-setup.pages.dev', allow_unconfigured=True)
        self.assertEqual(missing.call_count, 2)

    def test_existing_channel_requires_404(self):
        with patch.object(pipeline, 'missing_response', side_effect=pipeline.HTTPFailure(522)):
            with self.assertRaises(pipeline.HTTPFailure):
                pipeline.verify_absent_installers('https://ilium-setup.pages.dev')

    def test_gateway_tolerance_requires_authoritatively_empty_channels(self):
        baseline = {'previous_production': None, 'previous_latest': None}
        with patch.object(pipeline, 'assert_latest') as latest, patch.object(pipeline, 'assert_production') as production, patch.object(pipeline, 'verify_absent_installers') as absent:
            pipeline.verify_baseline_bytes(baseline)
        latest.assert_called_once_with(baseline)
        production.assert_called_once_with(baseline)
        absent.assert_called_once_with('https://' + pipeline.pages.HOST, allow_unconfigured=True)

    def test_changed_production_prevents_gateway_tolerance(self):
        baseline = {'previous_production': None, 'previous_latest': None}
        with patch.object(pipeline, 'assert_latest'), patch.object(pipeline, 'assert_production', side_effect=ValueError('production changed')), patch.object(pipeline, 'verify_absent_installers') as absent:
            with self.assertRaisesRegex(ValueError, 'production changed'):
                pipeline.verify_baseline_bytes(baseline)
        absent.assert_not_called()


if __name__ == '__main__':
    unittest.main()
