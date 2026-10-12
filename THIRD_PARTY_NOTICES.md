# Third-party notices

The project's own code is MIT licensed. Dependency licenses remain in effect;
MIT does not relicense third-party software.

The optional Buzz transport uses Block's official `buzz-sdk` and `buzz-core`,
pinned to commit `f87c10895ea35a9a2cea629fef1d7532a3fecbc5` of
https://github.com/block/buzz. They are Apache-2.0 licensed; the upstream license
is reproduced in `licenses/Buzz-Apache-2.0.txt`. No changes are made to those crates.
The relay examples follow the same upstream project's deployment conventions.

ACP uses the official `agent-client-protocol` Rust SDK. Nostr signing uses the
`nostr` Rust crate. `Cargo.lock` records all dependency versions and sources.
Release packages must include their generated dependency license inventory and
license texts alongside this notice and the project's license.

Open WebUI, Buzz, Zed, and optional container images are independently licensed
projects. They are not relicensed by this repository. The adapter is an independent
integration and does not claim affiliation with those projects.

The `webpki-roots` certificate data is CDLA-Permissive-2.0 licensed. Its required
license text is included in `licenses/webpki-roots-CDLA-Permissive-2.0.txt` and
must accompany binary distributions containing those roots.

The optional sidecar bundles upstream `buzz-acp` and `buzz` executables.
`buzz-acp`'s queue batch size is set to one event by
`deploy/sidecar/buzz-single-event.patch`. The pinned upstream lockfile contains the earlier Rustls/ChaCha20 fixes.
`deploy/sidecar/buzz-persona-yaml.patch` replaces the bundled persona parser's
unmaintained YAML dependency with serde-saphyr 1.3.0 (MIT OR Apache-2.0) and
updates its lockfile. Both patches are included for review. The unmodified `option-ext` 0.2.0
dependency of Buzz's CLI is MPL-2.0 licensed. Its covered source and license are
provided in `licenses/source/option-ext-0.2.0`; the project's MIT license does
not change those terms. The container also includes Tini's copyright/license
notice at `/usr/share/doc/openwebui-acp/tini-copyright`.

`licenses/generated/adapter` and `licenses/generated/buzz` contain the locked
normal/build dependency inventories in CycloneDX 1.6 format and collected
license texts. They include cross-platform build dependencies and may be a
superset of a particular executable's linked dependencies. The source URLs for
license texts absent from published crates are in `licenses/upstream/SOURCES.json`.
