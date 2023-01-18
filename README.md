# Market Data Feeder

<br /><p align="center"><img alt="Market Data Feeder" src="docs/price-feeder-logo.svg" width="100"/></p><br />

Market Data feeder is an off-chain service that collects prices from configured price providers and push them to the
Oracle contract
Currently only the Osmosis client is implemented
The Osmosis client reads prices from the Osmosis pools: https://lcd.osmosis.zone/gamm/v1beta1/pools

## Prerequisites

To connect to the oracle smart contract, gRPC port on the network should be enabled
To enable it edit `./networks/nolus/local-validator-1/config/app.toml` file and in the `grpc` section set `enable` to `true`

```shell
[grpc]
...
enable = true
...
```

## Setup

* Add new key to be used as Feeder:

```shell
nolusd keys add wallet

------------ Example Output-----------------
- name: wallet
  type: local
  address: nolus1um993zvsdp8upa5qvtspu0jdy66eahlcghm0w6
  pubkey: '{"@type":"/cosmos.crypto.secp256k1.PubKey","key":"A0MFMuJSqWpofT3GIQchGyL9bADlC5GEWu3QJHGL/XHZ"}'
  mnemonic: "<MNEMONIC PHRASE>"
```

Store the mnemonic phrase as it will be needed to start the service

* Set some environment variables

```shell
export CHAIN_ID="nolus-local"
export TXFLAG="--chain-id ${CHAIN_ID} --gas auto --gas-adjustment 1.3 --fees 15000unls"
```

* Register feeder address. Only oracle contract owner can register new feeder address. All contracts are deployed from
  wasm_admin

```shell
WALLET_ADDR=$(nolusd keys show -a wallet)
REGISTER='{"register_feeder":{"feeder_address":"'$WALLET_ADDR'"}}'
nolusd tx wasm execute $CONTRACT "$REGISTER" --amount 100unls --from wasm_admin $TXFLAG -y
```

* Send some money to feeder account

```shell
nolusd tx bank send $(nolusd keys show -a reserve) $(nolusd keys show -a wallet) 1000000unls --chain-id nolus-local --keyring-backend test --fees 500unls
```

* Build feeder service binary

  ```shell
  cargo build --release
  ```

* Configuration
  At the root of the repository there are two files: `market-data-feeder.main.toml` and `market-data-feeder.test.toml`.
  Depending on whether you want to run the feeder against the main-net or the test-net, rename the corresponding file to `market-data-feeder.toml`.

  Editing the `market-data-feeder.toml` file:

| Key            | Value                    | Default | Description                                                                                                                                       |
|----------------|--------------------------|---------|---------------------------------------------------------------------------------------------------------------------------------------------------|
| [`continuous`] | true or false            | true    | if false the service will push a price only once and exit                                                                                         |
| [`tick_time`]  | < time in seconds >      | 60      | push price on every X seconds                                                                                                                     |
| [`providers`]  |                          |         | List of price providers. A price provider is an off-chain service that provides prices for crypto or non-crypto assets                            |
| main_type      | crypto                   |         | currently only crypto provider is implemented - Osmosis                                                                                           |
| name           | osmosis                  |         | crypto provider type                                                                                                                              |
| base_address   | < URL >                  |         | Provider API address                                                                                                                              |
| [`oracle`]     |                          |         | Oracle contract configuration                                                                                                                     |
| contract_addrs | < oracle address >       |         | Oracle contract address                                                                                                                           |
| host_url       | < network node address > |         | Network address of node. Defaults to "http://localhost" for local node and "http://host.docker.internal" when ran from Docker and uses local node |
| grpc_port      |                          | 26615   | gRPC port. Use port set in configuration of the node under `grpc` section                                                                         |
| rpc_port       |                          | 26612   | JSON-RPC port. Use port set in configuration of the node under `rpc` section                                                                      |
| prefix         | nolus                    |         | Nolus prefix                                                                                                                                      |
| chain_id       |                          |         | The ID of the chain. This property is configured in the node's configuration. E.g.: nolus-local-v1.0                                              |
| fee_denom      | unls                     |         | Network denom                                                                                                                                     |
| funds_amount   |                          |         | Amount to be used for transactions                                                                                                                |
| gas_limit      |                          |         | Gas limit (Example: 500_000)                                                                                                                      |

## Start feeder service

From the same directory where `market-data-feeder.toml` is located

```shell
./target/release/feeder
```

# Diagnostics on release builds

To enable diagnostics by logging debug information, the service needs to be run
with the environment variable `MARKET_DATA_FEEDER_DEBUG` to one of the following:
* `1` (one)
* `y` (lowercase 'y')
* `Y` (uppercase 'y')

# Running in Docker

## Building binary

First you the project has to be compiled.
This has to be done whenever the codebase is changed.

Before running the command, if you are targeting the test-net, then run:

```shell
NET_NAME="test"
```

The command to do so is:

```shell
docker build --rm -f Compile.Dockerfile -t compile-market-data-feeder . && \
  docker run -v $(pwd):/code/ -v $(pwd)/artifacts/:/artifacts/ \
    -v market_data_feeder_cache:/code/target/ -v cargo_cache:/usr/local/cargo/ \
    --rm compile-market-data-feeder --build-arg net_name=${NET_NAME:-main}
```

## Building service's image

Before deploying a new version the service's image has to be rebuilt.

*N.B.: Whenever the configuration file is changed, the image, again,
has to be rebuilt as it's part of the image.*

The command to do so is the following:

```shell
docker build --rm -f Feeder.Dockerfile -t market-data-feeder .
```

## Running service

Running the service is done through the command below, which requires you to
pass it a registered key's mnemonic used.

```shell
echo $MNEMONIC | docker run -i -a stdin --add-host \
  host.docker.internal:host-gateway market-data-feeder
```

**OR**

```shell
cat $MNEMONIC_FILE | docker run -i -a stdin --add-host \
  host.docker.internal:host-gateway market-data-feeder
```
