# Local prerelease verification

Candidate: **0.1.0-rc.2**. Preparation date: **2026-10-12**. This is local stage-one evidence; no public repository, hosted CI execution, registry publication or public release is claimed. [COMPATIBILITY.md](COMPATIBILITY.md) records exact upstream versions and limits; [PUBLIC_RELEASE.md](PUBLIC_RELEASE.md) describes the owner handoff.

## Changes reviewed

- Added a single-agent Helm chart with an existing Secret, private projected-file initialization, a retained PVC, fixed one-replica Recreate lifecycle, and no inbound service or Kubernetes API credentials.
- Completed desktop, Linux Compose and Kubernetes setup/lifecycle guides, synthetic VM/Kind fixtures, external-test issue fields, and workflows covering all release targets. Manual artifact jobs depend on same-revision checks and run only from `main`; they still require hosted verification.
- Updated to Open WebUI 0.12.0, Buzz Desktop/SDK 0.5.28 and stable relay 0.2.1. Adapted API-key identity/permission lookup and message serialization, retaining the rc.1 checkpoint fingerprint representation.
- Kept the headless runner's one-event queue patch. Its newer upstream crypto lock supersedes the old dependency patch. Added a narrow persona YAML-parser patch to remove unmaintained unsafe-libyaml (RUSTSEC-2024-0449) from bundled executable graphs, with all 145 upstream persona tests passing.
- Fixed fresh-volume image directory modes and refreshed the distroless base to `sha256:e792ab3d241a468a4fd7519ddbbebe66b49b5f365771716ea688ad40b6c6f1c2`. The earlier base contained HIGH CVE-2026-84782 in libssl3t64; the replacement contains the fixed 3.5.7-1~deb13u3 package.

## Executed checks

| Check | Evidence |
| --- | --- |
| Rust formatting/Clippy | Pass, all targets, locked dependencies, warnings denied |
| Adapter boundaries | 47 passed separately on macOS ARM64, Linux ARM64 and emulated Linux x86_64; includes real crash/restart, stdout, permissions, isolation, cancellation and no caller-owned tools |
| Clean source | The same 47 tests passed from an allowlisted source export outside any Git worktree, without private files or live credentials; final archive/content checks are recorded in the handoff manifest |
| Helm contracts | Six tests pass, including image/version consistency, security/storage shape, existing claims and rejection of bad/insecure configuration; lint/template checks pass |
| Open WebUI integration | Eight scenarios/nine prompts pass against real disposable 0.12.0 with deterministic model provider: defaults, tool, MCP, Skill, Knowledge, disabled builtins, context and model-control defaults |
| Zed UI | Official 1.23.2, six prompts/two chats: Workspace tool, MCP, Skill, Knowledge, quit/reopen retains ACP/chat IDs and context, new session has separate context |
| Native Buzz runner | Official 0.5.28 bundled macOS executables pass tools, A/B chats, restart, nsec/owner attestation, exact recipients, lost-ack identical-event recovery, no model replay, denied author/channel and stock merged-batch rejection; final remapped binary was retested |
| Linux VM | Public Compose install passes in fresh Ubuntu 24.04 ARM64 and emulated x86_64 machines; original chat and exact replies survive machine restart. Final image upgrades/restart are included in handoff evidence |
| Kubernetes | Actual Kind 0.33.0/Kubernetes 1.37.0, Helm 4.3.0, published Buzz chart 0.1.11/relay 0.2.1: tools, A/B chats, private projected keys/state, denied author/channel, queued events, interrupted turn without inference replay, exact recipients, Pod replacement, Helm upgrade and retained-PVC reinstall pass. Final base-image replacement also checks tool/restart continuity |
| Bundled parser | All 145 upstream buzz-persona tests pass with the maintained parser patch |
| Rust dependencies/licenses | Cargo-deny 0.20.2 passes the adapter graph and both bundled executable roots on both Linux architectures; no advisory/yank exceptions |
| Inventories | 318 adapter and 356 bundled-runner normal/build components across targets; zero license-text gaps |
| OS image scans | Trivy 0.74.0 with refreshed advisory database: zero HIGH/CRITICAL findings in both final ARM64 and x86_64 images; scan results and image IDs are in the handoff manifest. Rust dependency graphs are audited separately |
| Export/privacy | Allowlisted source, Gitleaks 8.30.1 and private-reference review pass; no keys, deployment state, captures, private URLs or private Git history belong in the handoff |
| Documentation | Local file links, Python syntax, workflow/Compose YAML and workflow shell syntax pass. Whitespace review excludes verbatim generated upstream license texts and unified-diff patch context |

Scans are point-in-time evidence, not independent certification. Duplicate dependency versions and unused license allowances emit informational warnings; there are no vulnerability/yank waivers. The bundled CLI's scoped MPL license allowance is accompanied by covered source and license text.

## Artifact traceability and scope

The final remapped macOS executable SHA-256 is `a6b9f79c429155d3b9f006e6cba1d138871022bc58e21babe4f8fba640cb9c8a`. Zed UI used the same source/lockfile with ordinary build paths, SHA-256 `401afc7951b00c26e634c0485d25edc518b6df1fdb04b207512ff186147b93ca`; the remapped distribution binary was subsequently checked with the native Buzz suite. Linux executables were extracted from their matching image build stages, checked for architecture and GLIBC requirements, and recorded in the handoff. Path remapping is not a claim of bit-for-bit reproducibility.

Every source/binary archive has `BUILD.json` identifying the candidate commit, version, target and lockfile hash. The handoff's `VERIFICATION.json` records artifact hashes, local image IDs, final scan counts and extraction checks. Local IDs are not published registry digests. Public import creates new Git history and requires new release builds from that public commit.

The VM checks use isolated OrbStack machines with a shared kernel; x86_64 is emulated. They demonstrate machine/service restart continuity, not physical host reboot or bare-metal x86_64 certification. Kubernetes coverage uses one local-path Kind cluster, not arbitrary CSI, a managed cloud, HA or multi-tenant isolation. Stock Buzz Desktop burst support, Goose integration, terminal execution, interactive ACP approvals and exact 0.12 browser-request parity remain limited or unverified as documented.

Tests use synthetic local services, model responses and identities. No production inference/tool test was part of this release preparation, and installed agents were not upgraded. A test Zed launch initially selected the regular profile; it was stopped before any prompt, then relaunched with an explicitly verified isolated profile. The successful UI evidence is from that isolated fixture.

## Remaining public-release gates

The owner creates the empty public repository, reviews public attribution and enables monitored private vulnerability reporting. Import the reviewed source, execute hosted workflows, configure required checks, rebuild artifacts from the public commit and review a draft prerelease. None of those publication steps has been performed locally. The knowledge/oracle and ephemeral-development-environment system remains a separate project.
