"""Create fresh disposable identities, never reading operator credentials."""
import json, subprocess
import bootstrap
WORK = bootstrap.WORK
IMAGE = 'ghcr.io/block/buzz:0.2.1@sha256:4e31b7c7abb7d00b6f513dc559e58d2b980416f1dc400aa01bcf762cf2989cfc'

if __name__ == '__main__':
    public_path = WORK / 'buzz-identities.json'
    public = json.loads(public_path.read_text()) if public_path.exists() else {}
    for name in ('owner', 'agent', 'relay', 'denied'):
        if name in public:
            assert (WORK / ('buzz-' + name + '-key')).exists(), 'Incomplete disposable identity set'
            continue
        result = subprocess.check_output(['docker', 'run', '--rm', '--entrypoint', 'buzz-admin', IMAGE, 'generate-key'], text=True)
        lines = result.splitlines()
        public[name] = next(l.split(':', 1)[1].strip() for l in lines if l.startswith('Public key:'))
        key = next(l.split(':', 1)[1].strip() for l in lines if l.startswith('Secret key:'))
        bootstrap.secret_file('buzz-' + name + '-key', key + '\n')
        bootstrap.secret_file('buzz-identities.json', json.dumps(public))
    bootstrap.secret_file('relay.env', 'BUZZ_RELAY_PRIVATE_KEY=' + (WORK / 'buzz-relay-key').read_text().strip() + '\nRELAY_OWNER_PUBKEY=' + public['owner'] + '\n')
    print('Disposable relay identities prepared; no key material printed.')
