# Public repository handoff

Publication has two stages. Stage one prepared a reviewed, tested **0.1.0-rc.2** candidate locally. The owner has created [phillamb168/openwebui-acp](https://github.com/phillamb168/openwebui-acp); stage two imports the reviewed source with fresh history on `main` and executes hosted checks. Local preparation did not publish a repository, package, chart or container image. A downloadable prerelease remains a separate owner-reviewed step.

## Stage one: source ready for external validation

The candidate covers desktop ACP (Buzz/Zed), an always-on Linux VM through Compose, and an agent Helm chart beside Buzz's separate relay chart. [SETUP.md](SETUP.md) routes new users to the right installation. [COMPATIBILITY.md](COMPATIBILITY.md) distinguishes executed tests from limitations, and [RELEASE_VERIFICATION.md](RELEASE_VERIFICATION.md) records the local evidence.

The handoff includes reviewed source without Git history/private configuration, source and target-specific archives, checksums, chart, locally built container archives, dependency/license notices and build metadata. A fresh extraction must build/test without this workspace, a VPN, or real credentials. Local checksums identify artifacts; they are not signatures or public build attestations.

## Repository creation (completed)

The owner created the public repository and supplied its URL. The following records the original handoff; maintainers still need to verify their private-report notification preferences in GitHub.

1. In GitHub choose **New repository**, select the maintaining account/organization, and use `openwebui-acp` unless another name is intended.
2. Choose **Public**. Suggested description: `ACP v1 adapter for Open WebUI Workspace models, with desktop, Linux VM and Kubernetes deployment options.`
3. Leave automatic README, `.gitignore` and license initialization off. The source already includes them. Create the empty repository yourself.
4. Enable private vulnerability reporting in its Security settings and verify that maintainers receive notifications. No production credentials belong in this repository or its CI.
5. Supply the repository URL to begin stage two. This is the handoff point; local preparation does not push anything.

## Stage two: fresh import and hosted checks

Import only the reviewed source into fresh Git history on `main`. Do not push a private working tree/history. Review initial commit author name/email for public use. The source archive's generated `BUILD.json` records the local candidate and is ignored; do not force-add it. Public builds generate metadata from the new public commit.

Populate repository-specific links and maintainer/reporting details. Existing workflow definitions are prepared but have not run on GitHub. They use hosted runners, read-only tokens, pinned Actions and synthetic services; no production keys or self-hosted runners. Required checks cover three target architectures/platforms, formatting/Clippy, dependency/license and export/secret scans, both Linux images, chart contracts and disposable Open WebUI/Buzz/Kind tests. Configure branch protection after the first successful run, including protection against force pushes and deletion.

The manual package workflow depends on the reusable checks for the same revision, then builds unsigned source/binary/chart/container artifacts. It uploads temporary workflow artifacts only. Add a separate draft-release job after reviewing successful hosted results. Require a reviewed `main` commit, a tag matching Cargo/chart versions, and all checks on that commit. Never replace a published tag or release with different content. The intended first public prerelease is `v0.1.0-rc.2`.

Inspect the draft and let the owner publish it. Public release artifacts must be rebuilt from the public commit; local candidate archives must not be relabeled as those builds. Registry image/OCI-chart publication and signing can be configured in this stage as explicit owner decisions; neither is required to build/install from source.

## External validation request

Ask testers to choose one documented installation, record adapter/client/Open WebUI/relay versions, and use harmless synthetic data. Check first reply, same-thread follow-up, separate-thread isolation, idle restart continuity, model-default tools and clear failure on bad configuration. Report expected/observed behavior and sanitized errors through the issue template. Never request keys, raw state files, HARs or customer chats. Goose, terminal execution and interactive approval bridging are explicit limits, not implied supported features.

Client-specific knowledge/oracle services, agent subscription provisioning and ephemeral development environments remain a separate future project.
