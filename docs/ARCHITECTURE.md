# Architecture

`openwebui-acp` is an ACP v1 executable. The official Rust SDK handles JSON-RPC types and dispatch; `transport.rs` bounds line framing before JSON parsing. `acp.rs` owns session IDs, cancellation, thread selection, requester gates, and response delivery. It does not implement a model/tool loop.

`projection.rs` and `openwebui.rs` read Workspace metadata and the account's accessible configuration. They project UI-default `toolIds`, `terminalId`, feature toggles, and toggleable filters where required by the verified API. Server-attached Skills and Knowledge remain server-resolved. Model capability gates are not widened. The Workspace ID, rather than its underlying provider ID, is sent to `/api/chat/completions`; Open WebUI applies the model's prompt and generation parameters.

`session.rs` creates/updates a saved chat with a user node and assistant placeholder, records the submission phase, requests the full native loop, polls chat tasks, and reads the exact assistant node. Task-list emptiness alone is not success: the node must be complete, linked correctly, nonempty, and free of error/approval flags. Outbound requests never contain a caller-owned OpenAI `tools` field. The server may legitimately produce a `tools` field in its own downstream provider request.

The final visible answer is emitted as ACP assistant text. Structured reasoning/output is retained server-side and is not replayed as visible history. CWD is retained as metadata. ACP MCP-server configurations are parsed and discarded, retaining only their count; neither their credentials nor local files or client MCP services are exposed remotely.

## State and recovery

Default mappings are in memory. Optional `state.rs` checkpoints contain endpoint/model/account scope, ACP session metadata, conversation IDs, operation IDs, message IDs, a history fingerprint, and pending/delivered event records. State is capped at 256 sessions, 256 conversations, 10,000 delivery records, and 64 MiB. It is never silently evicted. File permissions, lock ownership, atomic rename, and fsync provide a single-process local journal; this is not a distributed database.

For generic ACP, a conversation is keyed by ACP session ID. With persistent Buzz thread mode, it is keyed by channel UUID and canonical thread-root event, beneath the instance's endpoint/account/model/relay/bot scope. Without persistence, the ACP session is also part of that key. Multiple processes cannot open the same state directory, and simultaneous turns in the same conversation are rejected.

Saving a chat and submitting a model turn are distinct non-idempotent writes. The journal is recorded before each write. Recovery of a recorded submission only polls/reads the known chat. Unknown outcomes while saving or before submission remain quarantined. Completed histories are checked against a fingerprint, preventing silent continuation after external edits.

## Buzz delivery

`buzz.rs` parses only supported context/event envelope headers, before message content, and requires a single unambiguous current event, channel and thread anchor. Both requester and channel must be explicitly allowed before Open WebUI HTTP activity. The launcher is trusted to authenticate inbound events.

The pinned official Buzz SDK constructs a kind-9 reply with the exact channel, thread parent, requester `p` tag, and assistant text. It does not resolve `@names` or add recipients from generated prose. The signed event is persisted before POST `/events`; fresh NIP-98 HTTP authentication wraps each attempt, while the reply event bytes and ID remain identical. Accepted event IDs must match. Acknowledged replies have their pending body removed from state.

Transient relay attempts are bounded to three and to a 30-second deadline. Non-idempotent Open WebUI writes are not retried. Cancellation attempts a bounded stop on the owned chat and never intentionally publishes a late answer. A previously pending signed reply can be recovered when the same thread is next requested.

## Deployment wrapper

The optional container adds a pinned Buzz runner and a separate launcher; the ACP binary remains a stdio process. Compose runs this pair on a VM. The Helm chart runs one Recreate Deployment with an existing Secret and persistent volume, without a Service, Ingress or Kubernetes API token. A non-root init mode converts confined Kubernetes Secret symlinks into private regular files. Runtime secret-file validation is unchanged. The journal remains a single-process filesystem store, not a distributed service.

## Limits

ACP frames: 8 MiB. Text prompts: 1 MiB / 128 blocks. HTTP responses and serialized chat: 16 MiB. Message-tree nodes: 4096. Final ACP text: 1 MiB; Buzz replies: 64 KiB. Secret files: 64 KiB. Active turns: 8. Configured turn deadline: 1–3600 seconds, default 600. Individual requests and cancellation are also bounded.

This release supports visible text, saved-chat history and server-owned tools. It does not provide live token/tool progress, multimodal input, local filesystem access, an ACP permission bridge, general MCP forwarding, a provider API abstraction, relay administration, or task-container orchestration.
