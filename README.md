# Market Data Feeder && Alarms Dispatcher

<br /><p align="center"><img alt="Market Data Feeder" src="docs/price-feeder-logo.svg" width="100"/></p><br />

**Market Data Feeder** is an off-chain service that collects prices from configured
price providers and pushes them to the Oracle contract.

Currently only the Osmosis client is implemented.

It reads prices from the Osmosis pools: `https://lcd.osmosis.zone/gamm/v1beta1/pools`

**Alarms Dispatcher** is а service that takes care of alarms in the system.

## Prerequisites

To connect to the oracle smart contract, the gRPC port on the network should be
enabled.

To enable it edit the following file:

`./nolus-core/networks/nolus/local-validator-1/config/app.toml`

In it, go to the `grpc` section and set `enable` to `true`.

```shell
[grpc]
...
enable = true
...
```

## Setup

### Set some environment variables

   ```shell
   export CHAIN_ID="nolus-Env-vXXX-XXX"
   export TXFLAG="--chain-id ${CHAIN_ID} --gas auto --gas-adjustment 1.3 --fees 15000unls"
   ```

   *Note: The CHAIN_ID can be found in the produced after network initiliazation 'networks' directory (`./nolus-core/networks/nolus/local-validator-1/genesis.json`).

### Add new key to be used as Feeder

   ```shell
   nolusd keys add feeder
   ```

   The output will look like this:

   ```yaml
   - name: feeder
     type: local
     address: nolus1um993zvsdp8upa5qvtspu0jdy66eahlcghm0w6
     pubkey: '{"@type":"/cosmos.crypto.secp256k1.PubKey","key":"A0MFMuJSqWpofT3GIQchGyL9bADlC5GEWu3QJHGL/XHZ"}'
     mnemonic: "<MNEMONIC PHRASE>"
   ```

   Store the mnemonic phrase as it will be needed to start the service.

### Add new key to be used as Dispatcher

  ```shell
  nolusd keys add dispatcher
  ```

  Store the mnemonic phrase as it will be needed to start the service.

### Register feeder address

  ```shell
   nolusd tx gov submit-proposal sudo-contract nolus1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsv3wzl4 \
   '{"register_feeder":{"feeder_address":"nolus1um993zvsdp8upa5qvtspu0jdy66eahlcghm0w6"}}' \
   --title "Register feeder" --description "Register feeder" --deposit 10000000unls --fees 900unls --gas auto --gas-adjustment 1.1 --from reserve
  ```

* **nolus1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsv3wzl4** - Leaser contract
* **nolus1um993zvsdp8upa5qvtspu0jdy66eahlcghm0w6** - Feeder address

### Send some money to feeder account

  ```shell
  nolusd tx bank send reserve $(nolusd keys show -a feeder) 1000000unls --fees 500unls
  ```

### Send some money to dispatcher account

  ```shell
  nolusd tx bank send reserve $(nolusd keys show -a dispatcher) 1000000unls --fees 500unls
  ```

### Build services

  ```shell
  cargo build --release
  ```

### Configure service

  At the root of the repository there is a directory called `configurations`.

  In there are several files: `market-data-feeder.main.toml`,
  `market-data-feeder.test.toml`, `market-data-feeder.dev.toml`, `alarms-dispatcher.main.toml` ...

  Depending on whether you want to run the feeder on the main-net, dev-net or the
  test-net, rename the corresponding file to `market-data-feeder.toml`/`alarms-dispatcher.toml`.

* Editing the `market-data-feeder.toml` file:

  * When running through `bash` - replace all instances of entries containing a dash, e.g. `osmosis-lcd`, with their counterpart that uses an underscore, e.g. `osmosis_lcd`.
  * When desired to run without a sanity check - remove the [providers.osmosis_lcd.comparison], [comparison_providers.sanity_check] and [comparison_providers.sanity_check.ticker_mapping] sections from the configuration file.

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

* Editing the `alarms-dispatcher.toml` file:
  * chain_id - The ID of the chain. This property is configured in the node's configuration. E.g.: nolus-local-v1.0

### Environment variables configuration

   There are also environment variables which are used for configuring the services.

   They are as follows:

* For feeder & dispatcher:
  * `DEBUG_LOGGING`
    Turns on debug logging when running a release build.
    Possible values:
    * 1
    * y
    * Y

  * `JSON_RPC_URL`
    JSON-RPC endpoint's URL.

  * `GRPC_URL`
    gRPC endpoint's URL.

