# syntax=docker/dockerfile:1.4
FROM rust:buster AS builder

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

# build for release
RUN rm ./target/release/deps/fourinarow_server*
RUN cargo build --release

# Run the binary
FROM debian:bullseye-slim

EXPOSE 7060
ENV BIND=0.0.0.0:7060

COPY --from=builder /code/target/release/fourinarow-server /fourinarow-server
COPY ./static /static
COPY ./config /config
COPY .env /.env

CMD [ "/fourinarow-server" ]