FROM rust:1.93-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libclang-dev \
    clang \
    cmake \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -f src/main.rs

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs
RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/bubblegum /app/bubblegum
COPY static /app/static
COPY init.sql /app/init.sql
COPY init_ch.sql /app/init_ch.sql

RUN useradd -r -s /bin/false bubblegum \
    && chown -R bubblegum:bubblegum /app

USER bubblegum

ENV RUST_ENV=production
ENV RUST_LOG=info

EXPOSE 3000

ENTRYPOINT ["/app/bubblegum"]
