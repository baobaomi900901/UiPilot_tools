"""Check clean DMG extraction and bundle replacement in a disposable Applications directory."""
import argparse
import hashlib
import json
import plistlib
import subprocess
import tempfile
from contextlib import contextmanager
from pathlib import Path


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout.strip()


def digest(path):
    sha = hashlib.sha256()
    with path.open('rb') as file:
        for block in iter(lambda: file.read(1024 * 1024), b''):
            sha.update(block)
    return sha.hexdigest()


def inspect(app):
    with (app / 'Contents/Info.plist').open('rb') as file:
        info = plistlib.load(file)
    if info['CFBundleIdentifier'] != 'com.uipilot.launcher':
        raise ValueError('Unexpected bundle identifier')
    binary = app / 'Contents/MacOS/UiPilot_tools'
    if info['CFBundleExecutable'] != binary.name or run('lipo', '-archs', binary) != 'arm64':
        raise ValueError('Unexpected main executable or architecture')
    run('codesign', '--verify', '--deep', '--strict', app)
    return {'version': info['CFBundleShortVersionString'], 'executableSHA256': digest(binary)}


@contextmanager
def mounted(image, mount):
    mount.mkdir()
    run('hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', mount, image)
    try:
        yield mount
    finally:
        run('hdiutil', 'detach', mount)


def check(image, previous, expected):
    actual = digest(image)
    if actual != expected.lower():
        raise ValueError('DMG SHA-256 does not match candidate')
    # Use a new private directory for every run. Never remove an installed app.
    with tempfile.TemporaryDirectory(prefix='uipilot-install-check-') as scratch:
        root = Path(scratch)
        apps = root / 'Applications'
        apps.mkdir()
        with mounted(image, root / 'volume') as volume:
            candidates = [p for p in volume.glob('*.app') if p.is_dir() and not p.is_symlink()]
            if len(candidates) != 1:
                raise ValueError('Expected exactly one application in the DMG')
            source, = candidates
            target = apps / source.name
            candidate = inspect(source)
            run('ditto', source, target)
            if inspect(target) != candidate:
                raise ValueError('Clean installation differs from candidate')
            target.rename(root / 'clean-install.app')
            old = inspect(previous)
            if old['executableSHA256'] == candidate['executableSHA256']:
                raise ValueError('Replacement test requires a distinct prior build')
            run('ditto', previous, target)
            if inspect(target) != old:
                raise ValueError('Could not stage previous installation')
            staged = apps / 'replacement.app'
            run('ditto', source, staged)
            target.rename(root / 'previous.app')
            staged.rename(target)
            if inspect(target) != candidate:
                raise ValueError('Replacement differs from candidate')
    return {'dmgSHA256': actual, 'previous': old, 'installed': candidate,
            'cleanCopy': 'passed', 'bundleReplacement': 'passed'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dmg', type=Path)
    parser.add_argument('--previous-app', required=True, type=Path)
    parser.add_argument('--sha256', required=True)
    args = parser.parse_args()
    print(json.dumps(check(args.dmg.resolve(), args.previous_app.resolve(), args.sha256), indent=2))
