FROM alpine:3.17.2

ARG config_name="main"

COPY "./alarms-dispatcher" "/service/"

COPY "./alarms-dispatcher.${config_name}.toml" "/service/alarms-dispatcher.toml"

WORKDIR "/service/"

ENTRYPOINT ["./alarms-dispatcher"]
