# Release candidate

Version **0.1.0-rc.2** is a source prerelease candidate in [phillamb168/openwebui-acp](https://github.com/phillamb168/openwebui-acp). Stage one prepared and tested local artifacts; stage two imports reviewed source and executes hosted checks. Follow [PUBLIC_RELEASE.md](PUBLIC_RELEASE.md) and the [local verification record](RELEASE_VERIFICATION.md). No downloadable GitHub release or registry image is published by the check/package workflows.

## Artifacts and limits

`scripts/package.py` produces source and executable archives for macOS ARM64, Linux ARM64 and Linux x86_64. Each includes source, docs, MIT license, third-party notices, license texts and CycloneDX 1.6 inventories. `BUILD.json` records revision, target, toolchain and lockfile hash; each archive has a SHA-256 file. Helm packages the agent chart separately. Docker archives contain architecture-specific, locally built sidecar images for operator loading; source remains the normal build path until registry publication is configured.

Executables are stripped release builds. They are unsigned; macOS builds are not Developer ID signed/notarized. Linux binaries require glibc 2.34+; musl/Alpine is not a supported binary target. Checksums detect changes but do not authenticate the publisher. Recorded emulated x86_64 testing does not establish bare-metal hardware coverage. No reproducible-build or independent security-certification claim is made.

## Build and package

Use Python 3.11+ and pinned Rust 1.98.1. Complete [TESTING.md](TESTING.md), review [AUDIT.md](AUDIT.md), refresh inventories/scans, then commit the final source. Packaging rejects a dirty tree, unapproved paths and executables with the wrong format/architecture.

```sh
export RUSTFLAGS="--remap-path-prefix=$PWD=/source --remap-path-prefix=$HOME/.cargo=/cargo"
cargo build --locked --release --bin openwebui-acp
python3 scripts/package.py --binary target/release/openwebui-acp --target aarch64-apple-darwin
python3 scripts/package.py --source
helm package deploy/chart --destination dist
```

Use the actual Linux target on a Linux builder (`aarch64-unknown-linux-gnu` or `x86_64-unknown-linux-gnu`). The Dockerfile's adapter build stage also produces the corresponding executable. A local package records the binary supplied; metadata is not a cryptographic build attestation. Verify build output, source revision and file together.

Build each sidecar architecture with versioned tags, scan it, and use `docker save` if handing off an offline image. Include its source revision, local image ID, platform and checksum in the handoff record. These local image IDs are not registry digests. If changing only included notices, rebuild the final image layer and rescan/package that resulting image.

The manual GitHub workflow calls the reusable checks before artifact jobs at the same revision. It builds all binary targets, Linux container archives and chart packages with read-only repository permissions. It does not create a release or push a registry image. Hosted workflows still require execution/verification after public source import.

## Verify a handoff

Run `shasum -a 256 -c ARCHIVE.tar.gz.sha256` on macOS, or `sha256sum -c ARCHIVE.tar.gz.sha256` on Linux, alongside the archive. Extract into a new directory. For binaries, compare the extracted executable with the build output, run `--version` on its target platform and a synthetic ACP check. For source, run the locked build and boundary suite outside any Git worktree. Initial dependency downloads are public; no live Open WebUI credentials are needed.

The export/package tools require Git, but ordinary Rust build/tests and deployment templates work in the extracted source. `BUILD.json` is ignored during fresh Git import. Never include `.env`, private configuration, keys, state, logs, captures, `.work`, target caches or Git history. Review archive member names/content, local links, YAML and Python syntax, and run the secret scanner on the exact export.

## Dependencies and container review

Regenerate `licenses/generated/adapter` with `scripts/dependencies.py`. For the image, check out the Dockerfile's exact Buzz revision and apply **both** `buzz-single-event.patch` and `buzz-persona-yaml.patch`. Run all upstream `buzz-persona` tests, generate its inventory with `--root buzz-acp --root buzz-cli`, and audit each executable root on both Linux architectures using cargo-deny and this repository's policy. Use `RUSTUP_TOOLCHAIN=1.98.1`; the workflow contains complete commands.

The parser patch removes unmaintained unsafe-libyaml from the bundled executable graphs. The old rc.1 crypto lockfile patch is superseded by upstream. Vulnerabilities, unmaintained advisories and yanked crates block the recorded policy; no advisory exceptions are configured. Duplicate-version and unused-license-allowance diagnostics remain informational.

Inventories cover normal/build dependencies across targets, excluding dev-only roots. They are not OS inventories. Scan the final images separately for OS vulnerabilities and retain required notices/covered source. The image includes Tini's notice and the CLI dependency's covered MPL source. Rebuilding an old pin does not refresh its security status; upgrades require new scans and integration checks.

Before public release, enable monitored private reporting, execute hosted CI, rebuild from the public commit and inspect the draft artifacts. Source/binary/chart/container publication, signing and article drafting remain owner-controlled steps. The later oracle/sandbox system is outside this release.
