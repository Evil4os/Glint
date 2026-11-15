#!/usr/bin/env python3
"""Export an explicit source allowlist instead of zipping the working directory."""
import hashlib
import json
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED

ROOT = Path(__file__).resolve().parents[1]
version = json.loads((ROOT / 'package.json').read_text())['version']
name = f'glint-{version}-source'
files = set()
for directory in ['src', 'tests', 'scripts', 'docs', '.github', 'src-tauri/src', 'src-tauri/icons', 'src-tauri/capabilities']:
    files.update(path for path in (ROOT / directory).rglob('*') if path.is_file() and not path.is_symlink())
for filename in ['README.md', 'PRIVACY.md', 'LICENSE', 'NOTICE', 'PRODUCT.md', 'CONTRIBUTING.md', 'SECURITY.md', '.gitignore', 'package.json', 'package-lock.json', 'index.html', 'vite.config.ts', 'tsconfig.json', 'tsconfig.node.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'src-tauri/tauri.conf.json', 'src-tauri/Info.plist', 'src-tauri/build.rs']:
    path = ROOT / filename
    if path.exists(): files.add(path)
files = {p for p in files if p.name != '.DS_Store' and '__pycache__' not in p.parts and p.suffix != '.pyc'}
for path in files:
    assert not path.name.startswith('.env')
    assert path.name not in ['auth.json', 'credentials.json', 'token.json']
    if path.suffix in ['.md', '.rs', '.ts', '.tsx', '.json', '.py', '.yml', '.css']:
        text = path.read_text()
        assert str(Path.home()) not in text and ('/var/' + 'folders/') not in text, f'Local path in {path.relative_to(ROOT)}'
output = ROOT / 'release/share'
output.mkdir(parents=True, exist_ok=True)
archive = output / f'{name}.zip'
with ZipFile(archive, 'w', ZIP_DEFLATED) as package:
    for path in sorted(files): package.write(path, Path(name) / path.relative_to(ROOT))
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix('.sha256').write_text(f'{checksum}  {archive.name}\n')
print(json.dumps({'archive': str(archive), 'files': len(files), 'sha256': checksum}, indent=2))
