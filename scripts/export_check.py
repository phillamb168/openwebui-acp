#!/usr/bin/env python3
"""Inspect only Git-visible source files; print paths/rules, never matched secrets."""
import argparse, pathlib, re, shutil, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
ALLOWED_ROOTS = {'.github', 'src', 'tests', 'examples', 'docs', 'deploy', 'licenses', 'scripts'}
ALLOWED_FILES = {'.gitignore', '.dockerignore', '.env.example', 'AGENTS.md', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'deny.toml', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'SECURITY.md', 'CONTRIBUTING.md'}
PATTERNS = {
    'personal-home-path': re.compile(rb'/(?:Users|home)/[^/\s]+/(?:Work|Library|\.codex|\.cargo|\.config)/'),
    'private-key-block': re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----'),
    'nostr-secret': re.compile(rb'\bnsec1[023456789acdefghjklmnpqrstuvwxyz]{40,}\b'),
    'provider-key': re.compile(rb'\bsk-[A-Za-z0-9_-]{30,}\b'),
    'github-token': re.compile(rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{50,})\b'),
}

def files():
    output = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT)
    return sorted({pathlib.PurePosixPath(p.decode()) for p in output.split(b'\0') if p})

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--copy-to', type=pathlib.Path, help='Copy the validated allowlisted tree to a new directory for external scanning')
    args = parser.parse_args()
    selected, failures = files(), []
    for path in selected:
        file = ROOT / path
        if path.parts[0] not in ALLOWED_ROOTS and str(path) not in ALLOWED_FILES:
            failures.append((path, 'unapproved-export-path')); continue
        if any(part in {'secrets', 'state', 'target', '.work', '__pycache__', '.git'} for part in path.parts) or (path.name.startswith('.env') and path.name != '.env.example') or path.suffix in {'.log', '.har', '.sqlite', '.db', '.pyc'}:
            failures.append((path, 'runtime-or-private-file')); continue
        if file.is_symlink() or not file.is_file() or file.stat().st_size > 5 * 1024 * 1024:
            failures.append((path, 'unexpected-file-type-or-size')); continue
        data = file.read_bytes()
        for name, pattern in PATTERNS.items():
            if pattern.search(data): failures.append((path, name))
    if failures:
        for path, rule in failures: print(f'{path}: {rule}')
        raise SystemExit(1)
    if args.copy_to:
        target = args.copy_to.resolve()
        if target == ROOT or ROOT in target.parents: raise SystemExit('Export copy must be outside this checkout')
        target.mkdir(parents=True, exist_ok=False)
        for path in selected:
            dest = target / path; dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
    print(f'Export boundary passed: {len(selected)} source files; no private matches printed.')
