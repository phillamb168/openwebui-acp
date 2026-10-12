# syntax=docker/dockerfile:1
FROM rust:1.98.1-bookworm@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa AS adapter
WORKDIR /src
ARG CARGO_BUILD_JOBS=4
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples/sidecar_launcher.rs ./examples/sidecar_launcher.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked --mount=type=cache,target=/usr/local/cargo/git,sharing=locked cargo build --locked --release --bin openwebui-acp --example sidecar_launcher

FROM rust:1.98.1-bookworm@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa AS runner
WORKDIR /buzz
ARG CARGO_BUILD_JOBS=4
RUN git init && git remote add origin https://github.com/block/buzz && git fetch --depth 1 origin f87c10895ea35a9a2cea629fef1d7532a3fecbc5 && git checkout --detach FETCH_HEAD
COPY deploy/sidecar/buzz-single-event.patch deploy/sidecar/buzz-persona-yaml.patch /tmp/
RUN git apply --check /tmp/buzz-single-event.patch /tmp/buzz-persona-yaml.patch && git apply /tmp/buzz-single-event.patch /tmp/buzz-persona-yaml.patch
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked --mount=type=cache,target=/usr/local/cargo/git,sharing=locked cargo +1.98.1 build --locked --release -p buzz-acp -p buzz-cli

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime-tools
RUN apt-get update && apt-get install -y --no-install-recommends tini && install -d -o 10001 -g 10001 -m 700 /rootfs/state /rootfs/home/runner

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:e792ab3d241a468a4fd7519ddbbebe66b49b5f365771716ea688ad40b6c6f1c2 AS runtime
COPY --from=runtime-tools /usr/bin/tini /usr/bin/tini
COPY --from=runtime-tools /usr/share/doc/tini/copyright /usr/share/doc/openwebui-acp/tini-copyright
# Copy the parent tree so each private directory is an entry with its own
# metadata. Copying an empty directory directly can leave destination defaults.
COPY --from=runtime-tools /rootfs/ /
COPY --from=adapter /src/target/release/openwebui-acp /usr/local/bin/
COPY --from=adapter /src/target/release/examples/sidecar_launcher /usr/local/bin/sidecar-launcher
COPY --from=runner /buzz/target/release/buzz-acp /buzz/target/release/buzz /usr/local/bin/
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/doc/openwebui-acp/
COPY licenses /usr/share/doc/openwebui-acp/licenses
USER 10001:10001
ENV TOKIO_WORKER_THREADS=2
WORKDIR /home/runner
ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/usr/local/bin/sidecar-launcher"]
