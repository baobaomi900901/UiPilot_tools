"""Record this uncommitted spike's actual source and generated artifact hashes."""
import hashlib
from pathlib import Path
import sys

source = Path(__file__).resolve().parent
output = Path(sys.argv[1])

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

sources = sorted(path for path in source.rglob('*') if path.is_file() and '__pycache__' not in path.parts)
# The transport implementation is reused verbatim, not copied or weakened.
sources += [source.parent / 'macos-jsc-runtime-probe' / 'supervisor.py']
(output / 'source-SHA256SUMS').write_text(''.join(
    f'{digest(path)}  {path.relative_to(source.parent)}\n' for path in sources))
artifacts = sorted(path for path in output.rglob('*') if path.is_file() and path.name in ['helper', 'bundle.js', 'fixture.h', 'modules.json'])
(output / 'artifact-SHA256SUMS').write_text(''.join(
    f'{digest(path)}  {path.relative_to(output)}\n' for path in artifacts))
