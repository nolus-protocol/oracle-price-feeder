ARG package

FROM docker.io/library/rust:latest AS compiled-base

ENV CARGO_INCREMENTAL="0"

RUN ["apt-get", "update"]

RUN ["apt-get", "upgrade", "--purge", "--yes"]

RUN ["apt-get", "install", "--yes", "libc6-dev"]

WORKDIR "/code/"

FROM scratch AS service-base

VOLUME ["/service/logs/"]

WORKDIR "/service/"

ENTRYPOINT ["/service/service"]

ENV ADMIN_CONTRACT_ADDRESS="###"
ENV BALANCE_REPORTER_IDLE_DURATION_SECONDS="600"
ENV BROADCAST_DELAY_DURATION_SECONDS="2"
ENV BROADCAST_RETRY_DELAY_DURATION_MILLISECONDS="500"
ENV FEE_TOKEN_DENOM="unls"
ENV GAS_FEE_CONF__GAS_ADJUSTMENT_NUMERATOR="12"
ENV GAS_FEE_CONF__GAS_ADJUSTMENT_DENOMINATOR="10"
ENV GAS_FEE_CONF__GAS_PRICE_NUMERATOR="1"
ENV GAS_FEE_CONF__GAS_PRICE_DENOMINATOR="400"
ENV GAS_FEE_CONF__FEE_ADJUSTMENT_NUMERATOR="5"
ENV GAS_FEE_CONF__FEE_ADJUSTMENT_DENOMINATOR="1"
ENV IDLE_DURATION_SECONDS="60"
ENV LOGS_DIRECTORY="/service/logs/"
ENV NODE_GRPC_URI="###"
ENV OUTPUT_JSON="0"
ENV SIGNING_KEY_MNEMONIC="###"
ENV TIMEOUT_DURATION_SECONDS="60"

FROM service-base AS alarms-dispatcher-base

ENV PRICE_ALARMS_GAS_LIMIT_PER_ALARM="500000"
ENV PRICE_ALARMS_MAX_ALARMS_GROUP="32"
ENV TIME_ALARMS_GAS_LIMIT_PER_ALARM="500000"
ENV TIME_ALARMS_MAX_ALARMS_GROUP="32"

FROM service-base AS market-data-feeder-base

ENV DURATION_SECONDS_BEFORE_START="600"
ENV GAS_LIMIT="###"
ENV UPDATE_CURRENCIES_INTERVAL_SECONDS="15"

FROM compiled-base AS compiled

ARG package

LABEL "package"="${package:?}"

ARG profile

LABEL "profile"="${profile:?}"

COPY --chown="0":"0" --chmod="0555" "." "/code/"

RUN --mount=type=cache,target="/usr/local/cargo/registry" \
    --mount=type=cache,target="/build-output-cached/" \
    "cargo" \
    "rustc" \
    "--bin" "${package:?}" \
    "--locked" \
    "--manifest-path" "/code/applications/${package:?}/Cargo.toml" \
    "--package" "${package:?}" \
    "--profile" "${profile:?}" \
    "--target" "x86_64-unknown-linux-gnu" \
    "--target-dir" "/build-output-cached/" \
    "--" \
    "-C" "target-feature=+crt-static"

RUN --mount=type=cache,target="/build-output-cached/" \
    [ \
      "cp", \
        "-R", \
        "/build-output-cached/", \
        "/build-output/" \
    ]


FROM ${package}-base AS service

ARG package

LABEL "package"="${package:?}"

ARG profile

LABEL "profile"="${profile:?}"

ARG profile_output_dir

COPY --from=compiled --chown="0":"0" --chmod="0100" \
    "/build-output/x86_64-unknown-linux-gnu/${profile_output_dir:?}/${package:?}" \
    "/service/service"
