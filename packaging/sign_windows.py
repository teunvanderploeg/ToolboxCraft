"""Sign and verify an executable without including credentials in error messages."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def verify(executable):
    subprocess.run(['pwsh', '-NoProfile', '-File', str(ROOT / 'packaging/verify_windows_signature.ps1'), '-Path', str(executable)], check=True)


def sign(executable):
    result = subprocess.run([
        'signtool', 'sign', '/f', os.environ['WINDOWS_SIGN_PFX'],
        '/p', os.environ.get('WINDOWS_SIGN_PASSWORD', ''), '/fd', 'SHA256',
        '/tr', 'http://timestamp.digicert.com', '/td', 'SHA256',
        '/d', 'ToolboxCraft', '/du', 'https://github.com/teunvanderploeg/ToolboxCraft',
        str(executable),
    ])
    if result.returncode:
        raise RuntimeError(f'Windows code signing failed for {Path(executable).name}')
    verify(executable)


if __name__ == '__main__':
    sign(Path(sys.argv[1]))
