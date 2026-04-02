FROM rust:1.82 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

# Install stellar CLI
RUN curl -fsSL https://github.com/stellar/stellar-cli/releases/latest/download/stellar-x86_64-unknown-linux-gnu.tar.gz \
    | tar xz -C /usr/local/bin/

COPY --from=builder /build/target/release/sep-xxxx-relayer /usr/local/bin/sep-xxxx-relayer
EXPOSE 8080
CMD ["sep-xxxx-relayer"]
