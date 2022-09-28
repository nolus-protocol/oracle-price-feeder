# Market Data Feeder
Market Data feeder is a  off-chain service that collects prices from configured price providers and push them to the Oracle contract
Currently only the Osmosis client is implemented
The Osmosis client reads prices from the Osmosis pools: https://lcd-osmosis.keplr.app/osmosis/gamm/v1beta1/pools


## Prerequisites

To connect to the oracle smart contract, GRPC port on the network should be enabled
To enable it edit ./networks/nolus/local-validator-1/config/app.toml file and change the grpc section to 

```sh
[grpc]
enable = true
address = "0.0.0.0:9090"
```

## Setup

* Add new key to be used as Feeder:

```sh
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
```
export CHAIN_ID="nolus-local"
export TXFLAG="--chain-id ${CHAIN_ID} --gas auto --gas-adjustment 1.3 --fees 15000unls"
```

* Register feeder address. Only oracle contract owner can register new feeder address. All contracts are deployed from wasm_admin
```
WALLET_ADDR=$(nolusd keys show -a wallet)
REGISTER='{"register_feeder":{"feeder_address":"'$WALLET_ADDR'"}}'
nolusd tx wasm execute $CONTRACT "$REGISTER" --amount 100unls --from wasm_admin $TXFLAG -y
```

* Send some money to feeder account
```
nolusd tx bank send $(nolusd keys show -a reserve) $(nolusd keys show -a wallet) 1000000unls --chain-id nolus-local --keyring-backend test --fees 500unls
```

* Build feeder service binary
```
cargo build --release
```

* Configuration
Edit market-data-feeder.toml file 

| Key                 | Value                 | Default     | Description |
|---------------------|-----------------------|-------------|-------------|
|  [`continuous`]     | true or false         | true        | if false the service will push a price only once and exit |
|  [`tick_time`]      | < time in seconds >   | 60          | push price on every X seconds |
|  [`providers`]      |                       |             | List of price providers. A price provider is an off-chain service that provides prices for crypto or non-crypto assets |
|  main_type          | crypto                |             | currently only crypto provider is implemented - Osmosis |
|  name               | osmosis               |             | crypto provider type |
|  base_address       | < URL >               |             | Provider API address |
|  [`oracle`]         |                       |             | Oracle contract configuration |
|  contract_addrs     |  < oracle address >   |             | Oracle contract address |
|  host_url           |  < network node address >   |             | "http://localhost" for local node; https://net-dev.nolus.io for dev network |
|  grpc_port          |                       |             | Grpc port; 26615 for local; 26625 for dev |
|  rpc_port           |                       |             | Rpc port; 26612 for local; 26612 for dev |
|  prefix             | nolus                 |             | Nolus prefix |
|  chain_id           |                       |             | nolus-local for local; nolus-dev-1 for dev |
|  fee_denom          | unls                  |             | Network denom |
|  funds_amount       |                       |             | Amount to be used for transansactions |
|  gas_limit          |                       |             | Gas limit (Example: 500_000)   |



* Start feeder service
```
./target/release/feeder
```
