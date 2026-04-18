# Phase 2 snarkjs handoff helper.
#
# A pinned-digest image that contributors run to take their turn on a
# circuit-specific Phase 2 round. It ships:
#
#   - ceremony_tool (built from the workspace) — used to export the final
#     Phase 1 SRS into snarkjs-friendly .ptau.
#   - snarkjs, pinned to a specific release.
#   - entrypoint.sh wrapper that exposes init / contribute / verify / beacon
#     / export-vk subcommands and emits an attestation receipt (zkey_hash
#     + snarkjs transcript hash + contributor name) alongside the zkey.
#
# The image is reproducible in the same way as ceremony_tool itself: all
# tool versions are pinned, entrypoint scripts are line-for-line stable,
# and the build is content-hash-addressable via the sha256 of its layers.

# ─── Stage 1: build ceremony_tool from the repo ───────────────────────
FROM rust:1.88.0-alpine3.20 AS rust-builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static
WORKDIR /build

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY crates ./crates

ENV CARGO_TERM_COLOR=never \
    CARGO_NET_RETRY=5 \
    SOURCE_DATE_EPOCH=0 \
    TZ=UTC \
    LC_ALL=C

RUN cargo build \
      --locked \
      --profile release-ceremony \
      --bin ceremony_tool \
    && mv target/release-ceremony/ceremony_tool /out-ceremony_tool

# ─── Stage 2: runtime ─────────────────────────────────────────────────
FROM node:20.19.0-alpine3.20

# Pin snarkjs to a specific release. Update in lockstep with the
# reference hash documented in docs/phase2-mpc-integration.md.
ENV SNARKJS_VERSION=0.7.5

RUN apk add --no-cache bash coreutils ca-certificates tini \
    && npm install -g snarkjs@${SNARKJS_VERSION} \
    && npm cache clean --force

COPY --from=rust-builder /out-ceremony_tool /usr/local/bin/ceremony_tool
RUN chmod +x /usr/local/bin/ceremony_tool

COPY docker/phase2-helper/entrypoint.sh /usr/local/bin/phase2-helper
RUN chmod +x /usr/local/bin/phase2-helper

WORKDIR /work
VOLUME ["/work"]

ENTRYPOINT ["tini", "--", "phase2-helper"]
CMD ["help"]
