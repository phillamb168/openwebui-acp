# Public release implementation

Status: local **0.1.0-rc.2** candidate. The public repository and hosted release remain the owner's next stage. See [compatibility](COMPATIBILITY.md), [verification](RELEASE_VERIFICATION.md) and [handoff](PUBLIC_RELEASE.md).

- [x] Separate generic source/export boundary; no private deployment/configuration in release inputs.
- [x] Generic model configuration, private files, HTTPS and bounded ACP/HTTP/history.
- [x] Durable scoped state, ACP load, exact signed Buzz outbox and safe crash recovery.
- [x] Requester/channel allowlists and per-thread chats.
- [x] Mocked protocol/security tests and deterministic real Open WebUI attachment fixtures.
- [x] Current Zed UI and native Buzz runner checks with synthetic accounts.
- [x] Linux ARM64/x86_64 containers and public Compose install/restart checks on disposable machines.
- [x] Single-agent Helm chart and actual Kind tests beside the published upstream relay chart.
- [x] Open WebUI 0.12 compatibility, retained checkpoint representation and private projected-secret initialization.
- [x] Bundled runner dependency review, narrow maintained YAML-parser patch and upstream tests.
- [x] Installation, recovery, compatibility and contributor docs; local packaging and CI definitions.
- [ ] Owner-created public repository, monitored vulnerability reporting and executed hosted checks.
- [ ] Verified draft prerelease built from the new public commit and owner-approved publication.

The deployment unit is one outbound runner/adapter with private keys and a single persistent journal. Relay administration, knowledge/oracle services and per-task development environments are separate responsibilities.
