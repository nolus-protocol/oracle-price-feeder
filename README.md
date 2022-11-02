# Market Data Feeder

Market Data feeder is an off-chain service that collects prices from configured price providers and push them to the
Oracle contract
Currently only the Osmosis client is implemented
The Osmosis client reads prices from the Osmosis pools: https://lcd.osmosis.zone/gamm/v1beta1/pools

## Prerequisites

To connect to the oracle smart contract, GRPC port on the network should be enabled
To enable it edit `./networks/nolus/local-validator-1/config/app.toml` file and change the grpc section to

```shell
[grpc]
enable = true
address = "0.0.0.0:9090"
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
  Edit `market-data-feeder.toml` file

| Key            | Value                    | Default | Description                                                                                                            |
|----------------|--------------------------|---------|------------------------------------------------------------------------------------------------------------------------|
| [`continuous`] | true or false            | true    | if false the service will push a price only once and exit                                                              |
| [`tick_time`]  | < time in seconds >      | 60      | push price on every X seconds                                                                                          |
| [`providers`]  |                          |         | List of price providers. A price provider is an off-chain service that provides prices for crypto or non-crypto assets |
| main_type      | crypto                   |         | currently only crypto provider is implemented - Osmosis                                                                |
| name           | osmosis                  |         | crypto provider type                                                                                                   |
| base_address   | < URL >                  |         | Provider API address                                                                                                   |
| [`oracle`]     |                          |         | Oracle contract configuration                                                                                          |
| contract_addrs | < oracle address >       |         | Oracle contract address                                                                                                |
| host_url       | < network node address > |         | "http://localhost" for local node; https://net-dev.nolus.io for dev network                                            |
| grpc_port      |                          |         | Grpc port; 26615 for local; 26625 for dev                                                                              |
| rpc_port       |                          |         | Rpc port; 26612 for local; 26612 for dev                                                                               |
| prefix         | nolus                    |         | Nolus prefix                                                                                                           |
| chain_id       |                          |         | nolus-local for local; nolus-dev-1 for dev                                                                             |
| fee_denom      | unls                     |         | Network denom                                                                                                          |
| funds_amount   |                          |         | Amount to be used for transactions                                                                                     |
| gas_limit      |                          |         | Gas limit (Example: 500_000)                                                                                           |

## Start feeder service

From the same directory where `market-data-feeder.toml` is located

```shell
./target/release/feeder
```

# Running in Docker

## Building binary

First you the project has to be compiled.
This has to be done whenever the codebase is changed.

The command to do so is:

```shell
docker build --rm -f compiling_docker -t compile-market-data-feeder . && \
  docker run -v $(pwd):/code/ -v $(pwd)/artifacts/:/artifacts/ \
    -v market_data_feeder_cache:/code/target/ -v cargo_cache:/usr/local/cargo/ \
    --rm compile-market-data-feeder
```

## Building service's image

Before deploying a new version the service's image has to be rebuilt.

*N.B.: Whenever the configuration file is changed, the image, again,
has to be rebuilt as it's part of the image.*

The command to do so is the following:

```shell
docker build --rm -f runnable_docker -t market-data-feeder .
```

## Running service

Running the service is done through the command below, which requires you to
pass it a registered key's mnemonic used.

```shell
echo $MNEMONIC | docker run -i -a stdin market-data-feeder
```

**OR**

```shell
cat $MNEMONIC_FILE | docker run -i -a stdin market-data-feeder
```
