import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import manifest


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.dist = self.root / 'dist'
        self.dist.mkdir()
        (self.root / 'packaging').mkdir()
        self.key = self.root / 'private.pem'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519', '-out', str(self.key)], check=True)
        self.public = subprocess.check_output(['openssl', 'pkey', '-in', str(self.key), '-pubout', '-outform', 'DER'])
        (self.root / 'packaging/update-public-key.hex').write_text(self.public[-32:].hex())
        self.environment = patch.dict(os.environ, {'UPDATE_PRIVATE_KEY': self.key.read_text()})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.publisher = patch.object(manifest, 'ROOT', self.root)
        self.publisher.start()
        self.addCleanup(self.publisher.stop)
        for platform in manifest.PLATFORMS:
            name = f'toolboxcraft-0.1.7-{platform}-update.zip'
            archive = self.dist / name
            archive.write_bytes(b'test package')
            metadata = dict(version='0.1.7', target=platform, package=dict(file=name, size=archive.stat().st_size, sha256=hashlib.sha256(archive.read_bytes()).hexdigest()))
            (self.dist / f'metadata-{platform}.json').write_text(json.dumps(metadata))

    def test_complete_release_has_valid_signature_and_checksums(self):
        manifest.assemble(self.dist, require_signature=True, require_all_platforms=True)
        signature = self.root / 'signature.bin'
        signature.write_bytes(base64.b64decode((self.dist / 'update.json.sig').read_bytes()))
        public = self.root / 'public.der'
        public.write_bytes(self.public)
        subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-keyform', 'DER', '-inkey', str(public), '-rawin', '-in', str(self.dist / 'update.json'), '-sigfile', str(signature)], check=True, stdout=subprocess.DEVNULL)
        self.assertEqual(len(json.loads((self.dist / 'update.json').read_text())['packages']), 5)
        for line in (self.dist / 'SHA256SUMS.txt').read_text().splitlines():
            digest, filename = line.split('  ')
            self.assertEqual(digest, hashlib.sha256((self.dist / filename).read_bytes()).hexdigest())
        self.assertFalse(list(self.dist.glob('metadata-*')))
        self.assertFalse(list(self.dist.glob('*.pem')))

    def test_missing_or_mismatched_private_key_stops_release(self):
        with patch.dict(os.environ, {'UPDATE_PRIVATE_KEY': ''}):
            with self.assertRaisesRegex(RuntimeError, 'must be configured'):
                manifest.assemble(self.dist, require_signature=True)
        (self.root / 'packaging/update-public-key.hex').write_text('ab' * 32)
        with self.assertRaisesRegex(RuntimeError, 'does not match'):
            manifest.assemble(self.dist, require_signature=True)
        self.assertFalse((self.dist / 'update.json.sig').exists())

    def test_incomplete_or_corrupted_packages_stop_release(self):
        (self.dist / 'metadata-windows-arm64.json').unlink()
        with self.assertRaisesRegex(RuntimeError, 'all five'):
            manifest.assemble(self.dist, require_all_platforms=True)
        (self.dist / 'toolboxcraft-0.1.7-macos-universal-update.zip').write_bytes(b'corrupted')
        with self.assertRaisesRegex(RuntimeError, 'does not match'):
            manifest.assemble(self.dist)

    def test_local_unsigned_manifest_removes_stale_signature(self):
        (self.dist / 'update.json.sig').write_bytes(b'stale')
        with patch.dict(os.environ, {'UPDATE_PRIVATE_KEY': ''}):
            manifest.assemble(self.dist)
        self.assertFalse((self.dist / 'update.json.sig').exists())
