"""Validate signing setup before producing public distribution packages."""
import os


def validate_distribution_signing(platform, required=False, environ=None):
    env = os.environ if environ is None else environ
    if platform.startswith('macos'):
        identity = env.get('MACOS_SIGN_IDENTITY', '')
        profile = env.get('MACOS_NOTARY_PROFILE', '')
        if profile and not identity:
            raise RuntimeError('macOS notarization requires a Developer ID Application signing identity')
        if identity and not identity.startswith('Developer ID Application:'):
            raise RuntimeError('Use a Developer ID Application certificate for macOS distribution')
        if required and (not identity or not profile):
            raise RuntimeError('Public macOS releases require Developer ID signing and notarization credentials')
    elif platform.startswith('windows'):
        if required and not env.get('WINDOWS_SIGN_PFX'):
            raise RuntimeError('Public Windows releases require a trusted code-signing certificate')


def validate_release_credentials(environ=None):
    env = os.environ if environ is None else environ
    required = [
        'MACOS_CERTIFICATE_BASE64', 'MACOS_CERTIFICATE_PASSWORD', 'MACOS_SIGN_IDENTITY',
        'MACOS_NOTARY_APPLE_ID', 'MACOS_NOTARY_TEAM_ID', 'MACOS_NOTARY_PASSWORD',
        'WINDOWS_CERTIFICATE_BASE64',
    ]
    missing = [name for name in required if not env.get(name)]
    if missing:
        raise RuntimeError('Configure distribution-signing repository secrets before publishing: ' + ', '.join(missing))
    validate_distribution_signing('macos-universal', True, {
        'MACOS_SIGN_IDENTITY': env['MACOS_SIGN_IDENTITY'], 'MACOS_NOTARY_PROFILE': 'configured',
    })


if __name__ == '__main__':
    validate_release_credentials()
