FROM alpine:3.17.2

ARG config_name="main"

COPY "./feeder" "/service/"

COPY "./market-data-feeder.${config_name}.toml" "/service/market-data-feeder.toml"

WORKDIR "/service/"

ENTRYPOINT ["./feeder"]
