# Release-candidate audit

Initial implementation review: 2026-09-12; native-client evidence added 2026-09-14; release preparation reviewed 2026-09-19 and deployment expansion reviewed 2026-10-12. This is an implementation review and reproducible regression record, not an independent penetration test or a guarantee that no defects remain.

The public candidate was constructed in a separate directory from an explicit source-file allowlist. Private configuration, account keys, deployment records, logs, chat captures, local build outputs and private history were excluded. Generic fixtures replace deployment-specific names. The original deployment was not used for integration testing.

## Applicability

The reusable boundary is **ACP client ↔ Open WebUI Workspace runtime**. It applies to organizations that configure tools and model behavior centrally in Open WebUI and want to use that runtime from an ACP client. It is independent of a particular business application: disposable tests cover ordinary chat, a Workspace Python tool, an MCP inventory server, an attached Skill and an attached Knowledge collection. A real Buzz relay/runner test exercises delivery and persistent thread isolation.

It remains intentionally specific to Open WebUI's runtime semantics. A generic OpenAI endpoint cannot substitute for the saved-chat/task APIs. Users who need local filesystem editing, arbitrary client MCP forwarding, live token progress, interactive ACP tool approvals or multi-tenant hosting need additional work. Those limits are documented rather than hidden behind a universal-agent claim.

## Findings and dispositions

| Finding / failure scenario | Priority | Disposition and evidence |
| --- | --- | --- |
| Production URLs, model/tool names, local paths or credentials could escape through copying a private working tree | Release blocker | Separate allowlisted derivative, fresh Git history, explicit export checker, external secret scan, synthetic fixtures and private evidence directory |
| Implicit environment-file discovery or insecure transport could send an operator's credentials to an unintended endpoint | High | Explicit env-file only; defined precedence; HTTPS default; reject URL userinfo/query/fragment; no redirects; origin/path checks; private key-file ownership, permissions and link/type/size checks |
| A FIFO supplied as a credential path could block before file validation | High availability | Open nonblocking, then require a regular private file; bounded FIFO regression test |
| ACP/HTTP/chat payloads and retained state could grow without clear limits | High availability | Frame, content, HTTP, tree, session, active-turn, outbox and state-file bounds; fail visibly instead of silent eviction |
| Volatile mappings lose chat continuity and duplicate-event knowledge after restart | High reliability | Opt-in owner-only atomic journal, exclusive lock, per-instance/account/model/relay/bot scope, persistent thread mapping and generic ACP `session/load` |
| A crash around a non-idempotent model/tool submission could replay its effects | High | Record write phases before network mutation; reconcile known submitted messages by reads only; ambiguous save/interruption states fail closed. Actual SIGKILL tests at chat-save and submitted-turn boundaries verify no completion replay |
| Empty task lists or partial assistant placeholders could be reported as successful turns | High reliability | Validate exact completed assistant node, nonempty content, parent links, error/approval state and saved-chat fingerprint; timeout and cancellation tests |
| A different Open WebUI account or externally edited chat could reuse stale state | High confidentiality/integrity | Account check after key rotation; strict stored scope; saved history fingerprint and ancestry validation; cross-scope/conflict tests |
| Authorization based only on “human” status or ownership would not fit a shared team relay | High | Explicit requester public-key and channel allowlists; approved delegating agents work; unknown authors/channels fail before Open WebUI requests |
| CLI text mention resolution could turn model output into additional notification recipients | High integrity | Official Buzz SDK constructs exact tags; generated names/URIs remain literal; one requester recipient, explicit channel/thread, signature/NIP-98 validation tests |
| Lost publish acknowledgements could duplicate answers or lose the only copy | High reliability | Persist signed event before POST; bounded retries resend identical bytes/ID with fresh HTTP auth; verify matching acknowledgement; remove pending text only after acknowledgement |
| Returning an ACP error for an already delivered event causes Buzz to retry it and can delay new work | Medium reliability | Delivered duplicates return a successful no-op before HTTP, with no additional output or publish. Verified against the real runner's restart replay |
| Buzz's merged textual batches do not provide safe per-event parsing of arbitrary unescaped content | High integrity | Reject ambiguous batches. Bundled headless runner applies a one-line, pinned batch-size=1 patch and uses queue handling; actual A/B/restart test passes. Unpatched Desktop burst/batch support remains limited |
| A general-purpose container base carried unnecessary high/critical OS findings | Release blocker for that image | Replaced with pinned distroless Debian 13, no shell/package manager, a separate minimal launcher and non-root/read-only runtime. Repeated OS scan found zero high/critical findings at the recorded scan date |
| Dependency license texts were absent from several published crates | Release blocker | Retrieve upstream license texts with recorded source revisions; generated build-dependency inventory and complete notices accompany packages |
| Fresh advisory scans identified RUSTSEC-2026-0285 in the adapter and bundled Buzz dependency locks | Release blocker | Update Rustls to 0.23.45; the current Buzz revision already contains the required crypto updates (the rc.1 lockfile patch was removed). Scan/build from those same locks without an advisory exception; final results are in RELEASE_VERIFICATION.md |
| The bundled Buzz lock retained yanked ChaCha20 0.10.0, with an upstream CPU-intrinsic compatibility fix available | Release hygiene | Update the bundled lock to 0.10.2 and make yanked dependencies an explicit release failure; rebuild and retest the sidecar |

