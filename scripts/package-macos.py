#!/usr/bin/env python3
"""Package only the distributable app and public notices; never local state."""
import argparse
import hashlib
import json
import plistlib
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--notarized', action='store_true', help='Require Developer ID and stapled notarization; never re-sign')
args = parser.parse_args()
version = json.loads((ROOT / 'package.json').read_text())['version']
app = ROOT / 'src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Glint.app'
binary = app / 'Contents/MacOS/glint'
assert binary.is_file(), 'Build the arm64 .app first.'
info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
assert info.get('LSUIElement') is True, 'Agent mode is required.'
assert info['CFBundleShortVersionString'] == version, 'Stale app version.'
assert subprocess.check_output(['lipo', '-archs', str(binary)], text=True).strip() == 'arm64'
if args.notarized:
    signature = subprocess.run(['codesign', '-dv', '--verbose=4', str(app)], capture_output=True, text=True, check=True)
    assert 'Authority=Developer ID Application:' in signature.stderr, 'Developer ID signature missing.'
    subprocess.run(['xcrun', 'stapler', 'validate', str(app)], check=True)
    subprocess.run(['spctl', '--assess', '--type', 'execute', str(app)], check=True)
else:
    subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(app)], check=True)
subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
output = ROOT / 'release/share'
output.mkdir(parents=True, exist_ok=True)
suffix = 'notarized' if args.notarized else 'adhoc'
name = f'glint-{version}-macos-arm64-{suffix}'
archive = output / f'{name}.zip'
with tempfile.TemporaryDirectory(prefix='quota-share-') as temp:
    folder = Path(temp) / name
    folder.mkdir()
    subprocess.run(['ditto', str(app), str(folder / app.name)], check=True)
    for filename in ['LICENSE', 'NOTICE', 'README.md', 'PRIVACY.md']:
        shutil.copy2(ROOT / filename, folder / filename)
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(folder), str(archive)], check=True)
    restored = Path(temp) / 'restored'
    subprocess.run(['ditto', '-x', '-k', str(archive), str(restored)], check=True)
    extracted = restored / name / app.name
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(extracted)], check=True)
    assert (extracted / 'Contents/MacOS/glint').read_bytes() == binary.read_bytes()
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix('.sha256').write_text(f'{checksum}  {archive.name}\n')
print(json.dumps({'archive': str(archive), 'bytes': archive.stat().st_size, 'sha256': checksum, 'signature': suffix}, indent=2))
