FROM alpine:3.17.2

ARG net_name="main"

COPY "./feeder" "/service/"

COPY "./market-data-feeder.${net_name}.toml" "/service/market-data-feeder.toml"

WORKDIR "/service/"

ENTRYPOINT ["./feeder"]
