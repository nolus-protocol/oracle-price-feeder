FROM docker.io/library/rust:latest as compile

RUN ["apt-get", "update"]

RUN ["apt-get", "upgrade", "--purge", "--yes"]

RUN ["apt-get", "install", "libc6-dev"]

COPY --chown="1000:1000" --chmod="0755" "./" "/code/"

ARG package

WORKDIR "/code/"${package}

USER "1000":"1000"

RUN ["cargo", "rustc", "--release", "--target", "x86_64-unknown-linux-gnu", \
    "--", "-C", "target-feature=+crt-static"]

FROM gcr.io/distroless/static:nonroot AS service

ARG package
ARG configuration

VOLUME "/service/logs/"

WORKDIR "/service/"

COPY --from=compile --chown="0:0" --chmod="0555" \
    "/code/target/x86_64-unknown-linux-gnu/release/"${package} "./service"

COPY --chown="0:0" --chmod="0444" \
    "./configurations/"${package}"."${configuration}".toml" \
    "./"${package}".toml"

ENTRYPOINT ["./service"]
