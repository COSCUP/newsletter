FROM rust:1.83-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY scripts/ scripts/
COPY migrations/ migrations/

RUN cargo build --release --bin coscup-newsletter

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/coscup-newsletter /app/coscup-newsletter
COPY src/templates/ /app/src/templates/
COPY migrations/ /app/migrations/

EXPOSE 8080

CMD ["/app/coscup-newsletter"]