| Open WebUI 0.12 rejects API keys on the browser profile route and null message flags | High compatibility | Use API-accessible status/permission routes, require User Status, omit absent wire flags; preserve rc.1 history hashes. Tests reject inaccessible identity before remote writes |
| Projected Kubernetes Secret symlinks conflict with strict runtime private-file rules | High confidentiality | Dedicated non-root init validation/copy into memory-backed 0600 regular files; confinement and unsafe-file tests; executed Pod inspection |
| Overlapping Pods or uninstall could lose journal continuity | High reliability | Fixed replica=1/Recreate, exclusive lock, retained PVC and existing-claim reinstall; interruption, upgrade and reinstall tests passed |
| Fresh image volumes inherited broad directory modes despite build-stage permissions | High confidentiality | Explicit COPY owner/mode for home/state and a fresh-volume integration test; reused volumes are insufficient evidence |
| New bundled Buzz persona parser depends on unmaintained unsafe-libyaml (RUSTSEC-2024-0449) | Release blocker under current advisory policy | Narrow maintained serde-saphyr parser patch, locked dependencies, all 145 upstream persona tests, both bundled roots audited on both Linux architectures; no advisory exception |

## Accepted boundaries and remaining gates

The October OS scan also detected HIGH-severity CVE-2026-84782 in the older distroless image's `libssl3t64` (3.5.7-1~deb13u2). The runtime base was repinned to the upstream image containing 3.5.7-1~deb13u3, then rebuilt and rescanned. The release gate applies to final images, not a historical scan of an earlier digest.

The ACP launcher, host account, Open WebUI instance, model providers and server tools are trusted. An allowed requester acts through the shared account's tool authority. The adapter cannot make a powerful server-side tool read-only through wording, prevent every prompt injection, or undo a completed external write. Server tool policy and separate client accounts are necessary deployment decisions.

Persistent state is plaintext under file permissions; pending replies are sensitive. There is no encrypted journal, distributed lock, automatic state migration/eviction or administrative recovery editor. A compromised local owner can replace state and process configuration. Network filesystem behavior is unverified.

Native Zed UI, the installed macOS Buzz runner against disposable services, and separately approved private Desktop deployment checks passed with the recorded earlier executable. See [COMPATIBILITY.md](COMPATIBILITY.md) for exact versions, evidence and limits, including stock-runner burst rejection and unverified Goose integration. These historical checks are not claims that every later build was retested in those UIs. Current Zed/Buzz versions and Kubernetes lifecycle coverage are recorded in RELEASE_VERIFICATION.md. Optional terminal execution and interactive approvals remain outside demonstrated coverage.

Release preparation produces a reviewed source archive and verification report. Public repository creation and hosted CI are a separate operator handoff described in [PUBLIC_RELEASE.md](PUBLIC_RELEASE.md). Before importing source, configure monitored private vulnerability reporting and review commit attribution. Rebuild and verify release artifacts from the final public commit. No public repository, package, container image or article has been published by this implementation.
