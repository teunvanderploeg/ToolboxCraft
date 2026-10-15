import unittest
from signing import validate_distribution_signing, validate_release_credentials


class SigningTests(unittest.TestCase):
    def test_public_releases_reject_unsigned_windows_and_ad_hoc_macos(self):
        for platform in ['macos-universal', 'windows-x64', 'windows-arm64']:
            with self.assertRaises(RuntimeError):
                validate_distribution_signing(platform, required=True, environ={})
        validate_distribution_signing('linux-x86_64', required=True, environ={})

    def test_local_artifacts_can_be_unsigned_but_partial_notarization_is_rejected(self):
        validate_distribution_signing('macos-universal', environ={})
        validate_distribution_signing('windows-x64', environ={})
        with self.assertRaisesRegex(RuntimeError, 'requires a Developer ID'):
            validate_distribution_signing('macos-universal', environ={'MACOS_NOTARY_PROFILE': 'profile'})

    def test_correct_distribution_identity_and_credentials_are_required(self):
        env = {'MACOS_SIGN_IDENTITY': 'Developer ID Application: Publisher (TEAM)', 'MACOS_NOTARY_PROFILE': 'profile'}
        validate_distribution_signing('macos-universal', required=True, environ=env)
        env['MACOS_SIGN_IDENTITY'] = 'Apple Development: Publisher'
        with self.assertRaisesRegex(RuntimeError, 'Developer ID Application'):
            validate_distribution_signing('macos-universal', required=True, environ=env)
        validate_distribution_signing('windows-x64', required=True, environ={'WINDOWS_SIGN_PFX': 'certificate.pfx'})
        with self.assertRaisesRegex(RuntimeError, 'MACOS_CERTIFICATE_BASE64'):
            validate_release_credentials({})
