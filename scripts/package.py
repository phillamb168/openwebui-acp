#!/usr/bin/env python3
"""Make unsigned release archives from a clean, scanned source revision."""
import argparse
import gzip
import hashlib
import io
import json
import pathlib
import struct
import subprocess
import tarfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
TARGETS = {
    'aarch64-apple-darwin': ('macho', 0x0100000C),
    'aarch64-unknown-linux-gnu': ('elf', 183),
    'x86_64-unknown-linux-gnu': ('elf', 62),
}


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def verify_binary(data, target):
    format, machine = TARGETS[target]
    if format == 'elf':
        valid = len(data) >= 20 and data[:6] == b'\x7fELF\x02\x01' and struct.unpack_from('<H', data, 18)[0] == machine
    else:
        valid = len(data) >= 8 and data[:4] == b'\xcf\xfa\xed\xfe' and struct.unpack_from('<I', data, 4)[0] == machine
    if not valid:
        raise SystemExit('Executable format/architecture does not match --target')
    from export_check import PATTERNS
    for rule, pattern in PATTERNS.items():
        if pattern.search(data):
            raise SystemExit(f'Executable export rejected: {rule} (matched content withheld)')


def archive(path, files, epoch):
    # Stable metadata, ordering and gzip header for repeatable packaging.
    with path.open('wb') as output, gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as tar:
            for name, data, executable in sorted(files):
                entry = tarfile.TarInfo(name)
                entry.size, entry.mtime, entry.mode = len(data), epoch, 0o755 if executable else 0o644
                tar.addfile(entry, io.BytesIO(data))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=pathlib.Path)
    parser.add_argument('--target', choices=TARGETS)
    parser.add_argument('--source', action='store_true')
    parser.add_argument('--out', type=pathlib.Path, default=ROOT / 'dist')
    args = parser.parse_args()
    if args.source == bool(args.binary) or bool(args.binary) != bool(args.target):
        parser.error('Choose --source OR --binary PATH --target TARGET')
    subprocess.run(['python3', str(ROOT / 'scripts/export_check.py')], cwd=ROOT, check=True)
    if git('status', '--porcelain').strip():
        raise SystemExit('Commit the reviewed source before packaging; working tree must be clean')
    revision = git('rev-parse', 'HEAD').decode().strip()
    epoch = int(git('show', '-s', '--format=%ct', 'HEAD'))
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    name = f'openwebui-acp-{version}-' + ('source' if args.source else args.target)
    files = []
    for raw in git('ls-files', '-z').split(b'\0'):
        if raw:
            path = pathlib.Path(raw.decode())
            files.append((str(path), (ROOT / path).read_bytes(), False))
    if not args.source:
        data = args.binary.read_bytes()
        verify_binary(data, args.target)
        files.append(('openwebui-acp', data, True))
    info = {
        'name': 'openwebui-acp', 'version': version, 'source_revision': revision,
        'rust_toolchain': '1.98.1', 'target': args.target or 'source',
        'cargo_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
        'signature': 'unsigned; checksums detect changes but do not authenticate a publisher',
    }
    files.append(('BUILD.json', (json.dumps(info, indent=2) + '\n').encode(), False))
    args.out.mkdir(parents=True, exist_ok=True)
    destination = args.out / (name + '.tar.gz')
    archive(destination, [(name + '/' + p, d, x) for p, d, x in files], epoch)
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    (args.out / (destination.name + '.sha256')).write_text(f'{digest}  {destination.name}\n')
    print(f'Packaged {destination.name}; {len(files)} files, SHA-256 sidecar written.')


if __name__ == '__main__':
    main()
