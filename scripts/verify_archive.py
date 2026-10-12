#!/usr/bin/env python3
"""Verify a package against its clean source/build, then extract only verified files."""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import tarfile
import tomllib

from package import ROOT, TARGETS, git, verify_binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=pathlib.Path)
    parser.add_argument('--binary', type=pathlib.Path)
    parser.add_argument('--target', choices=TARGETS)
    parser.add_argument('--source', action='store_true')
    parser.add_argument('--extract-to', type=pathlib.Path, required=True)
    args = parser.parse_args()
    if args.source == bool(args.binary) or bool(args.binary) != bool(args.target):
        parser.error('Choose --source OR --binary PATH --target TARGET')
    if args.extract_to.exists():
        raise SystemExit('Extraction destination must not already exist')
    digest = hashlib.sha256(args.archive.read_bytes()).hexdigest()
    checksum = args.archive.with_name(args.archive.name + '.sha256').read_text()
    if checksum != f'{digest}  {args.archive.name}\n':
        raise SystemExit('Archive checksum mismatch')

    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    target = args.target or 'source'
    root = f'openwebui-acp-{version}-{target}'
    if args.archive.name != root + '.tar.gz':
        raise SystemExit('Unexpected archive name')
    expected = {
        raw.decode(): (ROOT / raw.decode()).read_bytes()
        for raw in git('ls-files', '-z').split(b'\0') if raw
    }
    if args.binary:
        expected['openwebui-acp'] = args.binary.read_bytes()
        verify_binary(expected['openwebui-acp'], args.target)
        if args.target.endswith('linux-gnu'):
            versions = {
                tuple(map(int, match.split(b'.')))
                for match in re.findall(rb'GLIBC_([0-9]+\.[0-9]+)', expected['openwebui-acp'])
            }
            if not versions or max(versions) > (2, 34):
                raise SystemExit('Executable exceeds the documented glibc 2.34 baseline')
    actual = {}
    with tarfile.open(args.archive, 'r:gz') as archive:
        for member in archive:
            path = pathlib.PurePosixPath(member.name)
            if (not member.isfile() or path.is_absolute() or '..' in path.parts
                    or len(path.parts) < 2 or path.parts[0] != root):
                raise SystemExit('Unexpected archive member type or path')
            relative = str(path.relative_to(root))
            if relative in actual or member.size > 128 * 1024 * 1024:
                raise SystemExit('Duplicate or oversized archive member')
            if relative not in expected and relative != 'BUILD.json':
                raise SystemExit('Unexpected archive file')
            mode = 0o755 if relative == 'openwebui-acp' else 0o644
            if member.mode != mode:
                raise SystemExit('Unexpected archive file permissions')
            actual[relative] = archive.extractfile(member).read()
    if set(actual) != set(expected) | {'BUILD.json'}:
        raise SystemExit('Archive file inventory mismatch')
    if any(actual[name] != data for name, data in expected.items()):
        raise SystemExit('Archive contents differ from the source/build')
    info = json.loads(actual['BUILD.json'])
    metadata = {
        'name': 'openwebui-acp', 'version': version, 'target': target,
        'source_revision': git('rev-parse', 'HEAD').decode().strip(),
        'rust_toolchain': '1.98.1',
        'cargo_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
    }
    if any(info.get(key) != value for key, value in metadata.items()):
        raise SystemExit('Archive build metadata mismatch')

    # No tar extraction: paths, file types, bytes and permissions are checked above.
    args.extract_to.mkdir(parents=True, exist_ok=False)
    for name, data in actual.items():
        destination = args.extract_to / root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
        destination.chmod(0o755 if name == 'openwebui-acp' else 0o644)
    if args.binary:
        output = subprocess.check_output(
            [str((args.extract_to / root / 'openwebui-acp').resolve()), '--version'],
            timeout=15, text=True,
        ).strip()
        if output != f'openwebui-acp {version}':
            raise SystemExit('Extracted executable version mismatch')
    print(f'Verified {args.archive.name}: checksum, {len(actual)} files, source/build and metadata.')


if __name__ == '__main__':
    main()
