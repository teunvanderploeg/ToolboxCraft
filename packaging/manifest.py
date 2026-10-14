#!/usr/bin/env python3
"""Assemble release metadata and sign it using the launcher's pinned update key."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

from release_version import validate_release_version

ROOT = pathlib.Path(__file__).resolve().parents[1]
PLATFORMS = {'macos-universal', 'windows-x64', 'windows-arm64', 'linux-x86_64', 'linux-aarch64'}


def signing_key(directory):
    private = os.environ.get('UPDATE_PRIVATE_KEY')
    if not private:
        raise RuntimeError('TOOLBOXCRAFT_UPDATE_PRIVATE_KEY must be configured before releasing')
    key = directory / 'private.pem'
    key.write_text(private)
    key.chmod(0o600)
    public = subprocess.check_output(['openssl', 'pkey', '-in', str(key), '-pubout', '-outform', 'DER'])
    expected = bytes.fromhex((ROOT / 'packaging/update-public-key.hex').read_text().strip())
    # SubjectPublicKeyInfo for a 32-byte Ed25519 public key.
    if len(expected) != 32 or public != bytes.fromhex('302a300506032b6570032100') + expected:
        raise RuntimeError('Release signing key does not match the public key embedded in ToolboxCraft')
    return key


def assemble(root, require_signature=False, require_all_platforms=False):
    records = [json.loads(p.read_text()) for p in sorted(root.glob('metadata-*.json'))]
    if not records:
        raise RuntimeError('No platform metadata found')
    version = records[0]['version']
    validate_release_version(version)
    packages = {}
    for record in records:
        target = record['target']
        if record['version'] != version or target not in PLATFORMS or target in packages:
            raise RuntimeError('Invalid, duplicate, or inconsistent platform metadata')
        package = record['package']
        expected_name = f'toolboxcraft-{version}-{target}-update.zip'
        if package['file'] != expected_name:
            raise RuntimeError('Unexpected update archive filename')
        archive = root / expected_name
        if archive.stat().st_size != package['size'] or hashlib.sha256(archive.read_bytes()).hexdigest() != package['sha256']:
            raise RuntimeError('Update archive does not match its metadata')
        packages[target] = package
    if require_all_platforms and set(packages) != PLATFORMS:
        raise RuntimeError('Release requires update packages for all five platforms')
    manifest = root / 'update.json'
    manifest.write_bytes(json.dumps(dict(version=version, packages=packages), sort_keys=True, separators=(',', ':')).encode())
    signature_path = root / 'update.json.sig'
    signature_path.unlink(missing_ok=True)
    if os.environ.get('UPDATE_PRIVATE_KEY') or require_signature:
        with tempfile.TemporaryDirectory(prefix='toolboxcraft-sign-') as directory:
            key = signing_key(pathlib.Path(directory))
            signature = subprocess.check_output(['openssl', 'pkeyutl', '-sign', '-inkey', str(key), '-rawin', '-in', str(manifest)])
            signature_path.write_bytes(base64.b64encode(signature))
    files = [p for p in root.iterdir() if p.is_file() and not p.name.startswith('metadata-') and p.name != 'SHA256SUMS.txt']
    (root / 'SHA256SUMS.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in sorted(files)))
    for p in root.glob('metadata-*.json'):
        p.unlink()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--require-signature', action='store_true')
    parser.add_argument('--require-all-platforms', action='store_true')
    parser.add_argument('--check-signing-key', action='store_true')
    args = parser.parse_args()
    if args.check_signing_key:
        with tempfile.TemporaryDirectory(prefix='toolboxcraft-sign-') as directory:
            signing_key(pathlib.Path(directory))
    else:
        assemble(pathlib.Path('dist'), args.require_signature, args.require_all_platforms)
