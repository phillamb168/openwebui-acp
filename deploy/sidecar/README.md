# Always-on Linux agent

This Compose deployment runs one `buzz-acp` worker, which launches `openwebui-acp` over stdio. It connects outbound to an existing Buzz relay and Open WebUI. It exposes no ports and mounts neither a Docker socket nor task workspaces. A developer can close their laptop while this agent keeps running on the VM.

Use one deployment per trusted group, bot identity and Workspace model. This is not a relay installation, a coding sandbox, or a way to share subscription entitlements. For Kubernetes, use the separate [agent chart](../chart/README.md).

## Prerequisites

- Linux ARM64 or x86_64 with Docker Engine, BuildKit and Compose v2; outbound access for image/source downloads during the build. Ubuntu 24.04 was used for the VM tests. Its packages are `docker.io`, `docker-buildx`, and `docker-compose-v2`; enable Docker at boot. Docker access grants control of the host.
- The [Open WebUI prerequisites](../../docs/SETUP.md#open-webui-prerequisites), including a dedicated account, API key, Workspace model ID and tool OAuth grants.
- A reachable Buzz relay with a dedicated bot admitted as a relay member and joined to the intended channels. Obtain exact channel UUIDs and requester public keys. Start with human requesters only, then explicitly add any delegating agents.
- Trusted HTTPS/WSS endpoints. Use the relay's canonical hostname: a raw IP or differently named port-forward can select no community. This deployment does not install a VPN or terminate TLS.

The relay administrator can generate an identity with `buzz-admin generate-key` and admit its public key with `buzz-admin add-member --pubkey PUBLIC_HEX_KEY`, using the relay's administrative environment. Redirect key generation into a private file (`umask 077`) instead of a terminal/session log. Save only its secret-key value in the bot key file below. Use Buzz's normal invitation/channel membership flow, or run `buzz users set-profile --name NAME` and `buzz channels join --channel UUID` under that bot's identity. The CLI reads `BUZZ_PRIVATE_KEY`; keep it in a private process environment, never a command-line argument. The adapter does not administer memberships.

## Install

From a reviewed source checkout or extracted source archive:

```sh
cd deploy/sidecar
cp .env.example .env
mkdir -m 700 secrets
sudo install -d -o 10001 -g 10001 -m 700 state
sudo install -o 10001 -g 10001 -m 600 /path/to/saved-openwebui-key secrets/openwebui-key
sudo install -o 10001 -g 10001 -m 600 /path/to/saved-bot-key secrets/buzz-key
```

Edit `.env` with the endpoint/model and both allowlists. It contains settings only, never credentials. `OPENWEBUI_ACP_REQUIRED_TOOLS` optionally requires comma-separated model-default tool IDs; it does not grant or attach tools. Memory defaults to false unless Open WebUI explicitly enables it for the model. Keep `secrets/`, `state/` and their parent operator-controlled.

```sh
docker compose config --quiet
docker compose build
docker compose up -d
docker compose ps
docker compose logs --tail 50 agent
```

Linux bind mounts retain ownership. The runtime requires UID/GID `10001:10001`, regular key files at 0600 and state at 0700. A Compose `secrets` mode declaration alone would not satisfy this check. Do not broaden permissions to fix a startup failure.

Mention the bot in a new thread from an allowed requester. Verify an actual Buzz reply and a new saved Open WebUI chat. Continue that thread, then try another new thread to verify isolation. While idle, run `docker compose restart agent` and continue the original thread: its chat should be retained. Approve exact requests before testing a production model or write-capable tools.

The `unless-stopped` policy starts the container after Docker/VM restart unless an operator stopped it explicitly. The runtime is non-root, has a read-only root filesystem, drops capabilities, prevents privilege escalation, and limits temporary storage, CPU, memory and PIDs. It has no inbound service or HTTP health endpoint. A running process is not an end-to-end readiness check; verify a threaded reply.

## State, upgrades and rollback

One process exclusively owns `state/`; do not scale against a shared journal. Use independent identities and state for other groups. Open WebUI retains chats, Buzz retains messages, and the local journal retains mappings and unacknowledged replies. Back up state as sensitive data while the agent is stopped. Service backups follow those services' policies.

Before upgrading, finish work, stop the agent, back up `state/`, and retain the old image ID/tag. Build a new versioned image, set `OPENWEBUI_ACP_IMAGE` in `.env`, and run `docker compose up -d --no-build`. Keep endpoint/account/model/relay/bot settings unchanged for continuity. To roll back, stop the candidate and restore the previous image. Restore an older journal only after reconciling remote effects and pending deliveries. Never run both versions against one journal or blindly replay uncertain tools. See [RECOVERY.md](../../docs/RECOVERY.md).

To rotate a key within the same account/identity, stop the agent, atomically replace the file with the same owner/mode, and start it. Account/bot changes require separate state. `docker compose down` preserves these bind-mounted files. Deleting local state does not delete remote chats/messages.

## Runner compatibility and verification

The Dockerfile pins Buzz source `f87c10895ea35a9a2cea629fef1d7532a3fecbc5` (Desktop 0.5.28) and applies the one-line [single-event patch](buzz-single-event.patch): queue batches contain one event instead of 50. Upstream has no batch-size option at this revision. Queue handling and queue deduplication preserve individual events across restart; the adapter rejects ambiguous merged envelopes. The patch does not alter relay/SDK authorization or Desktop settings. The earlier Rustls/ChaCha20 lockfile patch is no longer needed.

The image also applies [buzz-persona-yaml.patch](buzz-persona-yaml.patch). Upstream persona parsing introduced deprecated `serde_yaml` and unmaintained `unsafe-libyaml` (RUSTSEC-2024-0449). This narrow patch uses `serde-saphyr` 1.3.0 with only deserialization enabled and adapts the optional Skill-name lookup through `serde_json::Value`. All 145 upstream persona tests pass. Lockfiles, inventories and CI use this same patch. It does not alter the adapter or relay; remove it once an equivalent upstream fix passes the checks.

The unmodified Desktop runner supports sequential mentions but can merge bursts. The bundled headless runner is the supported queue path. Remove its patch only after equivalent upstream behavior passes integration tests.

See [TESTING.md](../../docs/TESTING.md) and [COMPATIBILITY.md](../../docs/COMPATIBILITY.md) for exact evidence. The image is operator-built and unsigned; no public registry image is assumed. Review OS and Rust dependency scans when rebuilding. Inventories, notices and covered MPL source accompany the image. Egress policy, relay operation, backups and TLS remain operator responsibilities.
