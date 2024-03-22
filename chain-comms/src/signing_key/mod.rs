use std::env::{var, VarError};

use cosmrs::{
    bip32::{Language, Mnemonic},
    crypto::secp256k1::SigningKey,
};
use tokio::io::{AsyncBufReadExt, BufReader};

use self::error::{Error, Result};

pub mod error;

pub const DEFAULT_COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

pub async fn signing_key(
    derivation_path: &str,
    password: &str,
) -> Result<SigningKey> {
    let secret: String = match var("SIGNING_KEY_MNEMONIC") {
        Ok(secret) => secret,
        Err(VarError::NotPresent) => {
            println!("Enter dispatcher's account secret: ");

            let mut secret = String::new();

            // Returns number of read bytes, which is meaningless for current case.
            let _ = BufReader::new(tokio::io::stdin())
                .read_line(&mut secret)
                .await?;

            secret
        },
        Err(VarError::NotUnicode(_)) => return Err(Error::NonUnicodeMnemonic),
    };

    SigningKey::derive_from_path(
        Mnemonic::new(secret.trim(), Language::English)
            .map_err(Error::ParsingMnemonic)?
            .to_seed(password),
        &derivation_path
            .parse()
            .map_err(Error::ParsingDerivationPath)?,
    )
    .map_err(Error::DerivingKey)
}
