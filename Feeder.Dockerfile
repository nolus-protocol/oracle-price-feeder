FROM alpine:3.17.0

COPY "./artifacts/feeder" "/service/"

COPY "./market-data-feeder.toml" "/service/"

WORKDIR "/service/"

ENTRYPOINT ["./feeder"]
