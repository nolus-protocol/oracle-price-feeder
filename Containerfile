FROM docker.io/library/debian:bookworm-slim as code

COPY "./" "/code/"

RUN ["rm", "-r", "-f", "/code/configurations/"]

FROM docker.io/library/rust:latest as compile

RUN ["apt-get", "update"]

RUN ["apt-get", "upgrade", "--purge", "--yes"]

RUN ["apt-get", "install", "libc6-dev"]

USER "1000":"1000"

ENV CARGO_INCREMENTAL="0"

COPY --from=code --chown="1000:1000" --chmod="0755" "/code/" "/code/"

ARG package

WORKDIR "/code/"${package}

RUN ["cargo", "rustc", "--release", "--target", "x86_64-unknown-linux-gnu", \
    "--", "-C", "target-feature=+crt-static"]

FROM gcr.io/distroless/static:latest AS service

VOLUME "/service/logs/"

WORKDIR "/service/"

ARG package
ARG configuration

COPY --from=compile --chown="0:0" --chmod="0555" \
    "/code/target/x86_64-unknown-linux-gnu/release/"${package} "./service"

COPY --chown="0:0" --chmod="0444" \
    "./configurations/"${package}"."${configuration}".toml" \
    "./"${package}".toml"

ENTRYPOINT ["./service"]
