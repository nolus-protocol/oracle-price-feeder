pub mod account;
pub mod client;
pub mod configuration;
pub mod error;
pub mod messages;
pub mod signer;
pub mod tx;

#[macro_export]
macro_rules! log_error {
    ($expr: expr, $error: literal $(, $args: expr)* $(,)?) => {{
        let result: ::std::result::Result<_, _> = $expr;

        if let Err(error) = &result {
            ::tracing::error!(
                error = ?error,
                $error
                $(, $args)*,
            );
        }

        result
    }};
}
