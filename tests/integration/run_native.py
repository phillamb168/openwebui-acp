"""macOS Buzz runner + disposable relay/Open WebUI, synthetic identities.

Requires the named Compose stacks and `cargo run --example fixture_identity`.
No caller-supplied service URL, operator env file, or Desktop profile is accepted.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import signal
import socket
import subprocess
import threading
import time
import urllib.request

import bootstrap
from native_proxy import RelayProxy

ROOT = bootstrap.ROOT
WORK = ROOT / '.work/native'
COMPOSE = ['/usr/local/bin/docker', 'compose', '-p', 'openwebui-acp-native-test',
           '-f', str(ROOT / 'tests/integration/buzz-compose.yml'), '-f', str(WORK / 'compose.yml')]
IDENTITIES = json.loads((WORK / 'identities.json').read_text())


def secret_file(path, text):
    fd = os.open(path, os.O_CREAT | os.O_TRUNC | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as out:
        out.write(text)


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def eventually(check, label, seconds=40):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise RuntimeError('Native fixture timed out: ' + label)


class Fixture:
    def __init__(self, binary, app):
        self.binary = binary.resolve()
        self.app = app.resolve()
        self.buzz = str(self.app / 'Contents/MacOS/buzz')
        self.runner = str(self.app / 'Contents/MacOS/buzz-acp')
        self.child = None
        self.log = None
        self.http = 'http://127.0.0.1:' + str(free_port())
        self.ws = self.http.replace('http:', 'ws:')
        backend = free_port()
        secret_file(WORK / 'compose.yml', f'''services:
  relay:
    environment:
      RELAY_URL: {self.ws}
    ports: !override ["127.0.0.1:{backend}:3000"]
''')
        result = subprocess.run(COMPOSE + ['up', '-d', '--wait', 'relay'], capture_output=True, timeout=90)
        assert result.returncode == 0, 'Disposable relay could not start'
        self.proxy = RelayProxy(int(self.http.rsplit(':', 1)[1]), backend, IDENTITIES['agent'])
        threading.Thread(target=self.proxy.serve_forever, daemon=True).start()
        for name in ('owner', 'denied'):
            result = subprocess.run(COMPOSE + ['exec', '-T', 'relay', 'buzz-admin', 'add-member',
                                    '--pubkey', IDENTITIES[name]], capture_output=True, timeout=20)
            assert result.returncode == 0, 'Disposable membership failed'
            self.cli(name, 'users', 'set-profile', '--name', 'Fixture ' + name)
        # The agent deliberately has no direct relay membership. Its inherited
        # owner attestation must work for both the runner and adapter publisher.
        self.cli('agent', 'users', 'set-profile', '--name', 'Fixture agent')
        self.channel = self.create_channel('Native ACP Fixture')
        self.denied_channel = self.create_channel('Native ACP Denied Fixture')
        for channel in (self.channel, self.denied_channel):
            self.cli('agent', 'channels', 'join', '--channel', channel)
        self.cli('denied', 'channels', 'join', '--channel', self.channel)
        self.state_dir = WORK / ('state-' + str(time.time_ns()))
        self.state_dir.mkdir(mode=0o700)
        self.env_file = WORK / 'adapter.env'
        secret_file(self.env_file, f'''OPENWEBUI_URL={bootstrap.BASE}
OPENWEBUI_MODEL=fixture-tool
OPENWEBUI_API_KEY_FILE={bootstrap.WORK}/openwebui-key
OPENWEBUI_ACP_ALLOW_INSECURE_HTTP=true
OPENWEBUI_ACP_MEMORY_DEFAULT=false
OPENWEBUI_ACP_REQUIRED_TOOLS=fixture_tool
OPENWEBUI_ACP_BUZZ_AUTO_PUBLISH=true
OPENWEBUI_ACP_BUZZ_THREAD_CHATS=true
OPENWEBUI_ACP_BUZZ_ALLOWED_AUTHORS={IDENTITIES['owner']}
OPENWEBUI_ACP_BUZZ_ALLOWED_CHANNELS={self.channel}
OPENWEBUI_ACP_TIMEOUT_SECS=600
''')

    def env(self, identity):
        env = {'PATH': '/usr/bin:/bin',
               'BUZZ_PRIVATE_KEY': (WORK / (identity + '-key')).read_text().strip(),
               'BUZZ_RELAY_URL': self.ws}
        if identity == 'agent':
            env['BUZZ_AUTH_TAG'] = (WORK / 'auth-tag').read_text().strip()
        return env

    def cli(self, identity, *args):
        result = subprocess.run([self.buzz, '--relay', self.http, *args], env=self.env(identity),
                                cwd=WORK, capture_output=True, text=True, timeout=20)
        if result.returncode:
            # Never echo arbitrary output, request bodies or inherited secrets.
            raise RuntimeError('Disposable native Buzz CLI failed: ' + ' '.join(args[:2]) +
                               ' (exit ' + str(result.returncode) + ')')
        return json.loads(result.stdout)

    def create_channel(self, name):
        value = self.cli('owner', 'channels', 'create', '--name', name, '--type', 'stream', '--visibility', 'open')
        return value.get('id') or value.get('channel_id') or value['channel']['id']

    def messages(self):
        return self.cli('owner', 'messages', 'get', '--channel', self.channel)

    def send(self, text, root=None, identity='owner', channel=None):
        args = ['messages', 'send', '--channel', channel or self.channel,
                '--mention', IDENTITIES['agent'], '--content', text]
        if root:
            args += ['--reply-to', root]
        return self.cli(identity, *args)['event_id']

    def replies(self):
        return [m for m in self.messages() if m['pubkey'] == IDENTITIES['agent']]

    def state(self):
        return json.loads((self.state_dir / 'state.json').read_text())

    def calls(self):
        return len(bootstrap.call('/evidence', base=bootstrap.FIXTURE, token=False)['calls'])

    def start(self):
        env = self.env('agent')
        env.update({'BUZZ_ACP_AGENT_OWNER': IDENTITIES['owner'],
                    'BUZZ_ACP_AGENT_COMMAND': str(self.binary),
                    'BUZZ_ACP_AGENT_ARGS': '--env-file,' + str(self.env_file) + ',--state-dir,' + str(self.state_dir),
                    'BUZZ_ACP_MULTIPLE_EVENT_HANDLING': 'queue', 'BUZZ_ACP_DEDUP': 'queue',
                    'BUZZ_ACP_IDLE_TIMEOUT': '1500', 'BUZZ_ACP_MAX_TURN_DURATION': '1560',
                    'RUST_LOG': 'info'})
        self.log = (WORK / 'runner.log').open('w')
        self.child = subprocess.Popen([self.runner, '--agents', '1', '--respond-to', 'owner-only',
                     '--channels', self.channel + ',' + self.denied_channel,
                     '--session-policy', 'channel', '--context-message-limit', '0',
                     '--no-presence', '--no-typing', '--no-memory', '--no-base-prompt'],
                     env=env, cwd=WORK, stdout=self.log, stderr=self.log, start_new_session=True)
        def ready():
            assert self.child.poll() is None, 'Installed runner exited during startup; inspect private synthetic log'
            return 'connected to relay' in (WORK / 'runner.log').read_text()
        eventually(ready, 'runner connection')
        time.sleep(1)

    def stop(self):
        if self.child:
            os.killpg(self.child.pid, signal.SIGTERM)
            try:
                self.child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                os.killpg(self.child.pid, signal.SIGKILL)
                self.child.wait(timeout=5)
            self.child = None
            self.log.close()

    def turn(self, text, expected, root=None):
        before = {r['id'] for r in self.replies()}
        event = self.send(text, root)
        root = root or event
        replies = eventually(lambda: [r for r in self.replies() if r['id'] not in before], 'single reply')
        assert len(replies) == 1, 'Duplicate visible reply'
        reply = replies[0]
        assert expected in reply['content'], 'Unexpected synthetic answer'
        assert [t for t in reply['tags'] if t[0] == 'p'] == [['p', IDENTITIES['owner']]]
        assert ['h', self.channel] in reply['tags'] and ['e', root, '', 'reply'] in reply['tags']
        assert [t for t in reply['tags'] if t[0] == 'auth'] == [json.loads((WORK / 'auth-tag').read_text())]
        return root, reply

    def check(self):
        before = self.calls()
        self.send('CHECK_PLAIN historical mention before manual startup')
        time.sleep(7)  # Outside Buzz's five-second startup subscription overlap.
        self.start()
        time.sleep(2)
        assert self.calls() == before and not self.replies(), 'Manual start replayed history'
        a, _ = self.turn('CHECK_TOOL. Remember ORCHARD in this new thread.', 'WORKSPACE-TOOL-MARKER-4821')
        chat_a = self.state()['conversations'][f'buzz/{self.channel}/{a}']['chat_id']
        b, _ = self.turn('CHECK_CONTEXT in a different new thread.', 'MISSING_CONTEXT')
        chat_b = self.state()['conversations'][f'buzz/{self.channel}/{b}']['chat_id']
        assert chat_a != chat_b
        self.turn('CHECK_LITERAL', '@Fixture denied')
        self.stop()
        time.sleep(6)
        self.start()
        self.turn('CHECK_CONTEXT after a native runner restart.', 'ORCHARD', a)
        assert self.state()['conversations'][f'buzz/{self.channel}/{a}']['chat_id'] == chat_a

        # Relay accepts the message but the client loses its acknowledgement.
        # Recovery must reuse the signed event, without replaying that model turn.
        self.proxy.drop_next = True
        pending_root, reply = self.turn('CHECK_PLAIN lost acknowledgement fixture', 'PLAIN-SYSTEM-VERIFIED')
        eventually(lambda: any(e['status']['status'] == 'pending' for e in self.state()['events'].values()), 'durable pending reply')
        calls = self.calls()
        self.stop()
        time.sleep(6)
        self.start()
        assert self.calls() == calls
        self.turn('CHECK_PLAIN next command recovers prior delivery', 'PLAIN-SYSTEM-VERIFIED', pending_root)
        assert self.calls() == calls + 1, 'Recovery replayed the old inference'
        attempts = [e for e in self.proxy.attempts if e['id'] == reply['id']]
        assert len(attempts) == 2 and attempts[0]['body_sha256'] == attempts[1]['body_sha256']
        assert len([r for r in self.replies() if r['id'] == reply['id']]) == 1
        assert all(e['status']['status'] == 'delivered' for e in self.state()['events'].values())

        before, replies = self.calls(), len(self.replies())
        self.send('CHECK_PLAIN denied author', identity='denied')
        self.send('CHECK_PLAIN denied channel', channel=self.denied_channel)
        time.sleep(3)
        assert self.calls() == before and len(self.replies()) == replies

        # Hold one ordinary turn while two mentions queue, exercising the
        # installed runner's real merged-batch envelope. The adapter must refuse
        # that envelope before any additional inference/publication.
        slow = self.send('CHECK_SLOW')
        eventually(lambda: self.calls() == before + 1, 'slow turn begins')
        self.send('CHECK_PLAIN batched first')
        self.send('CHECK_PLAIN batched second')
        eventually(lambda: any(['e', slow, '', 'reply'] in r['tags'] for r in self.replies()), 'slow turn reply')
        time.sleep(3)
        assert self.calls() == before + 1 and len(self.replies()) == replies + 1, 'Merged batch was executed'
        version = plistlib.loads((self.app / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString']
        evidence = {'passed': True, 'desktop_bundle_version': version, 'native_runner': True,
                    'nsec_and_owner_attestation': True, 'agent_has_no_direct_membership': True,
                    'workspace_tool': True, 'thread_separation': True, 'restart_continuity': True,
                    'literal_mentions_single_recipient': True, 'pending_same_event_recovery': True,
                    'no_model_replay': True, 'denied_author_channel': True,
                    'manual_start_no_historical_replay': True, 'merged_batch_rejected': True,
                    'state_directory': str(self.state_dir.relative_to(ROOT))}
        secret_file(WORK / 'evidence.json', json.dumps(evidence, indent=2) + '\n')
        print('Native macOS runner: tools, A/B/restart, owner attestation, exact delivery, lost-ack recovery and gates: PASS')


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--buzz-app', type=Path, default=Path('/Applications/Buzz.app'),
                        help='Extracted or installed Buzz.app; Desktop is never opened')
    args = parser.parse_args()
    assert args.binary.is_file(), 'Build the candidate first'
    fixture = Fixture(args.binary, args.buzz_app)
    try:
        fixture.check()
    finally:
        fixture.stop()
        fixture.proxy.shutdown()
        fixture.proxy.server_close()
