# Public repository handoff

Publication has two stages. Stage one prepared a reviewed, tested **0.1.0-rc.2** candidate locally. Stage two imported that source with fresh history into [phillamb168/openwebui-acp](https://github.com/phillamb168/openwebui-acp), passed hosted checks, and protected `main`. The owner has authorized the finishing pass and prerelease publication for early alpha testing. Downloads are built from the public revision; local preparation artifacts are not relabeled.

## Stage one: source ready for external validation

The candidate covers desktop ACP (Buzz/Zed), an always-on Linux VM through Compose, and an agent Helm chart beside Buzz's separate relay chart. [SETUP.md](SETUP.md) routes new users to the right installation. [COMPATIBILITY.md](COMPATIBILITY.md) distinguishes executed tests from limitations, and [RELEASE_VERIFICATION.md](RELEASE_VERIFICATION.md) records the local evidence.

The handoff includes reviewed source without Git history/private configuration, source and target-specific archives, checksums, chart, locally built container archives, dependency/license notices and build metadata. A fresh extraction must build/test without this workspace, a VPN, or real credentials. Local checksums identify artifacts; they are not signatures or public build attestations.

## Repository creation (completed)

The owner created the public repository and supplied its URL. Private vulnerability reporting is enabled and the owner confirmed security notifications. The following records the original handoff for future maintainers.

1. In GitHub choose **New repository**, select the maintaining account/organization, and use `openwebui-acp` unless another name is intended.
2. Choose **Public**. Suggested description: `ACP v1 adapter for Open WebUI Workspace models, with desktop, Linux VM and Kubernetes deployment options.`
3. Leave automatic README, `.gitignore` and license initialization off. The source already includes them. Create the empty repository yourself.
4. Enable private vulnerability reporting in its Security settings and verify that maintainers receive notifications. No production credentials belong in this repository or its CI.
5. Supply the repository URL to begin stage two. This is the handoff point; local preparation does not push anything.

## Stage two: fresh import and hosted checks

Import only the reviewed source into fresh Git history on `main`. Do not push a private working tree/history. Review initial commit author name/email for public use. The source archive's generated `BUILD.json` records the local candidate and is ignored; do not force-add it. Public builds generate metadata from the new public commit.

Repository links and maintainer/reporting details are populated. [GitHub Actions](https://github.com/phillamb168/openwebui-acp/actions/workflows/check.yml) records hosted execution and results for each revision; local preparation evidence alone is not a passing hosted check. The workflows use hosted runners, read-only tokens, pinned Actions and synthetic services; no production keys or self-hosted runners. All five required checks have passed: three target architectures/platforms, formatting/Clippy, dependency/license and export/secret scans, both Linux images, chart contracts and disposable Open WebUI/Buzz/Kind tests. Branch protection requires pull requests and passing checks, enforces them for administrators, and blocks force pushes and deletion.

The manual package workflow depends on the reusable checks for the same revision, then builds unsigned source/binary/chart/container artifacts. It verifies the extracted downloads and uploads temporary workflow artifacts only. After a successful run, an authorized maintainer downloads and inspects those exact artifacts, records their hashes/build evidence, creates a draft prerelease and verifies the uploaded assets before publishing. Workflow tokens remain read-only. Require a reviewed `main` commit, a tag matching Cargo/chart versions, and all checks on that commit. Never replace a published tag or release with different content. The first public prerelease is `v0.1.0-rc.2`.

Publication requires owner authorization; the owner may perform it or explicitly delegate it after specifying the release work. Public release artifacts must be rebuilt from the public commit; local candidate archives must not be relabeled as those builds. Registry image/OCI-chart publication and signing remain separate decisions; this prerelease supplies downloadable container archives and a chart, with no project registry assumed.

## External validation request

Ask testers to choose one documented installation, record adapter/client/Open WebUI/relay versions, and use harmless synthetic data. Check first reply, same-thread follow-up, separate-thread isolation, idle restart continuity, model-default tools and clear failure on bad configuration. Report expected/observed behavior and sanitized errors through the issue template; use [Discussions](https://github.com/phillamb168/openwebui-acp/discussions) for feature ideas and setup questions. Never request keys, raw state files, HARs or customer chats. Goose, terminal execution, live token streaming and interactive approval bridging are explicit limits, not implied supported features. Stock Buzz Desktop requires sequential mentions; the bundled headless runner handles queued events individually.

Client-specific knowledge/oracle services, agent subscription provisioning and ephemeral development environments remain a separate future project.
