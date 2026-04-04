FROM rust:1.82 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl libdbus-1-3 && rm -rf /var/lib/apt/lists/*

# Install stellar CLI
RUN STELLAR_VERSION=$(curl -s https://api.github.com/repos/stellar/stellar-cli/releases/latest | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/') && \
    curl -fsSL "https://github.com/stellar/stellar-cli/releases/download/v${STELLAR_VERSION}/stellar-cli-${STELLAR_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
    | tar xz -C /usr/local/bin/

COPY --from=builder /build/target/release/sep-xxxx-relayer /usr/local/bin/sep-xxxx-relayer
EXPOSE 8080
CMD ["sep-xxxx-relayer"]
