"""Install both published Buzz and local agent charts in a disposable Kind cluster.

Requires the named Compose Open WebUI fixture, prepare_relay.py, a locally loaded
agent image, Helm and kubectl. Never uses default kubeconfig
or accepts operator credentials, service URLs, namespaces or non-Kind clusters.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

import bootstrap as b

NS = 'openwebui-acp-fixture'
RELEASE = 'fixture'
DEPLOYMENT = 'fixture-openwebui-acp'
IMAGE = 'openwebui-acp-sidecar:0.1.0-rc.2'
RELAY_DIGEST = 'sha256:4e31b7c7abb7d00b6f513dc559e58d2b980416f1dc400aa01bcf762cf2989cfc'
INSPECT_IMAGE = 'debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171'


def run(command, *, data=None, timeout=120):
    result = subprocess.run([str(s) for s in command], input=data, text=True,
                            capture_output=True, timeout=timeout)
    if result.returncode:
        # Tool output can contain a Secret or remote message; never echo it.
        raise RuntimeError('Fixture command failed: ' + str(command[0]) +
                           ' (exit ' + str(result.returncode) + ')')
    return result.stdout


def eventually(check, label, seconds=90):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.4)
    raise RuntimeError('Kubernetes fixture timed out: ' + label)


class Fixture:
    def __init__(self, args):
        self.args = args
        self.k = ['kubectl', '--kubeconfig', args.kubeconfig]
        context = run(self.k + ['config', 'current-context']).strip()
        assert context.startswith('kind-openwebui-acp-'), 'Only a named disposable Kind context is accepted'
        nodes = json.loads(run(self.k + ['get', 'nodes', '-o', 'json']))['items']
        assert len(nodes) == 1 and nodes[0]['metadata']['name'] == context[5:] + '-control-plane'
        self.node = nodes[0]['metadata']['name']
        self.k += ['--context', context, '-n', NS]
        self.h = [args.helm, '--kubeconfig', args.kubeconfig, '--kube-context', context, '-n', NS]
        self.ids = json.loads((b.WORK / 'buzz-identities.json').read_text())
        self.sequence = 0
        self.channel = None
        self.values_file = b.WORK / 'kubernetes-values.json'

    def apply(self, value):
        run(self.k + ['apply', '-f', '-'], data=json.dumps(value))

    def setup(self):
        namespaces = json.loads(run(self.k + ['get', 'namespace', '-o', 'json']))['items']
        assert all(n['metadata']['name'] != NS for n in namespaces), 'Remove the previous disposable namespace explicitly before rerunning'
        self.apply({'apiVersion': 'v1', 'kind': 'Namespace', 'metadata': {'name': NS,
                    'labels': {'openwebui-acp-fixture': 'true'}}})
        relay_values = {'quickstart': True, 'image': {'tag': '0.2.1', 'digest': RELAY_DIGEST},
                        'relayUrl': 'ws://fixture-buzz:3000', 'ownerPubkey': self.ids['owner'],
                        'postgresql': {'enabled': True}, 'redis': {'enabled': True},
                        'minio': {'enabled': True}, 'podDisruptionBudget': {'enabled': False},
                        'relay': {'extraEnv': [{'name': 'BUZZ_GIT_CONFORMANCE_PROBE', 'value': 'false'}]}}
        b.secret_file('kubernetes-relay-values.json', json.dumps(relay_values))
        run(self.h + ['install', 'fixture-buzz', 'oci://ghcr.io/block/buzz/charts/buzz',
                      '--version', '0.1.11', '-f', b.WORK / 'kubernetes-relay-values.json',
                      '--wait', '--timeout', '8m'], timeout=520)
        print('Published Buzz Helm chart installed and ready.', flush=True)
        self.apply({'apiVersion': 'v1', 'kind': 'Secret', 'metadata': {'name': 'fixture-cli'},
                    'stringData': {name: (b.WORK / ('buzz-' + name + '-key')).read_text().strip()
                                   for name in ('owner', 'agent', 'denied')}})
        for name in ('owner', 'agent', 'denied'):
            run(self.k + ['exec', 'deployment/fixture-buzz', '-c', 'relay', '--',
                          'buzz-admin', 'add-member', '--pubkey', self.ids[name]])
            self.cli(name, 'users', 'set-profile', '--name', 'Fixture ' + name)
        self.channel = self.create_channel('Kubernetes Fixture')
        self.denied_channel = self.create_channel('Kubernetes Denied Fixture')
        for channel in (self.channel, self.denied_channel):
            self.cli('agent', 'channels', 'join', '--channel', channel)
        self.cli('denied', 'channels', 'join', '--channel', self.channel)
        gateway = run(b.COMPOSE + ['ps', '-q', 'gateway']).strip()
        net = json.loads(run(['docker', 'inspect', gateway]))[0]['NetworkSettings']['Networks']
        if 'kind' not in net:
            run(['docker', 'network', 'connect', 'kind', gateway])
        # Docker can reassign dynamically published ports when a network is
        # attached. Discover the loopback fixture endpoints again afterwards.
        b.BASE, b.FIXTURE = b.port(8080), b.port(8000)
        gateway_ip = json.loads(run(['docker', 'inspect', gateway]))[0]['NetworkSettings']['Networks']['kind']['IPAddress']
        self.apply({'apiVersion': 'v1', 'kind': 'Secret', 'metadata': {'name': 'fixture-agent'},
                    'stringData': {'openwebui-key': (b.WORK / 'openwebui-key').read_text().strip(),
                                   'buzz-key': (b.WORK / 'buzz-agent-key').read_text().strip()}})
        self.values = {'openwebui': {'url': 'http://' + gateway_ip + ':8080', 'model': 'fixture-tool',
                                    'requiredTools': ['fixture_tool'], 'allowInsecureHttp': True},
                       'buzz': {'relayUrl': 'ws://fixture-buzz:3000', 'allowedAuthors': [self.ids['owner']],
                                'allowedChannels': [self.channel]},
                       'credentials': {'existingSecret': 'fixture-agent'}}
        self.install()
        self.apply({'apiVersion': 'v1', 'kind': 'Pod', 'metadata': {'name': 'state-inspector'}, 'spec': {
            'automountServiceAccountToken': False, 'restartPolicy': 'Never',
            'securityContext': {'runAsNonRoot': True, 'runAsUser': 10001, 'runAsGroup': 10001},
            'containers': [{'name': 'inspect', 'image': INSPECT_IMAGE, 'command': ['sleep', '7200'],
                            'securityContext': {'allowPrivilegeEscalation': False, 'readOnlyRootFilesystem': True,
                                                'capabilities': {'drop': ['ALL']}},
                            'volumeMounts': [{'name': 'state', 'mountPath': '/state', 'readOnly': True}]}],
            'volumes': [{'name': 'state', 'persistentVolumeClaim': {'claimName': DEPLOYMENT}}]}})
        run(self.k + ['wait', '--for=condition=Ready', 'pod/state-inspector', '--timeout=120s'], timeout=140)
        eventually(lambda: self.state_path_exists(), 'durable state created')
        print('Agent chart ready with projected credentials and persistent state.', flush=True)

    def cli(self, identity, *args):
        self.sequence += 1
        name = 'fixture-cli-' + str(self.sequence)
        self.apply({'apiVersion': 'v1', 'kind': 'Pod', 'metadata': {'name': name}, 'spec': {
            'automountServiceAccountToken': False, 'restartPolicy': 'Never',
            'securityContext': {'runAsNonRoot': True, 'runAsUser': 10001, 'runAsGroup': 10001},
            'containers': [{'name': 'cli', 'image': IMAGE, 'imagePullPolicy': 'Never',
                'command': ['/usr/local/bin/buzz', '--relay', 'http://fixture-buzz:3000', *args],
                'env': [{'name': 'BUZZ_PRIVATE_KEY', 'valueFrom': {'secretKeyRef': {'name': 'fixture-cli', 'key': identity}}}],
                'securityContext': {'allowPrivilegeEscalation': False, 'readOnlyRootFilesystem': True,
                                    'capabilities': {'drop': ['ALL']}}}]}})
        try:
            run(self.k + ['wait', '--for=jsonpath={.status.phase}=Succeeded', 'pod/' + name, '--timeout=30s'], timeout=35)
            return json.loads(run(self.k + ['logs', name]))
        finally:
            run(self.k + ['delete', 'pod', name, '--wait=false'])

    def create_channel(self, name):
        value = self.cli('owner', 'channels', 'create', '--name', name, '--type', 'stream', '--visibility', 'open')
        return value.get('id') or value.get('channel_id') or value['channel']['id']

    def state_path_exists(self):
        r = subprocess.run(self.k + ['exec', 'state-inspector', '--', 'test', '-f', '/state/state/state.json'], capture_output=True)
        return r.returncode == 0

    def state(self):
        return json.loads(run(self.k + ['exec', 'state-inspector', '--', 'cat', '/state/state/state.json']))

    def calls(self):
        return len(b.call('/evidence', base=b.FIXTURE, token=False)['calls'])

    def replies(self):
        return [m for m in self.cli('owner', 'messages', 'get', '--channel', self.channel)
                if m['pubkey'] == self.ids['agent']]

    def send(self, text, root=None, identity='owner', channel=None):
        args = ['messages', 'send', '--channel', channel or self.channel, '--mention', self.ids['agent'], '--content', text]
        if root:
            args += ['--reply-to', root]
        return self.cli(identity, *args)['event_id']

    def response(self, root, expected):
        replies = eventually(lambda: [m for m in self.replies() if ['e', root, '', 'reply'] in m['tags']
                                     and expected in m['content']], 'thread response')
        for reply in replies:
            assert [t for t in reply['tags'] if t[0] == 'p'] == [['p', self.ids['owner']]]
            assert ['h', self.channel] in reply['tags']
        return replies

    def turn(self, text, expected, root=None):
        before = len(self.replies())
        event = self.send(text, root)
        self.response(root or event, expected)
        assert len(self.replies()) == before + 1, 'Duplicate channel reply'
        return root or event

    def install(self, existing=False):
        if existing:
            self.values['persistence'] = {'existingClaim': DEPLOYMENT}
        b.secret_file(self.values_file.name, json.dumps(self.values))
        run(self.h + ['upgrade', '--install', RELEASE, b.ROOT / 'deploy/chart', '-f', self.values_file,
                      '--wait', '--timeout', '120s'], timeout=140)
        time.sleep(2)

    def restart(self):
        run(self.k + ['rollout', 'restart', 'deployment/' + DEPLOYMENT])
        run(self.k + ['rollout', 'status', 'deployment/' + DEPLOYMENT, '--timeout=90s'])
        time.sleep(2)

    def check(self):
        a = self.turn('CHECK_TOOL. Remember ORCHARD in this new thread.', 'WORKSPACE-TOOL-MARKER-4821')
        chat_a = self.state()['conversations'][f'buzz/{self.channel}/{a}']['chat_id']
        other = self.turn('CHECK_CONTEXT separate thread.', 'MISSING_CONTEXT')
        assert self.state()['conversations'][f'buzz/{self.channel}/{other}']['chat_id'] != chat_a
        self.restart()
        self.turn('CHECK_CONTEXT after pod replacement.', 'ORCHARD', a)
        assert self.state()['conversations'][f'buzz/{self.channel}/{a}']['chat_id'] == chat_a
        permissions = run(self.k + ['exec', 'state-inspector', '--', 'stat', '-c', '%u:%g:%a',
                                   '/state/state', '/state/state/state.json']).splitlines()
        assert permissions == ['10001:10001:700', '10001:10001:600']
        pod = json.loads(run(self.k + ['get', 'pods', '-l', 'app.kubernetes.io/name=openwebui-acp', '-o', 'json']))['items'][0]
        assert pod['spec']['automountServiceAccountToken'] is False
        private = '/var/lib/kubelet/pods/' + pod['metadata']['uid'] + '/volumes/kubernetes.io~empty-dir/private-secrets/keys/'
        modes = run(['docker', 'exec', self.node, 'stat', '-c', '%u:%g:%a:%F',
                     private + 'openwebui-key', private + 'buzz-key']).splitlines()
        assert modes == ['10001:10001:600:regular file'] * 2
        before, replies = self.calls(), len(self.replies())
        self.send('CHECK_PLAIN denied author', identity='denied')
        self.send('CHECK_PLAIN denied channel', channel=self.denied_channel)
        time.sleep(3)
        assert self.calls() == before and len(self.replies()) == replies
        b.call('/control', {'slow_seconds': 10}, base=b.FIXTURE, token=False)
        slow = self.send('CHECK_SLOW queue fixture')
        eventually(lambda: self.calls() == before + 1, 'slow request starts')
        queued = [self.send('CHECK_PLAIN queued ' + str(i)) for i in range(2)]
        self.response(slow, 'SLOW-FIXTURE-COMPLETE')
        for root in queued:
            self.response(root, 'PLAIN-SYSTEM-VERIFIED')
        assert self.calls() == before + 3 and len(self.replies()) == replies + 3
        before = self.calls()
        interrupted = self.send('CHECK_SLOW interrupted fixture')
        eventually(lambda: self.calls() == before + 1, 'interrupted request submitted')
        run(self.k + ['delete', 'pod', '-l', 'app.kubernetes.io/name=openwebui-acp', '--grace-period=1'])
        run(self.k + ['rollout', 'status', 'deployment/' + DEPLOYMENT, '--timeout=90s'])
        time.sleep(12)
        self.send('CHECK_PLAIN reconcile without repeating previous inference', interrupted)
        self.response(interrupted, 'SLOW-FIXTURE-COMPLETE')
        self.response(interrupted, 'PLAIN-SYSTEM-VERIFIED')
        assert self.calls() == before + 2, 'Interrupted inference was replayed'
        assert len(self.response(interrupted, 'SLOW-FIXTURE-COMPLETE')) == 1
        self.values['podAnnotations'] = {'fixture-upgrade': 'verified'}
        self.install()
        self.turn('CHECK_CONTEXT after Helm upgrade.', 'ORCHARD', a)
        run(self.h + ['uninstall', RELEASE, '--wait'])
        run(self.k + ['get', 'pvc', DEPLOYMENT])
        self.install(existing=True)
        self.turn('CHECK_CONTEXT after reinstall with retained claim.', 'ORCHARD', a)
        assert self.state()['conversations'][f'buzz/{self.channel}/{a}']['chat_id'] == chat_a
        evidence = {'passed': True, 'buzz_chart': '0.1.11', 'relay_version': '0.2.1',
                    'relay_digest': RELAY_DIGEST, 'agent_image': IMAGE,
                    'tool': True, 'thread_isolation': True, 'pod_restart_continuity': True,
                    'private_projected_keys': True, 'state_permissions': True,
                    'denied_author_channel': True, 'queued_individual_events': True,
                    'interrupted_no_model_replay': True, 'exact_recipients': True,
                    'helm_upgrade': True, 'uninstall_retains_pvc': True, 'reinstall_continuity': True}
        b.secret_file('kubernetes-evidence.json', json.dumps(evidence, indent=2) + '\n')
        print('Kubernetes: tool, isolation, permissions, restart, queue, recovery, upgrade and retained-PVC reinstall: PASS', flush=True)

    def close(self):
        b.call('/control', {'slow_seconds': 4}, base=b.FIXTURE, token=False)


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kubeconfig', type=Path, required=True)
    parser.add_argument('--helm', default='helm')
    fixture = Fixture(parser.parse_args())
    try:
        fixture.setup()
        fixture.check()
    finally:
        fixture.close()
