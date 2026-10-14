"""Keep published update manifests on the stable release channel."""
import os
from pathlib import Path
import re
import tomllib

STABLE_VERSION = re.compile(
    r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
    r'(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?'
)


def validate_release_version(version, tag=None):
    if not STABLE_VERSION.fullmatch(version):
        raise ValueError('Only stable SemVer versions can be published to the ToolboxCraft update channel')
    if tag is not None and tag != f'v{version}':
        raise ValueError('Tag must match Cargo.toml version')
    return version


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
    tag = os.environ.get('GITHUB_REF_NAME') if os.environ.get('GITHUB_REF_TYPE') == 'tag' else None
    validate_release_version(version, tag)
