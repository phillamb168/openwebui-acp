#!/usr/bin/env python3
"""Emit a path-free CycloneDX build dependency inventory and license texts.

Uses locked Cargo metadata; includes normal/build dependencies across targets,
excludes dev-only roots. No credentials, local source paths, or environment dump.
"""
import argparse, hashlib, json, os, pathlib, subprocess
from urllib.parse import quote

def inventory(manifest, roots, cargo):
    data = json.loads(subprocess.check_output([cargo, 'metadata', '--format-version', '1', '--locked', '--manifest-path', str(manifest)], text=True))
    packages = {p['id']: p for p in data['packages']}
    nodes = {n['id']: n for n in data['resolve']['nodes']}
    root_ids = [p['id'] for p in packages.values() if p['name'] in roots]
    assert len(root_ids) == len(roots), 'Missing or ambiguous inventory root'
    selected, edges = set(), {}
    def visit(id):
        if id in selected: return
        selected.add(id)
        deps = [d['pkg'] for d in nodes[id]['deps'] if any(k['kind'] != 'dev' for k in d['dep_kinds'])]
        edges[id] = deps
        for dep in deps: visit(dep)
    for id in root_ids: visit(id)
    def ref(id):
        p = packages[id]
        return f"{p['name']}@{p['version']}:{hashlib.sha256((p.get('source') or p['name']).encode()).hexdigest()[:12]}"
    components, notices, missing = [], [], []
    for id in sorted(selected, key=lambda id: ref(id)):
        p = packages[id]
        component = {'type': 'application' if id in root_ids else 'library', 'bom-ref': ref(id), 'name': p['name'], 'version': p['version'], 'purl': 'pkg:cargo/' + quote(p['name']) + '@' + quote(p['version']), 'licenses': [{'expression': p['license'].replace('/', ' OR ')}] if p.get('license') else []}
        if p.get('source'):
            component['properties'] = [{'name': 'cargo:source', 'value': p['source']}]
        components.append(component)
        root = pathlib.Path(p['manifest_path']).parent
        files = []
        for candidate_dir in [root, *list(root.parents)[:3]]:
            files = sorted(f for f in candidate_dir.iterdir() if f.is_file() and f.name.lower().startswith(('license', 'licence', 'copying', 'notice')))
            if files: break
            # Registry crates are self-contained; never search arbitrary host ancestors.
            source = p.get('source') or ''
            if source.startswith('registry+') or (not source and candidate_dir == pathlib.Path(data['workspace_root'])): break
        if p.get('license_file'):
            path = root / p['license_file']
            if path.is_file() and path not in files: files.append(path)
        authors = root / 'AUTHORS'
        if not files and authors.is_file() and 'LICENSE:' in authors.read_text(errors='replace'):
            files = [authors]
        override = pathlib.Path(__file__).resolve().parents[1] / 'licenses' / 'upstream' / (p['name'] + '.txt')
        if not files and override.is_file(): files = [override]
        if not files: missing.append(ref(id))
        notices.append('\n' + '=' * 72 + '\n' + p['name'] + ' ' + p['version'] + '\nLicense: ' + (p.get('license') or 'see text') + '\n')
        for f in files:
            notices.append('\n--- ' + f.name + ' ---\n' + f.read_text(errors='replace') + '\n')
    bom = {'bomFormat': 'CycloneDX', 'specVersion': '1.6', 'version': 1, 'metadata': {'properties': [{'name': 'inventory:scope', 'value': 'locked normal/build dependency graph across Cargo targets; dev-only roots excluded'}]}, 'components': components, 'dependencies': [{'ref': ref(id), 'dependsOn': sorted(ref(d) for d in edges[id])} for id in sorted(selected, key=ref)]}
    return bom, ''.join(notices), missing

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--manifest', type=pathlib.Path, default=pathlib.Path('Cargo.toml'))
    parser.add_argument('--root', action='append', default=[])
    parser.add_argument('--out', type=pathlib.Path, required=True)
    args = parser.parse_args()
    bom, notices, missing = inventory(args.manifest.resolve(), args.root or ['openwebui-acp'], os.environ.get('CARGO', 'cargo'))
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / 'bom.cdx.json').write_text(json.dumps(bom, indent=2) + '\n')
    (args.out / 'DEPENDENCY_LICENSES.txt').write_text(notices)
    (args.out / 'license-text-gaps.json').write_text(json.dumps(missing, indent=2) + '\n')
    print(f"Inventoried {len(bom['components'])} build dependencies; {len(missing)} license-text gaps.")
    if missing: raise SystemExit(1)
