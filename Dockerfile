# syntax=docker/dockerfile:1

FROM rust:1.88-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src-rs ./src-rs

RUN cargo build --locked --release --bin ghost-api-server

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    ghostscript \
    mupdf-tools \
    poppler-utils \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ghost-api-server /usr/local/bin/ghost-api-server

EXPOSE 9001

CMD ["/usr/local/bin/ghost-api-server"]
