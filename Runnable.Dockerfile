FROM alpine:3.16.2

COPY "./artifacts/" "/service/"

COPY "./market-data-feeder.toml" "/service/"

WORKDIR "/service/"

ENTRYPOINT ["./feeder"]
