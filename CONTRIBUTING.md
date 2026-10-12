# Contributing

`openwebui-acp` connects ACP clients to an Open WebUI Workspace runtime. Useful contributions include reproducible bugs, compatibility evidence, clearer setup instructions and improvements to the existing protocol boundaries. New clients and deployment examples should preserve this small adapter's scope.

Read [AGENTS.md](AGENTS.md), [the architecture](docs/ARCHITECTURE.md) and [SECURITY.md](SECURITY.md). Open WebUI owns the model/tool loop, stdout carries only ACP, and remote tools do not gain access to the client's filesystem. Discuss changes to those boundaries before implementing them.

Use [Discussions](https://github.com/phillamb168/openwebui-acp/discussions) for feature ideas, setup questions and reports of new use cases. Use [issues](https://github.com/phillamb168/openwebui-acp/issues) for reproducible bugs. Early alpha feedback is especially useful on first installation, supported client versions, thread isolation and restart behavior; include sanitized version/configuration details.

## Local development

Use macOS or Linux with a C toolchain and rustup; `rust-toolchain.toml` selects Rust 1.98.1. Python 3.11+ supports the release and integration scripts. Fork/clone [phillamb168/openwebui-acp](https://github.com/phillamb168/openwebui-acp), or use the reviewed source export, and create a branch for your change.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
python3 scripts/export_check.py
```

The ordinary suite needs no live Open WebUI, relay, account, VPN or credentials. First-time dependency downloads need public network access. See [TESTING.md](docs/TESTING.md) for opt-in disposable Docker/Compose stacks. Never substitute a production URL in those fixtures. From an archive without Git metadata, run the Rust checks directly; the export/package scripts require a Git checkout.

## Changes and reviews

Keep changes focused. Describe the observed problem, resulting behavior and checks actually executed in the pull request. Mark untested platforms or workflows explicitly. Runtime extensions need evidence from the supported upstream source and disposable integration tests; a new configuration example alone does not establish compatibility.

Test trust boundaries and observable behavior: no caller-owned `tools`, no duplicate model submission, cross-thread/account separation, bounded cancellation, exact signed delivery and safe credential handling. Add regression coverage when changing these behaviors. Documentation-only changes need link/example review rather than new implementation-mirroring tests.

Do not commit `.env` files, state, browser profiles, HAR captures, logs or private prompts. Use synthetic names, accounts and conversations. For a bug report, include the adapter/client/Open WebUI versions, platform, sanitized reproduction, expected result and actual result. Do not attach complete environment dumps or state files. Follow [SECURITY.md](SECURITY.md) for vulnerabilities instead of opening a public issue.

Changes to the saved-chat protocol, Buzz SDK pin, ACP SDK, or supported versions require updating compatibility evidence and the audit record. Keep provider-specific or client-specific behavior out of the generic core. Public publishing and deployment are separate operator actions.