* For feeder:
  * `PROVIDER_OSMOSIS_LCD_RPC_URL`
    Osmosis' GAMM module API endpoint's URL.

  * `PROVIDER_OSMOSIS_LCD_SECONDS_BEFORE_FEEDING`

  * `PROVIDER_OSMOSIS_LCD_MAX_DEVIATION`

  * `SIGNING_KEY_MNEMONIC`

  * `COMPARISON_PROVIDER_SANITY_CHECK_API_KEY`

On local network:

Feeder:

```shell
export DEBUG_LOGGING=1 ; export JSON_RPC_URL="http://localhost:26612" ; export GRPC_URL="http://localhost:26615" ; export PROVIDER_OSMOSIS_LCD_SECONDS_BEFORE_FEEDING=0 ; export PROVIDER_OSMOSIS_LCD_MAX_DEVIATION=1000 ; export PROVIDER_OSMOSIS_LCD_RPC_URL="https://lcd.osmotest5.osmosis.zone/osmosis/poolmanager/" ;
```

Dispatcher:

```shell
export DEBUG_LOGGING=1 ; export JSON_RPC_URL="http://localhost:26612" ; export GRPC_URL="http://localhost:26615" ;
```

### Start feeder service

From the same directory where `market-data-feeder.toml` is located:

```shell
./target/release/feeder
```

### Start dispatcher service

From the same directory where `alarms-dispatcher.toml` is located:

```shell
./target/release/alarms-dispatcher
```

## Running in Docker

### Building binary

First the project has to be compiled.
This has to be done whenever the codebase is changed.

The command to do so is:

```shell
docker build --rm -f Compile.Dockerfile -t compile-bots . && \
  docker run --rm -v "$(pwd):/code/" -v "$(pwd)/artifacts/:/artifacts/" \
    -v market_data_feeder_cache:/code/target/ -v cargo_cache:/usr/local/cargo/ \
    compile-bots
```

### Building service's image

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

### Running service

Running the service is done through the command below, which requires you to
pass the mnemonic of the key that will be used.

*Note: Host addresses, ports and other configurations might change
over time. These are provided as a guide.*

* Feeder - one of the following options:

  * ```shell
      echo $MNEMONIC | docker run -i -a stdin --add-host \
      --env 'GRPC_URL=https://rila-cl.nolus.network:9090' \
      --env 'JSON_RPC_URL=https://rila-cl.nolus.network:26657' \
      --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-test-cl.nolus.network:1317/osmosis/gamm/v1beta1/'
      host.docker.internal:host-gateway market-data-feeder
    ```

  * ```shell
      cat $MNEMONIC_FILE | docker run -i -a stdin --add-host \
      --env 'GRPC_URL=https://rila-cl.nolus.network:9090' \
      --env 'JSON_RPC_URL=https://rila-cl.nolus.network:26657' \
      --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-test-cl.nolus.network:1317/osmosis/gamm/v1beta1/'
      host.docker.internal:host-gateway market-data-feeder
    ```

  * ```shell
      docker run -i -a stdin --add-host --env "SIGNING_KEY_MNEMONIC=$MNEMONIC" \
      --env 'GRPC_URL=https://rila-cl.nolus.network:9090' \
      --env 'JSON_RPC_URL=https://rila-cl.nolus.network:26657' \
      --env 'PROVIDER_OSMOSIS_BASE_ADDRESS=https://osmo-test-cl.nolus.network:1317/osmosis/gamm/v1beta1/'
      host.docker.internal:host-gateway market-data-feeder
    ```

* Dispatcher - one of the following options:

  * ```shell
      echo $MNEMONIC | docker run -i -a stdin --add-host \
      --env 'GRPC_URL=https://rila-cl.nolus.network:9090' \
      --env 'JSON_RPC_URL=https://rila-cl.nolus.network:26657' \
      host.docker.internal:host-gateway alarms-dispatcher
    ```

  * ```shell
      cat $MNEMONIC_FILE | docker run -i -a stdin --add-host \
      --env 'GRPC_URL=https://rila-cl.nolus.network:9090' \
      --env 'JSON_RPC_URL=https://rila-cl.nolus.network:26657' \
      host.docker.internal:host-gateway alarms-dispatcher
    ```

  * ```shell
      docker run -i -a stdin --add-host --env "SIGNING_KEY_MNEMONIC=$MNEMONIC" \
      --env 'GRPC_URL=https://rila-net.nolus.io:1318' \
      --env 'JSON_RPC_URL=https://rila-net.nolus.io:26657' \
      host.docker.internal:host-gateway alarms-dispatcher
    ```
