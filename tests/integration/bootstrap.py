"""Provision only the disposable Compose stack; never accepts a production URL."""
import json, pathlib, subprocess, urllib.request, urllib.error, time, secrets
ROOT = pathlib.Path(__file__).resolve().parents[2]
COMPOSE = ["docker", "compose", "-f", str(ROOT / "tests/integration/compose.yml")]
WORK = ROOT / ".work" / "integration"
WORK.mkdir(parents=True, exist_ok=True, mode=0o700)

def port(container_port):
    address = subprocess.check_output(COMPOSE + ["port", "gateway", str(container_port)], text=True).strip()
    assert address.startswith("127.0.0.1:"), "Fixture must bind exclusively to loopback"
    return "http://" + address

BASE, FIXTURE = port(8080), port(8000)
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))
TOKEN = None

def call(path, value=None, method=None, *, base=BASE, token=True, content_type="application/json"):
    assert base in (BASE, FIXTURE)
    data = value if isinstance(value, bytes) else json.dumps(value).encode() if value is not None else None
    headers = {"Content-Type": content_type}
    if TOKEN and token: headers["Authorization"] = "Bearer " + TOKEN
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with OPENER.open(req, timeout=60) as response: return json.load(response)
    except urllib.error.HTTPError as e:
        # All data belongs to the disposable fixture; still avoid echoing response bodies/keys.
        raise RuntimeError(f"Fixture {path} failed with HTTP {e.code}") from None

def secret_file(name, value):
    path = WORK / name
    fd = __import__('os').open(path, __import__('os').O_CREAT | __import__('os').O_TRUNC | __import__('os').O_WRONLY, 0o600)
    with __import__('os').fdopen(fd, "w") as out: out.write(value)
    path.chmod(0o600)

if __name__ == "__main__":
    for attempt in range(60):
        try: call('/health', token=False); break
        except Exception:
            if attempt == 59: raise
            time.sleep(1)
    # Reuse only this stack's local test account when iterating; no external credentials.
    creds_path = WORK / 'account.json'
    if creds_path.exists():
        creds = json.loads(creds_path.read_text())
        result = call('/api/v1/auths/signin', creds, token=False)
    else:
        creds = {'email': 'fixture@example.invalid', 'password': secrets.token_urlsafe(24)}
        result = call('/api/v1/auths/signup', dict(creds, name='Disposable Fixture'), token=False)
        secret_file('account.json', json.dumps(creds))
    TOKEN = result['token']
    key_path = WORK / 'openwebui-key'
    if not key_path.exists():
        result = call('/api/v1/auths/api_key', {}, method='POST')
        secret_file('openwebui-key', result['api_key'])
    secret_file('webui.env', 'OPENWEBUI_API_KEY=' + key_path.read_text().strip() + '\nOPENWEBUI_ACP_MEMORY_DEFAULT=false\n')
    # Set the browser account preference explicitly to match API smoke tests.
    call('/api/v1/users/user/settings/update', {'ui': {'memory': False}}, method='POST')
    tool = {'id': 'fixture_tool', 'name': 'Fixture Tool', 'content': '''class Tools:
    def fixture_value(self) -> str:
        """Return the synthetic verification value, inside Open WebUI."""
        return "WORKSPACE-TOOL-MARKER-4821"
''', 'meta': {'description': 'Harmless test tool'}}
    existing = call('/api/v1/tools/')
    if not any(t['id'] == tool['id'] for t in existing): call('/api/v1/tools/create', tool)
    skill = {'id': 'fixture-skill', 'name': 'Fixture Skill', 'description': 'Read the attached skill to obtain its verification value', 'content': '# Fixture\nThe skill verification value is SKILL-MARKER-2648. Include it when requested.', 'meta': {}, 'is_active': True}
    skills = call('/api/v1/skills/')
    if not any(s['id'] == skill['id'] for s in skills): call('/api/v1/skills/create', skill)
    call('/control', {'skill_id': skill['id']}, base=FIXTURE, token=False)
    call('/api/v1/configs/tool_servers', {'TOOL_SERVER_CONNECTIONS': [{'url': 'http://fixture:8000/mcp', 'path': '', 'type': 'mcp', 'auth_type': 'none', 'key': '', 'config': {'enable': True}, 'info': {'id': 'fixture-inventory', 'name': 'Fixture Inventory'}}]})
    existing_knowledge = call('/api/v1/knowledge/')
    knowledge_items = existing_knowledge if isinstance(existing_knowledge, list) else existing_knowledge.get('items', [])
    knowledge = next((k for k in knowledge_items if k['name'] == 'Fixture Archive'), None)
    if knowledge is None:
        knowledge = call('/api/v1/knowledge/create', {'name': 'Fixture Archive', 'description': 'Synthetic archive containing a verification code'})
        boundary = 'fixture-upload-boundary'
        body = ('--' + boundary + '\r\nContent-Disposition: form-data; name="file"; filename="archive.txt"\r\nContent-Type: text/plain\r\n\r\nThe archive verification code is KNOWLEDGE-MARKER-7319.\r\n--' + boundary + '--\r\n').encode()
        file = call('/api/v1/files/?process=true', body, content_type='multipart/form-data; boundary=' + boundary)
        call('/api/v1/knowledge/' + knowledge['id'] + '/file/add', {'file_id': file['id']})
    call('/control', {'knowledge_id': knowledge['id']}, base=FIXTURE, token=False)
    models = [
        ('fixture-plain', {}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-tool', {'toolIds': ['fixture_tool']}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-mcp', {'toolIds': ['server:mcp:fixture-inventory']}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-skill', {'skillIds': [skill['id']]}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-knowledge', {'knowledge': [{'id': knowledge['id'], 'name': 'Fixture Archive', 'type': 'collection'}]}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-disabled', {'capabilities': {'builtin_tools': False}}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-controls', {}, 'SYSTEM_FIXTURE_LANTERN'),
        ('fixture-combined', {'toolIds': ['fixture_tool', 'server:mcp:fixture-inventory'], 'skillIds': [skill['id']], 'knowledge': [{'id': knowledge['id'], 'name': 'Fixture Archive', 'type': 'collection'}]}, 'SYSTEM_FIXTURE_LANTERN'),
    ]
    for id, extras, system in models:
        model = {'id': id, 'name': id, 'base_model_id': 'fixture-model', 'params': {'system': system, 'temperature': 0.125, 'function_calling': 'native'}, 'meta': {'capabilities': {'builtin_tools': True, 'web_search': False, 'code_interpreter': False, 'image_generation': False}, 'defaultFeatureIds': [], **extras}, 'is_active': True}
        if id == 'fixture-controls':
            model['params']['model_controls'] = {'thinking': {'label': 'Thinking', 'default': 'low', 'options': {'low': {'label': 'Low', 'params': {'reasoning_effort': 'low'}}}}}
        try: call('/api/v1/models/model?id=' + id)
        except RuntimeError: call('/api/v1/models/create', model)
    secret_file('connection.json', json.dumps({'url': BASE, 'fixture': FIXTURE}))
    print('Disposable account, API key, tool, MCP server and eight Workspace models configured. Credentials remain in ignored owner-only files.')
