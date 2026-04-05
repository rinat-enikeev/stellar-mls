FROM rust:1.82 AS builder
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/pn-relay /usr/local/bin/pn-relay
EXPOSE 8090
CMD ["pn-relay"]
