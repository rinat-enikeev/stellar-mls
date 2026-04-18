# syntax=docker/dockerfile:1

# Stage 1: build both ceremony-coordinator and the shared ceremony_tool binary
# from the same toolchain so the coordinator verifies with the same code
# participants run locally.
FROM rust:1.82 AS builder

WORKDIR /build

# Cache crates.io index and cargo registry across rebuilds.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY ceremony-coordinator ./ceremony-coordinator

# Build ceremony_tool from the workspace root crate with the ceremony profile
RUN cargo build --release --locked --bin ceremony_tool \
 && cp target/release/ceremony_tool /build/ceremony_tool

# Build the coordinator itself.
RUN cd ceremony-coordinator \
 && cargo build --release --locked --bin ceremony-coordinator \
 && cp target/release/ceremony-coordinator /build/ceremony-coordinator

# Stage 2: slim runtime. Contains both binaries so the coordinator shells out
# to the exact same ceremony_tool it ships.
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/ceremony_tool /usr/local/bin/ceremony_tool
COPY --from=builder /build/ceremony-coordinator /usr/local/bin/ceremony-coordinator
COPY ceremony-coordinator/migrations /app/migrations

RUN mkdir -p /app/data /app/work

WORKDIR /app
EXPOSE 9090

CMD ["ceremony-coordinator"]
