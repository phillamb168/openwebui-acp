# Testing

Ordinary tests use mocks and read no adjacent `.env` or real account. From the checkout:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo install cargo-deny --version 0.20.2 --locked
cargo deny check advisories licenses sources bans
python3 scripts/export_check.py
python3 tests/chart.py
```

The 47 Rust tests cover ACP/stdout, HTTP/default projection, chat trees, requester/channel routing, signed replies, outbox recovery, account/state isolation, private files, bounds, cancellation and real SIGKILL at write boundaries. They also cover rc.1 checkpoint fingerprint compatibility and Kubernetes projected-secret validation. Six chart tests require Helm (`HELM=/absolute/path/helm` may override PATH), but no cluster. They check render/security/storage contracts and invalid configuration rejection. Python 3.11+ is sufficient for fixture scripts.

## Disposable Open WebUI

Docker/Compose are required only for integration tests. A pinned real Open WebUI runs behind a loopback-only gateway with a deterministic synthetic provider and harmless MCP server. No external model account or provider charges are needed.

```sh
cargo build --locked --examples --bins
docker compose -f tests/integration/compose.yml up -d --wait
python3 tests/integration/bootstrap.py
python3 tests/integration/run_smoke.py
```

Bootstrap creates a local account, key, tool, Skill, Knowledge collection and Workspace models. It discovers ports on the named Compose gateway and accepts no arbitrary endpoint. Eight scenarios/nine prompts exercise plain defaults, Workspace tool, MCP, Skill, Knowledge, disabled builtins, multi-turn context and model-control defaults. Keep generated credentials and evidence under ignored `.work/integration/` with private permissions. After deleting fixture volumes, also archive/remove that directory's old credentials; they belong to the old disposable account.

`browser_capture.py` is an optional Playwright browser/default-projection check. The historical capture used Playwright 1.58.0 and Open WebUI 0.11.3; selector compatibility with newer UI releases is not part of ordinary tests. It records configuration fields only, never HARs, cookies, auth headers or complete messages. Neither Playwright nor a browser is a runtime dependency.

## Real Linux Buzz runner

Keep the Open WebUI fixture running. These commands create a separate relay, database, Redis and object store with synthetic identities and loopback ports:

```sh
docker build -t openwebui-acp-sidecar:local -t openwebui-acp-sidecar:0.1.0-rc.2 -f deploy/sidecar/Dockerfile .
python3 tests/integration/prepare_relay.py
docker compose -f tests/integration/buzz-compose.yml up -d --wait relay
python3 tests/integration/buzz_fixture.py
docker compose -f tests/integration/buzz-compose.yml --profile agent up -d agent
python3 tests/integration/run_buzz.py
```

The test obtains a real server tool result, checks A/B thread isolation, restarts the runner, continues the original chat, and verifies exact recipient/thread tags and state ownership. The Dockerfile's documented runner patches are included. Run suites using this provider sequentially: their global inference counters deliberately detect unexpected calls.

## Fresh Linux VM

Use a disposable Ubuntu 24.04 ARM64 or x86_64 machine. Install Docker/BuildKit/Compose and Python 3, transfer only reviewed source, then build the image above (or load the exact tested architecture-specific image). Start/bootstrap Open WebUI, run `prepare_relay.py`, and start the fixture relay as above. Do not start the fixture's optional agent: this test uses the public Compose deployment instead.

```sh
python3 tests/integration/run_vm.py prepare
# Restart only this disposable machine using its VM manager.
python3 tests/integration/run_vm.py verify
```

The first phase refuses an existing deployment `.env`, creates only synthetic credentials and exercises `deploy/sidecar/compose.yml` through a fixture network override. The second checks changed container start time, same saved chat and exact reply after machine restart. It never reboots a host or accepts operator endpoints. Recorded OrbStack machines share a kernel; machine restart is not evidence of a physical-host/kernel reboot. The emulated x86_64 machine additionally used Docker's cgroupfs driver, and both nested daemons used vfs storage; those are fixture accommodations, not required product settings.

## Kubernetes

Use Helm 4.3.0, Kind 0.33.0 and kubectl. Start/bootstrap the named Open WebUI fixture, prepare synthetic relay identities, and build the version-tagged agent image above. Use a **new disposable cluster** with a dedicated kubeconfig:

```sh
kind create cluster --name openwebui-acp-test --kubeconfig "$PWD/.work/kind-kubeconfig" \
  --image kindest/node:v1.37.0@sha256:a1ed56cfb0e7b93589bdf97c8cd566405a265939e3620fc4f5de89adff580ae5 --wait 120s
