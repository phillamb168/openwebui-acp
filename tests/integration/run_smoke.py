"""Run the official Rust ACP smoke client only against this disposable stack."""
import json, os, pathlib, subprocess
import bootstrap
ROOT, WORK = bootstrap.ROOT, bootstrap.WORK
connection = json.loads((WORK / 'connection.json').read_text())
assert connection['url'] == bootstrap.BASE, 'Re-run bootstrap after stack recreation'
bootstrap.TOKEN = (WORK / 'openwebui-key').read_text().strip()
RESULTS = []
for model, prompts, expected in [
    ('fixture-plain', ['CHECK_PLAIN'], 'PLAIN-SYSTEM-VERIFIED'),
    ('fixture-tool', ['CHECK_TOOL'], 'WORKSPACE-TOOL-MARKER-4821'),
    ('fixture-mcp', ['CHECK_MCP'], 'INVENTORY-MARKER-4821'),
    ('fixture-skill', ['CHECK_SKILL'], 'SKILL-MARKER-2648'),
    ('fixture-knowledge', ['CHECK_KNOWLEDGE'], 'KNOWLEDGE-MARKER-7319'),
    ('fixture-disabled', ['CHECK_PLAIN'], 'PLAIN-SYSTEM-VERIFIED'),
    ('fixture-controls', ['CHECK_PLAIN'], 'PLAIN-SYSTEM-VERIFIED'),
    ('fixture-plain', ['Remember ORCHARD', 'CHECK_CONTEXT'], 'ORCHARD'),
]:
    before = len(bootstrap.call('/evidence', base=bootstrap.FIXTURE, token=False)['calls'])
    command = [str(ROOT / 'target/debug/examples/smoke'), '--binary', str(ROOT / 'target/debug/openwebui-acp'), '--url', bootstrap.BASE, '--env-file', str(WORK / 'webui.env'), '--model', model, '--allow-insecure-http', '--execute']
    for index, prompt in enumerate(prompts):
        path = WORK / f'{model}-{index}.txt'; path.write_text(prompt)
        command += ['--prompt-file', str(path)]
    environment = {key: os.environ[key] for key in ('PATH', 'TMPDIR', 'SYSTEMROOT') if key in os.environ}
    environment['RUST_LOG'] = 'warn'
    result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True, timeout=90)
    # Synthetic output only; retained outside the public source and never printed wholesale.
    bootstrap.secret_file(model + '-smoke.txt', result.stdout + '\n' + result.stderr)
    passed = result.returncode == 0 and expected in result.stdout
    if model == 'fixture-disabled':
        calls = bootstrap.call('/evidence', base=bootstrap.FIXTURE, token=False)['calls'][before:]
        passed = passed and bool(calls) and all(not c['tools'] for c in calls)
    if model == 'fixture-controls':
        calls = bootstrap.call('/evidence', base=bootstrap.FIXTURE, token=False)['calls'][before:]
        passed = passed and bool(calls) and all(c.get('reasoning_effort') == 'low' for c in calls)
    RESULTS.append({'model': model, 'prompts': len(prompts), 'expected': expected, 'passed': passed})
    print(model + ': ' + ('PASS' if passed else 'FAIL'))
    if not passed:
        print('Sanitized diagnostics:', '\n'.join(line for line in result.stderr.splitlines() if 'ACP' in line or 'Open WebUI' in line))
        break
bootstrap.secret_file('smoke-results.json', json.dumps(RESULTS, indent=2))
if not all(r['passed'] for r in RESULTS) or len(RESULTS) != 8: raise SystemExit(1)
evidence = bootstrap.call('/evidence', base=bootstrap.FIXTURE, token=False)
bootstrap.secret_file('provider-evidence.json', json.dumps(evidence, indent=2))
assert evidence['mcp_calls'] > 0
assert any('tool' in call['roles'] for call in evidence['calls'])
assert any(call['system_present'] and call['temperature'] == 0.125 for call in evidence['calls'])
print('Server-side tool execution, system/parameter preservation, and multi-turn ACP verified.')
