# openwebui-acp contributor instructions

Open WebUI owns the model and tool loop. Use its Workspace model ID, project verified UI defaults, and never send a caller-owned `tools` field. Do not execute Workspace/MCP tools locally or expose the ACP CWD as a remote filesystem.

Use the official ACP Rust SDK with the stable v1 protocol. stdout is ACP only; diagnostics on stderr must exclude prompts, credentials and raw remote errors. Buzz delivery is an explicit, narrow integration, not a general tool bridge.

State persistence is opt-in. Never replay an uncertain model/tool submission. Keep instance/account/model/relay/thread state separated and pending delivery durable. Publish only to the validated destination and intended requester.

Use synthetic local tests by default. Production requests and deployment changes require explicit approval for the specific action. Do not load adjacent projects' credentials, copy their private data, or modify their deployments. Only disposable services belong in integration tests. Do not claim tests or platform compatibility that have not actually passed.

Before release run fmt, clippy, boundary tests, dependency/license and export scans, and the disposable integration suite. Maintain the audit and compatibility records. No public publication is authorized by local implementation work.
