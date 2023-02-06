FROM alpine:3.17.0

ARG net_name="main"

COPY "./artifacts/alarms-dispatcher" "/service/"

COPY "./alarms-dispatcher.${net_name}.toml" "/service/alarms-dispatcher.toml"

WORKDIR "/service/"

ENTRYPOINT ["./alarms-dispatcher"]
