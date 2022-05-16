# Market Data Feeder

## Prerequisites

To connect to the oracle smart contract, GRPC port on the network should be enabled
To enable it edit ./networks/nolus/local-validator-1/config/app.toml file and change the grpc section to 

```sh
[grpc]
enable = true
address = "0.0.0.0:9090"
```
