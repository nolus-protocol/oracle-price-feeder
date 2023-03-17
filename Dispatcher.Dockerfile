FROM alpine:3.17.2

ARG net_name="main"

COPY "./alarms-dispatcher" "/service/"

COPY "./alarms-dispatcher.${net_name}.toml" "/service/alarms-dispatcher.toml"

WORKDIR "/service/"

ENTRYPOINT ["./alarms-dispatcher"]
