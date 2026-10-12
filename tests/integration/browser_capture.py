"""Opt-in browser-default capture. Requires Playwright; no production URL input.

Stores only request configuration, never cookies, auth headers, or chat bodies.
"""
import json
from playwright.sync_api import sync_playwright
import bootstrap as b

if __name__ == '__main__':
    captured = []
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()
        page.goto(b.BASE + '/auth', wait_until='domcontentloaded')
        account = json.loads((b.WORK / 'account.json').read_text())
        page.get_by_placeholder('Enter Your Email').fill(account['email'])
        page.locator('input[name=password]').fill(account['password'])
        page.get_by_role('button', name='Sign in', exact=True).click()
        page.locator('#chat-input').wait_for(timeout=20000)
        welcome = page.get_by_role('button', name="Okay, Let's Go!")
        if welcome.is_visible(): welcome.click()
        page.get_by_role('button', name='Selected model: fixture-model').click()
        page.get_by_text('fixture-combined', exact=True).click()
        def request(req):
            if req.url == b.BASE + '/api/chat/completions' and req.method == 'POST':
                data = req.post_data_json
                captured.append({k: data.get(k) for k in ('model', 'stream', 'tool_ids', 'skill_ids', 'terminal_id', 'features', 'filter_ids', 'params')} | {'has_session_id': bool(data.get('session_id')), 'has_chat_id': bool(data.get('chat_id')), 'has_tools_key': 'tools' in data})
        page.on('request', request)
        page.locator('#chat-input').fill('CHECK_TOOL')
        page.locator('#chat-input').press('Enter')
        page.get_by_text('SERVER_TOOL_RESULT: WORKSPACE-TOOL-MARKER-4821', exact=False).last.wait_for(timeout=60000)
        assert len(captured) == 1
        c = captured[0]
        b.secret_file('browser-defaults.json', json.dumps(captured, indent=2))
        assert c['model'] == 'fixture-combined'
        assert set(c['tool_ids']) == {'fixture_tool', 'server:mcp:fixture-inventory'}
        # This frontend may submit its first unsaved UI turn without chat_id.
        # The adapter deliberately saves its chat first for durable ACP recovery.
        assert c['has_session_id'] and c['stream']
        assert not c['has_tools_key']
        browser.close()
    print('Disposable browser default projection capture: PASS')
