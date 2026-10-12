"""Real relay + Linux runner + real Open WebUI; only synthetic, new threads."""
import io, json, subprocess, tarfile, time
import buzz_fixture as b
import bootstrap

CHANNEL = json.loads((b.WORK / 'buzz-channel.json').read_text())['id']

def state():
    container = subprocess.check_output(b.COMPOSE + ['ps', '-q', 'agent'], text=True).strip()
    result = subprocess.check_output(['docker', 'cp', container + ':/state/state.json', '-'])
    with tarfile.open(fileobj=io.BytesIO(result)) as archive:
        member = archive.getmembers()[0]
        assert member.uid == 10001 and member.mode & 0o077 == 0
        return json.load(archive.extractfile(member))

def turn(text, expected, root=None):
    before = {m['id'] for m in b.cli('owner', 'messages', 'get', '--channel', CHANNEL)}
    args = ['messages', 'send', '--channel', CHANNEL, '--mention', b.IDENTITIES['agent'], '--content', text]
    if root: args += ['--reply-to', root]
    sent = b.cli('owner', *args)['event_id']
    root = root or sent
    for _ in range(30):
        replies = [m for m in b.cli('owner', 'messages', 'get', '--channel', CHANNEL) if m['id'] not in before and m['pubkey'] == b.IDENTITIES['agent']]
        if replies: break
        time.sleep(1)
    assert len(replies) == 1, 'Expected exactly one new reply'
    reply = replies[0]
    assert expected in reply['content'], 'Unexpected synthetic reply'
    assert [t for t in reply['tags'] if t[0] == 'p'] == [['p', b.IDENTITIES['owner']]]
    assert ['e', root, '', 'reply'] in reply['tags']
    assert ['h', CHANNEL] in reply['tags']
    return root, reply['id']

def check():
    root_a, reply_a = turn('CHECK_TOOL. Remember ORCHARD in this new thread.', 'WORKSPACE-TOOL-MARKER-4821')
    first = state()['conversations'][f'buzz/{CHANNEL}/{root_a}']['chat_id']
    root_b, _ = turn('CHECK_CONTEXT in this other new thread.', 'MISSING_CONTEXT')
    second = state()['conversations'][f'buzz/{CHANNEL}/{root_b}']['chat_id']
    assert first != second
    subprocess.run(b.COMPOSE + ['restart', 'agent'], check=True, capture_output=True)
    # Wait for stdio child startup after the container has started.
    for _ in range(20):
        try: state(); break
        except (subprocess.CalledProcessError, json.JSONDecodeError): pass
        time.sleep(1)
    _, reply_c = turn('CHECK_CONTEXT after the runner restart.', 'ORCHARD', root_a)
    final = state()
    assert final['conversations'][f'buzz/{CHANNEL}/{root_a}']['chat_id'] == first
    assert final['conversations'][f'buzz/{CHANNEL}/{root_b}']['chat_id'] == second
    assert all(e['status']['status'] == 'delivered' for e in final['events'].values())
    evidence = {'passed': True, 'threads_separated': True, 'restart_continuity': True, 'real_workspace_tool': True, 'single_recipient': True, 'state_owner_uid': 10001}
    bootstrap.secret_file('buzz-smoke-evidence.json', json.dumps(evidence, indent=2))
    print('Real Linux Buzz sidecar: tool, A/B isolation, restart, exact reply destination: PASS')
    return root_a, first

if __name__ == '__main__':
    check()
