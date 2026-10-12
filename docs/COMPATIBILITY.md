# Compatibility evidence

Current candidate: **0.1.0-rc.2**, checked 2026-10-12. Pass means an executed test, not a configuration example or CI definition. See [RELEASE_VERIFICATION.md](RELEASE_VERIFICATION.md) for final artifact and scan evidence.

| Surface | Recorded version | Coverage |
| --- | --- | --- |
| ACP | Stable v1; official Rust SDK 2.1.0 / schema 1.7.0 | SDK transcripts, sessions, stdout purity, cancellation, `session/load`, SIGKILL recovery |
| macOS ARM64 | Rust 1.98.1 | 47 boundary tests, fmt/Clippy, optimized build; actual Zed UI and native Buzz runner |
| Linux ARM64 and x86_64 | Rust 1.98.1 / Debian builder | 47 boundary tests on each architecture, including child-process crash tests; x86_64 executes under emulation on an ARM host |
| Open WebUI | 0.12.0; source `15818d88fd7bde1b73de33590f50d15d21e8d7c1` | Eight fixture scenarios/nine prompts using real server APIs, tools, MCP, Skills, Knowledge and model controls |
| Buzz SDK/headless runner | Desktop 0.5.28 source `f87c10895ea35a9a2cea629fef1d7532a3fecbc5` | SDK unmodified; bundled runner carries documented single-event and YAML-parser patches |
| Buzz relay | 0.2.1; source `6e5c462ac524de60d7edb46c66130fd779cc9006` | Authenticated fixture relay with memberships, signed replies and exact recipients |
| Buzz Desktop bundled runner | Official 0.5.28 macOS ARM64 executables | Native launch, tools, A/B threads, restart, owner attestation, lost-ack recovery without model replay, denied author/channel, merged-batch rejection; Desktop UI setup is separate |
| Zed | Official 1.23.2 macOS ARM64 | Six actual UI prompts/two chats; Workspace tool, MCP, Skill, Knowledge, app quit/reopen with same ACP/chat IDs, new-session isolation |
| VM deployment | Ubuntu 24.04 ARM64 and emulated x86_64; Docker 29.1.3 / Compose 2.40.3 | Public Compose installation and same-chat continuation after disposable machine restart; exact final evidence is in the verification record |
| Kubernetes | Kind 0.33.0 / Kubernetes 1.37.0 / Helm 4.3.0 | Agent chart beside published Buzz chart 0.1.11/relay 0.2.1; permissions, tools, threads, recovery, queue, upgrade and retained-PVC reinstall |
| Hosted CI | Definitions in this checkout | Not run on GitHub; operator-created public repository is stage two |
| Other platforms | Windows, macOS Intel, musl/Alpine, other storage drivers/clusters | Not validated targets |

Open WebUI image: `ghcr.io/open-webui/open-webui:v0.12.0@sha256:1565038903151ced924b5a3226f73f40f16963df4b910441f6391f10144afdaa`.

Relay image: `ghcr.io/block/buzz:0.2.1@sha256:4e31b7c7abb7d00b6f513dc559e58d2b980416f1dc400aa01bcf762cf2989cfc`. Its independent release series is not the Desktop version. Buzz chart 0.1.11 has an older default app image, so the fixture explicitly overrides it. Operator guides keep the agent and relay charts separate.

## Open WebUI semantics

| Fixture | Demonstrated behavior |
| --- | --- |
| Plain Workspace model | Model system prompt and generation parameters reach the deterministic provider |
| Workspace tool | Open WebUI imports/executes a harmless tool and finishes its native multi-round loop |
| MCP | Open WebUI calls the fixture MCP inventory server; the adapter executes no MCP code |
| Attached Skill | Native `view_skill` returns a marker present only in the attached Skill |
| Attached Knowledge | Native retrieval returns a marker from an uploaded synthetic collection through vector/embedding services |
| Disabled builtins | Provider receives no builtin tool schemas |
| Multi-turn session | Second turn recalls the first turn's marker in the same saved chat |
| Model controls | Server applies model-default reasoning effort from Workspace `params.model_controls` |

