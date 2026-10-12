# First installation

Choose [desktop](#desktop-clients), [Linux VM](../deploy/sidecar/README.md), or [Kubernetes](../deploy/chart/README.md). All three use the same executable and an existing Open WebUI installation. Start with a disposable account/model while validating your setup.

## Open WebUI prerequisites

1. Use the version recorded in [COMPATIBILITY.md](COMPATIBILITY.md). Saved-chat APIs are version-sensitive; retest before upgrading either side.
2. Create or select a **Workspace model**, then copy its exact model ID from the Workspace model editor. A display name or its base provider model ID is not a substitute. Configure its prompt, parameters, native function calling, tools, MCP connections, Skills and Knowledge in Open WebUI.
3. Use an account that can access that model and its intended attachments. An administrator may need to grant access and enable API keys, but the adapter does not require an administrator account. For a shared bot, use a dedicated account and restrict its accessible tools/data.
4. Obtain an API key from that account's settings and save it directly to a private file. Complete any tool OAuth sign-in in Open WebUI for that same account. Do not paste credentials into a client command, Git, issue, or chat.
5. On Open WebUI 0.12.0, **User Status must be enabled**. The adapter uses its API-accessible profile endpoint to bind persisted state to the account. The browser session-info endpoint now rejects API keys. If this feature is disabled, the adapter fails before creating a chat; it does not bypass the identity check.
6. Both the executable and, for Buzz, its runner must reach the configured endpoints with trusted TLS certificates. A VPN may be needed for a private installation. Public deployment does not require a specific VPN product.

If the server restricts API-key endpoints, allow only the routes needed by this account:

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/models`, `/api/config` | Effective models and server feature gates |
| GET | `/api/v1/models/model?id=MODEL_ID` | Workspace defaults |
| GET | `/api/v1/users/user/status`, `/api/v1/users/permissions`, `/api/v1/users/user/settings` | Account identity, permissions, UI defaults |
| GET | `/api/v1/tools/` | Default-tool access and authentication status |
| GET | `/api/v1/terminals/` | Only when the model selects a terminal |
| POST | `/api/v1/chats/new`, `/api/v1/chats/CHAT_ID` | Save the conversation tree |
| GET | `/api/v1/chats/CHAT_ID` | Read completed turns/recover known state |
| POST | `/api/chat/completions` | Start the server-owned agent loop |
| GET | `/api/tasks/chat/CHAT_ID` | Wait for completion |
| POST | `/api/tasks/chat/CHAT_ID/stop` | Cancellation/bounded cleanup |

The adapter does not administer models, tools, memberships or keys. Restricting this route list does not make a write-capable Workspace tool read-only; enforce that policy at the tool/account layer.

## Desktop clients

Build with `cargo build --locked --release`, or verify/extract a target-specific archive using [RELEASE.md](RELEASE.md). Keep the executable at a stable absolute path. Create a private directory and an explicitly selected environment file:

```sh
install -m 700 -d "$HOME/.config/openwebui-acp"
install -m 600 /path/to/saved-api-key "$HOME/.config/openwebui-acp/api-key"
install -m 600 .env.example "$HOME/.config/openwebui-acp/agent.env"
```

Edit `agent.env` with the HTTPS URL, Workspace model ID and absolute API-key-file path. Environment files do not expand `$HOME` or `~`; enter complete paths. Use a separate private state directory for each independently running instance. No adjacent `.env` is automatically loaded.

### Zed

Merge [examples/zed-settings.json](../examples/zed-settings.json) into Zed's user settings, replacing all three absolute paths. Keep the agent name distinct from existing entries. The `--state-dir` argument enables Zed's `session/load` path so restored conversations can resume their Open WebUI chats.

Open the command palette, run **agent: toggle**, then use the **+** at the top of the Agent panel and select **Open WebUI** under External Agents. Send a harmless first request. The completed reply should appear in that panel and in a new Open WebUI chat. A second message in that thread should keep the same chat; **+ → Open WebUI** creates another session/chat.

For an isolated macOS test, launch the app's `Contents/MacOS/zed` executable directly with `--user-data-dir /absolute/disposable/profile`, and put settings in that profile's `config/settings.json`. Verify the process arguments and fixture-agent name before prompting. Do not rely on a generic app launcher to preserve isolation arguments. Zed's OS keychain integration and home-level skill discovery are separate from the data directory; use only the explicit fixture agent.

One running workspace owns each adapter state directory. Opening another workspace with the same state path fails its exclusive lock; give independent workspaces independent paths.

### Buzz Desktop

Create a new managed agent. In its harness/runtime picker, choose **Add custom harness…**. Fill Name and Command using [examples/buzz-harness.json](../examples/buzz-harness.json); add each `args` item as its own argument. The command and configuration/state paths must be absolute. The JSON example documents the catalog fields; it is not an instruction to replace Buzz's existing agent records.

In that agent's environment file, enable Buzz publication and set **both** allowlists to exact requester hex public keys and channel UUIDs. Obtain these from the account/channel details or Buzz's `users` / `channels` CLI output, not display names. Start with your own human key only. Delegating agents require explicit addition of their own public keys.

Use one worker, direct mentions, queue handling, and the selected channels. Buzz Desktop supplies the bot identity and relay credentials to the process; do not put a different bot key in its environment file. The account that owns the agent must be a relay member; delegated owner attestations additionally require the relay's `BUZZ_ALLOW_NIP_OA_AUTH=true` setting. A relay administrator owns that change.

Start the agent manually and mention it in a **new thread** with a harmless request. Confirm an actual channel/thread reply, not only Activity text. Check the saved Open WebUI chat. Send another request in the same thread, then one in a new thread to verify continuity and isolation. Wait for a reply before sending the next request: the stock Desktop runner can merge bursts, which this adapter rejects. The bundled headless runner handles queued mentions individually through its documented one-line patch.

## If setup fails

| Symptom | Check |
| --- | --- |
| Profile 403 before a chat exists | User Status enabled; API-key route access; account still active |
| 401/403 on another route | Key belongs to this instance/account and has access to the model/tools |
| Required tool missing or OAuth link returned | Model-default tool IDs and that account's Open WebUI OAuth connection |
| Private-file/state error | Correct owner, 0600 key files, 0700 state, no links; one process per state directory |
| Relay says no community for this host | Connect to the relay's configured canonical hostname; a raw IP/port-forward hostname may select no community |
| Activity answer without channel response | Buzz publication enabled; both allowlists correct; publishing authenticated |
| Merged/batched context rejected | Send sequential Desktop mentions, or use the packaged headless runner |
| Timeout / uncertain turn | Follow [RECOVERY.md](RECOVERY.md); inspect remote effects before reissuing work |

For external testing, record versions, install path, expected behavior and sanitized errors using the issue template. Avoid HAR files, raw environments, state journals, keys and customer conversations.
