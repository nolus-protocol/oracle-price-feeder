pub mod account;
pub mod client;
pub mod configuration;
pub mod error;
pub mod messages;
pub mod signer;
pub mod tx;

#[macro_export]
macro_rules! context_message {
    ($message: literal) => {
        ::core::concat!(::core::module_path!(), ": ", $message)
    };
}
