# Market Data Feeder

<br /><p align="center"><img alt="Market Data Feeder" src="docs/price-feeder-logo.svg" width="100"/></p><br />

Market Data feeder is an off-chain service that collects prices from configured
price providers and pushes them to the Oracle contract.

Currently only the Osmosis client is implemented.
<br />
It reads prices from the Osmosis pools: https://lcd.osmosis.zone/gamm/v1beta1/pools

## Prerequisites

To connect to the oracle smart contract, the gRPC port on the network should be
enabled.
<br />
To enable it edit the following file:
<br />
`./networks/nolus/local-validator-1/config/app.toml`

In it, go to the `grpc` section and set `enable` to `true`.

```shell
[grpc]
...
enable = true
...
```

## Setup

1. Add new key to be used as Feeder:

   ```shell
   nolusd keys add wallet
   ```

   The output will look like this:
   ```yaml
   - name: wallet
     type: local
     address: nolus1um993zvsdp8upa5qvtspu0jdy66eahlcghm0w6
     pubkey: '{"@type":"/cosmos.crypto.secp256k1.PubKey","key":"A0MFMuJSqWpofT3GIQchGyL9bADlC5GEWu3QJHGL/XHZ"}'
     mnemonic: "<MNEMONIC PHRASE>"
   ```

   Store the mnemonic phrase as it will be needed to start the service.

2. Set some environment variables

   ```shell
   export CHAIN_ID="nolus-local"
   export TXFLAG="--chain-id ${CHAIN_ID} --gas auto --gas-adjustment 1.3 --fees 15000unls"
   ```

3. Register feeder address. Only oracle contract owner can register new feeder
   address. All contracts are deployed from wasm_admin.

   ```shell
   WALLET_ADDR=$(nolusd keys show -a wallet)
   REGISTER='{"register_feeder":{"feeder_address":"'"$WALLET_ADDR"'"}}'
   nolusd tx wasm execute $CONTRACT "$REGISTER" --amount 100unls \
       --from wasm_admin $TXFLAG -y
   ```

4. Send some money to feeder account

   ```shell
   nolusd tx bank send $(nolusd keys show -a reserve) \
       $(nolusd keys show -a wallet) 1000000unls --chain-id nolus-local \
       --keyring-backend test --fees 500unls
   ```

5. Build feeder service binary

    ```shell
    cargo build --release
    ```

6. Configure service

   At the root of the repository there is a directory called `configurations`.
   <br />
   In there are two files: `market-data-feeder.main.toml` and
   `market-data-feeder.test.toml`.
   <br />
   Depending on whether you want to run the feeder on the main-net or the
   test-net, rename the corresponding file to `market-data-feeder.toml`.
   <br />
   Editing the `market-data-feeder.toml` file:

   |      Key       |            Value             | Default | Description                                                                                                                                       |
      |:--------------:|:----------------------------:|:-------:|:--------------------------------------------------------------------------------------------------------------------------------------------------|
   | [`continuous`] |      `true` or `false`       |  true   | if false the service will push a price only once and exit                                                                                         |
   | [`tick_time`]  |   &lt;time in seconds&gt;    |   60    | push price on every X seconds                                                                                                                     |
   | [`providers`]  |                              |         | List of price providers. A price provider is an off-chain service that provides prices for crypto or non-crypto assets                            |
   |   main_type    |            crypto            |         | currently, the only crypto provider that is implemented - Osmosis                                                                                 |
   |      name      |           osmosis            |         | crypto provider type                                                                                                                              |
   |   [`oracle`]   |                              |         | Oracle contract configuration                                                                                                                     |
   | contract_addrs |    &lt;oracle address&gt;    |         | Oracle contract address                                                                                                                           |
   |     prefix     |            nolus             |         | Nolus prefix                                                                                                                                      |
   |    chain_id    |                              |         | The ID of the chain. This property is configured in the node's configuration. E.g.: nolus-local-v1.0                                              |
   |   fee_denom    |             unls             |         | Network denom                                                                                                                                     |
   |  funds_amount  |                              |         | Amount to be used for transactions                                                                                                                |
   |   gas_limit    |                              |         | Gas limit (Example: 500_000)                                                                                                                      |