Open WebUI 0.12 rejects API keys on `/api/v1/auths/`. The adapter now uses `/api/v1/users/user/status` for identity and `/api/v1/users/permissions` for effective gates. **User Status must be enabled**; disabling it causes a bounded failure before chat creation. User messages omit the optional `done` field instead of sending null, while checkpoint hashing preserves rc.1's representation. This prevents a wire-format fix from invalidating unchanged saved history. It does not guarantee continuity after unrelated server-side history migrations or manual edits.

Current source verification covers [frontend request assembly](https://github.com/open-webui/open-webui/blob/15818d88fd7bde1b73de33590f50d15d21e8d7c1/src/lib/components/chat/Chat.svelte) and [server middleware](https://github.com/open-webui/open-webui/blob/15818d88fd7bde1b73de33590f50d15d21e8d7c1/backend/open_webui/utils/middleware.py). The adapter projects tool/terminal IDs, features and toggleable filters, and leaves model-attached Skills/Knowledge server-resolved. It never supplies a caller-owned `tools` array. Historical 0.11.3 browser capture confirmed selected defaults and a nonempty session ID; that exact capture was not rerun against 0.12.0. Current runtime/attachment tests are independent executed evidence.

Fixtures prove transport/runtime integration, not real-LLM judgment, business-tool safety or OAuth reliability. Real terminal execution, third-party write tools and interactive ACP approval bridging remain untested/unsupported as documented.

## Delivery and deployment limits

The native Buzz suite uses queue handling, one worker and adapter-owned thread maps. A synthetic bot without direct relay membership authenticates via owner attestation; the relay must allow that method. A lost reply acknowledgement is recovered by sending identical signed event bytes/ID, with one visible reply and no repeated inference. Literal generated `@names` add no recipients.

Stock Desktop can merge multiple pending events. The adapter rejects this before inference/publication; the runner retries with bounded backoff. Sequential mentions work. The bundled headless runner's pinned one-event patch handles queued events individually. An older mention was ignored on manual startup in the fixture, but a recent event inside the runner's subscription overlap can replay; durable delivery IDs handle already-delivered events.

The Kubernetes chart fixes replicas at one with Recreate and a private journal. Kind local-path storage is the tested backend. It is not high availability, tenant isolation, a distributed journal, an arbitrary-CSI guarantee or cloud-cluster certification. OrbStack VM machines share a kernel, so their restart check verifies machine/container service continuity rather than physical host reboot. The x86_64 run uses emulation.

Zed's isolated profile is passed to the app executable directly; launcher wrappers can drop those arguments. OS keychain and home-level skill discovery are separate from its data directory. Only the explicit synthetic agent was prompted. One active workspace owns a persistent state directory. Native UI cancellation and simultaneous editor processes sharing that directory were not tested; exclusive-lock behavior has boundary coverage.

## Earlier client evidence and Goose

September's 0.1.0-rc.1 used Open WebUI 0.11.3, Buzz Desktop 0.5.23 and a pinned development relay. Its 41 boundary tests, native Buzz recovery suite and Linux ARM64 sidecar passed. Zed 1.19.2 also passed eight UI prompts/two chats and process replacement. Those are historical observations, not current-version or current-advisory guarantees.

Goose 1.29.1 was inspected, then rechecked at 1.50.0 (source `4cc49a8485f7960efeceba37ac5f572459c52c7c`). Its actual manual-provider UI and [registry](https://github.com/aaif-goose/goose/blob/4cc49a8485f7960efeceba37ac5f572459c52c7c/crates/goose/src/providers/init.rs) expose agent-specific ACP integrations rather than arbitrary executable registration. The [custom provider engines](https://github.com/aaif-goose/goose/blob/4cc49a8485f7960efeceba37ac5f572459c52c7c/crates/goose-providers/src/declarative.rs) are API providers. [ACP discovery](https://github.com/aaif-goose/goose/blob/4cc49a8485f7960efeceba37ac5f572459c52c7c/crates/goose/src/acp/provider.rs) requires model-category `config_options`, which this adapter does not advertise. External Backend uses HTTP/WebSocket, not a stdio executable. No supported Goose setup, end-to-end test or restart claim is made; no experimental build was created.
