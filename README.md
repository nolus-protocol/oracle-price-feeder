# Market Data Feeder

## Prerequisites

To connect to the oracle smart contract, GRPC port on the network should be enabled
To enable it edit ./networks/nolus/local-validator-1/config/app.toml file and change the grpc section to 

```sh
[grpc]
enable = true
address = "0.0.0.0:9090"
```

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

* Start feeder service
```
./target/release/feeder
```