kind load docker-image openwebui-acp-sidecar:0.1.0-rc.2 --name openwebui-acp-test
python3 tests/integration/run_kubernetes.py --kubeconfig "$PWD/.work/kind-kubeconfig"
```

The driver accepts only a named single-node `kind-openwebui-acp-*` context. It refuses a preexisting `openwebui-acp-fixture` namespace. It installs the published Buzz chart 0.1.11 with relay 0.2.1, then this agent chart with an existing Secret. CLI Pods use the relay's canonical in-cluster hostname. The fixture gateway is attached to Kind's Docker network; externally published loopback ports may change, so rediscover them before another suite.

Checks include server tools, A/B chats, Pod replacement, key/state owner/modes, no service-account token, denied author/channel, individually queued events, interruption without inference replay, exact recipients, Helm upgrade, retained PVC on uninstall, and same-chat reinstall using that claim. No production kubeconfig, relay credentials or cloud account is used. The quickstart relay/database/object store are fixtures, not production recommendations.

## Native macOS Buzz runner

Run the unmodified bundled `buzz-acp` and `buzz` directly against disposable services. Do not open a second Desktop app with synthetic variables and assume it is isolated. An official extracted app bundle can be selected without replacing the user's installation:

```sh
cargo build --release --locked
cargo run --locked --example fixture_identity
python3 tests/integration/prepare_relay.py
python3 tests/integration/run_native.py --binary target/release/openwebui-acp --buzz-app /absolute/test/Buzz.app
```

Omitting `--buzz-app` selects `/Applications/Buzz.app`. The script currently expects Docker at `/usr/local/bin/docker`. It creates `openwebui-acp-native-test` and a loopback proxy that replaces one successful publish acknowledgement. Synthetic nsec identities, owner attestation, state and logs stay in `.work/native/`. The agent has no direct relay membership, testing delegated authentication with the relay's explicit attestation setting.

The suite checks server tools, thread isolation, process restart, exact tags, identical-event lost-ack recovery, no repeated inference, denied authors/channels, old-mention startup behavior, and real stock-runner merged-batch rejection. It disables thread-context fetch so recall comes from the saved chat. It does not configure the Desktop catalog or replace its managed agents.

## Zed UI

Use an empty workspace and a separate app profile. On macOS launch the official app's `Contents/MacOS/zed` executable **directly**, passing `--user-data-dir /absolute/disposable/profile /absolute/empty/workspace`. Verify process arguments and the selected fixture-agent label before any prompt. Generic launchers can drop isolation arguments. Home-level skills and OS keychain integration are independent of Zed's data directory.

Put a custom agent entry in the isolated profile's `config/settings.json`, based on `examples/zed-settings.json`. Use the fixture loopback URL/key, model `fixture-combined`, `--allow-insecure-http true`, `--memory-default false`, `--buzz-auto-publish false`, and a dedicated state directory. Do not reuse deployment credentials or shared state. Select **+ → External Agents → Open WebUI Fixture** in the Agent panel.

1. Send `CHECK_TOOL. Remember ORCHARD.`, then `CHECK_MCP`, `CHECK_SKILL`, and `CHECK_KNOWLEDGE`, waiting for each result. Check server records as well as visible markers.
2. Quit only the isolated app while idle; verify its adapter exited. Reopen with the same explicit profile and workspace. In the restored conversation send `CHECK_CONTEXT`; expect ORCHARD and identical ACP session/chat IDs.
3. Create another external-agent thread and send `CHECK_CONTEXT`; expect MISSING_CONTEXT and a different chat. Save sanitized version/result evidence, then close the test app.

These six prompts verify the real UI, attachments, process replacement and isolation. A recalled marker alone does not prove restart persistence: check exact IDs. Give independent client processes separate state directories. Goose remains unverified; see [COMPATIBILITY.md](COMPATIBILITY.md).

## Cleanup and live tests

Delete only the disposable cluster/namespace and Compose projects you created. For the names above:

```sh
kind delete cluster --name openwebui-acp-test
docker compose -p openwebui-acp-native-test -f tests/integration/buzz-compose.yml -f .work/native/compose.yml down
docker compose -f tests/integration/buzz-compose.yml --profile agent down -v
docker compose -f tests/integration/compose.yml down -v
```

The native cleanup needs its generated override and preserves fixture volumes. VM cleanup occurs inside its disposable machine. Never substitute a production project/context into fixture commands.

`cargo run --example smoke -- --help` describes the direct ACP client. Without `--execute` it previews the request and does not read credentials, spawn the adapter or contact a server. With `--execute`, it creates a chat and runs the supplied prompts; server tools can have external effects. Prepare exact requests and obtain the operator's specific approval before a production test.
