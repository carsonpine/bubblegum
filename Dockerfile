FROM rust:1.93-alpine AS builder

RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    clang \
    llvm-dev \
    clang-libs \
    cmake \
    make \
    g++ \
    pkgconfig

WORKDIR /build

COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -f src/main.rs

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs
RUN cargo build --release

FROM alpine:3.20 AS runtime

RUN apk add --no-cache \
    libssl3 \
    ca-certificates \
    tzdata

WORKDIR /app

COPY --from=builder /build/target/release/bubblegum /app/bubblegum
COPY static /app/static
COPY init.sql /app/init.sql
COPY init_ch.sql /app/init_ch.sql

RUN adduser -D -s /bin/false bubblegum \
    && chown -R bubblegum:bubblegum /app

USER bubblegum

ENV RUST_ENV=production
ENV RUST_LOG=info

EXPOSE 3000

ENTRYPOINT ["/app/bubblegum"]
