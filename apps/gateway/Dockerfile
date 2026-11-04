FROM rust:1.82-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY apps/orders/Cargo.toml apps/orders/
COPY apps/gateway/Cargo.toml apps/gateway/
RUN mkdir -p apps/orders/src apps/gateway/src && \
    echo "fn main() {}" > apps/orders/src/main.rs && \
    echo "fn main() {}" > apps/gateway/src/main.rs && \
    cargo build --release --bin gateway 2>/dev/null || true
COPY . .
RUN cargo build --release --bin gateway
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/gateway /usr/local/bin/gateway
EXPOSE 8080
CMD ["gateway"]
