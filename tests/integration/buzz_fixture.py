"""Provision and exercise only the named disposable Buzz Compose stack."""
import json, os, pathlib, subprocess, time
import bootstrap
ROOT, WORK = bootstrap.ROOT, bootstrap.WORK
COMPOSE = ['docker', 'compose', '-f', str(ROOT / 'tests/integration/buzz-compose.yml')]
IMAGE = 'openwebui-acp-sidecar:local'
IDENTITIES = json.loads((WORK / 'buzz-identities.json').read_text())

def cli(identity, *args):
    command = ['docker', 'run', '--rm', '--network', 'openwebui-acp-relay-test_default', '-e', 'BUZZ_PRIVATE_KEY', '--entrypoint', '/usr/local/bin/buzz', IMAGE, '--relay', 'http://relay:3000', *args]
    environment = os.environ.copy()
    environment['BUZZ_PRIVATE_KEY'] = (WORK / ('buzz-' + identity + '-key')).read_text().strip()
    result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError('Disposable Buzz CLI failed: ' + result.stderr)
    return json.loads(result.stdout)

def setup():
    for name in ('owner', 'agent'):
        subprocess.run(COMPOSE + ['exec', '-T', 'relay', 'buzz-admin', 'add-member', '--pubkey', IDENTITIES[name]], check=True, capture_output=True)
        cli(name, 'users', 'set-profile', '--name', 'Fixture ' + name)
    channel_path = WORK / 'buzz-channel.json'
    if channel_path.exists():
        channel = json.loads(channel_path.read_text())['id']
    else:
        result = cli('owner', 'channels', 'create', '--name', 'Disposable ACP Fixture', '--type', 'stream', '--visibility', 'open')
        bootstrap.secret_file('buzz-create-result.json', json.dumps(result))
        channel = result.get('id') or result.get('channel_id') or result.get('channel', {}).get('id')
        assert channel, 'Inspect synthetic channel-create response'
        bootstrap.secret_file('buzz-channel.json', json.dumps({'id': channel}))
    cli('agent', 'channels', 'join', '--channel', channel)
    bootstrap.secret_file('agent.env', 'OPENWEBUI_ACP_BUZZ_ALLOWED_AUTHORS=' + IDENTITIES['owner'] + '\nOPENWEBUI_ACP_BUZZ_ALLOWED_CHANNELS=' + channel + '\nBUZZ_ACP_AGENT_OWNER=' + IDENTITIES['owner'] + '\n')
    subprocess.run(['docker', 'run', '--rm', '--user', '0:0', '--mount', f'type=bind,source={WORK},target=/fixture,readonly', '--mount', 'type=volume,source=openwebui-acp-relay-test_agent-secrets,target=/secrets', '--entrypoint', '/bin/sh', 'debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171', '-ec', 'install -o 10001 -g 10001 -m 600 /fixture/openwebui-key /secrets/openwebui-key; install -o 10001 -g 10001 -m 600 /fixture/buzz-agent-key /secrets/buzz-key'], check=True)
    print('Disposable relay membership, channel, and non-root sidecar credentials configured.')

if __name__ == '__main__': setup()
