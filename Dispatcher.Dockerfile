FROM alpine:3.17.0

COPY "./artifacts/alarms-dispatcher" "/service/"

COPY "./alarms-dispatcher.toml" "/service/"

WORKDIR "/service/"

ENTRYPOINT ["./alarms-dispatcher"]
