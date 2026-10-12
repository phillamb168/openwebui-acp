"""Exercise the public Compose deployment inside a fresh disposable Linux VM.

Run prepare after bootstrapping the named synthetic Open WebUI/Buzz fixtures.
Reboot the disposable VM yourself, then run verify. This never reboots a host,
accepts a real service URL, or reads an operator's configuration.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

import bootstrap as fixture
import buzz_fixture as b

DEPLOY = b.ROOT / 'deploy/sidecar'
OVERRIDE = b.WORK / 'vm-compose.json'
COMPOSE = ['docker', 'compose', '-f', str(DEPLOY / 'compose.yml'), '-f', str(OVERRIDE)]


def run(command):
    result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    assert result.returncode == 0, 'Disposable VM command failed; inspect local services'
    return result.stdout


def wait_ready():
    run(fixture.COMPOSE + ['up', '-d', '--wait'])
    # Bootstrap ports may change after reboot. Internal service names do not.
    run(b.COMPOSE + ['up', '-d', '--wait', 'relay'])
    run(COMPOSE + ['up', '-d'])
    b.wait_agent(COMPOSE)


def started_at():
    container = run(COMPOSE + ['ps', '-q', 'agent']).strip()
    return json.loads(run(['docker', 'inspect', container]))[0]['State']['StartedAt']


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=['prepare', 'verify'])
    args = parser.parse_args()
    assert platform.system() == 'Linux', 'Run inside the disposable VM'
    assert Path('/etc/os-release').read_text().find('VERSION_ID="24.04"') >= 0, 'Expected the documented Ubuntu fixture'
    if args.phase == 'prepare':
        assert not (DEPLOY / '.env').exists(), 'Refusing to replace an existing deployment configuration'
        b.setup()
        channel = json.loads((b.WORK / 'buzz-channel.json').read_text())['id']
        (DEPLOY / '.env').write_text('OPENWEBUI_ACP_IMAGE=openwebui-acp-sidecar:0.1.0-rc.2\n'
            'OPENWEBUI_URL=http://webui:8080\nOPENWEBUI_MODEL=fixture-tool\nBUZZ_RELAY_URL=ws://relay:3000\n'
            'OPENWEBUI_ACP_REQUIRED_TOOLS=fixture_tool\nOPENWEBUI_ACP_BUZZ_ALLOWED_AUTHORS=' + b.IDENTITIES['owner'] +
            '\nOPENWEBUI_ACP_BUZZ_ALLOWED_CHANNELS=' + channel + '\n')
        fixture.secret_file(OVERRIDE.name, json.dumps({'services': {'agent': {
            'environment': {'OPENWEBUI_ACP_ALLOW_INSECURE_HTTP': 'true', 'RUST_LOG': 'warn,buzz_acp=info'}, 'networks': ['webui', 'relay']}},
            'networks': {'webui': {'external': True, 'name': 'openwebui-acp-test_test'},
                         'relay': {'external': True, 'name': 'openwebui-acp-relay-test_default'}}}))
        run(['docker', 'run', '--rm', '--user', '0:0', '--mount', f'type=bind,source={b.WORK},target=/fixture,readonly',
             '--mount', f'type=bind,source={DEPLOY},target=/deploy', '--entrypoint', '/bin/sh',
             'debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171', '-ec',
             'mkdir /deploy/secrets /deploy/state; chmod 700 /deploy/secrets /deploy/state; '
             'chown 10001:10001 /deploy/state; '
             'install -o 10001 -g 10001 -m 600 /fixture/openwebui-key /deploy/secrets/openwebui-key; '
             'install -o 10001 -g 10001 -m 600 /fixture/buzz-agent-key /deploy/secrets/buzz-key'])
        # Only these disposable dependencies need restart policies for this VM test.
        for compose, services in [(fixture.COMPOSE, ['gateway', 'fixture', 'webui']),
                                  (b.COMPOSE, ['relay', 'postgres', 'redis', 'minio'])]:
            ids = run(compose + ['ps', '-q', *services]).split()
            run(['docker', 'update', '--restart', 'unless-stopped', *ids])
        wait_ready()
        import run_buzz
        b.COMPOSE = COMPOSE
        root, chat = run_buzz.check()
        fixture.secret_file('vm-before-reboot.json', json.dumps({'root': root, 'chat': chat,
                            'container_started_at': started_at(),
                            'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip()}))
        print('Public Compose install and initial turns: PASS. Reboot this disposable VM, then run verify.')
    else:
        previous = json.loads((b.WORK / 'vm-before-reboot.json').read_text())
        # OrbStack machines share a VM kernel. A changed container start time is
        # recorded as well; the verification report names the actual reboot scope.
        wait_ready()
        assert started_at() != previous['container_started_at'], 'The agent has not restarted'
        import run_buzz
        b.COMPOSE = COMPOSE
        run_buzz.turn('CHECK_CONTEXT after VM restart.', 'ORCHARD', previous['root'])
        final = run_buzz.state()
        channel = run_buzz.CHANNEL
        assert final['conversations'][f'buzz/{channel}/{previous["root"]}']['chat_id'] == previous['chat']
        evidence = {'passed': True, 'platform': platform.machine(), 'public_compose': True,
                    'thread_continuity_after_machine_restart': True,
                    'kernel_boot_id_changed': Path('/proc/sys/kernel/random/boot_id').read_text().strip() != previous['boot_id']}
        fixture.secret_file('vm-evidence.json', json.dumps(evidence, indent=2))
        print('Public Compose after machine restart: original chat and exact reply: PASS')


if __name__ == '__main__':
    main()