## Start feeder service

From the same directory where `market-data-feeder.toml` is located

```shell
./target/release/feeder
```

# Diagnostics on release builds

To enable diagnostics by logging debug information, the service needs to be run
with the environment variable `DEBUG_LOGGING` to one of the following:

* `1` (one)
* `y` (lowercase 'y')
* `Y` (uppercase 'y')

# Running in Docker

## Building binary

First the project has to be compiled.
This has to be done whenever the codebase is changed.

The command to do so is:

```shell
docker build --rm -f Compile.Dockerfile -t compile-bots . && \
  docker run --rm -v "$(pwd):/code/" -v "$(pwd)/artifacts/:/artifacts/" \
    -v market_data_feeder_cache:/code/target/ -v cargo_cache:/usr/local/cargo/ \
    compile-bots
```

## Building service's image

Before deploying a new version the service's image has to be rebuilt.

*N.B.: Whenever the configuration file is changed, the image has to be rebuilt
as it's part of the image.*

Before running the command, if you are targeting the test-net, then run:

```shell
CONFIG_NAME="test"
```

The command to do so is the following:

* Feeder
  ```shell
  docker build --rm --build-arg config_name=${CONFIG_NAME:-main} \
    -f Feeder.Dockerfile -t market-data-feeder ./artifacts/
  ```

* Dispatcher
  ```shell
  docker build --rm --build-arg config_name=${CONFIG_NAME:-main} \
    -f Dispatcher.Dockerfile -t alarms-dispatcher ./artifacts/
  ```

## Running service

Running the service is done through the command below, which requires you to
pass the mnemonic of the key that will be used.

*Note: Host addresses, ports and other configurations might change
over time. These are provided as a guide.*

* Feeder - one of the following options:
    * ```shell
      echo $MNEMONIC | docker run -i -a stdin --add-host \
      --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
      --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
      --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
      --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-net.nolus.io:1317/osmosis/gamm/v1beta1/'
      host.docker.internal:host-gateway market-data-feeder
      ```

    * ```shell
      cat $MNEMONIC_FILE | docker run -i -a stdin --add-host \
      --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
      --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
      --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
      --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-net.nolus.io:1317/osmosis/gamm/v1beta1/'
      host.docker.internal:host-gateway market-data-feeder
      ```

    * ```shell
      docker run -i -a stdin --add-host --env "SIGNING_KEY_MNEMONIC=$MNEMONIC" \
        --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
        --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
        --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
        --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-net.nolus.io:1317/osmosis/gamm/v1beta1/'
        host.docker.internal:host-gateway market-data-feeder
      ```

* Dispatcher - one of the following options:
    * ```shell
      echo $MNEMONIC | docker run -i -a stdin --add-host \
        --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
        --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
        --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
        host.docker.internal:host-gateway alarms-dispatcher
      ```

    * ```shell
      cat $MNEMONIC_FILE | docker run -i -a stdin --add-host \
        --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
        --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
        --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
        host.docker.internal:host-gateway alarms-dispatcher
      ```

    * ```shell
      docker run -i -a stdin --add-host --env "SIGNING_KEY_MNEMONIC=$MNEMONIC" \
        --env 'GRPC_HOST=rila-net.nolus.io' --env 'GRPC_PORT=1318' \
        --env 'GRPC_PROTO=https' --env 'JSON_RPC_HOST=rila-net.nolus.io' \
        --env 'JSON_RPC_PORT=26657' --env 'JSON_RPC_PROTO=https' \
        host.docker.internal:host-gateway alarms-dispatcher
      ```
