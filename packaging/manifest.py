#!/usr/bin/env python3
"""Merge platform metadata, generate checksums, optionally sign the exact manifest bytes."""
import base64, hashlib, json, os, pathlib, subprocess
root = pathlib.Path('dist')
records = [json.loads(p.read_text()) for p in root.glob('metadata-*.json')]
assert records, 'No platform metadata found'
version = records[0]['version']
assert all(r['version'] == version for r in records), 'Versions differ between platform builds'
manifest = dict(version=version, packages={r['target']: r['package'] for r in records})
(root / 'update.json').write_bytes(json.dumps(manifest, sort_keys=True, separators=(',', ':')).encode())
if os.environ.get('UPDATE_PRIVATE_KEY'):
    # OpenSSL pkeyutl accepts an Ed25519 PEM key and produces the raw 64-byte signature.
    key = root / '.signing-key.pem'
    key.write_text(os.environ['UPDATE_PRIVATE_KEY'])
    key.chmod(0o600)
    try:
        signature = subprocess.check_output(['openssl', 'pkeyutl', '-sign', '-inkey', str(key), '-rawin', '-in', str(root / 'update.json')])
        (root / 'update.json.sig').write_bytes(base64.b64encode(signature))
    finally: key.unlink()
files = [p for p in root.iterdir() if p.is_file() and not p.name.startswith('metadata-') and p.name != 'SHA256SUMS.txt']
(root / 'SHA256SUMS.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in sorted(files)))
for p in root.glob('metadata-*.json'): p.unlink()
