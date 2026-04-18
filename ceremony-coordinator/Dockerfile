# syntax=docker/dockerfile:1

# Stage 1: build both ceremony-coordinator and the shared ceremony_tool binary
# from the same toolchain so the coordinator verifies with the same code
# participants run locally. The base image must match `rust-toolchain.toml`
# at the repo root — recent transitive deps (base64ct ≥1.8) require
# edition2024, which only landed in Rust 1.85+.
FROM rust:1.88 AS builder

WORKDIR /build

# Cache crates.io index and cargo registry across rebuilds. rust-toolchain.toml
# is copied in so rustup honors the workspace's pinned channel even when the
# base image is a different minor version.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY ceremony-coordinator ./ceremony-coordinator

# Stage artifacts under /out so the final-stage COPY never collides with
# source directories that happen to share a binary name (notably
# /build/ceremony-coordinator, which is the coordinator's source tree).
RUN mkdir -p /out

# Build ceremony_tool from the workspace root crate with the ceremony profile
RUN cargo build --release --locked --bin ceremony_tool \
 && cp target/release/ceremony_tool /out/ceremony_tool

# Build the coordinator itself.
RUN cd ceremony-coordinator \
 && cargo build --release --locked --bin ceremony-coordinator \
 && cp target/release/ceremony-coordinator /out/ceremony-coordinator

# Stage 2: slim runtime. Contains both binaries so the coordinator shells out
# to the exact same ceremony_tool it ships.
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/ceremony_tool /usr/local/bin/ceremony_tool
COPY --from=builder /out/ceremony-coordinator /usr/local/bin/ceremony-coordinator
COPY ceremony-coordinator/migrations /app/migrations

RUN mkdir -p /app/data /app/work

WORKDIR /app
EXPOSE 9090

CMD ["ceremony-coordinator"]
