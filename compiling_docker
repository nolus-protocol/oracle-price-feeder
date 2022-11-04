FROM rust:1.65.0-alpine

VOLUME ["/artifacts", "/code", "/code/target", "/usr/local/cargo"]

RUN ["apk", "add", "musl-dev"]

WORKDIR "/code"

ENTRYPOINT ["sh", "-c", "cargo build --release --target x86_64-unknown-linux-musl && cp /code/target/x86_64-unknown-linux-musl/release/feeder /artifacts"]
