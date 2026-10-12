# Security boundary

This release candidate has a documented code review and regression suite, not an independent security certification. Review [audit findings](docs/AUDIT.md) and [version evidence](docs/COMPATIBILITY.md).

The host account, ACP launcher, selected Open WebUI instance, its administrators, configured providers, and configured tools are trusted. A compromised model/tool can produce misleading content or use any authority Open WebUI grants it. This adapter does not sandbox those remote tools or enforce per-tool human approvals.

Buzz mode trusts `buzz-acp` to authenticate the source event and construct its structured envelope. The adapter validates envelope structure, requester public key, channel, and reply anchor; it does not receive or independently verify the original signed source event. Do not expose stdio through an unauthenticated socket or allow untrusted programs to construct ACP envelopes. Prompt-injected text is not allowed to choose new publish destinations or notification recipients.

Use separate Open WebUI accounts/keys, Buzz bot identities, allowlists, and state directories for separate clients or trust groups. Within one instance, approved people and approved delegating agents share a tool identity and account authority. Server memory and attached Knowledge can span chats. Configure server-side permissions accordingly; thread isolation alone is not a security boundary between customers.

## Credentials, storage, and transport

- HTTPS/WSS by default; no redirects with credentials. Plain HTTP requires explicit opt-in. Protect proxy configuration and the host's trust store.
- Load keys from private regular files or the process environment. File paths must be controlled by the operator, with trusted parent directories. Final symlinks, hard links, wrong ownership, broad permissions, oversized files, and FIFOs are rejected.
- State is optional, owner-only, bounded, locked to one process, atomically replaced and synced. It is namespaced by endpoint/account/model/relay/bot. Local administrators and the owning account can read or alter it; it is not encrypted at rest or a tamper-proof ledger.
- Pending replies are stored as signed events, including their text, until acknowledged. Delivered records retain IDs. No API key, bot secret, or complete transcript belongs in adapter state.
- Open WebUI stores chats, and Buzz stores messages under their own retention policies. Deleting local state does not delete either remote record.
- The adapter excludes prompts, keys, auth headers, and raw HTTP error bodies from its diagnostics. ACP output intentionally carries the assistant answer; protect client logs. Buzz/Open WebUI/provider logs have their own policies.

The Kubernetes chart's explicit init step accepts projected Secret symlinks only inside its read-only source mount and copies them into private regular files in memory. The runtime still rejects links and broad permissions. Cluster administrators, Secret readers and identities able to exec into Pods remain trusted. The chart provides no tenant isolation, default egress policy or distributed lock; it fixes one replica and retains its PVC on Helm uninstall. Namespace/PVC deletion can still destroy state.

Signed replies are written to the outbox before network submission. A lost acknowledgement causes recovery to resend the identical signed event. This depends on the pinned relay's duplicate-event handling and is not a claim of globally exactly-once tool execution. Interrupted or uncertain model writes require reconciliation; never fix them by editing the journal into an earlier phase.

## Reports

The project is maintained by [@phillamb168](https://github.com/phillamb168). Use [GitHub private vulnerability reporting](https://github.com/phillamb168/openwebui-acp/security/advisories/new), also available under **Security → Report a vulnerability**. Repository maintainers must keep private reporting enabled and verify their notification preferences; see [the publication handoff](docs/PUBLIC_RELEASE.md).

A report should identify the affected revision, a synthetic reproduction, impact and the observed boundary failure. If private reporting is unavailable, a public issue may request that it be restored, without describing the vulnerability. Do not include credentials, customer data, full chats or exploitable production details in public issues. No response-time commitment or independent security certification is claimed for this prerelease.
