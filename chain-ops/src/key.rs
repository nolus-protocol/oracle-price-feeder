use anyhow::{Context as _, Result};
use bip32::{Language, Mnemonic};

pub type Signing = cosmrs::crypto::secp256k1::SigningKey;

pub type Public = cosmrs::crypto::PublicKey;

pub fn derive_from_mnemonic(phrase: &str, password: &str) -> Result<Signing>
where
    Signing: Send + Sync + 'static,
{
    const DEFAULT_COSMOS_DERIVATION_PATH: &str = "m/44'/118'/0'/0/0";

    DEFAULT_COSMOS_DERIVATION_PATH
        .parse()
        .context("Failed to parse key derivation path!")
        .and_then(|derivation_path| {
            Mnemonic::new(phrase, Language::English)
                .map(|phrase| phrase.to_seed(password))
                .context("Failed to parse mnemonic!")
                .and_then(|seed| {
                    Signing::derive_from_path(seed, &derivation_path)
                        .context("Failed to derive signing key!")
                })
        })
}
