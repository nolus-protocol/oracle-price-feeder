FROM alpine:3.17.0

ARG net_name="main"

COPY "./artifacts/feeder" "/service/"

COPY "./market-data-feeder.${net_name}.toml" "/service/market-data-feeder.toml"

WORKDIR "/service/"

ENTRYPOINT ["./feeder"]
