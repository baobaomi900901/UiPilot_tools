"""Capture exact independent dependencies, fixture bytes and uncommitted probe source."""
import hashlib
import json
from pathlib import Path
import re
import sys

source = Path(__file__).resolve().parent
repo = source.parent.parent
output = Path(sys.argv[1])
metadata = json.loads((output / 'cargo-metadata.json').read_text())
packages = {item['id']: item for item in metadata['packages']}
engine = next(item for item in packages.values() if item['name'] == 'rquickjs-sys')
engine_root = Path(engine['manifest_path']).parent
header = (engine_root / 'quickjs/quickjs.h').read_text()
version = '.'.join(re.search(rf'#define QJS_VERSION_{part}\s+(\d+)', header)[1] for part in ['MAJOR', 'MINOR', 'PATCH'])
assert version == '0.16.2'
features = {}
expected_features = {
    'rquickjs': {'std', 'loader'},
    'rquickjs-core': {'std', 'loader', 'relative-path'},
    'rquickjs-sys': set(),
}
for node in metadata['resolve']['nodes']:
    name = packages[node['id']]['name']
    if name in ['rquickjs', 'rquickjs-core', 'rquickjs-sys']:
        assert packages[node['id']]['version'] == '0.14.0'
        assert set(node['features']) == expected_features[name], (name, node['features'])
        features[name] = node['features']
assert set(features['rquickjs']) == {'std', 'loader'}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

sources = sorted(path for path in source.rglob('*') if path.is_file() and not {'target', '__pycache__'}.intersection(path.parts))
shared = source.parent / 'macos-jsc-esm-probe/fixtures'
sources += [shared / f'{name}.js' for name in ['strict', 'private-channel', 'intrinsics', 'wrong-request', 'oversized', 'throw']]
sources += [repo / 'examples/public-plugins/com.uipilot.demo-return/package/dist/runtime.js']
(output / 'source-SHA256SUMS').write_text(''.join(f'{digest(path)}  {path.relative_to(repo)}\n' for path in sources))
(output / 'dependency-evidence.json').write_text(json.dumps({
    'rquickjs': '0.14.0', 'quickjsNg': version, 'features': features,
    'cargoLockSha256': digest(source / 'Cargo.lock'),
    'sysHeaderSha256': digest(engine_root / 'quickjs/quickjs.h'),
    'sysSourceSha256': digest(engine_root / 'quickjs/quickjs.c'),
    'sysBuildSha256': digest(engine_root / 'build.rs'),
    'helperSha256': digest(output / 'helper'),
    'sysCargoVcs': json.loads((engine_root / '.cargo_vcs_info.json').read_text()),
}, indent=2) + '\n')
