# syntax=docker/dockerfile:1.4
# Crate is edition 2024 (see Cargo.toml) and deps like `chacha20 0.10` are edition-2024
# crates, so Rust >= 1.85 is required. Pinned for reproducible CI: the floating
# `rust:1`/`rust:buster` tags drift and introduce new hard errors/lints over time.
# Debian 12 base (libssl3) matches the runtime stage below. Bump deliberately after verifying.
FROM rust:1.96-bookworm AS builder

# create a new empty shell project
RUN USER=root cargo new --bin code
WORKDIR /code

# copy manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# cache dependencies
RUN cargo build --release
RUN rm src/*.rs

# copy your source tree
COPY ./src ./src
# sqlx::migrate!("./migrations") reads this dir at COMPILE time and embeds the SQL
# into the binary, so it must be present for the release build below.
COPY ./migrations ./migrations

# build for release
RUN rm ./target/release/deps/fourinarow_server*
RUN cargo build --release

# Run the binary
FROM debian:bookworm-slim

# native-tls / openssl-sys are linked in (sqlx runtime-tokio-native-tls + reqwest),
# so the binary needs libssl at runtime and ca-certificates for outbound HTTPS.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

EXPOSE 7060
ENV BIND=0.0.0.0:7060

COPY --from=builder /code/target/release/fourinarow-server /fourinarow-server
COPY ./static /static
COPY ./config /config
COPY .env /.env

CMD [ "/fourinarow-server" ]