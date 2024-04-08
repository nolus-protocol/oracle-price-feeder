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

ENV ADDRESS_PREFIX="nolus"
ENV BETWEEN_TX_MARGIN_SECONDS="2"
ENV DEBUG_LOGGING="0"
ENV FEE_DENOM="unls"
ENV FEE_ADJUSTMENT_NUMERATOR="3"
ENV FEE_ADJUSTMENT_DENOMINATOR="1"
ENV GAS_ADJUSTMENT_NUMERATOR="1075"
ENV GAS_ADJUSTMENT_DENOMINATOR="1000"
ENV GAS_PRICE_NUMERATOR="1"
ENV GAS_PRICE_DENOMINATOR="400"
ENV GRPC_URI="###"
ENV POLL_TIME_SECONDS="15"
ENV TICK_TIME_SECONDS="60"
ENV SIGNING_KEY_MNEMONIC="###"

ENTRYPOINT ["./service"]

ARG package

COPY --from=compile --chown="0:0" --chmod="0111" \
    "/code/target/x86_64-unknown-linux-gnu/release/"${package} "./service"

FROM service AS alarms-dispatcher

ENV ADMIN_CONTRACT="###"
ENV PRICE_ALARMS_GAS_LIMIT_PER_ALARM="500000"
ENV PRICE_ALARMS_MAX_ALARMS_GROUP="32"
ENV TIME_ALARMS_GAS_LIMIT_PER_ALARM="500000"
ENV TIME_ALARMS_MAX_ALARMS_GROUP="32"

FROM service AS market-data-feeder

ENV SECONDS_BEFORE_FEEDING="###"

ARG configuration

COPY --chown="0:0" --chmod="0444" \
    "./configurations/"${package}"."${configuration}".toml" \
    "./"${package}".toml"
