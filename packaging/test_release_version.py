import unittest

from release_version import validate_release_version


class ReleaseVersionTests(unittest.TestCase):
    def test_stable_versions_and_matching_tags_are_accepted(self):
        for version in ['0.1.7', '1.0.0', '10.20.30', '1.2.3+build.001']:
            with self.subTest(version=version):
                self.assertEqual(validate_release_version(version, f'v{version}'), version)

    def test_prereleases_cannot_mask_the_last_stable_release(self):
        for version in ['0.1.8-beta.1', '1.0.0-rc.1', '1.2.3-alpha+build.1']:
            with self.subTest(version=version):
                with self.assertRaisesRegex(ValueError, 'Only stable'):
                    validate_release_version(version, f'v{version}')

    def test_mismatched_or_unprefixed_tags_are_rejected(self):
        for tag in ['v0.1.6', '0.1.7', 'v0.1.7-beta.1']:
            with self.subTest(tag=tag):
                with self.assertRaisesRegex(ValueError, 'must match'):
                    validate_release_version('0.1.7', tag)

    def test_manual_runs_still_require_valid_stable_versions(self):
        self.assertEqual(validate_release_version('0.1.7'), '0.1.7')
        for version in ['01.2.3', '1.2', 'v1.2.3', '1.2.3+', '1.2.3+build..1', '1.2.3\n']:
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    validate_release_version(version)
